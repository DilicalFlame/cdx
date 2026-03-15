use std::cmp;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use colored::*;
use console::{Key, Term};

fn sort_paths(paths: &mut Vec<PathBuf>) {
    // Sort by Number of Components (Depth) first, then Alphabetically secondary
    paths.sort_by(|a, b| {
        let count_a = a.components().count();
        let count_b = b.components().count();
        match count_a.cmp(&count_b) {
            cmp::Ordering::Equal => a.cmp(b),
            other => other,
        }
    });
}

fn chunks(paths: &[PathBuf], size: usize) -> Vec<Vec<PathBuf>> {
    paths.chunks(size).map(|c| c.to_vec()).collect()
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
    lines_drawn: &mut usize,
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

pub fn run_tui(
    term_str: &str,
    current_dir: &Path,
    page_size: usize,
    rx: Receiver<PathBuf>,
    is_done: Arc<AtomicBool>,
) -> Option<PathBuf> {
    let mut term = Term::stdout();
    let _ = term.hide_cursor();

    let mut cached_paths: Vec<PathBuf> = Vec::new();
    let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let mut frame_idx = 0;
    
    // Draw initial loading spinner
    let mut spinner_drawn = true;
    let _ = term.write_line(&format!(" {} {}", spinner_frames[frame_idx].cyan(), "Crawling file system...".dimmed()));

    // Collect all matches across the entire drive instantly into memory
    loop {
        // Pull continuously while there is data
        while let Ok(p) = rx.try_recv() {
            cached_paths.push(p);
        }

        if is_done.load(Ordering::SeqCst) {
            // Drain the last few items
            while let Ok(p) = rx.try_recv() {
                cached_paths.push(p);
            }
            break;
        }

        // UI Spinner feedback loop so the user knows it's doing heavy I/O
        frame_idx = (frame_idx + 1) % spinner_frames.len();
        let _ = term.clear_last_lines(1);
        let _ = term.write_line(&format!(" {} {} (Found {} so far)", 
            spinner_frames[frame_idx].cyan(), 
            "Crawling file system...".dimmed(),
            cached_paths.len().to_string().yellow()
        ));
        
        std::thread::sleep(Duration::from_millis(50));
    }

    if spinner_drawn {
        let _ = term.clear_last_lines(1);
    }

    if cached_paths.is_empty() {
        eprintln!("{}", "No matching directory found.".red());
        return None;
    }

    // Single global deterministic sort: Depth-First, then Alphabetical!
    sort_paths(&mut cached_paths);

    // Auto-select if there is exactly 1 match
    if cached_paths.len() == 1 {
        return Some(cached_paths[0].clone());
    }

    let paged_cache = chunks(&cached_paths, page_size);

    let mut curr_page = 0;
    let mut curr_sel = 0;
    let mut lines_drawn = 0;
    let mut selected_path: Option<PathBuf> = None;

    loop {
        let current_items = &paged_cache[curr_page];

        render_page(
            &mut term,
            term_str,
            current_items,
            current_dir,
            curr_sel,
            curr_page,
            paged_cache.len(),
            true, // is_done is always true now because we pre-collected
            page_size,
            &mut lines_drawn,
        );

        match term.read_key().unwrap() {
            Key::ArrowUp => {
                if curr_sel > 0 {
                    curr_sel -= 1;
                } else if curr_page > 0 {
                    // Wrap to bottom of previous page
                    curr_page -= 1;
                    curr_sel = paged_cache[curr_page].len() - 1;
                }
            }
            Key::ArrowDown => {
                let current_items = &paged_cache[curr_page];
                if curr_sel + 1 < current_items.len() {
                    curr_sel += 1;
                } else {
                    // Dive into next page
                    if curr_page + 1 < paged_cache.len() {
                        curr_page += 1;
                        curr_sel = 0;
                    }
                }
            }
            Key::ArrowLeft => {
                if curr_page > 0 {
                    curr_page -= 1;
                    curr_sel = cmp::min(curr_sel, paged_cache[curr_page].len() - 1);
                }
            }
            Key::ArrowRight => {
                if curr_page + 1 < paged_cache.len() {
                    curr_page += 1;
                    curr_sel = cmp::min(curr_sel, paged_cache[curr_page].len() - 1);
                }
            }
            Key::Enter => {
                selected_path = Some(paged_cache[curr_page][curr_sel].clone());
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

    selected_path
}
