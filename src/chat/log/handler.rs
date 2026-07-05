use std::sync::{Arc, RwLock};

use super::TenonLog;
use super::indexer::{ChatLogIndexer, IndexedLog};
use super::window::LogWindow;

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
}
