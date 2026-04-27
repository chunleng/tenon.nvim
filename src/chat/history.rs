use rig::completion::Usage;
use serde::{Deserialize, Serialize};
use std::{
    collections::LinkedList,
    sync::{Arc, RwLock},
};

use super::TenonLog;

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
}

pub fn save_to_history(
    id: &str,
    title: Option<&str>,
    agent_name: &str,
    model_display: &str,
    logs: &Arc<RwLock<LinkedList<TenonLog>>>,
    usage: &Arc<RwLock<Option<Usage>>>,
    history_directory: &str,
) {
    let logs_vec = logs
        .read()
        .ok()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default();
    let usage_val = usage.read().ok().and_then(|u| *u);

    let history = ChatHistory {
        id: id.to_string(),
        title: title.map(|s| s.to_string()),
        agent_name: agent_name.to_string(),
        model_display: model_display.to_string(),
        usage: usage_val,
        logs: logs_vec,
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
            let path = dir.join(format!("{}.json", id));
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
