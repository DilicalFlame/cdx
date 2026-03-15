mod config;
mod search;
mod tui;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;

use clap::Parser;

/// A simple CLI tool to quickly navigate to project directories.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The starting sequence of the directory name to search for (e.g., "101", "400.17").
    search_term: String,

    /// Evaluate the search term as a Regular Expression rather than a simple prefix.
    #[arg(short, long)]
    regex: bool,

    /// Optional file path to write the selected directory to (for shell integration).
    #[arg(long, hide = true)]
    out: Option<PathBuf>,
}

fn finish_selection(path: &Path, out: Option<PathBuf>) {
    if let Some(out_path) = out {
        if let Err(e) = fs::write(&out_path, path.display().to_string()) {
            eprintln!("Failed to write to external file: {}", e);
        }
    } else {
        println!("{}", path.display());
    }
}

fn main() {
    let args = Args::parse();
    let config = config::load_config();

    let current_dir = env::current_dir().expect("Failed to get current directory.");
    
    // We use a bounded channel. When full, Walker threads block automatically!
    let (tx, rx) = mpsc::sync_channel(config.page_size * 2);

    let is_done = Arc::new(AtomicBool::new(false));

    search::spawn_search_thread(
        current_dir.clone(),
        args.search_term.clone(),
        config.clone(),
        args.regex,
        tx,
        Arc::clone(&is_done),
    );

    let selected_path = tui::run_tui(
        &args.search_term,
        &current_dir,
        config.page_size,
        rx,
        is_done,
    );

    if let Some(path) = selected_path {
        finish_selection(&path, args.out);
    }
}
