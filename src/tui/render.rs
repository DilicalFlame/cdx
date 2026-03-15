use std::cmp;
use std::io::Write;
use std::path::{Path, PathBuf};

use colored::*;
use console::Term;
use regex::RegexBuilder;
use terminal_size::{terminal_size, Height, Width};

/// Highlight search term matches in a path string and chunk it into lines
/// that fit within the terminal width.
pub fn highlight_and_chunk_path(text: &str, term: &str, is_regex: bool, term_width: usize) -> Vec<String> {
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

/// Split a slice of paths into page-sized chunks.
pub fn chunks(paths: &[PathBuf], size: usize) -> Vec<Vec<PathBuf>> {
    paths.chunks(size).map(|c| c.to_vec()).collect()
}

/// Render a single page of TUI results to the terminal.
pub fn render_page(
    term: &mut Term,
    term_str: &str,
    is_regex: bool,
    paginate: bool,
    page_items: &[PathBuf],
    current_dir: &Path,
    curr_sel: usize,
    curr_page: usize,
    total_pages: usize,
    total_results: usize,
    page_size: usize,
    term_rows: usize,
    lines_drawn: &mut usize,
    in_search_mode: bool,
    tui_search_query: &str,
    still_searching: bool,
    spinner_frame: &str,
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
    
    let term_width = (cols as usize).saturating_sub(1).max(40);

    // --- Header ---
    let has_next = curr_page + 1 < total_pages;
    let next_indicator = if has_next { "►" } else { " " };
    let prev_indicator = if curr_page > 0 { "◄" } else { " " };
    let page_info = format!("Page {} {} {}", prev_indicator, curr_page + 1, next_indicator);
    let controls = "[↑/↓] Navigate  [←/→] Pages  [↵] Select  [Esc] Quit";

    if still_searching {
        lines.push(format!(" {} {} {} {}",
            spinner_frame.cyan(),
            "Searching:".cyan(),
            term_str.cyan().bold(),
            format!("({} found)", total_results).yellow()
        ));
    } else {
        lines.push(format!(" {} {} {}",
            "🔍 Search results for:".cyan(),
            term_str.cyan().bold(),
            format!("({} total)", total_results).dimmed()
        ));
    }
    lines.push(format!(" {}   {}", page_info.yellow(), controls.dimmed()));
    lines.push(format!("{}", "─".repeat(term_width).dimmed()));

    // --- Footer (computed first so we know its height) ---
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

    // --- Item list (dynamically trimmed to fit footer) ---
    let max_visible = if paginate {
        term_rows.saturating_sub(3 + footer.len()).max(1)
    } else {
        page_size
    };

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

            let dir_display = if dir_width > max_dir_width {
                let trunc: String = dir_name.chars().take(max_dir_width - 3).collect();
                format!("{}...", trunc)
            } else {
                dir_name
            };

            let act_dir_width = console::measure_text_width(&dir_display);
            let base_width = 8 + act_dir_width + 2;

            let display_rel_path = if base_width < term_width {
                let available = term_width - base_width;
                if rel_width > available {
                    if available > 5 {
                        let allowed = available - 3;
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

    // --- Padding + Footer ---
    if paginate {
        let target_len = term_rows.saturating_sub(footer.len());
        while lines.len() < target_len {
            lines.push(String::new());
        }
    }

    for fl in footer {
        lines.push(fl);
    }

    // --- Flush to terminal ---
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
