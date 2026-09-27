use crate::chat::WorkQueue;

use rig::tool::{ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

use crate::tools::{ToolCore, ToolCoreCall};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PopTaskArgs {
    pub group: String,
}

pub struct PopTask;

pub struct PopTaskCall {
    output: String,
}

impl ToolCore for PopTask {
    fn name(&self) -> String {
        "pop_task".to_string()
    }
    type Error = ToolExecutionError;
    type Args = PopTaskArgs;
    type Output = String;
    type Call = PopTaskCall;

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
        match guard.pop(&args.group) {
            Some(entry) => {
                let output = serde_yaml::to_string(&entry)
                    .map_err(|e| ToolExecutionError::other(e.to_string()))?;
                Ok(PopTaskCall { output })
            }
            None => Err(ToolExecutionError::other(format!(
                "No queued tasks in group '{}'.",
                args.group
            ))),
        }
    }
}

impl ToolCoreCall for PopTaskCall {
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

    fn args(group: &str) -> PopTaskArgs {
        PopTaskArgs {
            group: group.to_string(),
        }
    }

    fn context_with_queue(queue: &Arc<RwLock<WorkQueue>>) -> ToolContext {
        let mut context = ToolContext::new();
        context.insert(queue.clone());
        context
    }

    #[tokio::test]
    async fn test_missing_context_returns_error() {
        let tool = PopTask;
        let mut context = ToolContext::new();

        let result = tool.init_call(&mut context, args("refactor")).await;

        let err = result
            .map(|_| ())
            .expect_err("missing context value must error");
        assert!(
            err.to_string().contains("was not found"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn test_pop_task_returns_yaml_and_removes_entry() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        queue
            .write()
            .unwrap()
            .push("refactor".into(), "fix X".into(), "long X".into());
        let tool = PopTask;
        let mut context = context_with_queue(&queue);

        let call = tool
            .init_call(&mut context, args("refactor"))
            .await
            .expect("init_call should succeed");
        assert!(queue.read().unwrap().is_empty());

        let output = call
            .result(&mut context)
            .await
            .expect("result should succeed");
        assert_eq!(output, "group: refactor\nid: fix X\ndetails: long X\n");
    }

    #[tokio::test]
    async fn test_pop_task_empty_or_unknown_group_returns_error() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        let tool = PopTask;
        let mut context = context_with_queue(&queue);

        let result = tool.init_call(&mut context, args("bugs")).await;

        let err = result.map(|_| ()).expect_err("empty group must error");
        assert!(err.to_string().contains("No queued tasks in group 'bugs'."));
    }

    #[tokio::test]
    async fn test_popped_task_disappears_from_context() {
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        queue
            .write()
            .unwrap()
            .push("bugs".into(), "fix crash".into(), "crash details".into());
        let tool = PopTask;
        let mut context = context_with_queue(&queue);

        let call = tool
            .init_call(&mut context, args("bugs"))
            .await
            .expect("init_call should succeed");
        let _ = call.result(&mut context).await;

        assert!(queue.read().unwrap().render_context().is_none());
    }
}
