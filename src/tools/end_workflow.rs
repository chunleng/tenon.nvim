use crate::chat::ActiveWorkflow;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

fn lock_err(e: impl std::fmt::Display, context: &str) -> ToolExecutionError {
    ToolExecutionError::other(format!("Failed to {}: {}", context, e))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndWorkflowArgs {
    pub step_artifact: Option<String>,
}

#[derive(Clone)]
pub struct EndWorkflow {
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
}

impl Tool for EndWorkflow {
    const NAME: &'static str = "end_workflow";
    type Error = ToolExecutionError;
    type Args = EndWorkflowArgs;
    type Output = String;

    fn description(&self) -> String {
        "End workflow. Use when complete".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "step_artifact": {
                    "type": "string",
                    "description": "Artifact of workflow, according to \"Workflow Step Artifact\" section. If section is missing, this should be omitted"
                }
            }
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let active_workflow_guard = self
            .active_workflow
            .read()
            .map_err(|e| lock_err(e, "read active_workflow"))?;

        if active_workflow_guard.is_none() {
            return Err(ToolExecutionError::invalid_args("No active workflow"));
        }

        Ok(format!(
            "workflow completed. output: {}",
            args.step_artifact.as_deref().unwrap_or("")
        ))
    }
}
