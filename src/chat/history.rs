use rig::completion::Usage;
use serde::{Deserialize, Serialize};
use std::{
    collections::LinkedList,
    sync::{Arc, RwLock},
};

use super::TenonLog;

/// Serializable snapshot of a chat process, written to `.tenon/history/<id>.json`
/// on `StreamItem::Final`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatHistory {
    pub id: String,
    pub agent_name: String,
    pub model_display: String,
    pub usage: Option<Usage>,
    pub logs: Vec<TenonLog>,
}

pub fn save_to_history(
    id: &str,
    agent_name: &str,
    model_display: &str,
    logs: &Arc<RwLock<LinkedList<TenonLog>>>,
    usage: &Arc<RwLock<Option<Usage>>>,
) {
    let logs_vec = logs
        .read()
        .ok()
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default();
    let usage_val = usage.read().ok().and_then(|u| *u);

    let history = ChatHistory {
        id: id.to_string(),
        agent_name: agent_name.to_string(),
        model_display: model_display.to_string(),
        usage: usage_val,
        logs: logs_vec,
    };

    if let Ok(cwd) = std::env::current_dir() {
        let dir = cwd.join(".tenon").join("history");
        if std::fs::create_dir_all(&dir).is_ok() {
            let path = dir.join(format!("{}.json", id));
            if let Ok(json) = serde_json::to_string_pretty(&history) {
                let _ = std::fs::write(&path, json);
            }
        }
    }
}
