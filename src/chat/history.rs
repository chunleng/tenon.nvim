use chrono::{DateTime, Local};
use rig::completion::Usage;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

use super::log::TenonLog;
use super::log_indexer::ChatLogIndexer;

fn session_datetime_now() -> DateTime<Local> {
    Local::now()
}

/// Serializable snapshot of a chat session, written to `.tenon/history/<id>.json`
/// on `StreamItem::Final`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatHistory {
    pub id: String,
    pub title: Option<String>,
    pub agent_name: String,
    pub model_display: String,
    pub usage: Option<Usage>,
    pub logs: Vec<TenonLog>,
    /// Datetime when the session was created. Defaults to current time for legacy history files.
    #[serde(default = "session_datetime_now")]
    pub session_datetime: DateTime<Local>,
}

/// Session metadata needed to save a chat history.
pub struct SessionMetadata<'a> {
    pub id: &'a str,
    pub title: Option<&'a str>,
    pub agent_name: &'a str,
    pub model_display: &'a str,
    pub session_datetime: DateTime<Local>,
}

pub fn save_to_history(
    metadata: SessionMetadata<'_>,
    log_indexer: &ChatLogIndexer,
    usage: &Arc<RwLock<Option<Usage>>>,
    history_directory: &str,
) {
    let logs_vec: Vec<TenonLog> = log_indexer.logs.iter().map(|arc| (**arc).clone()).collect();
    let usage_val = usage.read().ok().and_then(|u| *u);

    let history = ChatHistory {
        id: metadata.id.to_string(),
        title: metadata.title.map(|s| s.to_string()),
        agent_name: metadata.agent_name.to_string(),
        model_display: metadata.model_display.to_string(),
        usage: usage_val,
        logs: logs_vec,
        session_datetime: metadata.session_datetime,
    };

    if let Ok(cwd) = std::env::current_dir() {
        let dir = std::path::Path::new(history_directory);
        // If path is relative, make it relative to cwd
        let dir = if dir.is_relative() {
            cwd.join(dir)
        } else {
            dir.to_path_buf()
        };
        if std::fs::create_dir_all(&dir).is_ok() {
            let path = dir.join(format!("{}.json", metadata.id));
            if let Ok(json) = serde_json::to_string_pretty(&history) {
                let _ = std::fs::write(&path, json);
            }
        }
    }
}

pub fn load_history_entries(history_directory: &str) -> Vec<ChatHistory> {
    let mut entries = Vec::new();
    let Ok(cwd) = std::env::current_dir() else {
        return entries;
    };
    let dir = std::path::Path::new(history_directory);
    let dir = if dir.is_relative() {
        cwd.join(dir)
    } else {
        dir.to_path_buf()
    };
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return entries;
    };

    for entry in read_dir {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(history) = serde_json::from_str::<ChatHistory>(&contents) {
            entries.push(history);
        }
    }

    // Sort by id descending (newest first, since id starts with YYYY-MM-DD)
    entries.sort_by(|a, b| b.id.cmp(&a.id));
    entries
}
