use std::sync::{Arc, PoisonError, RwLock};

use super::indexer::IndexedLog;
use super::{TenonLog, TenonLogData};

#[derive(Clone)]
pub struct LogWindow {
    pub logs: Vec<IndexedLog>,
}

impl LogWindow {
    /// Returns the total token count of active chat logs.
    pub fn active_context_token_count(&self) -> usize {
        self.logs
            .iter()
            .filter(|indexed| indexed.active)
            .map(|indexed| {
                indexed
                    .log
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .token_count()
            })
            .sum()
    }

    /// Returns active messages that will be sent to LLM as chat context.
    /// Active logs are those with active=true.
    /// Excludes the last item if it's a user message (the current prompt is
    /// passed separately to the LLM, not as part of history).
    pub fn active_history_log(&self) -> Vec<Arc<RwLock<TenonLog>>> {
        let active: Vec<Arc<RwLock<TenonLog>>> = self
            .logs
            .iter()
            .filter(|indexed| indexed.active)
            .map(|indexed| indexed.log.clone())
            .collect();
        let len = active.len();
        let last_is_user = len > 0
            && active[len - 1]
                .read()
                .map_or(true, |log| matches!(log.data(), TenonLogData::User(_)));
        if last_is_user {
            active[..len - 1].to_vec()
        } else {
            active
        }
    }

    /// Returns all logs, excluding the last item if it's a user message.
    pub fn history_log(&self) -> Vec<Arc<RwLock<TenonLog>>> {
        let len = self.logs.len();
        let skip_last = len > 0
            && self.logs[len - 1]
                .log
                .read()
                // On read failure, default to skipping the last item,
                // consistent with `active_history_log`.
                .map_or(true, |log| matches!(log.data(), TenonLogData::User(_)));
        self.logs
            .iter()
            .take(if skip_last { len - 1 } else { len })
            .map(|indexed| indexed.log.clone())
            .collect()
    }

    /// Returns inactive logs that will go through RAG filter.
    /// These are logs that have been excluded from active context (active=false).
    pub fn inactive_log(&self) -> Vec<Arc<RwLock<TenonLog>>> {
        self.logs
            .iter()
            .filter(|indexed| !indexed.active)
            .map(|indexed| indexed.log.clone())
            .collect()
    }

    /// Finds the index of the first user message in the entire log.
    pub fn find_first_user_index(&self) -> Option<usize> {
        self.logs.iter().position(|indexed| {
            indexed
                .log
                .read()
                .ok()
                .is_some_and(|log| matches!(log.data(), TenonLogData::User(_)))
        })
    }

    /// Determines if the log at the given index is "in choreo".
    pub fn is_log_in_choreo(&self, log_idx: usize) -> bool {
        self.logs[..log_idx]
            .iter()
            .rev()
            .filter_map(|indexed| {
                let log = indexed.log.read().ok()?;
                match log.data() {
                    TenonLogData::Choreo(choreo_log) => Some(choreo_log.r#move.is_some()),
                    _ => None,
                }
            })
            .next()
            .unwrap_or(false)
    }

    /// Prunes trailing incomplete tool calls (those without results) from the logs
    /// to prevent sending broken history to the LLM.
    pub fn prune_incomplete_messages(&mut self) {
        let logs = &self.logs;
        let last_non_tool_index = logs.iter().enumerate().rfind(|(_, log)| {
            !log.log
                .read()
                .map_or(true, |l| matches!(l.data(), TenonLogData::Tool(_)))
        });

        if let Some((index, _)) = last_non_tool_index {
            let mut new_logs = Vec::with_capacity(logs.len());
            new_logs.extend_from_slice(&logs[..=index]);

            for log in &logs[index + 1..] {
                let keep = log.log.read().ok().is_some_and(|l| match l.data() {
                    TenonLogData::Tool(tool_log) => tool_log.tool_result.is_some(),
                    _ => false,
                });
                if keep {
                    new_logs.push(log.clone());
                }
            }
            self.logs = new_logs;
        } else {
            // If all messages are tools, we only keep the ones with results
            self.logs = logs
                .iter()
                .filter(|log| {
                    log.log.read().ok().is_none_or(|l| match l.data() {
                        TenonLogData::Tool(tool_log) => tool_log.tool_result.is_some(),
                        _ => true,
                    })
                })
                .cloned()
                .collect();
        }
    }

    /// Finds the last checkpoint index in the log.
    /// Uses history_log() so the last user message is excluded from the search
    /// (it's the current prompt, not part of history to search for checkpoints).
    pub fn find_last_checkpoint(&self, before: Option<usize>) -> Option<usize> {
        let logs = self.history_log();
        let end = before.unwrap_or(logs.len());
        if end == 0 {
            return None;
        }

        let last_idx = end - 1;
        let log_to_search = &logs[..end];
        if self.is_log_in_choreo(last_idx) {
            log_to_search.iter().rposition(|indexed| {
                indexed.read().ok().is_some_and(|log| match log.data() {
                    TenonLogData::Choreo(choreo_log) => choreo_log.r#move.is_some(),
                    _ => false,
                })
            })
        } else {
            log_to_search.iter().rposition(|indexed| {
                indexed
                    .read()
                    .ok()
                    .is_some_and(|log| matches!(log.data(), TenonLogData::User(_)))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{
        TenonAssistantMessage, TenonAssistantMessageContent, TenonChoreoLog,
        log::{
            TenonImageLog, TenonLog, TenonLogData, TenonToolCall, TenonToolLog, TenonToolResult,
            TenonUserMessage,
        },
    };

    fn image_tool_log(filepath: &str, tool_result: TenonToolResult) -> TenonToolLog {
        TenonToolLog {
            tool_call: TenonToolCall {
                id: "call-1".to_string(),
                internal_call_id: "call-1".to_string(),
                item_id: None,
                name: "load_image".to_string(),
                args: serde_json::json!({ "filepath": filepath }),
            },
            // convert_log only runs after set_tool_result, so an image log
            // always carries a result
            tool_result: Some(Ok(tool_result)),
            progress: vec![],
        }
    }

    fn create_user_log(token_count: usize) -> TenonLog {
        let mut log = TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            "x".repeat(token_count),
        )));
        log.token_count = token_count;
        log
    }

    fn create_assistant_log(token_count: usize) -> TenonLog {
        let mut log = TenonLog::new(TenonLogData::Assistant(TenonAssistantMessage {
            content: vec![TenonAssistantMessageContent::Text("x".repeat(token_count))],
            reasoning: None,
        }));
        log.token_count = token_count;
        log
    }

    fn create_choreo_log(id: &str, move_number: Option<usize>) -> TenonLog {
        TenonLog::new(TenonLogData::Choreo(TenonChoreoLog::new(
            id,
            if move_number.is_some() {
                "choreo move"
            } else {
                "Choreo ended"
            },
            move_number,
            TenonToolLog {
                tool_call: TenonToolCall {
                    id: "test-id".to_string(),
                    internal_call_id: "test-internal-id".to_string(),
                    item_id: None,
                    name: "navigate_choreo".to_string(),
                    args: serde_json::json!({}),
                },
                tool_result: None,
                progress: vec![],
            },
        )))
    }

    fn create_log_window(logs: Vec<TenonLog>) -> LogWindow {
        LogWindow {
            logs: logs
                .into_iter()
                .map(|log| IndexedLog {
                    log: Arc::new(RwLock::new(log)),
                    active: true,
                })
                .collect(),
        }
    }

    #[test]
    fn test_is_log_in_choreo() {
        let logs = vec![
            create_user_log(1),               // 0: no choreo before
            create_choreo_log("wf", Some(1)), // 1: choreo start
            create_user_log(1),               // 2: in choreo
            create_choreo_log("wf", None),    // 3: choreo end
            create_user_log(1),               // 4: after choreo ended
        ];
        let log_window = create_log_window(logs);
        assert!(!log_window.is_log_in_choreo(0)); // before choreo
        assert!(log_window.is_log_in_choreo(2)); // in choreo
        assert!(!log_window.is_log_in_choreo(4)); // after choreo ended
    }

    #[test]
    fn test_find_last_checkpoint_in_choreo_uses_choreo_tool() {
        let logs = vec![
            create_user_log(1),
            create_choreo_log("test_choreo", Some(1)), // start
            create_user_log(1),
            create_choreo_log("test_choreo", Some(2)), // navigate to move 2
            create_user_log(1),
        ];
        let log_window = create_log_window(logs);
        assert_eq!(log_window.find_last_checkpoint(None), Some(3));
    }

    #[test]
    fn test_find_last_checkpoint_not_in_choreo_uses_user_message() {
        use crate::chat::{TenonAssistantMessage, TenonAssistantMessageContent};

        fn create_assistant_log(token_count: usize) -> TenonLog {
            let mut log = TenonLog::new(TenonLogData::Assistant(TenonAssistantMessage {
                content: vec![TenonAssistantMessageContent::Text("x".repeat(token_count))],
                reasoning: None,
            }));
            log.token_count = token_count;
            log
        }

        let logs = vec![
            create_user_log(1),
            create_assistant_log(1),
            create_user_log(1),
            create_assistant_log(1),
        ];
        let log_window = create_log_window(logs);
        assert_eq!(log_window.find_last_checkpoint(None), Some(2));
    }

    #[test]
    fn test_find_last_checkpoint_after_choreo_ends() {
        let logs = vec![
            create_choreo_log("test_choreo", Some(1)),
            create_user_log(1),
            create_choreo_log("test_choreo", None), // end
            create_user_log(1),
        ];
        let log_window = create_log_window(logs);
        assert_eq!(log_window.find_last_checkpoint(None), Some(0));
    }

    #[test]
    fn test_find_last_checkpoint_with_before() {
        let logs = vec![
            create_user_log(1), // 0
            create_user_log(1), // 1
            create_user_log(1), // 2
            create_user_log(1), // 3
        ];
        let log_window = create_log_window(logs);
        assert_eq!(log_window.find_last_checkpoint(None), Some(2));
        assert_eq!(log_window.find_last_checkpoint(Some(3)), Some(2));
    }

    fn create_tool_indexed_log(name: &str, has_result: bool) -> IndexedLog {
        let tool_call = TenonToolCall {
            id: "1".into(),
            internal_call_id: "1".into(),
            item_id: None,
            name: name.into(),
            args: serde_json::json!({}),
        };
        let tool_result = if has_result {
            Some(Ok(TenonToolResult::Text(rig::agent::Text {
                text: "ok".into(),
                ..Default::default()
            })))
        } else {
            None
        };
        IndexedLog {
            log: Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
                TenonToolLog {
                    tool_call,
                    tool_result,
                    progress: vec![],
                },
            )))),
            active: true,
        }
    }

    fn create_user_indexed_log(text: &str) -> IndexedLog {
        IndexedLog {
            log: Arc::new(RwLock::new(TenonLog::new(TenonLogData::User(
                TenonUserMessage::Text(text.to_string()),
            )))),
            active: true,
        }
    }

    #[test]
    fn test_history_log_excludes_last_user_message() {
        let logs = vec![
            create_user_log(1),
            create_assistant_log(1),
            create_user_log(1),
        ];
        let log_window = create_log_window(logs);

        // Last item is user → excluded
        let history = log_window.history_log();
        assert_eq!(history.len(), 2);
        assert!(matches!(
            history[0].read().unwrap().data(),
            TenonLogData::User(_)
        ));
        assert!(matches!(
            history[1].read().unwrap().data(),
            TenonLogData::Assistant(_)
        ));

        // Last item is not user → all included
        let logs = vec![create_user_log(1), create_assistant_log(1)];
        let log_window = create_log_window(logs);
        let history = log_window.history_log();
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_active_history_log_excludes_last_user_message() {
        let logs = vec![
            create_user_log(1),
            create_assistant_log(1),
            create_user_log(1),
        ];
        let log_window = create_log_window(logs);

        // Last active item is user → excluded
        let history = log_window.active_history_log();
        assert_eq!(history.len(), 2);
        assert!(matches!(
            history[0].read().unwrap().data(),
            TenonLogData::User(_)
        ));
        assert!(matches!(
            history[1].read().unwrap().data(),
            TenonLogData::Assistant(_)
        ));

        // With inactive logs: only active items, excluding last user
        let mut log_window = create_log_window(vec![
            create_user_log(1),      // 0 - inactive
            create_assistant_log(1), // 1 - inactive
            create_user_log(1),      // 2 - active
            create_assistant_log(1), // 3 - active
            create_user_log(1),      // 4 - active (last, excluded)
        ]);
        log_window.logs[0].active = false;
        log_window.logs[1].active = false;

        let history = log_window.active_history_log();
        assert_eq!(history.len(), 2);
        assert!(matches!(
            history[0].read().unwrap().data(),
            TenonLogData::User(_)
        ));
        assert!(matches!(
            history[1].read().unwrap().data(),
            TenonLogData::Assistant(_)
        ));

        // Last item is not user → all active included
        let log_window = create_log_window(vec![create_user_log(1), create_assistant_log(1)]);
        let history = log_window.active_history_log();
        assert_eq!(history.len(), 2);
    }

    #[test]
    fn test_active_history_log_includes_last_image_log() {
        // The image log is produced by swapping the last tool log in place, so
        // it is the last log during the continuation request. It must be
        // included in history, otherwise the model never sees the image and
        // calls load_image again.
        let logs = vec![
            create_user_log(1),
            create_assistant_log(1),
            TenonLog::new(TenonLogData::Image(TenonImageLog::Tool(image_tool_log(
                "./img/diagram.png",
                TenonToolResult::Text(rig::agent::Text::default()),
            )))),
        ];
        let log_window = create_log_window(logs);

        let history = log_window.active_history_log();
        assert_eq!(history.len(), 3);
        assert!(matches!(
            history[2].read().unwrap().data(),
            TenonLogData::Image(_)
        ));
    }

    #[test]
    fn test_prune_incomplete_messages() {
        let mut log_window = LogWindow {
            logs: vec![
                create_user_indexed_log("Hello"),
                create_tool_indexed_log("tool1", false), // Incomplete
                create_tool_indexed_log("tool2", true),  // Complete
                create_tool_indexed_log("tool3", false), // Incomplete
            ],
        };

        log_window.prune_incomplete_messages();

        assert_eq!(log_window.logs.len(), 2);
        assert!(matches!(
            log_window.logs[0].log.read().unwrap().data(),
            TenonLogData::User(_)
        ));
        assert!(matches!(
            log_window.logs[1].log.read().unwrap().data(),
            TenonLogData::Tool(_)
        ));
        if let TenonLogData::Tool(tl) = &log_window.logs[1].log.read().unwrap().data() {
            assert!(tl.tool_result.is_some());
        }
    }
}
