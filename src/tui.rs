use std::cmp;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use colored::*;
use console::{Key, Term};
use regex::RegexBuilder;
use terminal_size::{terminal_size, Height, Width};

fn highlight_and_chunk_path(text: &str, term: &str, is_regex: bool, term_width: usize) -> Vec<String> {
    let mut highlighted_spans = Vec::new();
    
    let re_res = if is_regex {
        RegexBuilder::new(term).case_insensitive(true).build().ok()
    } else {
        RegexBuilder::new(&regex::escape(term)).case_insensitive(true).build().ok()
    };

    let chars: Vec<char> = text.chars().collect();

    if let Some(re) = re_res {
        if !re.as_str().is_empty() {
            for mat in re.find_iter(text) {
                // translate byte indices to char indices for iteration
                let start_char = text[..mat.start()].chars().count();
                let end_char = text[..mat.end()].chars().count();
                highlighted_spans.push((start_char, end_char));
            }
        }
    }

    let mut lines = Vec::new();
    let mut current_idx = 0;
    let mut first_line = true;

    while current_idx < chars.len() {
        let indent = if first_line { 10 } else { 10 };
        let avail = term_width.saturating_sub(indent).max(10);
        let chunk_end = cmp::min(current_idx + avail, chars.len());
        let chunk = &chars[current_idx..chunk_end];
        
        let mut colored_chunk = String::new();
        let mut current_style_highlighted = false;
        let mut temp_str = String::new();

        for (i, &c) in chunk.iter().enumerate() {
            let global_i = current_idx + i;
            let is_highlighted = highlighted_spans.iter().any(|&(s, e)| global_i >= s && global_i < e);
            
            if i == 0 {
                current_style_highlighted = is_highlighted;
            }

            if is_highlighted == current_style_highlighted {
                temp_str.push(c);
            } else {
                if current_style_highlighted {
                    colored_chunk.push_str(&temp_str.black().on_yellow().bold().to_string());
                } else {
                    colored_chunk.push_str(&temp_str.cyan().to_string());
                }
                temp_str.clear();
                temp_str.push(c);
                current_style_highlighted = is_highlighted;
            }
        }

        if !temp_str.is_empty() {
            if current_style_highlighted {
                colored_chunk.push_str(&temp_str.black().on_yellow().bold().to_string());
            } else {
                colored_chunk.push_str(&temp_str.cyan().to_string());
            }
        }

        if first_line {
            lines.push(format!(" {} {}", "📂 Path:".yellow(), colored_chunk));
            first_line = false;
        } else {
            lines.push(format!("          {}", colored_chunk));
        }
        
        current_idx = chunk_end;
    }
    
    lines
}

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
    is_regex: bool,
    paginate: bool,
    page_items: &[PathBuf],
    current_dir: &Path,
    curr_sel: usize,
    curr_page: usize,
    total_pages: usize,
    page_size: usize,
    term_rows: usize,
    lines_drawn: &mut usize,
    in_search_mode: bool,
    tui_search_query: &str,
) {
    if !paginate && *lines_drawn > 0 {
        let _ = term.clear_last_lines(*lines_drawn);
    }

    let mut lines = Vec::new();
    let (_, cols) = if let Some((Width(w), Height(h))) = terminal_size() {
        (h as u16, w as u16)
    } else {
        term.size()
    };
    
    let term_width = (cols as usize).saturating_sub(1).max(40); // Provide safe margin and minimum bounds

    let has_next = curr_page + 1 < total_pages;
    let next_indicator = if has_next { "►" } else { " " };
    let prev_indicator = if curr_page > 0 { "◄" } else { " " };
    let page_info = format!("Page {} {} {}", prev_indicator, curr_page + 1, next_indicator);
    let controls = "[↑/↓] Navigate  [←/→] Pages  [↵] Select  [Esc] Quit";

    lines.push(format!(" {} {}", "🔍 Search results for:".cyan(), term_str.cyan().bold()));
    lines.push(format!(" {}   {}", page_info.yellow(), controls.dimmed()));
    lines.push(format!("{}", "─".repeat(term_width).dimmed()));

    // Build the footer first so we know its height
    let mut footer = Vec::new();
    footer.push(format!("{}", "─".repeat(term_width).dimmed()));

    if in_search_mode {
        footer.push(format!(" {}", format!("/{}", tui_search_query).yellow().bold()));
    } else if let Some(hover_path) = page_items.get(curr_sel) {
        let abs_path = hover_path.display().to_string();
        let preview_lines = highlight_and_chunk_path(&abs_path, term_str, is_regex, term_width);
        for line in preview_lines {
            footer.push(line);
        }
    }
    footer.push(format!("{}", "─".repeat(term_width).dimmed()));

    // Determine how many items we can actually show given the footer size
    // Header = 3 lines, so available for items = term_rows - 3 - footer.len()
    let max_visible = if paginate {
        term_rows.saturating_sub(3 + footer.len()).max(1)
    } else {
        page_size // Non-paginate mode: show all page items as usual
    };

    // Compute visible window: trim from bottom, but ensure selected item is visible
    let visible_count = cmp::min(max_visible, page_items.len());
    let vis_start = if curr_sel >= visible_count {
        curr_sel - visible_count + 1
    } else {
        0
    };
    let vis_end = vis_start + visible_count;

    let max_dir_width = cmp::max(20, term_width / 2);

    for i in vis_start..vis_end {
        if i < page_items.len() {
            let path = &page_items[i];
            let dir_name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            let rel_path = path.strip_prefix(current_dir).unwrap_or(path);
            let rel_path = rel_path.parent().map(|p| p.display().to_string()).unwrap_or_default();

            let dir_width = console::measure_text_width(&dir_name);
            let rel_width = console::measure_text_width(&rel_path);

            // Safety measure to ensure rel_path gets enough space: Truncate dir_name if it is too massive
            let dir_display = if dir_width > max_dir_width {
                let trunc: String = dir_name.chars().take(max_dir_width - 3).collect();
                format!("{}...", trunc)
            } else {
                dir_name
            };

            let act_dir_width = console::measure_text_width(&dir_display);

            // " ❯ 📁 " + dir_name + "  " -> Approx width footprint (8 cols)
            let base_width = 8 + act_dir_width + 2;

            // Truncate from the middle of the relative path if it overflows terminal width
            let display_rel_path = if base_width < term_width {
                let available = term_width - base_width;
                if rel_width > available {
                    if available > 5 {
                        let allowed = available - 3;
                        // Keep 1/3 of the start, and 2/3 of the end
                        let start_len = allowed / 3;
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
        } else if paginate {
            lines.push("".to_string());
        }
    }

    if paginate {
        // Pad between items and footer to push footer to the bottom
        let target_len = term_rows.saturating_sub(footer.len());
        while lines.len() < target_len {
            lines.push(String::new());
        }
    }

    for fl in footer {
        lines.push(fl);
    }

    if paginate {
        *lines_drawn = lines.len();
        let output = lines.into_iter().map(|l| format!("{}\x1B[K", l)).collect::<Vec<_>>().join("\n");
        print!("\x1B[H{}", output);
        let _ = std::io::stdout().flush();
    } else {
        *lines_drawn = lines.len();
        let output = lines.join("\n");
        let _ = term.write_line(&output);
    }
}

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
    let mut frame_idx = 0;
    
    // Draw initial loading spinner
    let spinner_drawn = true;
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

    let mut curr_page = 0;
    let mut curr_sel = 0;
    let mut lines_drawn = 0;
    let mut selected_path: Option<PathBuf> = None;
    let mut tui_search_query = String::new();
    let mut in_search_mode = false;

    loop {
        // Compute page size with fixed minimal footer (separator + 1 path line + separator = 3 lines)
        // This keeps chunking stable. render_page handles the visual trimming dynamically.
        let (term_rows, term_cols) = if paginate {
            if let Some((Width(w), Height(h))) = terminal_size() {
                (h as usize, w as usize)
            } else {
                let (r, c) = term.size();
                (r as usize, c as usize)
            }
        } else {
            (0, 0) // Not used in non-paginate mode
        };

        if paginate {
            // 3 header + 3 minimal footer = 6 lines reserved
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

        let paged_cache = chunks(&cached_paths, page_size);
        if paged_cache.is_empty() { break; }

        if curr_page >= paged_cache.len() {
            curr_page = paged_cache.len().saturating_sub(1);
            curr_sel = paged_cache[curr_page].len().saturating_sub(1);
        }

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
            page_size,
            term_rows,
            &mut lines_drawn,
            in_search_mode,
            &tui_search_query,
        );

        match term.read_key().unwrap() {
            Key::Escape => {
                if in_search_mode {
                    in_search_mode = false;
                } else {
                    break;
                }
            }
            Key::Enter => {
                if in_search_mode {
                    in_search_mode = false;
                    if let Some(pos) = cached_paths.iter().position(|p| {
                        p.display().to_string().to_lowercase().contains(&tui_search_query.to_lowercase())
                    }) {
                        curr_page = pos / page_size;
                        curr_sel = pos % page_size;
                    }
                } else {
                    selected_path = Some(paged_cache[curr_page][curr_sel].clone());
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
                if !in_search_mode {
                    if curr_page > 0 {
                        curr_page -= 1;
                        curr_sel = cmp::min(curr_sel, paged_cache[curr_page].len().saturating_sub(1));
                    }
                }
            }
            Key::ArrowRight => {
                if !in_search_mode {
                    if curr_page + 1 < paged_cache.len() {
                        curr_page += 1;
                        curr_sel = cmp::min(curr_sel, paged_cache[curr_page].len().saturating_sub(1));
                    }
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
                                p.display().to_string().to_lowercase().contains(&tui_search_query.to_lowercase())
                            }) {
                                let pos = global_idx + 1 + offset;
                                curr_page = pos / page_size;
                                curr_sel = pos % page_size;
                            } else if let Some(pos) = cached_paths.iter().position(|p| {
                                p.display().to_string().to_lowercase().contains(&tui_search_query.to_lowercase())
                            }) {
                                curr_page = pos / page_size;
                                curr_sel = pos % page_size;
                            }
                        }
                        'N' => {
                            let global_idx = curr_page * page_size + curr_sel;
                            let mut found = None;
                            for i in (0..global_idx).rev() {
                                if cached_paths[i].display().to_string().to_lowercase().contains(&tui_search_query.to_lowercase()) {
                                    found = Some(i);
                                    break;
                                }
                            }
                            if found.is_none() {
                                for i in (global_idx..cached_paths.len()).rev() {
                                    if cached_paths[i].display().to_string().to_lowercase().contains(&tui_search_query.to_lowercase()) {
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
                            break;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
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
