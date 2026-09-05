use crate::chat::WorkQueue;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskItem {
    pub title: String,
    pub details: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushTasksArgs {
    pub group: String,
    pub tasks: Vec<TaskItem>,
}

#[derive(Clone)]
pub struct PushTasks {
    pub work_queue: Arc<RwLock<WorkQueue>>,
}

impl Tool for PushTasks {
    const NAME: &'static str = "push_tasks";
    type Error = ToolExecutionError;
    type Args = PushTasksArgs;
    type Output = String;

    fn description(&self) -> String {
        "Push tasks to the work queue to be worked later. Include enough detail that anyone picking up the task later can understand how to work on it".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "group": {
                    "type": "string",
                    "description": "Task category"
                },
                "tasks": {
                    "type": "array",
                    "description": "Tasks to queue",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "One-line summary shown in the context tag while queued"
                            },
                            "details": {
                                "type": "string",
                                "description": "Full details: what the work is, where (files/locations), and why"
                            }
                        },
                        "required": ["title", "details"]
                    }
                }
            },
            "required": ["group", "tasks"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let mut queue = self
            .work_queue
            .write()
            .map_err(|e| ToolExecutionError::other(format!("Failed to write work_queue: {}", e)))?;
        for task in &args.tasks {
            queue.push(args.group.clone(), task.title.clone(), task.details.clone());
        }
        Ok(format!(
            "{} task(s) queued under group '{}'. They will be worked when the current task is done or the user asks.",
            args.tasks.len(),
            args.group
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::{Tool, ToolContext};

    #[tokio::test]
    async fn test_push_tasks_stores_entry_in_queue() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        let tool = PushTasks {
            work_queue: queue.clone(),
        };

        let output = tool
            .call(
                &mut ToolContext::new(),
                PushTasksArgs {
                    group: "refactor".to_string(),
                    tasks: vec![TaskItem {
                        title: "fix X".to_string(),
                        details: "long X".to_string(),
                    }],
                },
            )
            .await
            .unwrap();

        assert!(output.contains("refactor"));
        let guard = queue.read().unwrap();
        assert_eq!(guard.entries.len(), 1);
        assert_eq!(guard.entries[0].group, "refactor");
        assert_eq!(guard.entries[0].title, "fix X");
        assert_eq!(guard.entries[0].details, "long X");
    }

    #[tokio::test]
    async fn test_push_tasks_stores_all_entries() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        let tool = PushTasks {
            work_queue: queue.clone(),
        };

        let output = tool
            .call(
                &mut ToolContext::new(),
                PushTasksArgs {
                    group: "docs".to_string(),
                    tasks: vec![
                        TaskItem {
                            title: "a".to_string(),
                            details: "long a".to_string(),
                        },
                        TaskItem {
                            title: "b".to_string(),
                            details: "long b".to_string(),
                        },
                    ],
                },
            )
            .await
            .unwrap();

        assert!(output.contains("docs"));
        let guard = queue.read().unwrap();
        assert_eq!(guard.entries.len(), 2);
        assert_eq!(guard.entries[0].group, "docs");
        assert_eq!(guard.entries[0].title, "a");
        assert_eq!(guard.entries[1].group, "docs");
        assert_eq!(guard.entries[1].title, "b");
    }
}
