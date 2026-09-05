use crate::chat::WorkQueue;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PopTaskArgs {
    pub group: String,
}

#[derive(Clone)]
pub struct PopTask {
    pub work_queue: Arc<RwLock<WorkQueue>>,
}

impl Tool for PopTask {
    const NAME: &'static str = "pop_task";
    type Error = ToolExecutionError;
    type Args = PopTaskArgs;
    type Output = String;

    fn description(&self) -> String {
        "Pop the next task from the work queue and work on it".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "group": {
                    "type": "string",
                    "description": "Task category"
                }
            },
            "required": ["group"]
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
        match queue.pop(&args.group) {
            Some(entry) => Ok(serde_yaml::to_string(&entry)
                .map_err(|e| ToolExecutionError::other(e.to_string()))?),
            None => Ok(format!("No queued tasks in group '{}'.", args.group)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::tool::{Tool, ToolContext};

    #[tokio::test]
    async fn test_pop_task_returns_yaml_and_removes_entry() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        queue
            .write()
            .unwrap()
            .push("refactor".into(), "fix X".into(), "long X".into());
        let tool = PopTask {
            work_queue: queue.clone(),
        };

        let output = tool
            .call(
                &mut ToolContext::new(),
                PopTaskArgs {
                    group: "refactor".to_string(),
                },
            )
            .await
            .unwrap();

        assert_eq!(output, "group: refactor\ntitle: fix X\ndetails: long X\n");
        assert!(queue.read().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_pop_task_empty_or_unknown_group_returns_notice() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        let tool = PopTask {
            work_queue: queue.clone(),
        };

        let output = tool
            .call(
                &mut ToolContext::new(),
                PopTaskArgs {
                    group: "bugs".to_string(),
                },
            )
            .await
            .unwrap();

        assert_eq!(output, "No queued tasks in group 'bugs'.");
    }

    #[tokio::test]
    async fn test_popped_task_disappears_from_context() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        queue
            .write()
            .unwrap()
            .push("bugs".into(), "fix crash".into(), "crash details".into());
        let tool = PopTask {
            work_queue: queue.clone(),
        };

        tool.call(
            &mut ToolContext::new(),
            PopTaskArgs {
                group: "bugs".to_string(),
            },
        )
        .await
        .unwrap();

        assert!(queue.read().unwrap().render_context().is_none());
    }
}
