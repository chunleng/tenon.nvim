use crate::chat::{ActiveWorkflow, TenonLog, TenonLogData};
use crate::get_workflow_registry;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

fn lock_err(e: impl std::fmt::Display, context: &str) -> ToolError {
    ToolError::ToolCallError(Box::new(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("Failed to {}: {}", context, e),
    )))
}

#[derive(Deserialize)]
pub struct NavigateWorkflowArgs {
    pub step: usize,
    pub step_output: String,
}

#[derive(Clone)]
pub struct NavigateWorkflow {
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub logs: Arc<RwLock<Vec<Arc<TenonLog>>>>,
}

impl Tool for NavigateWorkflow {
    const NAME: &'static str = "navigate_workflow";
    type Error = ToolError;
    type Args = NavigateWorkflowArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "navigate_workflow".to_string(),
            description:
                "Navigate workflow steps. NEVER USE halfway through current step. use after current step COMPLETES."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "step": {
                        "type": "integer",
                        "description": "The step number to navigate to (1-indexed)"
                    },
                    "step_output": {
                        "type": "string",
                        "description": "Message to pass to the next step"
                    }
                },
                "required": ["step", "step_output"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Acquire write lock upfront so check-and-mutate is atomic (no TOCTOU gap)
        let mut active_workflow_guard = self
            .active_workflow
            .write()
            .map_err(|e| lock_err(e, "write active_workflow"))?;

        let active_workflow = match active_workflow_guard.as_ref() {
            Some(w) => w.clone(),
            None => {
                return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "No active workflow",
                ))));
            }
        };

        let registry = get_workflow_registry();
        let workflow = registry
            .get(&active_workflow.id)
            .expect("active workflow id must exist in workflow registry");

        // Validate that the active workflow's id matches the workflow definition
        if active_workflow.id != workflow.id {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Active workflow id '{}' does not match expected '{}'",
                    active_workflow.id, workflow.id
                ),
            ))));
        }

        let current_step = active_workflow.step;
        let target_step = args.step;
        let total_steps = workflow.steps.len();

        // Validate navigation — enforce structural bounds only;
        // goto_instructions are already communicated to the LLM via the prompt.
        let is_valid_navigation =
            target_step > 0 && target_step <= current_step + 1 && target_step <= total_steps;

        if !is_valid_navigation {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Invalid navigation from step {} to step {}",
                    current_step, target_step
                ),
            ))));
        }

        if let Some(ref mut workflow_ref) = active_workflow_guard.as_mut() {
            workflow_ref.step = target_step;
        }

        let step_title = workflow
            .steps
            .get(target_step - 1)
            .map(|s| s.title.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        // Add workflow log for the target step
        {
            let mut logs_guard = self.logs.write().map_err(|e| lock_err(e, "write logs"))?;
            if let Ok(workflow_log) = workflow.generate_log(target_step) {
                logs_guard.push(Arc::new(TenonLog::new(TenonLogData::Workflow(
                    workflow_log,
                ))));
            }
        }

        Ok(format!(
            "Navigated to step {} ({}). Output: {}",
            target_step, step_title, args.step_output
        ))
    }
}
