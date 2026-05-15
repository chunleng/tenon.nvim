use crate::chat::{ActiveWorkflow, ChatLogIndexer, TenonLog, TenonLogData};
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
pub struct StartWorkflowArgs {
    pub workflow_id: String,
}

#[derive(Clone)]
pub struct StartWorkflow {
    pub workflow_ids: Vec<String>,
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub log_indexer: Arc<RwLock<ChatLogIndexer>>,
}

impl Tool for StartWorkflow {
    const NAME: &'static str = "start_workflow";
    type Error = ToolError;
    type Args = StartWorkflowArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "start_workflow".to_string(),
            description: format!(
                "Start a workflow. Each workflow has a condition when it should be used. Available workflows: {}. Use this tool when the user's request matches a workflow's condition.",
                self.workflow_ids.join(", ")
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "workflow_id": {
                        "type": "string",
                        "description": "The ID of the workflow to start"
                    }
                },
                "required": ["workflow_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Check if agent has this workflow configured
        if !self.workflow_ids.iter().any(|id| id == &args.workflow_id) {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Workflow '{}' is not available for this agent. Available workflows: {}",
                    args.workflow_id,
                    self.workflow_ids.join(", ")
                ),
            ))));
        }

        // Validate workflow exists in registry
        let registry = get_workflow_registry();
        let workflow = registry.get(&args.workflow_id).ok_or_else(|| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Workflow '{}' not found in registry. Available workflows: {}",
                    args.workflow_id,
                    registry.keys().cloned().collect::<Vec<_>>().join(", ")
                ),
            )))
        })?;

        // Set active workflow to step 1
        {
            let mut active_workflow_guard = self
                .active_workflow
                .write()
                .map_err(|e| lock_err(e, "write active_workflow"))?;
            *active_workflow_guard = Some(ActiveWorkflow::new(workflow.id.clone(), 1));
        }

        // Add workflow log for step 1
        {
            let mut indexer = self
                .log_indexer
                .write()
                .map_err(|e| lock_err(e, "write log_indexer"))?;
            if let Ok(workflow_log) = workflow.generate_log(1) {
                indexer
                    .logs
                    .push(Arc::new(TenonLog::new(TenonLogData::Workflow(
                        workflow_log,
                    ))));
            }
        }

        Ok(format!(
            "Workflow '{}' ({}): {}.",
            workflow.id, workflow.title, workflow.steps[0].title
        ))
    }
}
