mod config;
mod search;
mod tui;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use clap::Parser;

/// A simple CLI tool to quickly navigate to project directories.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The sequence of the directory name to search for (e.g., "101", "400.17", or a regex like "^data.*").
    search_term: String,

    /// Evaluate the search term as a Regular Expression rather than a simple substring.
    #[arg(short, long)]
    regex: bool,

    /// Dynamically paginate the results depending on the terminal height.
    #[arg(short, long)]
    paginate: bool,

    /// Ignore the ignore list and search among everything possible.
    #[arg(short, long)]
    all: bool,

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
    
    // We use an unbounded channel. The Walker threads blast through the disk at max speed
    // and push everything into the channel/cache, so we can sort them globally by shortest-path!
    let (tx, rx) = crossbeam_channel::unbounded();

    let is_done = Arc::new(AtomicBool::new(false));

    search::spawn_search_thread(
        current_dir.clone(),
        args.search_term.clone(),
        config.clone(),
        args.regex,
        args.all,
        tx,
        Arc::clone(&is_done),
    );

    let selected_path = tui::run_tui(
        &args.search_term,
        args.regex,
        args.paginate,
        &current_dir,
        config.page_size,
        rx,
        is_done,
    );

    if let Some(path) = selected_path {
        finish_selection(&path, args.out);
    }
}
