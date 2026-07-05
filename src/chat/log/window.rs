use std::sync::Arc;

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
            .map(|indexed| indexed.log.token_count())
            .sum()
    }

    /// Returns active messages that will be sent to LLM as chat context.
    /// Active logs are those with active=true.
    /// Excludes the last item if it's a user message (the current prompt is
    /// passed separately to the LLM, not as part of history).
    pub fn active_history_log(&self) -> Vec<Arc<TenonLog>> {
        let active: Vec<Arc<TenonLog>> = self
            .logs
            .iter()
            .filter(|indexed| indexed.active)
            .map(|indexed| indexed.log.clone())
            .collect();
        let len = active.len();
        if len > 0 && matches!(active[len - 1].data(), TenonLogData::User(_)) {
            active[..len - 1].to_vec()
        } else {
            active
        }
    }

    /// Returns all logs, excluding the last item if it's a user message.
    pub fn history_log(&self) -> Vec<Arc<TenonLog>> {
        let len = self.logs.len();
        let skip_last = len > 0 && matches!(self.logs[len - 1].log.data(), TenonLogData::User(_));
        self.logs
            .iter()
            .take(if skip_last { len - 1 } else { len })
            .map(|indexed| indexed.log.clone())
            .collect()
    }

    /// Returns inactive logs that will go through RAG filter.
    /// These are logs that have been excluded from active context (active=false).
    pub fn inactive_log(&self) -> Vec<Arc<TenonLog>> {
        self.logs
            .iter()
            .filter(|indexed| !indexed.active)
            .map(|indexed| indexed.log.clone())
            .collect()
    }

    /// Finds the index of the first user message in the entire log.
    pub fn find_first_user_index(&self) -> Option<usize> {
        self.logs
            .iter()
            .position(|indexed| matches!(&indexed.log.data(), TenonLogData::User(_)))
    }

    /// Determines if the log at the given index is "in workflow".
    pub fn is_log_in_workflow(&self, log_idx: usize) -> bool {
        self.logs[..log_idx]
            .iter()
            .rev()
            .find(|indexed| matches!(indexed.log.data(), TenonLogData::Workflow(_)))
            .map(|indexed| match indexed.log.data() {
                TenonLogData::Workflow(wf) => wf.step.is_some(),
                _ => false,
            })
            .unwrap_or(false)
    }

    /// Prunes trailing incomplete tool calls (those without results) from the logs
    /// to prevent sending broken history to the LLM.
    pub fn prune_incomplete_messages(&mut self) {
        let logs = &self.logs;
        let last_non_tool_index = logs
            .iter()
            .enumerate()
            .rfind(|(_, log)| !matches!(log.log.data(), TenonLogData::Tool(_)));

        if let Some((index, _)) = last_non_tool_index {
            let mut new_logs = Vec::with_capacity(logs.len());
            new_logs.extend_from_slice(&logs[..=index]);

            for log in &logs[index + 1..] {
                if let TenonLogData::Tool(tool_log) = log.log.data()
                    && tool_log.tool_result.is_some()
                {
                    new_logs.push(log.clone());
                }
            }
            self.logs = new_logs;
        } else {
            // If all messages are tools, we only keep the ones with results
            self.logs = logs
                .iter()
                .filter(|log| {
                    if let TenonLogData::Tool(tool_log) = log.log.data() {
                        tool_log.tool_result.is_some()
                    } else {
                        true
                    }
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
        if self.is_log_in_workflow(last_idx) {
            log_to_search
                .iter()
                .rposition(|indexed| match indexed.data() {
                    TenonLogData::Workflow(wf) => wf.step.is_some(),
                    _ => false,
                })
        } else {
            log_to_search
                .iter()
                .rposition(|indexed| matches!(indexed.data(), TenonLogData::User(_)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{
        TenonAssistantMessage, TenonAssistantMessageContent, TenonWorkflowLog,
        log::{
            TenonLog, TenonLogData, TenonToolCall, TenonToolLog, TenonToolResult, TenonUserMessage,
            TenonUserTextMessage,
        },
    };

    fn create_user_log(token_count: usize) -> TenonLog {
        let mut log = TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            TenonUserTextMessage("x".repeat(token_count)),
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

    fn create_workflow_log(id: &str, step: Option<usize>) -> TenonLog {
        TenonLog::new(TenonLogData::Workflow(TenonWorkflowLog::new(
            id,
            if step.is_some() {
                "workflow step"
            } else {
                "Workflow ended"
            },
            step,
        )))
    }

    fn create_log_window(logs: Vec<TenonLog>) -> LogWindow {
        LogWindow {
            logs: logs
                .into_iter()
                .map(|log| IndexedLog {
                    log: Arc::new(log),
                    active: true,
                })
                .collect(),
        }
    }

    #[test]
    fn test_is_log_in_workflow() {
        let logs = vec![
            create_user_log(1),                 // 0: no workflow before
            create_workflow_log("wf", Some(1)), // 1: workflow start
            create_user_log(1),                 // 2: in workflow
            create_workflow_log("wf", None),    // 3: workflow end
            create_user_log(1),                 // 4: after workflow ended
        ];
        let log_window = create_log_window(logs);
        assert!(!log_window.is_log_in_workflow(0)); // before workflow
        assert!(log_window.is_log_in_workflow(2)); // in workflow
        assert!(!log_window.is_log_in_workflow(4)); // after workflow ended
    }

    #[test]
    fn test_find_last_checkpoint_in_workflow_uses_workflow_tool() {
        let logs = vec![
            create_user_log(1),
            create_workflow_log("test_workflow", Some(1)), // start
            create_user_log(1),
            create_workflow_log("test_workflow", Some(2)), // navigate to step 2
            create_user_log(1),
        ];
        let log_window = create_log_window(logs);
        assert_eq!(log_window.find_last_checkpoint(None), Some(3));
    }

    #[test]
    fn test_find_last_checkpoint_not_in_workflow_uses_user_message() {
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
    fn test_find_last_checkpoint_after_workflow_ends() {
        let logs = vec![
            create_workflow_log("test_workflow", Some(1)),
            create_user_log(1),
            create_workflow_log("test_workflow", None), // end
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
            name: name.into(),
            args: serde_json::json!({}),
        };
        let tool_result = if has_result {
            Some(Ok(TenonToolResult::Text(rig::agent::Text {
                text: "ok".into(),
            })))
        } else {
            None
        };
        IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::Tool(TenonToolLog {
                tool_call,
                tool_result,
            }))),
            active: true,
        }
    }

    fn create_user_indexed_log(text: &str) -> IndexedLog {
        IndexedLog {
            log: Arc::new(TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
                TenonUserTextMessage(text.to_string()),
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
        assert!(matches!(history[0].data(), TenonLogData::User(_)));
        assert!(matches!(history[1].data(), TenonLogData::Assistant(_)));

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
        assert!(matches!(history[0].data(), TenonLogData::User(_)));
        assert!(matches!(history[1].data(), TenonLogData::Assistant(_)));

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
        assert!(matches!(history[0].data(), TenonLogData::User(_)));
        assert!(matches!(history[1].data(), TenonLogData::Assistant(_)));

        // Last item is not user → all active included
        let log_window = create_log_window(vec![create_user_log(1), create_assistant_log(1)]);
        let history = log_window.active_history_log();
        assert_eq!(history.len(), 2);
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
            log_window.logs[0].log.data(),
            TenonLogData::User(_)
        ));
        assert!(matches!(
            log_window.logs[1].log.data(),
            TenonLogData::Tool(_)
        ));
        if let TenonLogData::Tool(tl) = &log_window.logs[1].log.data() {
            assert!(tl.tool_result.is_some());
        }
    }
}
