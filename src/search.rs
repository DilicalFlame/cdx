use regex::Regex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::thread;

use crate::config::Config;
use ignore::WalkBuilder;

pub fn spawn_search_thread(
    current_dir: PathBuf,
    term_str: String,
    config: Config,
    is_regex: bool,
    tx: SyncSender<PathBuf>,
    is_done: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut builder = WalkBuilder::new(&current_dir);

        // Add support for custom .cdxignore files (works exactly like .gitignore)
        builder.add_custom_ignore_filename(".cdxignore");

        let ignored: Vec<String> = config.ignored_folders.clone();

        builder.filter_entry(move |entry| {
            if let Some(name) = entry.file_name().to_str() {
                if ignored.iter().any(|ignored_name| name == ignored_name) {
                    return false;
                }
            }
            // Avoid matching the starting root itself as a valid return target
            true
        });

        // Pre-compile the regex if provided
        let regex_opt = if is_regex {
            Regex::new(&term_str).ok()
        } else {
            None
        };

        // Convert search term to lowercase for a case-insensitive contains match fallback
        let term_lower = term_str.to_lowercase();

        // Force deterministic alphabetical sorting for siblings
        builder.sort_by_file_name(|a, b| a.cmp(b));

        // Standard single-threaded traversal (required for strict tree sorting)
        for result in builder.build() {
            if let Ok(entry) = result {
                if entry.depth() == 0 {
                    continue;
                }

                if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                    if let Some(file_name) = entry.file_name().to_str() {
                        let is_match = if let Some(ref re) = regex_opt {
                            re.is_match(file_name)
                        } else {
                            file_name.to_lowercase().contains(&term_lower)
                        };

                        if is_match {
                            // Send blocks when channel is full, efficiently pausing the search!
                            if tx.send(entry.into_path()).is_err() {
                                break;
                            }
                        }
                    }
                }
            }
        }

        is_done.store(true, Ordering::SeqCst);
    });
}
