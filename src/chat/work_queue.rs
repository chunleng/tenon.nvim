use serde::{Deserialize, Serialize};

/// A task stashed on the work queue for later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkQueueEntry {
    pub group: String,
    pub title: String,
    pub details: String,
}

/// Work queue shared between the push/pop tools and the prompt builder.
/// Entries are popped FIFO within a group.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkQueue {
    pub entries: Vec<WorkQueueEntry>,
}

impl WorkQueue {
    pub fn push(&mut self, group: String, title: String, details: String) {
        self.entries.push(WorkQueueEntry {
            group,
            title,
            details,
        });
    }

    /// Removes and returns the first entry matching `group`.
    pub fn pop(&mut self, group: &str) -> Option<WorkQueueEntry> {
        let index = self.entries.iter().position(|e| e.group == group)?;
        Some(self.entries.remove(index))
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Renders queued titles for context injection.
    /// Returns `None` when the queue is empty (no section in the prompt).
    pub fn render_context(&self) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let lines: Vec<String> = self
            .entries
            .iter()
            .map(|e| format!("{}: {}", e.group, e.title))
            .collect();
        Some(format!("<work_queue>\n{}\n</work_queue>", lines.join("\n")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_pop_fifo_within_group() {
        let mut queue = WorkQueue::default();
        queue.push("refactor".into(), "fix X".into(), "long X".into());
        queue.push("refactor".into(), "fix Y".into(), "long Y".into());
        queue.push(
            "docs".into(),
            "write README".into(),
            "README details".into(),
        );

        let popped = queue.pop("refactor").unwrap();
        assert_eq!(popped.title, "fix X");
        assert_eq!(popped.details, "long X");

        let popped = queue.pop("refactor").unwrap();
        assert_eq!(popped.title, "fix Y");
        assert_eq!(popped.details, "long Y");

        assert!(queue.pop("refactor").is_none());
        assert!(!queue.is_empty());
    }

    #[test]
    fn test_render_context_groups_by_group_name() {
        let mut queue = WorkQueue::default();
        queue.push("refactor".into(), "fix X".into(), "long X".into());
        queue.push(
            "docs".into(),
            "write README".into(),
            "README details".into(),
        );
        queue.push("refactor".into(), "fix Y".into(), "long Y".into());

        let rendered = queue.render_context().unwrap();
        assert!(rendered.starts_with("<work_queue>"));
        assert!(rendered.ends_with("</work_queue>"));
        assert!(rendered.contains("refactor: fix X"));
        assert!(rendered.contains("docs: write README"));
        assert!(rendered.contains("refactor: fix Y"));
    }

    #[test]
    fn test_render_context_empty_returns_none() {
        let queue = WorkQueue::default();
        assert!(queue.render_context().is_none());
    }

    #[test]
    fn test_entry_without_details_fails_to_deserialize() {
        // details is required in JSON; entries missing it are rejected.
        let json = r#"{"group":"bugs","title":"fix crash"}"#;
        assert!(serde_json::from_str::<WorkQueueEntry>(json).is_err());
    }
}
