use regex::Regex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

use crate::config::Config;
use ignore::WalkBuilder;

/// Zero-allocation, case-insensitive substring match.
/// `needle` MUST already be lowercase ASCII.
///
/// Uses memchr for fast scanning of the first byte candidate,
/// then verifies the rest without any allocation.
#[inline(always)]
fn contains_ignore_case(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.len() > haystack.len() {
        return false;
    }

    let first_lower = needle[0]; // Already lowercase
    let first_upper = first_lower.to_ascii_uppercase();

    // Scan for both cases of the first byte using memchr
    let mut offset = 0;
    while offset + needle.len() <= haystack.len() {
        // Find next occurrence of first byte (either case) using memchr
        let remaining = &haystack[offset..];
        let pos = if first_lower == first_upper {
            memchr::memchr(first_lower, remaining)
        } else {
            memchr::memchr2(first_lower, first_upper, remaining)
        };

        match pos {
            Some(p) => {
                let start = offset + p;
                if start + needle.len() > haystack.len() {
                    return false;
                }
                // Verify the rest of the match
                let candidate = &haystack[start..start + needle.len()];
                if candidate
                    .iter()
                    .zip(needle.iter())
                    .all(|(h, n)| h.to_ascii_lowercase() == *n)
                {
                    return true;
                }
                offset = start + 1;
            }
            None => return false,
        }
    }

    false
}

pub fn spawn_search_thread(
    current_dir: PathBuf,
    term_str: String,
    config: Config,
    is_regex: bool,
    search_all: bool,
    tx: crossbeam_channel::Sender<PathBuf>,
    is_done: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut builder = WalkBuilder::new(&current_dir);

        // Add support for custom .cdxignore files
        builder.add_custom_ignore_filename(".cdxignore");

        // Performance: use more threads for Windows where syscall latency is higher.
        // This saturates I/O by having more in-flight readdir operations.
        let thread_count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        builder.threads(thread_count.max(8));

        // Don't skip hidden directories — folders like AppData on Windows are hidden
        builder.hidden(false);

        // Don't honour .gitignore — we want to find ALL directories
        builder.git_ignore(false);
        builder.git_global(false);
        builder.git_exclude(false);

        // Only apply ignore list if -a (all) flag is not set
        if !search_all {
            let ignored: Vec<String> = config.ignored_folders.clone();
            builder.filter_entry(move |entry| {
                if let Some(name) = entry.file_name().to_str() {
                    if ignored.iter().any(|ignored_name| name == ignored_name) {
                        return false;
                    }
                }
                true
            });
        }

        let regex_opt = if is_regex {
            Regex::new(&term_str).ok()
        } else {
            None
        };

        // Pre-lowercase the search term once (zero-alloc matching uses this)
        let term_lower: Vec<u8> = term_str.bytes().map(|b| b.to_ascii_lowercase()).collect();

        // Parallel traversal (Lightning fast, doesn't re-scan)
        builder.build_parallel().run(|| {
            let tx = tx.clone();
            let regex_clone = regex_opt.clone();
            let term_lower = term_lower.clone();

            Box::new(move |result| {
                if let Ok(entry) = result {
                    if entry.depth() == 0 {
                        return ignore::WalkState::Continue;
                    }

                    if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                        let file_name = entry.file_name();
                        let name_bytes = file_name.as_encoded_bytes();

                        let is_match = if let Some(ref re) = regex_clone {
                            // Regex path: unavoidable string conversion
                            if let Some(name_str) = file_name.to_str() {
                                re.is_match(name_str)
                            } else {
                                false
                            }
                        } else {
                            // Fast path: zero-allocation case-insensitive match
                            contains_ignore_case(name_bytes, &term_lower)
                        };

                        if is_match {
                            if tx.send(entry.into_path()).is_err() {
                                return ignore::WalkState::Quit;
                            }
                        }
                    }
                }
                ignore::WalkState::Continue
            })
        });

        is_done.store(true, Ordering::SeqCst);
    });
}
