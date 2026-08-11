use crate::chat::ActiveWorkflow;
use crate::chat::workflow::Workflow;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

fn lock_err(e: impl std::fmt::Display, context: &str) -> ToolExecutionError {
    ToolExecutionError::other(format!("Failed to {}: {}", context, e))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartWorkflowArgs {
    pub workflow_id: String,
}

#[derive(Clone)]
pub struct StartWorkflow {
    pub workflows: Vec<Arc<Workflow>>,
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
}

impl Tool for StartWorkflow {
    const NAME: &'static str = "start_workflow";
    type Error = ToolExecutionError;
    type Args = StartWorkflowArgs;
    type Output = String;

    fn description(&self) -> String {
        let candidate_workflow = self
            .workflows
            .iter()
            .map(|wf| format!("- {} — {}", wf.id, wf.description))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Start a workflow to execute a task through a predefined procedure with built-in \
             verification and iteration.
             Prefer a workflow over ad-hoc tool calls whenever a workflow description fits the problem.\
             \n\n
             \nAvailable Workflow ID — description:\
             \n{}",
            candidate_workflow
        )
    }

    fn parameters(&self) -> serde_json::Value {
        let workflow_ids = self
            .workflows
            .iter()
            .map(|wf| wf.id.clone())
            .collect::<Vec<_>>();
        json!({
            "type": "object",
            "properties": {
                "workflow_id": {
                    "type": "string",
                    "enum": workflow_ids,
                    "description": "Workflow ID to start",
                }
            },
            "required": ["workflow_id"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        // Find the workflow by id from the agent's configured workflows
        let workflow = self
            .workflows
            .iter()
            .find(|wf| wf.id == args.workflow_id)
            .ok_or_else(|| {
                ToolExecutionError::invalid_args(format!(
                    "Workflow '{}' is not available for this agent. Available workflows: {}",
                    args.workflow_id,
                    self.workflows
                        .iter()
                        .map(|wf| wf.id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?
            .clone();

        // Set active workflow to step 1
        {
            let mut active_workflow_guard = self
                .active_workflow
                .write()
                .map_err(|e| lock_err(e, "write active_workflow"))?;
            *active_workflow_guard = Some(ActiveWorkflow::new(workflow.clone(), 1));
        }

        Ok(format!(
            "Workflow '{}' ({}): {}.",
            workflow.id, workflow.title, workflow.steps[0].title
        ))
    }
}
