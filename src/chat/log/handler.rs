use std::sync::{Arc, RwLock};

use rig::completion::Message;

use super::TenonLog;
use super::indexer::{ChatLogIndexer, IndexedLog};
use super::window::LogWindow;
use crate::chat::TenonUserMessage;
use crate::chat::log::TenonLogData;

#[derive(Clone)]
pub struct ChatLogHandler {
    pub indexer: Arc<RwLock<ChatLogIndexer>>,
    pub log_window: Arc<RwLock<LogWindow>>,
}

impl ChatLogHandler {
    /// Creates a new empty ChatLogHandler.
    pub fn new() -> Self {
        Self {
            indexer: Arc::new(RwLock::new(ChatLogIndexer::new())),
            log_window: Arc::new(RwLock::new(LogWindow { logs: Vec::new() })),
        }
    }

    /// Replaces log_window contents in-place from existing logs (for history restoration).
    /// All logs are initialized as active by default, then context truncation is applied.
    pub fn load(&mut self, logs: Vec<TenonLog>) {
        let mut log_window = LogWindow {
            logs: logs
                .into_iter()
                .map(|log| IndexedLog {
                    log: Arc::new(RwLock::new(log)),
                    active: true,
                })
                .collect(),
        };

        if let Ok(indexer) = self.indexer.read() {
            indexer.apply_context_truncation(&mut log_window);
        }

        if let Ok(mut existing) = self.log_window.write() {
            *existing = log_window;
        }
    }

    pub fn add_user_message(&self, message: String) {
        if let Ok(mut log_window) = self.log_window.write() {
            log_window.prune_incomplete_messages();
            log_window.add_log(IndexedLog {
                log: Arc::new(RwLock::new(TenonLog::new(TenonLogData::User(
                    TenonUserMessage::Text(message),
                )))),
                active: true,
            });
        }
    }

    pub fn get_user_prompt(&self) -> String {
        if let Ok(log_window) = self.log_window.read()
            && !log_window.logs.is_empty()
            && let Some(last) = log_window.logs.last()
            && let Ok(log) = last.log.read()
        {
            match log.data() {
                TenonLogData::User(TenonUserMessage::Text(text_msg)) => {
                    return text_msg.clone();
                }
                TenonLogData::Question(question) => {
                    return question.answer().to_string();
                }
                _ => {}
            }
        }
        "".to_string()
    }

    pub fn get_chat_history(&mut self, user_message: &str) -> Vec<Message> {
        if let Ok(indexer) = self.indexer.read() {
            let mut log_window = match self.log_window.write() {
                Ok(x) => x,
                Err(_) => return Vec::new(),
            };
            indexer.apply_context_truncation(&mut log_window);
            let log_window_clone = log_window.clone();
            // Prevent long running rag to hold on to the lock for too long
            drop(log_window);
            indexer.retrieve_chatlog_with_context(log_window_clone, user_message)
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::chat::log::TenonAssistantMessageContent;
    use crate::chat::{
        TenonAssistantMessage, TenonQuestionLog, TenonToolCall, TenonToolLog, TenonToolResult,
    };

    fn handler_with_last_log(data: TenonLogData) -> ChatLogHandler {
        let mut handler = ChatLogHandler::new();
        handler.load(vec![TenonLog::new(data)]);
        handler
    }

    #[test]
    fn test_get_user_prompt_question_last_returns_answer() {
        let handler = handler_with_last_log(TenonLogData::Question(TenonQuestionLog {
            tool_log: TenonToolLog {
                tool_call: TenonToolCall {
                    id: "call-1".to_string(),
                    internal_call_id: "call-1".to_string(),
                    item_id: None,
                    name: "ask_question".to_string(),
                    args: serde_json::json!({ "question": "Use the default mode?" }),
                },
                tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text::new(
                    "Yes".to_string(),
                )))),
                progress: vec![],
            },
        }));

        assert_eq!(handler.get_user_prompt(), "Yes");
    }

    #[test]
    fn test_get_user_prompt_user_last_returns_text() {
        let handler = handler_with_last_log(TenonLogData::User(TenonUserMessage::Text(
            "Fix the truncation bug".to_string(),
        )));

        assert_eq!(handler.get_user_prompt(), "Fix the truncation bug");
    }

    #[test]
    fn test_get_user_prompt_other_last_returns_empty() {
        let handler = handler_with_last_log(TenonLogData::Assistant(TenonAssistantMessage {
            reasoning: None,
            content: vec![TenonAssistantMessageContent::Text("Done".to_string())],
            complete: true,
        }));

        assert_eq!(handler.get_user_prompt(), "");
    }
}
