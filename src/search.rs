use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::Arc;
use std::thread;

use ignore::{WalkBuilder, WalkState};
use crate::config::Config;

pub fn spawn_search_thread(
    current_dir: PathBuf,
    term_str: String,
    config: Config,
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

        // Parallel traversal
        builder.build_parallel().run(|| {
            let tx = tx.clone();
            let term = term_str.clone();
            Box::new(move |result| {
                if let Ok(entry) = result {
                    if entry.depth() == 0 {
                        return WalkState::Continue;
                    }

                    if entry.file_type().is_some_and(|ft| ft.is_dir()) {
                        if let Some(file_name) = entry.file_name().to_str() {
                            if file_name.starts_with(&term) {
                                // Send blocks when channel is full, efficiently pausing the search!
                                if tx.send(entry.into_path()).is_err() {
                                    return WalkState::Quit;
                                }
                            }
                        }
                    }
                }
                WalkState::Continue
            })
        });

        is_done.store(true, Ordering::SeqCst);
    });
}
