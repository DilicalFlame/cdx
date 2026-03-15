// Filename: src/main.rs

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;

use clap::Parser;
use colored::*;
use console::{Key, Term};
use directories::BaseDirs;
use ignore::{WalkBuilder, WalkState};
use serde::{Deserialize, Serialize};

/// A simple CLI tool to quickly navigate to project directories.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// The starting sequence of the directory name to search for (e.g., "101", "400.17").
    search_term: String,

    /// Optional file path to write the selected directory to (for shell integration).
    #[arg(long, hide = true)]
    out: Option<PathBuf>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct Config {
    ignored_folders: Vec<String>,
    #[serde(alias = "max_matches")]
    page_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            ignored_folders: vec![
                "node_modules".into(),
                "target".into(),
                ".venv".into(),
                ".idea".into(),
                ".vscode".into(),
                ".git".into(),
            ],
            page_size: 10,
        }
    }
}

fn load_config() -> Config {
    if let Some(base_dirs) = BaseDirs::new() {
        let config_dir = base_dirs.home_dir().join(".config");
        
        if !config_dir.exists() {
            let _ = fs::create_dir_all(&config_dir);
        }
        
        let config_path = config_dir.join("cdx.toml");
        
        if config_path.exists() {
            if let Ok(contents) = fs::read_to_string(&config_path) {
                if let Ok(config) = toml::from_str(&contents) {
                    return config;
                }
            }
        } else {
            let default_config = Config::default();
            if let Ok(toml_str) = toml::to_string(&default_config) {
                let _ = fs::write(config_path, toml_str);
            }
            return default_config;
        }
    }
    Config::default()
}

fn render_page(
    term: &mut Term,
    term_str: &str,
    page_items: &[PathBuf], 
    current_dir: &Path, 
    curr_sel: usize, 
    curr_page: usize, 
    total_pages: usize, 
    is_done: bool,
    page_size: usize,
    lines_drawn: &mut usize
) {
    if *lines_drawn > 0 {
        let _ = term.clear_last_lines(*lines_drawn);
    }

    let mut lines = Vec::new();
    let (_, cols) = term.size();
    let term_width = (cols as usize).saturating_sub(1).max(40); // Provide safe margin and minimum bounds
    
    lines.push(format!(" {} {}", "🔍 Search results for:".cyan(), term_str.cyan().bold()));
    lines.push(format!("{}", "─".repeat(term_width).dimmed()));
    
    for i in 0..page_size {
        if i < page_items.len() {
            let path = &page_items[i];
            let dir_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let rel_path = path.strip_prefix(current_dir).unwrap_or(path).display().to_string();
            
            let dir_width = console::measure_text_width(&dir_name);
            let rel_width = console::measure_text_width(&rel_path);
            
            // " ❯ 📁 " + dir_name + "  " -> Approx width footprint (8 cols)
            let base_width = 8 + dir_width + 2; 

            // Truncate from the middle of the relative path if it overflows terminal width
            let display_rel_path = if base_width < term_width {
                let available = term_width - base_width;
                if rel_width > available {
                    if available > 5 {
                        let allowed = available - 3;
                        // Keep 2/3 of the start, and 1/3 of the end
                        let start_len = (allowed * 2) / 3;
                        let end_len = allowed - start_len;
                        let start_part: String = rel_path.chars().take(start_len).collect();
                        let end_part: String = rel_path.chars().rev().take(end_len).collect::<Vec<_>>().into_iter().rev().collect();
                        format!("{}...{}", start_part, end_part)
                    } else {
                        rel_path.chars().take(available).collect()
                    }
                } else {
                    rel_path
                }
            } else {
                "".to_string()
            };
            
            // Safety measure if dir_name itself is longer than terminal width
            let dir_display = if base_width >= term_width && term_width > 12 {
                let allowed = term_width - 11;
                let trunc: String = dir_name.chars().take(allowed).collect();
                format!("{}...", trunc)
            } else {
                dir_name
            };

            if i == curr_sel {
                lines.push(format!(" {} 📁 {}  {}", 
                    "❯".green().bold(), 
                    dir_display.green().bold(), 
                    display_rel_path.green().dimmed()
                ));
            } else {
                let colors = [
                    "bright_red", "bright_yellow", "bright_green", 
                    "bright_cyan", "bright_blue", "bright_magenta"
                ];
                let color_idx = (curr_page * page_size + i) % colors.len();
                let color = colors[color_idx];
                
                lines.push(format!("   📁 {}  {}", 
                    dir_display.color(color), 
                    display_rel_path.dimmed()
                ));
            }
        } else {
            lines.push("".to_string()); // Padding to keep height consistent between pages
        }
    }
    
    lines.push(format!("{}", "─".repeat(term_width).dimmed()));
    
    let has_next = !is_done || curr_page + 1 < total_pages;
    let next_indicator = if has_next { "►" } else { " " };
    let prev_indicator = if curr_page > 0 { "◄" } else { " " };
    
    let page_info = format!("Page {} {} {}", prev_indicator, curr_page + 1, next_indicator);
    let controls = "[↑/↓] Navigate  [←/→] Pages  [↵] Select  [Esc] Quit";
    
    lines.push(format!(" {} │ {}", page_info.yellow(), controls.dimmed()));

    let output = lines.join("\n");
    let _ = term.write_line(&output);
    
    *lines_drawn = lines.len(); 
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
    let config = load_config();

    let current_dir = env::current_dir().expect("Failed to get current directory.");
    
    // We use a bounded channel. When full, Walker threads block automatically!
    let (tx, rx) = mpsc::sync_channel(config.page_size * 2);
    
    let config_clone = config.clone();
    let term_str = args.search_term.clone();
    let term_str_for_thread = term_str.clone();
    let current_dir_clone = current_dir.clone();
    
    let is_done = Arc::new(AtomicBool::new(false));
    let is_done_clone = Arc::clone(&is_done);

    std::thread::spawn(move || {
        let mut builder = WalkBuilder::new(&current_dir_clone);
        
        // Add support for custom .cdxignore files (works exactly like .gitignore)
        builder.add_custom_ignore_filename(".cdxignore");
        
        let ignored: Vec<String> = config_clone.ignored_folders.clone();
        
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
            let term = term_str_for_thread.clone();
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
        
        is_done_clone.store(true, Ordering::SeqCst);
    });

    let mut cached_pages: Vec<Vec<PathBuf>> = Vec::new();
    let page_size = config.page_size;

    // Load first page
    let mut first_page = Vec::new();
    while first_page.len() < page_size {
        if let Ok(p) = rx.recv() {
            first_page.push(p);
        } else {
            break;
        }
    }

    if first_page.is_empty() {
        eprintln!("{}", "No matching directory found.".red());
        return;
    }

    // Auto-select if there is exactly 1 match and search is completely finished
    if first_page.len() == 1 && is_done.load(Ordering::SeqCst) && rx.try_recv().is_err() {
        finish_selection(&first_page[0], args.out);
        return;
    }

    cached_pages.push(first_page);

    let mut term = Term::stdout();
    let _ = term.hide_cursor();

    let mut curr_page = 0;
    let mut curr_sel = 0;
    let mut lines_drawn = 0;
    let mut selected_path: Option<PathBuf> = None;

    loop {
        let is_search_finished = is_done.load(Ordering::SeqCst);
        let current_items = &cached_pages[curr_page];

        render_page(
            &mut term,
            &term_str,
            current_items, 
            &current_dir, 
            curr_sel, 
            curr_page, 
            cached_pages.len(), 
            is_search_finished,
            page_size,
            &mut lines_drawn
        );

        match term.read_key().unwrap() {
            Key::ArrowUp => {
                if curr_sel > 0 {
                    curr_sel -= 1;
                } else if curr_page > 0 {
                    // Wrap to bottom of previous page
                    curr_page -= 1;
                    curr_sel = cached_pages[curr_page].len() - 1;
                }
            }
            Key::ArrowDown => {
                let current_items = &cached_pages[curr_page];
                if curr_sel + 1 < current_items.len() {
                    curr_sel += 1;
                } else {
                    // Dive into next page 
                    if curr_page + 1 < cached_pages.len() {
                        curr_page += 1;
                        curr_sel = 0;
                    } else if !is_done.load(Ordering::SeqCst) {
                        let mut next_page = Vec::new();
                        while next_page.len() < page_size {
                            if let Ok(p) = rx.recv() {
                                next_page.push(p);
                            } else {
                                break;
                            }
                        }
                        if !next_page.is_empty() {
                            cached_pages.push(next_page);
                            curr_page += 1;
                            curr_sel = 0;
                        }
                    }
                }
            }
            Key::ArrowLeft => {
                if curr_page > 0 {
                    curr_page -= 1;
                    // Clamp selection to not overflow bound of previous page
                    curr_sel = std::cmp::min(curr_sel, cached_pages[curr_page].len() - 1);
                }
            }
            Key::ArrowRight => {
                if curr_page + 1 < cached_pages.len() {
                    curr_page += 1;
                    curr_sel = std::cmp::min(curr_sel, cached_pages[curr_page].len() - 1);
                } else {
                    let mut next_page = Vec::new();
                    while next_page.len() < page_size {
                        if let Ok(p) = rx.recv() {
                            next_page.push(p);
                        } else {
                            break;
                        }
                    }
                    if !next_page.is_empty() {
                        cached_pages.push(next_page);
                        curr_page += 1;
                        curr_sel = 0; 
                    }
                }
            }
            Key::Enter => {
                selected_path = Some(cached_pages[curr_page][curr_sel].clone());
                break;
            }
            Key::Escape | Key::Char('q') | Key::Char('c') => {
                break;
            }
            _ => {}
        }
    }

    if lines_drawn > 0 {
        let _ = term.clear_last_lines(lines_drawn);
    }
    
    let _ = term.show_cursor();

    if let Some(path) = selected_path {
        finish_selection(&path, args.out);
    }
}
