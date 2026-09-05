use chrono::{DateTime, Local};
use rig::completion::Usage;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

use super::SessionUsage;
use super::log::TenonLog;
use super::log::window::LogWindow;
use super::work_queue::WorkQueue;

fn session_datetime_now() -> DateTime<Local> {
    Local::now()
}

/// Serializable snapshot of a chat session, written to `.tenon/history/<id>.json`
/// on `StreamItem::CompletionCall`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatHistory {
    pub id: String,
    pub title: Option<String>,
    pub agent_name: String,
    pub model_display: String,
    #[serde(default)]
    pub usage: Usage,
    pub logs: Vec<TenonLog>,
    /// Datetime when the session was created. Defaults to current time for legacy history files.
    #[serde(default = "session_datetime_now")]
    pub session_datetime: DateTime<Local>,
    /// Work queue snapshot. Defaults to empty for legacy history files.
    #[serde(default)]
    pub work_queue: WorkQueue,
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
    log_window: &LogWindow,
    usage: &Arc<RwLock<SessionUsage>>,
    work_queue: &Arc<RwLock<WorkQueue>>,
    history_directory: &str,
) {
    let logs_vec: Vec<TenonLog> = log_window
        .logs
        .iter()
        .map(|indexed| (*indexed.log).clone())
        .collect();
    let usage_val = usage.read().map(|u| u.accumulated).unwrap_or_default();
    let work_queue_val = work_queue.read().map(|q| q.clone()).unwrap_or_default();

    let history = ChatHistory {
        id: metadata.id.to_string(),
        title: metadata.title.map(|s| s.to_string()),
        agent_name: metadata.agent_name.to_string(),
        model_display: metadata.model_display.to_string(),
        usage: usage_val,
        logs: logs_vec,
        session_datetime: metadata.session_datetime,
        work_queue: work_queue_val,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_history(work_queue: WorkQueue) -> ChatHistory {
        ChatHistory {
            id: "2025-01-01-120000-test".to_string(),
            title: None,
            agent_name: "default".to_string(),
            model_display: "test-model".to_string(),
            usage: Usage::default(),
            logs: vec![],
            session_datetime: Local::now(),
            work_queue,
        }
    }

    #[test]
    fn test_history_round_trips_work_queue() {
        let mut queue = WorkQueue::default();
        queue.push("refactor".into(), "fix X".into(), "long X".into());
        queue.push(
            "docs".into(),
            "write README".into(),
            "README details".into(),
        );

        let history = test_history(queue);
        let json = serde_json::to_string(&history).unwrap();
        let restored: ChatHistory = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.work_queue.entries.len(), 2);
        assert_eq!(restored.work_queue.entries[0].group, "refactor");
        assert_eq!(restored.work_queue.entries[0].title, "fix X");
        assert_eq!(restored.work_queue.entries[0].details, "long X");
        assert_eq!(restored.work_queue.entries[1].group, "docs");
    }

    #[test]
    fn test_legacy_history_without_work_queue_deserializes_empty() {
        // Simulate a legacy file: same shape but without the work_queue field
        let mut value = serde_json::to_value(test_history(WorkQueue::default())).unwrap();
        value.as_object_mut().unwrap().remove("work_queue");
        let json = serde_json::to_string(&value).unwrap();

        let restored: ChatHistory = serde_json::from_str(&json).unwrap();
        assert!(restored.work_queue.is_empty());
    }

    #[test]
    fn test_save_to_history_persists_work_queue() {
        let mut queue = WorkQueue::default();
        queue.push("refactor".into(), "fix X".into(), "long X".into());

        let dir = std::env::temp_dir().join(format!(
            "tenon-history-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let metadata = SessionMetadata {
            id: "2025-01-01-120000-queue-save",
            title: None,
            agent_name: "default",
            model_display: "test-model",
            session_datetime: Local::now(),
        };

        save_to_history(
            metadata,
            &LogWindow { logs: vec![] },
            &Arc::new(RwLock::new(SessionUsage::default())),
            &Arc::new(RwLock::new(queue)),
            dir.to_str().unwrap(),
        );

        let entries = load_history_entries(dir.to_str().unwrap());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].work_queue.entries.len(), 1);
        assert_eq!(entries[0].work_queue.entries[0].group, "refactor");
        assert_eq!(entries[0].work_queue.entries[0].details, "long X");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
