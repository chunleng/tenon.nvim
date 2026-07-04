use std::sync::{Arc, RwLock};

use super::indexer::ChatLogIndexer;

#[derive(Clone)]
pub struct ChatLogHandler {
    pub indexer: Arc<RwLock<ChatLogIndexer>>,
}
