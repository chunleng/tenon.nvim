use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use crate::chat::{ChatSession, TenonLog, TenonLogData};
use crate::ui::widget::chat_display::format::DisplayChatFormatter;

pub struct StreamUpdate {
    pub replace_line_start: usize,
    pub replace_line_end: usize,
    pub lines: Vec<String>,
    pub line_hl_group: String,
    pub sign: String,
    pub sign_hl_group: String,
}

enum RenderedLocation {
    Hidden,
    Shown {
        line_start: usize,
        line_count: usize,
    },
}

struct RenderedLogEntry {
    log: Arc<TenonLog>,
    render_location: RenderedLocation,
    last_updated_at: DateTime<Utc>,
}

pub struct ChatLogCache {
    pub chat_session: Arc<RwLock<ChatSession>>,
    /// The index into the chat log vector marking the position from which logs may be stale and
    /// need to be checked for updates.
    check_from_index: std::sync::atomic::AtomicUsize,

    rendered_entries: Vec<RenderedLogEntry>,
}

use std::sync::atomic::Ordering;

fn process_log_lines(current_log: &TenonLogData, next_log: Option<&TenonLogData>) -> Vec<String> {
    // Treat system tools as hidden (no next_log)
    let next_log = next_log.filter(|n| !n.is_hidden_system_tool());

    let mut x: Vec<String> = current_log.lines().into_iter().collect();

    // For assistant reasoning, limit displayed lines
    if let TenonLogData::Assistant(msg) = current_log
        && msg.content.is_empty()
        && msg.reasoning.is_some()
    {
        let display_last_x = if next_log.is_some() { 1 } else { 3 };
        let total_lines = x.len();
        if total_lines > display_last_x {
            let skip = total_lines.saturating_sub(display_last_x);
            x = x.into_iter().skip(skip).collect();
            if let Some(first) = x.first_mut() {
                *first = format!("... {}", first);
            }
        }
    }

    // Add empty line separator unless both current and next are Tools
    if !(matches!(current_log, TenonLogData::Tool(_))
        && next_log.is_some_and(|n| matches!(n, TenonLogData::Tool(_))))
    {
        x.extend(vec!["".to_string()]);
    }

    x
}

impl ChatLogCache {
    pub fn new(chat_session: Arc<RwLock<ChatSession>>) -> Self {
        Self {
            chat_session,
            check_from_index: std::sync::atomic::AtomicUsize::new(0),
            rendered_entries: vec![],
        }
    }

    /// Returns the log entry that occupies the given 0-based buffer line, or
    /// `None` if the line doesn't belong to any rendered entry. Hidden entries
    /// are skipped.
    pub fn get_log_at_line(&self, line: usize) -> Option<Arc<TenonLog>> {
        self.rendered_entries
            .iter()
            .find_map(|entry| match &entry.render_location {
                RenderedLocation::Shown {
                    line_start,
                    line_count,
                } if *line_start <= line && line < *line_start + *line_count => {
                    Some(entry.log.clone())
                }
                _ => None,
            })
    }

    /// Updates an existing rendered entry or inserts a new one.
    /// Returns `(render_location, new_current_line)` where:
    /// - `render_location` is `Some(Shown { line_start, line_end })` when entry changed/new and not System tool (needs render)
    /// - `render_location` is `Some(Hidden)` when entry is System tool (no render)
    /// - `render_location` is `None` when entry unchanged (skip render)
    /// - `new_current_line` is always provided for position tracking
    fn upsert_entry_if_changed(
        &mut self,
        log_index: usize,
        log: &Arc<TenonLog>,
        lines: &[String],
        current_line: usize,
    ) -> (Option<RenderedLocation>, usize) {
        if let Some(existing) = self.rendered_entries.get_mut(log_index) {
            // At this point, render_location is Shown
            let (line_start, line_count) = match &existing.render_location {
                RenderedLocation::Shown {
                    line_start,
                    line_count,
                } => (*line_start, *line_count),
                RenderedLocation::Hidden => {
                    if log.data.is_hidden_system_tool() {
                        return (None, current_line);
                    }
                    existing.log = log.clone();
                    existing.render_location = RenderedLocation::Shown {
                        line_start: current_line,
                        line_count: lines.len(),
                    };
                    existing.last_updated_at = log.last_updated_at;
                    return (
                        Some(RenderedLocation::Shown {
                            line_start: current_line,
                            line_count: 0,
                        }),
                        current_line + lines.len(),
                    );
                }
            };

            // Check if separator changed (line count differs for Tool logs)
            let separator_changed = line_count != lines.len();

            // Unchanged entry: return position for tracking, but no render needed
            if Arc::ptr_eq(log, &existing.log)
                && log.last_updated_at <= existing.last_updated_at
                && !separator_changed
            {
                return (None, line_start + line_count);
            }

            existing.log = log.clone();
            existing.render_location = RenderedLocation::Shown {
                line_start: current_line,
                line_count: lines.len(),
            };
            existing.last_updated_at = log.last_updated_at;

            return (
                Some(RenderedLocation::Shown {
                    line_start: current_line,
                    line_count: line_count.saturating_sub(current_line.saturating_sub(line_start)),
                    // This is to ensure that even if the line has shifted because previous
                    // rendered_entries changes, it will still replace the area that was once
                    // occupied by the previous render
                }),
                current_line + lines.len(),
            );
        }

        // New entry
        if log.data.is_hidden_system_tool() {
            self.rendered_entries.push(RenderedLogEntry {
                log: log.clone(),
                render_location: RenderedLocation::Hidden,
                last_updated_at: log.last_updated_at,
            });
            (Some(RenderedLocation::Hidden), current_line)
        } else {
            self.rendered_entries.push(RenderedLogEntry {
                log: log.clone(),
                render_location: RenderedLocation::Shown {
                    line_start: current_line,
                    line_count: lines.len(),
                },
                last_updated_at: log.last_updated_at,
            });

            (
                Some(RenderedLocation::Shown {
                    line_start: current_line,
                    line_count: 0, // For new entries, replace from start
                }),
                current_line + lines.len(),
            )
        }
    }

    pub fn poll_render_update(&mut self) -> (Vec<StreamUpdate>, usize) {
        let check_from = self.check_from_index.load(Ordering::SeqCst);

        // Collect new logs while holding the lock
        let current_count = if let Ok(chat_session) = self.chat_session.read()
            && let Ok(log_window) = chat_session.log_handler.log_window.read()
        {
            let current_count = log_window.logs.len();

            // No new logs to render
            if check_from >= current_count {
                return (Vec::new(), 0);
            }

            current_count
        } else {
            return (Vec::new(), 0);
        };

        // Get the line_end of the rendered entry at check_from_index (or 0 if none)
        let mut current_line = {
            if check_from == 0 {
                0
            } else {
                self.rendered_entries
                    .get(check_from.saturating_sub(1))
                    .map(|entry| match &entry.render_location {
                        RenderedLocation::Shown {
                            line_start,
                            line_count,
                        } => line_start + line_count,
                        RenderedLocation::Hidden => 0, // Hidden entries don't affect line position
                    })
                    .unwrap_or(0)
            }
        };

        // Collect StreamUpdates for new logs
        // First, collect log data while holding the lock
        let logs_data: Vec<(usize, Arc<TenonLog>, Vec<String>)> = {
            let chat_session = self.chat_session.read().unwrap();
            let log_window = chat_session.log_handler.log_window.read().unwrap();

            log_window.logs[check_from..current_count]
                .iter()
                .enumerate()
                .map(|(offset, indexed_log)| {
                    // Find next visible log (skip system tools)
                    let next_log = log_window.logs[check_from + offset + 1..]
                        .iter()
                        .find(|n| !n.log.data.is_hidden_system_tool())
                        .map(|n| &n.log.data);

                    let x = process_log_lines(&indexed_log.log.data, next_log);

                    (check_from + offset, indexed_log.log.clone(), x)
                })
                .collect()
        };

        // Now process logs without holding the lock
        let updates: Vec<StreamUpdate> = logs_data
            .into_iter()
            .filter_map(|(log_index, log, lines)| {
                let (render_location, new_current_line) =
                    self.upsert_entry_if_changed(log_index, &log, &lines, current_line);

                current_line = new_current_line;

                // Only create StreamUpdate for Shown entries
                let (line_start, line_count) = match render_location? {
                    RenderedLocation::Shown {
                        line_start,
                        line_count,
                    } => (line_start, line_count),
                    RenderedLocation::Hidden => return None,
                };

                Some(StreamUpdate {
                    replace_line_start: line_start,
                    replace_line_end: line_start + line_count,
                    lines,
                    line_hl_group: log.data.line_hl_group(),
                    sign: log.data.sign(),
                    sign_hl_group: log.data.sign_hl_group(),
                })
            })
            .collect();

        // Update check_from_index:
        // - If last rendered entry is assistant, keep it at check_from to allow re-rendering (streaming support)
        // - If last rendered entry is tool, find first tool in the consecutive tool chain
        // - Otherwise, progress to current_count
        let new_check_from = self
            .rendered_entries
            .last()
            .map_or(current_count, |last_entry| match &last_entry.log.data {
                TenonLogData::Assistant(_) => current_count - 1,
                TenonLogData::Tool(_) => (0..current_count - 1)
                    .rev()
                    .take_while(|&i| {
                        self.rendered_entries
                            .get(i)
                            .is_some_and(|e| matches!(e.log.data, TenonLogData::Tool(_)))
                    })
                    .last()
                    .unwrap_or(current_count - 1),
                _ => current_count,
            });
        self.check_from_index
            .store(new_check_from, Ordering::SeqCst);

        (updates, current_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::{
        TenonAssistantMessage, TenonLog, TenonLogData, TenonUserMessage, TenonUserTextMessage,
    };
    use std::sync::Arc;

    fn init_test_cache() -> ChatLogCache {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();
        let session = crate::chat::ChatSession::new();
        ChatLogCache::new(Arc::new(RwLock::new(session)))
    }

    fn add_user_log(cache: &mut ChatLogCache, text: &str) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
                TenonUserTextMessage(text.to_string()),
            )))),
            active: true,
        });
    }

    fn add_assistant_reasoning(cache: &mut ChatLogCache, reasoning: &str) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Assistant(
                TenonAssistantMessage {
                    reasoning: Some(reasoning.to_string()),
                    content: vec![],
                },
            ))),
            active: true,
        });
    }

    fn update_assistant_reasoning(cache: &mut ChatLogCache, index: usize, reasoning: &str) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs[index] = crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Assistant(
                TenonAssistantMessage {
                    reasoning: Some(reasoning.to_string()),
                    content: vec![],
                },
            ))),
            active: true,
        };
    }

    fn update_assistant_content(cache: &mut ChatLogCache, index: usize, content: &str) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs[index] = crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Assistant(
                TenonAssistantMessage {
                    reasoning: None,
                    content: vec![crate::chat::log::TenonAssistantMessageContent::Text(
                        content.to_string(),
                    )],
                },
            ))),
            active: true,
        };
    }

    fn add_tool_log(cache: &mut ChatLogCache, name: &str, id: usize) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Tool(
                crate::chat::log::TenonToolLog {
                    tool_call: crate::chat::log::TenonToolCall {
                        id: id.to_string(),
                        internal_call_id: id.to_string(),
                        name: name.to_string(),
                        args: serde_json::json!({}),
                    },
                    tool_result: None,
                },
            ))),
            active: true,
        });
    }

    #[test]
    fn test_poll_render_update_with_check_from_index() {
        let mut cache = init_test_cache();

        let (updates, current_line) = cache.poll_render_update();
        assert!(
            updates.is_empty(),
            "should return empty Vec when no new logs"
        );
        assert_eq!(current_line, 0, "current_line should be 0 when no logs");

        add_user_log(&mut cache, "Hello");

        let (updates, current_line) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            current_line, 2,
            "current_line should be 2 after 'Hello' log (2 lines)"
        );
        assert_eq!(
            updates[0].lines,
            vec!["Hello", ""],
            "should show all lines of user log with separator"
        );
        assert_eq!(updates[0].sign, " ", "should show User sign");
        assert_eq!(
            updates[0].sign_hl_group, "TenonSignUser",
            "should show User sign hl group"
        );
        assert_eq!(updates[0].line_hl_group, "", "User has no line hl group");
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 0);

        // Call again - should return empty Vec (check_from_index updated, no new logs)
        let (updates, current_line) = cache.poll_render_update();
        assert!(
            updates.is_empty(),
            "should return empty Vec after check_from_index updated"
        );
        assert_eq!(current_line, 0, "current_line should be 0 when no updates");

        add_user_log(&mut cache, "World");
        add_user_log(&mut cache, "Test");

        let (updates, current_line) = cache.poll_render_update();
        assert_eq!(updates.len(), 2);
        assert_eq!(
            current_line, 6,
            "current_line should be 6 after 3 logs (2 lines each)"
        );
        assert_eq!(
            updates[0].lines,
            vec!["World", ""],
            "first log lines should be ['World', '']"
        );
        assert_eq!(
            updates[1].lines,
            vec!["Test", ""],
            "second log lines should be ['Test', '']"
        );
        assert_eq!(updates[0].replace_line_start, 2);
        assert_eq!(updates[0].replace_line_end, 2);
        assert_eq!(updates[1].replace_line_start, 4);
        assert_eq!(updates[1].replace_line_end, 4);

        assert!(
            cache.poll_render_update().0.is_empty(),
            "should return empty Vec after all logs consumed"
        );
    }

    #[test]
    fn test_check_from_index_stops_before_assistant_log() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "tool1", 1);
        add_assistant_reasoning(&mut cache, "Thinking...");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 3);
        assert_eq!(updates[2].lines, vec!["Thinking...", ""]);
        assert_eq!(updates[2].replace_line_start, 4);
        assert_eq!(updates[2].replace_line_end, 4);

        update_assistant_reasoning(&mut cache, 2, "Thinking...\nMore thoughts");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Thinking...", "More thoughts", ""]);
        assert_eq!(updates[0].replace_line_start, 4);
        assert_eq!(updates[0].replace_line_end, 6, "should use stored line_end");

        add_user_log(&mut cache, "Hello");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(
            updates.len(),
            2,
            "assistant reasoning should re-render with limited lines when has_next, plus user log"
        );
        // Assistant re-rendered with limited lines
        assert_eq!(updates[0].lines, vec!["... More thoughts", ""]);
        assert_eq!(updates[0].replace_line_start, 4);
        assert_eq!(
            updates[0].replace_line_end, 7,
            "should replace old 3-line assistant"
        );
        // User log
        assert_eq!(updates[1].lines, vec!["Hello", ""]);
        assert_eq!(
            updates[1].replace_line_start, 6,
            "user log starts after assistant (2 lines)"
        );
        assert_eq!(updates[1].replace_line_end, 6);

        let (updates, _) = cache.poll_render_update();
        assert!(
            updates.is_empty(),
            "check_from_index should progress when last log is not assistant"
        );
    }

    #[test]
    fn test_check_from_index_for_tool_chain() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "tool1", 1);
        add_tool_log(&mut cache, "tool2", 2);

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 3);

        // User log with separator
        assert_eq!(updates[0].lines, vec!["Hello", ""]);
        // Tool → Tool: no separator between consecutive tools
        assert!(updates[1].lines[0].contains("tool1"));
        assert_eq!(
            updates[1].lines.len(),
            1,
            "tool1 should have no separator when next is tool"
        );
        // Last tool has separator
        assert!(updates[2].lines[0].contains("tool2"));
        assert_eq!(updates[2].lines.len(), 2, "last tool should have separator");
        assert_eq!(updates[2].lines[1], "");

        assert_eq!(cache.check_from_index.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_shrinking_log_updates_positions() {
        let mut cache = init_test_cache();

        add_assistant_reasoning(&mut cache, "Thinking...\nMore thoughts");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Thinking...", "More thoughts", ""]);
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 0);

        update_assistant_content(&mut cache, 0, "Final answer");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Final answer", ""]);
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 3, "should use stored line_end");

        add_user_log(&mut cache, "Hello");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(
            updates.len(),
            1,
            "should skip unchanged assistant log, return only user log"
        );
        assert_eq!(updates[0].lines, vec!["Hello", ""]);
        assert_eq!(
            updates[0].replace_line_start, 2,
            "user log starts after assistant (1 line + separator)"
        );
        assert_eq!(updates[0].replace_line_end, 2);
    }

    #[test]
    fn test_poll_render_update_multiline_log() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Line 1\nLine 2\nLine 3");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0].lines,
            vec!["Line 1", "Line 2", "Line 3", ""],
            "should capture all lines from multi-line log"
        );
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 0);

        add_user_log(&mut cache, "Second log");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Second log", ""]);
        assert_eq!(updates[0].replace_line_start, 4);
    }

    #[test]
    fn test_rendered_entries_updates_on_log_change() {
        let mut cache = init_test_cache();

        add_assistant_reasoning(&mut cache, "Thinking...");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Thinking...", ""]);

        let (updates, _) = cache.poll_render_update();
        assert!(
            updates.is_empty(),
            "should skip render when log not updated"
        );

        update_assistant_reasoning(&mut cache, 0, "Thinking...\nMore thoughts");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Thinking...", "More thoughts", ""]);

        let (updates, _) = cache.poll_render_update();
        assert!(
            updates.is_empty(),
            "should skip render after re-render when log not updated again"
        );
    }

    #[test]
    fn test_process_log_lines_tool_separator() {
        use crate::chat::log::{
            TenonLogData, TenonToolCall, TenonToolLog, TenonUserMessage, TenonUserTextMessage,
        };

        let user_log = TenonLogData::User(TenonUserMessage::Text(TenonUserTextMessage(
            "Hello".to_string(),
        )));
        let tool1_log = TenonLogData::Tool(TenonToolLog {
            tool_call: TenonToolCall {
                id: "1".into(),
                internal_call_id: "1".into(),
                name: "tool1".into(),
                args: serde_json::json!({}),
            },
            tool_result: None,
        });
        let tool2_log = TenonLogData::Tool(TenonToolLog {
            tool_call: TenonToolCall {
                id: "2".into(),
                internal_call_id: "2".into(),
                name: "tool2".into(),
                args: serde_json::json!({}),
            },
            tool_result: None,
        });

        // Tool → Tool: no separator between consecutive tools
        let lines = process_log_lines(&tool1_log, Some(&tool2_log));
        assert!(lines[0].contains("tool1"));
        assert_eq!(
            lines.len(),
            1,
            "tool1 should have no trailing empty line when next is tool"
        );

        // Tool → User: has separator
        let lines = process_log_lines(&tool2_log, Some(&user_log));
        assert!(lines[0].contains("tool2"));
        assert_eq!(
            lines.len(),
            2,
            "tool2 should have separator when next is user"
        );
        assert_eq!(lines[1], "");

        // User → Tool: has separator
        let lines = process_log_lines(&user_log, Some(&tool1_log));
        assert_eq!(lines, vec!["Hello", ""]);

        // Tool as last: has separator
        let lines = process_log_lines(&tool1_log, None);
        assert!(lines[0].contains("tool1"));
        assert_eq!(lines.len(), 2, "last tool should have separator");
    }

    #[test]
    fn test_process_log_lines_assistant_reasoning() {
        use crate::chat::log::{
            TenonAssistantMessage, TenonLogData, TenonUserMessage, TenonUserTextMessage,
        };

        let reasoning_log = TenonLogData::Assistant(TenonAssistantMessage {
            reasoning: Some("Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_string()),
            content: vec![],
        });

        // When last: show last 3 lines
        let lines = process_log_lines(&reasoning_log, None);
        assert_eq!(lines, vec!["... Line 3", "Line 4", "Line 5", ""]);

        // When has next: show last 1 line
        let user_log = TenonLogData::User(TenonUserMessage::Text(TenonUserTextMessage(
            "Hello".to_string(),
        )));
        let lines = process_log_lines(&reasoning_log, Some(&user_log));
        assert_eq!(lines, vec!["... Line 5", ""]);
    }

    #[test]
    fn test_system_tools_excluded_from_render() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "start_workflow", 1);
        add_tool_log(&mut cache, "read_file", 2);

        let (updates, _) = cache.poll_render_update();

        assert_eq!(
            updates.len(),
            2,
            "System tools should be excluded from render"
        );
        assert_eq!(updates[0].lines, vec!["Hello", ""]);
        assert!(updates[1].lines[0].contains("read_file"));
    }

    #[test]
    fn test_process_log_lines_uses_next_visible_log() {
        use crate::chat::log::{TenonAssistantMessage, TenonLogData, TenonToolCall, TenonToolLog};

        // Assistant reasoning followed by system tool only → should treat as last (show 3 lines)
        let reasoning_log = TenonLogData::Assistant(TenonAssistantMessage {
            reasoning: Some("Line 1\nLine 2\nLine 3\nLine 4\nLine 5".to_string()),
            content: vec![],
        });
        let system_tool_log = TenonLogData::Tool(TenonToolLog {
            tool_call: TenonToolCall {
                id: "1".into(),
                internal_call_id: "1".into(),
                name: "start_workflow".into(),
                args: serde_json::json!({}),
            },
            tool_result: None,
        });

        // Current: next_log is system tool → shows 1 line (has_next = true)
        // Desired: next_log is None (system tool hidden) → shows 3 lines (last visible)
        let lines = process_log_lines(&reasoning_log, Some(&system_tool_log));
        assert_eq!(
            lines,
            vec!["... Line 3", "Line 4", "Line 5", ""],
            "Assistant reasoning followed by system tool should be treated as last visible log"
        );
    }

    #[test]
    fn test_tool_chain_line_break_updates_on_log_replacement() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "tool1", 1);
        add_tool_log(&mut cache, "tool2", 2);
        add_tool_log(&mut cache, "tool3", 3);

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 4);

        assert_eq!(updates[0].lines, vec!["Hello", ""]);
        assert!(updates[1].lines[0].contains("tool1"));
        assert!(updates[2].lines[0].contains("tool2"));
        assert!(updates[3].lines[0].contains("tool3"));
        assert_eq!(updates[3].lines[1], "");

        cache.poll_render_update();

        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs.remove(3); // Remove tool3
        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
                TenonUserTextMessage("World".to_string()),
            )))),
            active: true,
        });
        drop(log_window);
        drop(session);

        assert_eq!(cache.check_from_index.load(Ordering::SeqCst), 1);

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 2);

        // Tool2: now has separator (became last tool)
        assert!(updates[0].lines[0].contains("tool2"));
        assert_eq!(updates[0].lines.len(), 2);
        assert_eq!(updates[0].lines[1], "");
        assert_eq!(updates[0].replace_line_start, 3);
        assert_eq!(updates[0].replace_line_end, 4);

        // User log: ["World", ""]
        assert_eq!(updates[1].lines, vec!["World", ""]);
        assert_eq!(updates[1].replace_line_start, 5);
        assert_eq!(updates[1].replace_line_end, 6);
    }

    #[test]
    fn test_get_log_at_line() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_assistant_reasoning(&mut cache, "Thinking...");
        add_tool_log(&mut cache, "tool1", 1);

        cache.poll_render_update();

        // User log occupies lines [0, 2)
        assert!(
            matches!(cache.get_log_at_line(0), Some(log) if matches!(log.data, TenonLogData::User(_)))
        );
        assert!(
            matches!(cache.get_log_at_line(1), Some(log) if matches!(log.data, TenonLogData::User(_)))
        );

        // Assistant log occupies lines [2, 4)
        assert!(
            matches!(cache.get_log_at_line(2), Some(log) if matches!(log.data, TenonLogData::Assistant(_)))
        );
        assert!(
            matches!(cache.get_log_at_line(3), Some(log) if matches!(log.data, TenonLogData::Assistant(_)))
        );

        // Tool log occupies lines [4, 6)
        assert!(
            matches!(cache.get_log_at_line(4), Some(log) if matches!(log.data, TenonLogData::Tool(_)))
        );
        assert!(
            matches!(cache.get_log_at_line(5), Some(log) if matches!(log.data, TenonLogData::Tool(_)))
        );

        // Out of range
        assert!(cache.get_log_at_line(100).is_none());
    }

    fn add_system_tool_log_with_error(cache: &mut ChatLogCache, name: &str, id: usize) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Tool(
                crate::chat::log::TenonToolLog {
                    tool_call: crate::chat::log::TenonToolCall {
                        id: id.to_string(),
                        internal_call_id: id.to_string(),
                        name: name.to_string(),
                        args: serde_json::json!({}),
                    },
                    tool_result: Some(Err(crate::chat::log::TenonToolError(
                        "Toolset error: something went wrong".into(),
                    ))),
                },
            ))),
            active: true,
        });
    }

    fn add_system_tool_log_with_ok(cache: &mut ChatLogCache, name: &str, id: usize) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Tool(
                crate::chat::log::TenonToolLog {
                    tool_call: crate::chat::log::TenonToolCall {
                        id: id.to_string(),
                        internal_call_id: id.to_string(),
                        name: name.to_string(),
                        args: serde_json::json!({}),
                    },
                    tool_result: Some(Ok(crate::chat::log::TenonToolResult::Text(
                        rig::agent::Text { text: "ok".into() },
                    ))),
                },
            ))),
            active: true,
        });
    }

    fn update_tool_log_to_error(cache: &mut ChatLogCache, index: usize) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        let log = Arc::make_mut(&mut log_window.logs[index].log);
        log.set_tool_result(Some(Err(crate::chat::log::TenonToolError(
            "Toolset error: something went wrong".into(),
        ))));
    }

    #[test]
    fn test_system_tool_error_visible() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_system_tool_log_with_error(&mut cache, "start_workflow", 1);

        let (updates, _) = cache.poll_render_update();

        assert_eq!(updates.len(), 2, "System tool with error should be visible");
        assert_eq!(updates[0].lines, vec!["Hello", ""]);
        assert!(
            updates[1].lines[0].contains("start_workflow"),
            "errored system tool should be rendered"
        );
    }

    #[test]
    fn test_system_tool_ok_hidden() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_system_tool_log_with_ok(&mut cache, "start_workflow", 1);

        let (updates, _) = cache.poll_render_update();

        assert_eq!(
            updates.len(),
            1,
            "System tool with Ok result should be hidden"
        );
        assert_eq!(updates[0].lines, vec!["Hello", ""]);
    }

    #[test]
    fn test_system_tool_pending_hidden() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "start_workflow", 1);

        let (updates, _) = cache.poll_render_update();

        assert_eq!(
            updates.len(),
            1,
            "System tool with pending result should be hidden"
        );
        assert_eq!(updates[0].lines, vec!["Hello", ""]);
    }

    #[test]
    fn test_system_tool_pending_to_error_transition() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "start_workflow", 1);

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1, "pending system tool should be hidden");

        update_tool_log_to_error(&mut cache, 1);

        let (updates, _) = cache.poll_render_update();
        assert_eq!(
            updates.len(),
            1,
            "errored system tool should become visible"
        );
        assert!(
            updates[0].lines[0].contains("start_workflow"),
            "transitioned system tool should be rendered"
        );
    }

    #[test]
    fn test_get_log_at_line_skips_hidden_system_tools() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "start_workflow", 1);

        cache.poll_render_update();

        // System tool is Hidden; user log still occupies lines [0, 2)
        assert!(
            matches!(cache.get_log_at_line(0), Some(log) if matches!(log.data, TenonLogData::User(_)))
        );
        assert!(
            matches!(cache.get_log_at_line(1), Some(log) if matches!(log.data, TenonLogData::User(_)))
        );
    }
}
