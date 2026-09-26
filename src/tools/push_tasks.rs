use crate::chat::WorkQueue;

use rig::tool::{ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

use crate::tools::{ToolCore, ToolCoreCall};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskItem {
    pub id: String,
    pub details: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushTasksArgs {
    pub group: String,
    pub tasks: Vec<TaskItem>,
}

pub struct PushTasks;

pub struct PushTasksCall {
    output: String,
}

impl ToolCore for PushTasks {
    const NAME: &'static str = "push_tasks";
    type Error = ToolExecutionError;
    type Args = PushTasksArgs;
    type Output = String;
    type Call = PushTasksCall;

    fn description(&self) -> String {
        "Push tasks to the work queue to be worked later. Include enough detail for anyone picking up the task later to work on it".to_string()
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
                            "id": {
                                "type": "string",
                                "description": "Task identifier that conveys what the task is (e.g. 'fix-auth-bug'). Match it in the context tag to confirm it is queued"
                            },
                            "details": {
                                "type": "string",
                                "description": "Full details: what the work is, where (files/locations), and why"
                            }
                        },
                        "required": ["id", "details"]
                    }
                }
            },
            "required": ["group", "tasks"]
        })
    }

    async fn init_call(
        &self,
        context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        let queue = context
            .require::<Arc<RwLock<WorkQueue>>>()
            .map_err(ToolExecutionError::from_error)?;

        let mut guard = queue
            .write()
            .map_err(|e| ToolExecutionError::other(format!("Failed to write work_queue: {}", e)))?;
        for task in &args.tasks {
            guard.push(args.group.clone(), task.id.clone(), task.details.clone());
        }
        let output = format!(
            "{} task(s) queued under group '{}'. They will be worked when the current task is done or the user asks.",
            args.tasks.len(),
            args.group
        );
        Ok(PushTasksCall { output })
    }
}

impl ToolCoreCall for PushTasksCall {
    type Output = String;
    type Error = ToolExecutionError;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        Ok(self.output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolCore;
    use rig::tool::ToolContext;
    use std::sync::{Arc, RwLock};

    fn args(group: &str, tasks: &[(&str, &str)]) -> PushTasksArgs {
        PushTasksArgs {
            group: group.to_string(),
            tasks: tasks
                .iter()
                .map(|(id, details)| TaskItem {
                    id: (*id).to_string(),
                    details: (*details).to_string(),
                })
                .collect(),
        }
    }

    fn context_with_queue(queue: &Arc<RwLock<WorkQueue>>) -> ToolContext {
        let mut context = ToolContext::new();
        context.insert(queue.clone());
        context
    }

    #[tokio::test]
    async fn test_missing_context_returns_error() {
        let tool = PushTasks;
        let mut context = ToolContext::new();

        let result = tool
            .init_call(&mut context, args("refactor", &[("fix X", "long X")]))
            .await;

        let err = result
            .map(|_| ())
            .expect_err("missing context value must error");
        assert!(
            err.to_string().contains("was not found"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_push_tasks_stores_entry_in_queue() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        let tool = PushTasks;
        let mut context = context_with_queue(&queue);

        let call = tool
            .init_call(&mut context, args("refactor", &[("fix X", "long X")]))
            .await
            .expect("init_call should succeed");

        let guard = queue.read().unwrap();
        assert_eq!(guard.entries.len(), 1);
        assert_eq!(guard.entries[0].group, "refactor");
        assert_eq!(guard.entries[0].id, "fix X");
        assert_eq!(guard.entries[0].details, "long X");
        drop(guard);

        let output = call
            .result(&mut context)
            .await
            .expect("result should succeed");
        assert!(output.contains("refactor"));
    }

    #[tokio::test]
    async fn test_push_tasks_stores_all_entries() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        let tool = PushTasks;
        let mut context = context_with_queue(&queue);

        let call = tool
            .init_call(
                &mut context,
                args("docs", &[("a", "long a"), ("b", "long b")]),
            )
            .await
            .expect("init_call should succeed");

        let guard = queue.read().unwrap();
        assert_eq!(guard.entries.len(), 2);
        assert_eq!(guard.entries[0].group, "docs");
        assert_eq!(guard.entries[0].id, "a");
        assert_eq!(guard.entries[1].group, "docs");
        assert_eq!(guard.entries[1].id, "b");
        drop(guard);

        let output = call
            .result(&mut context)
            .await
            .expect("result should succeed");
        assert!(output.contains("docs"));
    }
}
