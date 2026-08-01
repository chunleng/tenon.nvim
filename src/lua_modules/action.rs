use nvim_oxi::{
    Dictionary, Function, Object,
    api::{Buffer, opts::OptionOpts, types::LogLevel},
    conversion::FromObject,
};

use crate::{get_chat_window, utils::format_path_relative};

type InsertSelectionParams = (i32, usize, Option<usize>, usize, Option<usize>);

pub fn create_lua_action_module() -> Dictionary {
    let mut action_dict = Dictionary::new();
    action_dict.insert("insert_selection", Object::from(insert_selection_fn()));
    action_dict.insert("select_chat", Object::from(select_chat_fn()));
    action_dict.insert("continue_chat", Object::from(continue_chat_fn()));
    action_dict.insert("show_detail", Object::from(show_detail_fn()));
    action_dict.insert("show_chat", Object::from(show_chat_fn()));
    action_dict
}

/// Insert a code selection from a buffer into the chat input.
///
/// Parameters (from Lua, 1-indexed):
/// - bufnr: buffer number
/// - start_line: start line (1-indexed)
/// - start_col: start column byte offset (1-indexed, or nil for whole line)
/// - end_line: end line (1-indexed)
/// - end_col: end column byte offset (1-indexed, inclusive, or nil for whole line)
fn insert_selection_fn() -> Function<InsertSelectionParams, ()> {
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
            let col_end = end_col.unwrap_or(last_line_text.len());

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
            let last_line_full = end_col.is_none() || end_col.unwrap_or(0) >= last_line_text.len();

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
                format!("{}\n", quoted)
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
                    "{}\n```{}\n{}\n```\n",
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

/// Show a picker to select and load a chat session.
fn select_chat_fn() -> Function<(), ()> {
    use crate::{
        chat::CHAT_SESSIONS,
        ui::picker::{FzfOption, SelectMode, box_single_select, pick},
    };
    use nvim_oxi::api::types::LogLevel;

    Function::from_fn({
        move |()| {
            // Get current chat index on main thread
            let current_index = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                Some(
                    win.loaded_chat_index
                        .load(std::sync::atomic::Ordering::SeqCst),
                )
            })()
            .unwrap_or(0);

            // Spawn thread to read sessions and show picker
            std::thread::spawn(move || {
                let sessions = CHAT_SESSIONS.lock().unwrap();
                if sessions.is_empty() {
                    crate::utils::GLOBAL_EXECUTION_HANDLER
                        .notify_on_main_thread("no chat sessions", LogLevel::Warn);
                    return;
                }

                // Build display options: "Chat N | Title"
                let options: Vec<String> = sessions
                    .iter()
                    .enumerate()
                    .map(|(i, session)| {
                        let guard = session.read().unwrap();
                        let title = guard
                            .title_handler
                            .title()
                            .unwrap_or("Untitled".to_string());
                        format!("Chat {} | {}", i + 1, title)
                    })
                    .collect();

                let options_clone = options.clone();
                let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
                let current_selection = options.get(current_index).map(|s| s.as_str());

                if let Err(e) = pick(
                    &options_refs,
                    FzfOption {
                        prompt: "Select Chat".to_string(),
                        select_mode: SelectMode::single(current_selection),
                        callback: box_single_select(move |selected| {
                            if let Some(selection) = selected {
                                let idx = options_clone.iter().position(|s| *s == selection);
                                if let Some(idx) = idx
                                    && let Err(e) = crate::utils::GLOBAL_EXECUTION_HANDLER
                                        .execute_rust_on_main_thread(move || {
                                            let win_arc = get_chat_window();
                                            if let Ok(mut win) = win_arc.lock()
                                                && let Err(e) = win.load_chat(idx)
                                            {
                                                crate::utils::notify(
                                                    format!("failed to load chat: {}", e),
                                                    LogLevel::Error,
                                                );
                                            }
                                            Ok(())
                                        })
                                {
                                    crate::utils::GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                                        format!("failed to load chat: {}", e),
                                        LogLevel::Error,
                                    );
                                }
                            }
                        }),
                        ..Default::default()
                    },
                ) {
                    crate::utils::GLOBAL_EXECUTION_HANDLER
                        .notify_on_main_thread(format!("picker error: {}", e), LogLevel::Error);
                }
            });
        }
    })
}

/// Continue the chat without adding a new user message.
fn continue_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.continue_chat()
            {
                crate::utils::notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

/// Swap the output window from chat display to the detail buffer.
fn show_detail_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.show_detail_view()
            {
                crate::utils::notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

/// Swap the output window from the detail buffer back to chat display.
fn show_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.show_chat_view()
            {
                crate::utils::notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}
