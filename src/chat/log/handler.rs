use std::sync::{Arc, RwLock};

use rig::completion::Message;

use super::TenonLog;
use super::indexer::{ChatLogIndexer, IndexedLog};
use super::window::LogWindow;
use crate::chat::log::TenonLogData;
use crate::chat::{TenonUserMessage, TenonUserTextMessage};

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

    /// Creates a ChatLogHandler from existing logs (for history restoration).
    /// All logs are initialized as active by default, then context truncation is applied.
    pub fn from_logs(logs: Vec<TenonLog>) -> Self {
        let mut log_window = LogWindow {
            logs: logs
                .into_iter()
                .map(|log| IndexedLog {
                    log: Arc::new(log),
                    active: true,
                })
                .collect(),
        };

        let indexer = ChatLogIndexer::new();
        indexer.apply_context_truncation(&mut log_window);

        Self {
            indexer: Arc::new(RwLock::new(indexer)),
            log_window: Arc::new(RwLock::new(log_window)),
        }
    }

    pub fn add_user_message(&self, message: String) {
        if let Ok(mut log_window) = self.log_window.write() {
            log_window.prune_incomplete_messages();
            log_window.logs.push(IndexedLog {
                log: Arc::new(TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
                    TenonUserTextMessage(message),
                )))),
                active: true,
            });
        }
    }

    pub fn get_user_prompt(&self) -> String {
        if let Ok(log_window) = self.log_window.read() {
            let len = log_window.logs.len();
            if len > 0
                && let TenonLogData::User(TenonUserMessage::Text(text_msg)) =
                    log_window.logs[len - 1].log.data()
            {
                return text_msg.0.clone();
            }
        }
        "<context></context>".to_string()
    }

    pub fn get_chat_history(&mut self, user_message: &str) -> Vec<Message> {
        if let Ok(indexer) = self.indexer.read() {
            let mut log_window = match self.log_window.write() {
                Ok(x) => x,
                Err(_) => return Vec::new(),
            };
            indexer.apply_context_truncation(&mut log_window);
            let log_window_clone = log_window.clone();
            drop(log_window);
            indexer.retrieve_chatlog_with_context(log_window_clone, &user_message)
        } else {
            Vec::new()
        }
    }
}
