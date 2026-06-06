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

struct RenderedLogEntry {
    log: Arc<TenonLog>,
    line_start: usize,
    line_end: usize,
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

    /// Updates an existing rendered entry or inserts a new one.
    /// Returns `(render_info, new_current_line)` where:
    /// - `render_info` is `Some((line_start, line_end))` when entry changed/new (needs render)
    /// - `render_info` is `None` when entry unchanged (skip render)
    /// - `new_current_line` is always provided for position tracking
    fn upsert_entry_if_changed(
        &mut self,
        log_index: usize,
        log: &Arc<TenonLog>,
        lines: &[String],
        current_line: usize,
    ) -> (Option<(usize, usize)>, usize) {
        if let Some(existing) = self.rendered_entries.get_mut(log_index) {
            // Check if separator changed (line count differs for Tool logs)
            let separator_changed = existing.line_end - existing.line_start != lines.len();

            // Unchanged entry: return position for tracking, but no render needed
            if log.last_updated_at <= existing.last_updated_at && !separator_changed {
                return (None, existing.line_end);
            }

            // Changed entry: use stored line_end for replace, then update it
            let replace_line_end = existing.line_end;
            existing.line_end = existing.line_start + lines.len();
            existing.last_updated_at = log.last_updated_at;

            return (
                Some((existing.line_start, replace_line_end)),
                existing.line_end,
            );
        }

        // New entry
        let line_start = current_line;
        let line_end = current_line + lines.len();

        self.rendered_entries.push(RenderedLogEntry {
            log: log.clone(),
            line_start,
            line_end,
            last_updated_at: log.last_updated_at,
        });

        (Some((line_start, line_start)), line_end)
    }

    pub fn poll_render_update(&mut self) -> Vec<StreamUpdate> {
        let check_from = self.check_from_index.load(Ordering::SeqCst);

        // Collect new logs while holding the lock
        let current_count = if let Ok(chat_session) = self.chat_session.read()
            && let Ok(indexer) = chat_session.log_indexer.read()
        {
            let current_count = indexer.logs.len();

            // No new logs to render
            if check_from >= current_count {
                return Vec::new();
            }

            current_count
        } else {
            return Vec::new();
        };

        // Get the line_end of the rendered entry at check_from_index (or 0 if none)
        let mut current_line = self
            .rendered_entries
            .get(check_from.saturating_sub(1))
            .map(|entry| entry.line_end)
            .unwrap_or(0);

        // Collect StreamUpdates for new logs
        // First, collect log data while holding the lock
        let logs_data: Vec<(usize, Arc<TenonLog>, Vec<String>)> = {
            let chat_session = self.chat_session.read().unwrap();
            let indexer = chat_session.log_indexer.read().unwrap();

            indexer.logs[check_from..current_count]
                .iter()
                .enumerate()
                .map(|(offset, indexed_log)| {
                    let next_log = indexer
                        .logs
                        .get(check_from + offset + 1)
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
                let (render_info, new_current_line) =
                    self.upsert_entry_if_changed(log_index, &log, &lines, current_line);

                current_line = new_current_line;

                render_info.map(|(replace_line_start, replace_line_end)| StreamUpdate {
                    replace_line_start,
                    replace_line_end,
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

        updates
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
        let mut indexer = session.log_indexer.write().unwrap();
        indexer.logs.push(crate::chat::log_indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
                TenonUserTextMessage(text.to_string()),
            )))),
            active: true,
        });
    }

    fn add_assistant_reasoning(cache: &mut ChatLogCache, reasoning: &str) {
        let session = cache.chat_session.write().unwrap();
        let mut indexer = session.log_indexer.write().unwrap();
        indexer.logs.push(crate::chat::log_indexer::IndexedLog {
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
        let mut indexer = session.log_indexer.write().unwrap();
        indexer.logs[index] = crate::chat::log_indexer::IndexedLog {
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
        let mut indexer = session.log_indexer.write().unwrap();
        indexer.logs[index] = crate::chat::log_indexer::IndexedLog {
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
        let mut indexer = session.log_indexer.write().unwrap();
        indexer.logs.push(crate::chat::log_indexer::IndexedLog {
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

        assert!(
            cache.poll_render_update().is_empty(),
            "should return empty Vec when no new logs"
        );

        add_user_log(&mut cache, "Hello");

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
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
        assert!(
            cache.poll_render_update().is_empty(),
            "should return empty Vec after check_from_index updated"
        );

        add_user_log(&mut cache, "World");
        add_user_log(&mut cache, "Test");

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 2);
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
            cache.poll_render_update().is_empty(),
            "should return empty Vec after all logs consumed"
        );
    }

    #[test]
    fn test_check_from_index_stops_before_assistant_log() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "tool1", 1);
        add_assistant_reasoning(&mut cache, "Thinking...");

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 3);
        assert_eq!(updates[2].lines, vec!["Thinking...", ""]);
        assert_eq!(updates[2].replace_line_start, 4);
        assert_eq!(updates[2].replace_line_end, 4);

        update_assistant_reasoning(&mut cache, 2, "Thinking...\nMore thoughts");

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Thinking...", "More thoughts", ""]);
        assert_eq!(updates[0].replace_line_start, 4);
        assert_eq!(updates[0].replace_line_end, 6, "should use stored line_end");

        add_user_log(&mut cache, "Hello");

        let updates = cache.poll_render_update();
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

        assert!(
            cache.poll_render_update().is_empty(),
            "check_from_index should progress when last log is not assistant"
        );
    }

    #[test]
    fn test_check_from_index_for_tool_chain() {
        let mut cache = init_test_cache();

        add_user_log(&mut cache, "Hello");
        add_tool_log(&mut cache, "tool1", 1);
        add_tool_log(&mut cache, "tool2", 2);

        let updates = cache.poll_render_update();
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

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Thinking...", "More thoughts", ""]);
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 0);

        update_assistant_content(&mut cache, 0, "Final answer");

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Final answer", ""]);
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 3, "should use stored line_end");

        add_user_log(&mut cache, "Hello");

        let updates = cache.poll_render_update();
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

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0].lines,
            vec!["Line 1", "Line 2", "Line 3", ""],
            "should capture all lines from multi-line log"
        );
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 0);

        add_user_log(&mut cache, "Second log");

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Second log", ""]);
        assert_eq!(updates[0].replace_line_start, 4);
    }

    #[test]
    fn test_rendered_entries_updates_on_log_change() {
        let mut cache = init_test_cache();

        add_assistant_reasoning(&mut cache, "Thinking...");

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Thinking...", ""]);

        assert!(
            cache.poll_render_update().is_empty(),
            "should skip render when log not updated"
        );

        update_assistant_reasoning(&mut cache, 0, "Thinking...\nMore thoughts");

        let updates = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].lines, vec!["Thinking...", "More thoughts", ""]);

        assert!(
            cache.poll_render_update().is_empty(),
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
}
