mod render;

use std::cmp;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use colored::*;
use console::{Key, Term};
use terminal_size::{terminal_size, Height, Width};

use render::{chunks, render_page};

/// Main TUI entry point. Streams results live from the search channel,
/// renders pages, and handles user input.
pub fn run_tui(
    term_str: &str,
    is_regex: bool,
    paginate: bool,
    current_dir: &Path,
    mut page_size: usize,
    rx: crossbeam_channel::Receiver<PathBuf>,
    is_done: Arc<AtomicBool>,
) -> Option<PathBuf> {
    if paginate {
        print!("\x1B[?1049h\x1B[H");
        let _ = std::io::stdout().flush();
    }

    let mut term = Term::stdout();
    let _ = term.hide_cursor();

    let mut cached_paths: Vec<PathBuf> = Vec::new();
    let spinner_frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let mut frame_idx: usize = 0;

    let mut curr_page = 0;
    let mut curr_sel = 0;
    let mut lines_drawn = 0;
    let mut selected_path: Option<PathBuf> = None;
    let mut tui_search_query = String::new();
    let mut in_search_mode = false;

    // Spawn a dedicated key-reader thread so we can poll for keys non-blockingly
    let (key_tx, key_rx) = crossbeam_channel::unbounded();
    let key_term = Term::stdout();
    std::thread::spawn(move || {
        loop {
            match key_term.read_key() {
                Ok(key) => {
                    if key_tx.send(key).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    loop {
        // --- Phase 1: Drain new results from the search channel ---
        while let Ok(p) = rx.try_recv() {
            cached_paths.push(p);
        }

        let still_searching = !is_done.load(Ordering::SeqCst);

        // If we have nothing yet, show a brief loading state and continue
        if cached_paths.is_empty() {
            if !still_searching {
                if paginate {
                    print!("\x1B[?1049l");
                    let _ = std::io::stdout().flush();
                }
                let _ = term.show_cursor();
                eprintln!("{}", "No matching directory found.".red());
                return None;
            }
            frame_idx = (frame_idx + 1) % spinner_frames.len();
            if !paginate {
                if lines_drawn > 0 {
                    let _ = term.clear_last_lines(lines_drawn);
                }
                let _ = term.write_line(&format!(" {} {} ",
                    spinner_frames[frame_idx].cyan(),
                    "Crawling file system...".dimmed(),
                ));
                lines_drawn = 1;
            } else {
                print!("\x1B[H {} {} \x1B[K",
                    spinner_frames[frame_idx].cyan(),
                    "Crawling file system...".dimmed(),
                );
                let _ = std::io::stdout().flush();
            }
            std::thread::sleep(Duration::from_millis(30));
            continue;
        }

        // Auto-select if search is done and exactly 1 match
        if !still_searching && cached_paths.len() == 1 {
            selected_path = Some(cached_paths[0].clone());
            break;
        }

        // --- Phase 2: Compute page size ---
        let (term_rows, _term_cols) = if paginate {
            if let Some((Width(w), Height(h))) = terminal_size() {
                (h as usize, w as usize)
            } else {
                let (r, c) = term.size();
                (r as usize, c as usize)
            }
        } else {
            (0, 0)
        };

        if paginate {
            let new_page_size = term_rows.saturating_sub(6).max(1);
            if new_page_size != page_size {
                let gi = if page_size > 0 { curr_page * page_size + curr_sel } else { 0 };
                page_size = new_page_size;
                if !cached_paths.is_empty() {
                    curr_page = cmp::min(gi / page_size, cached_paths.len().saturating_sub(1) / page_size);
                    let page_len = cmp::min(page_size, cached_paths.len() - curr_page * page_size);
                    curr_sel = cmp::min(gi % page_size, page_len.saturating_sub(1));
                }
            }
        }

        // --- Phase 3: Render ---
        let paged_cache = chunks(&cached_paths, page_size);
        if paged_cache.is_empty() {
            std::thread::sleep(Duration::from_millis(30));
            continue;
        }

        if curr_page >= paged_cache.len() {
            curr_page = paged_cache.len().saturating_sub(1);
            curr_sel = paged_cache[curr_page].len().saturating_sub(1);
        }
        if curr_sel >= paged_cache[curr_page].len() {
            curr_sel = paged_cache[curr_page].len().saturating_sub(1);
        }

        frame_idx = (frame_idx + 1) % spinner_frames.len();

        let current_items = &paged_cache[curr_page];

        render_page(
            &mut term,
            term_str,
            is_regex,
            paginate,
            current_items,
            current_dir,
            curr_sel,
            curr_page,
            paged_cache.len(),
            cached_paths.len(),
            page_size,
            term_rows,
            &mut lines_drawn,
            in_search_mode,
            &tui_search_query,
            still_searching,
            spinner_frames[frame_idx],
        );

        // --- Phase 4: Poll for key input (non-blocking with timeout) ---
        let timeout = if still_searching {
            Duration::from_millis(60)
        } else {
            Duration::from_millis(500)
        };

        let first_key = key_rx.recv_timeout(timeout).ok();
        let mut keys: Vec<Key> = Vec::new();
        if let Some(k) = first_key {
            keys.push(k);
            while let Ok(k) = key_rx.try_recv() {
                keys.push(k);
            }
        }

        let mut should_break = false;
        for key in keys {
            match key {
                Key::Escape => {
                    if in_search_mode {
                        in_search_mode = false;
                    } else {
                        should_break = true;
                        break;
                    }
                }
                Key::Enter => {
                    if in_search_mode {
                        in_search_mode = false;
                        if let Some(pos) = cached_paths.iter().position(|p| {
                            p.file_name().unwrap_or_default().to_string_lossy().to_lowercase().contains(&tui_search_query.to_lowercase())
                        }) {
                            curr_page = pos / page_size;
                            curr_sel = pos % page_size;
                        }
                    } else {
                        selected_path = Some(paged_cache[curr_page][curr_sel].clone());
                        should_break = true;
                        break;
                    }
                }
                Key::Backspace => {
                    if in_search_mode {
                        tui_search_query.pop();
                    }
                }
                Key::ArrowUp => {
                    if !in_search_mode {
                        if curr_sel > 0 {
                            curr_sel -= 1;
                        } else if curr_page > 0 {
                            curr_page -= 1;
                            curr_sel = paged_cache[curr_page].len() - 1;
                        }
                    }
                }
                Key::ArrowDown => {
                    if !in_search_mode {
                        let current_items = &paged_cache[curr_page];
                        if curr_sel + 1 < current_items.len() {
                            curr_sel += 1;
                        } else if curr_page + 1 < paged_cache.len() {
                            curr_page += 1;
                            curr_sel = 0;
                        }
                    }
                }
                Key::ArrowLeft => {
                    if !in_search_mode && curr_page > 0 {
                        curr_page -= 1;
                        curr_sel = cmp::min(curr_sel, paged_cache[curr_page].len().saturating_sub(1));
                    }
                }
                Key::ArrowRight => {
                    if !in_search_mode && curr_page + 1 < paged_cache.len() {
                        curr_page += 1;
                        curr_sel = cmp::min(curr_sel, paged_cache[curr_page].len().saturating_sub(1));
                    }
                }
                Key::Char(c) => {
                    if in_search_mode {
                        tui_search_query.push(c);
                    } else {
                        match c {
                            'k' => {
                                if curr_sel > 0 {
                                    curr_sel -= 1;
                                } else if curr_page > 0 {
                                    curr_page -= 1;
                                    curr_sel = paged_cache[curr_page].len() - 1;
                                }
                            }
                            'j' => {
                                let current_items = &paged_cache[curr_page];
                                if curr_sel + 1 < current_items.len() {
                                    curr_sel += 1;
                                } else if curr_page + 1 < paged_cache.len() {
                                    curr_page += 1;
                                    curr_sel = 0;
                                }
                            }
                            'h' => {
                                if curr_page > 0 {
                                    curr_page -= 1;
                                    curr_sel = cmp::min(curr_sel, paged_cache[curr_page].len().saturating_sub(1));
                                }
                            }
                            'l' => {
                                if curr_page + 1 < paged_cache.len() {
                                    curr_page += 1;
                                    curr_sel = cmp::min(curr_sel, paged_cache[curr_page].len().saturating_sub(1));
                                }
                            }
                            '/' => {
                                in_search_mode = true;
                                tui_search_query.clear();
                            }
                            'n' => {
                                let global_idx = curr_page * page_size + curr_sel;
                                if let Some(offset) = cached_paths.iter().skip(global_idx + 1).position(|p| {
                                    p.file_name().unwrap_or_default().to_string_lossy().to_lowercase().contains(&tui_search_query.to_lowercase())
                                }) {
                                    let pos = global_idx + 1 + offset;
                                    curr_page = pos / page_size;
                                    curr_sel = pos % page_size;
                                } else if let Some(pos) = cached_paths.iter().position(|p| {
                                    p.file_name().unwrap_or_default().to_string_lossy().to_lowercase().contains(&tui_search_query.to_lowercase())
                                }) {
                                    curr_page = pos / page_size;
                                    curr_sel = pos % page_size;
                                }
                            }
                            'N' => {
                                let global_idx = curr_page * page_size + curr_sel;
                                let mut found = None;
                                for i in (0..global_idx).rev() {
                                    if cached_paths[i].file_name().unwrap_or_default().to_string_lossy().to_lowercase().contains(&tui_search_query.to_lowercase()) {
                                        found = Some(i);
                                        break;
                                    }
                                }
                                if found.is_none() {
                                    for i in (global_idx..cached_paths.len()).rev() {
                                        if cached_paths[i].file_name().unwrap_or_default().to_string_lossy().to_lowercase().contains(&tui_search_query.to_lowercase()) {
                                            found = Some(i);
                                            break;
                                        }
                                    }
                                }
                                if let Some(pos) = found {
                                    curr_page = pos / page_size;
                                    curr_sel = pos % page_size;
                                }
                            }
                            'q' | 'c' => {
                                should_break = true;
                                break;
                            }
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
            if should_break { break; }
        }
        if should_break { break; }
    }

    if paginate {
        print!("\x1B[?1049l");
        let _ = std::io::stdout().flush();
    } else if lines_drawn > 0 {
        let _ = term.clear_last_lines(lines_drawn);
    }

    let _ = term.show_cursor();

    selected_path
}
