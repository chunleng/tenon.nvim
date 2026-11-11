use nvim_oxi::{
    Function,
    api::{Buffer, opts::OptionOpts},
    conversion::FromObject,
};

use crate::{get_chat_window, utils::format_path_relative};

pub type InsertSelectionParams = (i32, usize, Option<usize>, usize, Option<usize>);

/// Insert a code selection from a buffer into the chat input.
///
/// Parameters (from Lua, 1-indexed):
/// - bufnr: buffer number
/// - start_line: start line (1-indexed)
/// - start_col: start column byte offset (1-indexed, or nil for whole line)
/// - end_line: end line (1-indexed)
/// - end_col: end column byte offset (1-indexed, inclusive, or nil for whole line)
pub fn insert_selection_fn() -> Function<InsertSelectionParams, ()> {
    Function::from_fn(
        |(bufnr, start_line, start_col, end_line, end_col): InsertSelectionParams| {
            let buf = Buffer::from(bufnr);

            // Get buftype first to determine formatting approach
            let option_opt = OptionOpts::builder().buf(buf.clone()).build();
            let buftype: String = nvim_oxi::api::get_option_value("buftype", &option_opt)
                .ok()
                .and_then(|obj| String::from_object(obj).ok())
                .unwrap_or_default();

            // Convert from 1-indexed (Lua) to 0-indexed (nvim_oxi)
            // For lines 5-6 (1-indexed), we want 0-indexed rows 4-5, range 4..6
            let line_start = start_line.saturating_sub(1);
            let line_end = end_line.saturating_sub(1);

            let col_start = start_col.map(|c| c.saturating_sub(1)).unwrap_or(0);

            // For end column, None means full line - we need to get the line length
            let last_line_idx = end_line.saturating_sub(1);
            let last_line_text: String = buf
                .get_lines(last_line_idx..end_line, false)
                .ok()
                .and_then(|lines| lines.into_iter().next())
                .map(|s| s.to_string())
                .unwrap_or_default();
            let col_end = selection_end_exclusive(&last_line_text, end_col);

            // Get the selected text
            let text_lines: Vec<String> = buf
                .get_text(
                    line_start..line_end,
                    col_start,
                    col_end,
                    &nvim_oxi::api::opts::GetTextOpts::default(),
                )
                .map(|iter| iter.map(|s| s.to_string()).collect())
                .unwrap_or_default();

            if text_lines.is_empty() {
                return;
            }

            // Detect partial lines (full line when col is None)
            let first_line_full = start_col.is_none() || start_col.unwrap_or(1) <= 1;
            let last_line_full = end_col.is_none() || col_end >= last_line_text.len();

            // Build snippet with ellipsis for partial lines
            let text_lines_len = text_lines.len();
            let mut snippet_lines = Vec::new();
            for (i, line) in text_lines.into_iter().enumerate() {
                let mut line_text = line;
                if i == 0 && !first_line_full {
                    line_text = format!("...{}", line_text);
                }
                if i == text_lines_len - 1 && !last_line_full {
                    line_text = format!("{}...", line_text);
                }
                snippet_lines.push(line_text);
            }
            let snippet_text = snippet_lines.join("\n");

            // Format based on buftype
            let markdown = if buftype == "nofile" {
                // Blockquote format for nofile buffers
                let quoted = snippet_text
                    .lines()
                    .map(|line| format!("> {}", line))
                    .collect::<Vec<_>>()
                    .join("\n");
                format!("{}\n\n", quoted)
            } else {
                // Code block format for regular buffers
                // Get snippet-specific metadata
                let filepath = buf.get_name().ok().map(|p| p.to_string_lossy().to_string());
                let filename = filepath.as_ref().map(|path| format_path_relative(path));
                let filetype: String = nvim_oxi::api::get_option_value("filetype", &option_opt)
                    .ok()
                    .and_then(|obj| String::from_object(obj).ok())
                    .unwrap_or("text".to_string());

                // end_line from Vim is exclusive (line after last), so display_end = end_line - 1
                let header = if let Some(ref name) = filename {
                    if start_line == end_line {
                        format!("Snippet from {} Line {}\n", name, start_line)
                    } else {
                        format!("Snippet from {} Lines {}-{}\n", name, start_line, end_line)
                    }
                } else if start_line == end_line {
                    format!("Snippet Line {}\n", start_line)
                } else {
                    format!("Snippet Lines {}-{}\n", start_line, end_line)
                };

                // Escape triple backticks at line start
                let escaped_snippet = snippet_text
                    .lines()
                    .map(|line| {
                        if let Some(stripped) = line.strip_prefix("```") {
                            format!("\\`\\`\\`{}", stripped)
                        } else {
                            line.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                format!(
                    "{}\n```{}\n{}\n```\n\n",
                    header.trim_end(),
                    filetype,
                    escaped_snippet
                )
            };

            // Append to chat input using the existing method
            if let Ok(mut win) = get_chat_window().lock() {
                let _ = win.insert_to_input(markdown);
            }
        },
    )
}

/// Find the exclusive selection endpoint without cutting a multibyte character in half.
fn selection_end_exclusive(line: &str, end_col: Option<usize>) -> usize {
    let Some(col) = end_col else {
        return line.len();
    };
    let start = col.saturating_sub(1);
    line.get(start..)
        .and_then(|suffix| suffix.chars().next())
        .map_or(col, |ch| start + ch.len_utf8())
}

#[cfg(test)]
mod tests {
    use super::selection_end_exclusive;

    #[test]
    fn inclusive_end_column_includes_entire_character() {
        let line = "aé中😀z";

        assert_eq!(selection_end_exclusive(line, Some(1)), 1);
        assert_eq!(selection_end_exclusive(line, Some(2)), 3);
        assert_eq!(selection_end_exclusive(line, Some(4)), 6);
        assert_eq!(selection_end_exclusive(line, Some(7)), 10);
        assert_eq!(selection_end_exclusive(line, Some(11)), 11);
        assert_eq!(selection_end_exclusive(line, None), 11);
        assert_eq!(selection_end_exclusive("a😀", Some(2)), 5);
    }
}
