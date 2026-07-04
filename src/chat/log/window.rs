use std::sync::Arc;

use super::indexer::IndexedLog;
use super::{TenonLog, TenonLogData};
use rig::completion::Message;

#[derive(Clone)]
pub struct LogWindow {
    pub logs: Vec<IndexedLog>,
}

impl LogWindow {
    /// Returns active messages that will be sent to LLM as chat context.
    /// Active logs are those with active=true.
    /// Each TenonLog is converted to Vec<Message> (some logs produce multiple messages).
    pub fn active_messages(&self) -> Vec<Message> {
        self.logs
            .iter()
            .filter(|indexed| indexed.active)
            .flat_map(|indexed| Vec::<Message>::from(TenonLog::clone(&indexed.log)))
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

    /// Finds the last checkpoint index in the log.
    pub fn find_last_checkpoint(&self) -> Option<usize> {
        if self.logs.is_empty() {
            return None;
        }

        let last_idx = self.logs.len().saturating_sub(1);
        if self.is_log_in_workflow(last_idx) {
            self.logs
                .iter()
                .rposition(|indexed| match indexed.log.data() {
                    TenonLogData::Workflow(wf) => wf.step.is_some(),
                    _ => false,
                })
        } else {
            self.logs
                .iter()
                .rposition(|indexed| matches!(indexed.log.data(), TenonLogData::User(_)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{
        TenonWorkflowLog,
        log::{TenonLog, TenonLogData, TenonUserMessage, TenonUserTextMessage},
    };

    fn create_user_log(token_count: usize) -> TenonLog {
        let mut log = TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            TenonUserTextMessage("x".repeat(token_count)),
        )));
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
        assert_eq!(log_window.find_last_checkpoint(), Some(3));
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
        assert_eq!(log_window.find_last_checkpoint(), Some(2));
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
        assert_eq!(log_window.find_last_checkpoint(), Some(3));
    }
}
