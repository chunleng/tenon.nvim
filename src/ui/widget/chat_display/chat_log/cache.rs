use std::sync::{Arc, RwLock};

use chrono::{DateTime, Utc};

use crate::chat::{ChatSession, TenonLog, TenonLogData};
use crate::ui::widget::chat_display::format::DisplayChatFormatter;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderType {
    Normal,
    Tail(usize),
}

pub struct StreamUpdate {
    pub replace_line_start: usize,
    pub replace_line_end: usize,
    pub target_log: Arc<TenonLog>,
    pub render_type: RenderType,
    pub line_separator_after: bool,
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

/// Updates an existing rendered entry or inserts a new one.
/// Returns `(render_location, new_current_line)` where:
/// - `render_location` is `Some(Shown { line_start, line_end })` when entry changed/new and not System tool (needs render)
/// - `render_location` is `Some(Hidden)` when entry is System tool (no render)
/// - `render_location` is `None` when entry unchanged (skip render)
/// - `new_current_line` is always provided for position tracking
fn upsert_entry_if_changed(
    rendered_entries: &mut Vec<RenderedLogEntry>,
    log_index: usize,
    log: &Arc<TenonLog>,
    render_type: RenderType,
    line_separator_after: bool,
    current_line: usize,
) -> (Option<RenderedLocation>, usize) {
    let content_lines = match render_type {
        RenderType::Normal => log.data.lines().len(),
        RenderType::Tail(x) => log.data.lines().len().min(x),
    };
    let total_lines = content_lines + line_separator_after as usize;
    if let Some(existing) = rendered_entries.get_mut(log_index) {
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
                    line_count: total_lines,
                };
                existing.last_updated_at = log.last_updated_at;
                return (
                    Some(RenderedLocation::Shown {
                        line_start: current_line,
                        line_count: 0,
                    }),
                    current_line + total_lines,
                );
            }
        };

        // Check if separator changed (line count differs for Tool logs)
        let separator_changed = line_count != total_lines;

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
            line_count: total_lines,
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
            current_line + total_lines,
        );
    }

    // New entry
    if log.data.is_hidden_system_tool() {
        rendered_entries.push(RenderedLogEntry {
            log: log.clone(),
            render_location: RenderedLocation::Hidden,
            last_updated_at: log.last_updated_at,
        });
        (Some(RenderedLocation::Hidden), current_line)
    } else {
        rendered_entries.push(RenderedLogEntry {
            log: log.clone(),
            render_location: RenderedLocation::Shown {
                line_start: current_line,
                line_count: total_lines,
            },
            last_updated_at: log.last_updated_at,
        });

        (
            Some(RenderedLocation::Shown {
                line_start: current_line,
                line_count: 0, // For new entries, replace from start
            }),
            current_line + total_lines,
        )
    }
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

    pub fn poll_render_update(&mut self) -> (Vec<StreamUpdate>, usize) {
        let check_from = self.check_from_index.load(Ordering::SeqCst);

        // Get the line_end of the nearest preceding Shown entry (or 0 if none)
        let mut current_line = if check_from == 0 {
            0
        } else {
            (0..check_from)
                .rev()
                .find_map(|i| {
                    self.rendered_entries
                        .get(i)
                        .and_then(|entry| match &entry.render_location {
                            RenderedLocation::Shown {
                                line_start,
                                line_count,
                            } => Some(line_start + line_count),
                            RenderedLocation::Hidden => None,
                        })
                })
                .unwrap_or(0)
        };

        if let Ok(chat_session) = self.chat_session.read()
            && let Ok(log_window) = chat_session.log_handler.log_window.read()
        {
            let current_count = log_window.logs.len();

            if check_from >= current_count {
                return (Vec::new(), 0);
            }

            let updates: Vec<StreamUpdate> = log_window.logs[check_from..current_count]
                .iter()
                .enumerate()
                .filter_map(|(offset, indexed_log)| {
                    let next_log = log_window.logs[check_from + offset + 1..]
                        .iter()
                        .find(|n| !n.log.data.is_hidden_system_tool())
                        .map(|n| &n.log.data);

                    let current_log = &indexed_log.log.data;

                    let render_type = match current_log {
                        TenonLogData::Assistant(msg)
                            if msg.content.is_empty() && msg.reasoning.is_some() =>
                        {
                            if next_log.is_some() {
                                RenderType::Tail(1)
                            } else {
                                RenderType::Normal
                            }
                        }
                        TenonLogData::Thought(thought_log) if thought_log.summary.is_none() => {
                            RenderType::Tail(3)
                        }
                        _ => RenderType::Normal,
                    };

                    // Add line separator unless both current and next are Tools
                    let line_separator_after = !(matches!(current_log, TenonLogData::Tool(_))
                        && next_log.is_some_and(|n| matches!(n, TenonLogData::Tool(_))));

                    let log_index = check_from + offset;
                    let log = &indexed_log.log;

                    let (render_location, new_current_line) = upsert_entry_if_changed(
                        &mut self.rendered_entries,
                        log_index,
                        log,
                        render_type,
                        line_separator_after,
                        current_line,
                    );

                    current_line = new_current_line;

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
                        target_log: log.clone(),
                        render_type,
                        line_separator_after,
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
                    TenonLogData::Assistant(_) | TenonLogData::Thought(_) => current_count - 1,
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

            return (updates, current_line);
        }
        return (Vec::new(), 0);
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

    fn add_thought_log(cache: &mut ChatLogCache, thought: &str) {
        let session = cache.chat_session.write().unwrap();
        let mut log_window = session.log_handler.log_window.write().unwrap();
        log_window.logs.push(crate::chat::log::indexer::IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Thought(
                crate::chat::log::TenonThoughtLog {
                    thought: thought.to_string(),
                    summary: None,
                },
            ))),
            active: true,
        });
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
            updates[0].target_log.data.lines(),
            vec!["Hello"],
            "should show all lines of user log"
        );
        assert!(updates[0].line_separator_after);
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
            updates[0].target_log.data.lines(),
            vec!["World"],
            "first log lines should be ['World']"
        );
        assert!(updates[0].line_separator_after);
        assert_eq!(
            updates[1].target_log.data.lines(),
            vec!["Test"],
            "second log lines should be ['Test']"
        );
        assert!(updates[1].line_separator_after);
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
        assert_eq!(updates[2].target_log.data.lines(), vec!["Thinking..."]);
        assert!(updates[2].line_separator_after);
        assert_eq!(updates[2].replace_line_start, 4);
        assert_eq!(updates[2].replace_line_end, 4);

        update_assistant_reasoning(&mut cache, 2, "Thinking...\nMore thoughts");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0].target_log.data.lines(),
            vec!["Thinking...", "More thoughts"]
        );
        assert!(updates[0].line_separator_after);
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
        assert_eq!(
            updates[0].target_log.data.lines(),
            vec!["Thinking...", "More thoughts"]
        );
        assert!(updates[0].line_separator_after);
        assert!(matches!(updates[0].render_type, RenderType::Tail(1)));
        assert_eq!(updates[0].replace_line_start, 4);
        assert_eq!(
            updates[0].replace_line_end, 7,
            "should replace old 3-line assistant"
        );
        // User log
        assert_eq!(updates[1].target_log.data.lines(), vec!["Hello"]);
        assert!(updates[1].line_separator_after);
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
        assert_eq!(updates[0].target_log.data.lines(), vec!["Hello"]);
        assert!(updates[0].line_separator_after);
        // Tool → Tool: no separator between consecutive tools
        assert!(updates[1].target_log.data.lines()[0].contains("tool1"));
        assert_eq!(
            updates[1].target_log.data.lines().len(),
            1,
            "tool1 should have no separator when next is tool"
        );
        assert!(!updates[1].line_separator_after);
        // Last tool has separator
        assert!(updates[2].target_log.data.lines()[0].contains("tool2"));
        assert_eq!(
            updates[2].target_log.data.lines().len(),
            1,
            "last tool should have separator"
        );
        assert!(updates[2].line_separator_after);

        assert_eq!(cache.check_from_index.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_shrinking_log_updates_positions() {
        let mut cache = init_test_cache();

        add_assistant_reasoning(&mut cache, "Thinking...\nMore thoughts");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0].target_log.data.lines(),
            vec!["Thinking...", "More thoughts"]
        );
        assert!(updates[0].line_separator_after);
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 0);

        update_assistant_content(&mut cache, 0, "Final answer");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].target_log.data.lines(), vec!["Final answer"]);
        assert!(updates[0].line_separator_after);
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 3, "should use stored line_end");

        add_user_log(&mut cache, "Hello");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(
            updates.len(),
            1,
            "should skip unchanged assistant log, return only user log"
        );
        assert_eq!(updates[0].target_log.data.lines(), vec!["Hello"]);
        assert!(updates[0].line_separator_after);
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
            updates[0].target_log.data.lines(),
            vec!["Line 1", "Line 2", "Line 3"],
            "should capture all lines from multi-line log"
        );
        assert!(updates[0].line_separator_after);
        assert_eq!(updates[0].replace_line_start, 0);
        assert_eq!(updates[0].replace_line_end, 0);

        add_user_log(&mut cache, "Second log");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].target_log.data.lines(), vec!["Second log"]);
        assert!(updates[0].line_separator_after);
        assert_eq!(updates[0].replace_line_start, 4);
    }

    #[test]
    fn test_rendered_entries_updates_on_log_change() {
        let mut cache = init_test_cache();

        add_assistant_reasoning(&mut cache, "Thinking...");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].target_log.data.lines(), vec!["Thinking..."]);
        assert!(updates[0].line_separator_after);

        let (updates, _) = cache.poll_render_update();
        assert!(
            updates.is_empty(),
            "should skip render when log not updated"
        );

        update_assistant_reasoning(&mut cache, 0, "Thinking...\nMore thoughts");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0].target_log.data.lines(),
            vec!["Thinking...", "More thoughts"]
        );
        assert!(updates[0].line_separator_after);

        let (updates, _) = cache.poll_render_update();
        assert!(
            updates.is_empty(),
            "should skip render after re-render when log not updated again"
        );
    }

    #[test]
    fn test_assistant_reasoning_no_next_log_normal() {
        let mut cache = init_test_cache();

        add_assistant_reasoning(&mut cache, "Line 1\nLine 2\nLine 3\nLine 4\nLine 5");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        // With Normal, all 5 lines count toward content_lines (not truncated to 3)
        assert!(
            matches!(updates[0].render_type, RenderType::Normal),
            "assistant reasoning with no next log should be Normal"
        );
        assert_eq!(
            updates[0].target_log.data.lines().len(),
            5,
            "all 5 lines should be present"
        );
    }

    #[test]
    fn test_thought_without_summary_always_tail_3() {
        // Thought with next log: Tail(3), not Tail(1)
        let mut cache = init_test_cache();
        add_thought_log(&mut cache, "Thought 1\nThought 2\nThought 3\nThought 4");
        add_user_log(&mut cache, "Hello");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 2);
        assert!(
            matches!(updates[0].render_type, RenderType::Tail(3)),
            "thought without summary with next log should be Tail(3)"
        );

        // Thought without next log: Tail(3)
        let mut cache = init_test_cache();
        add_thought_log(&mut cache, "Thought 1\nThought 2\nThought 3\nThought 4");

        let (updates, _) = cache.poll_render_update();
        assert_eq!(updates.len(), 1);
        assert!(
            matches!(updates[0].render_type, RenderType::Tail(3)),
            "thought without summary without next log should be Tail(3)"
        );
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
        assert_eq!(updates[0].target_log.data.lines(), vec!["Hello"]);
        assert!(updates[0].line_separator_after);
        assert!(updates[1].target_log.data.lines()[0].contains("read_file"));
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

        assert_eq!(updates[0].target_log.data.lines(), vec!["Hello"]);
        assert!(updates[0].line_separator_after);
        assert!(updates[1].target_log.data.lines()[0].contains("tool1"));
        assert!(updates[2].target_log.data.lines()[0].contains("tool2"));
        assert!(updates[3].target_log.data.lines()[0].contains("tool3"));
        assert!(updates[3].line_separator_after);

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
        assert!(updates[0].target_log.data.lines()[0].contains("tool2"));
        assert_eq!(updates[0].target_log.data.lines().len(), 1);
        assert!(updates[0].line_separator_after);
        assert_eq!(updates[0].replace_line_start, 3);
        assert_eq!(updates[0].replace_line_end, 4);

        // User log: ["World"]
        assert_eq!(updates[1].target_log.data.lines(), vec!["World"]);
        assert!(updates[1].line_separator_after);
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
                        rig::agent::Text {
                            text: "ok".into(),
                            ..Default::default()
                        },
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
        assert_eq!(updates[0].target_log.data.lines(), vec!["Hello"]);
        assert!(updates[0].line_separator_after);
        assert!(
            updates[1].target_log.data.lines()[0].contains("start_workflow"),
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
        assert_eq!(updates[0].target_log.data.lines(), vec!["Hello"]);
        assert!(updates[0].line_separator_after);
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
        assert_eq!(updates[0].target_log.data.lines(), vec!["Hello"]);
        assert!(updates[0].line_separator_after);
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
            updates[0].target_log.data.lines()[0].contains("start_workflow"),
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
