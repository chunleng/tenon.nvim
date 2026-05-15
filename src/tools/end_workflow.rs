use crate::chat::{ActiveWorkflow, ChatLogIndexer, TenonLog, TenonLogData, TenonWorkflowLog};
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

fn lock_err(e: impl std::fmt::Display, context: &str) -> ToolError {
    ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
        "Failed to {}: {}",
        context, e
    ))))
}

#[derive(Deserialize)]
pub struct EndWorkflowArgs {
    pub output: String,
}

#[derive(Clone)]
pub struct EndWorkflow {
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub log_indexer: Arc<RwLock<ChatLogIndexer>>,
}

impl Tool for EndWorkflow {
    const NAME: &'static str = "end_workflow";
    type Error = ToolError;
    type Args = EndWorkflowArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "end_workflow".to_string(),
            description: "End workflow. Use when complete".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "output": {
                        "type": "string",
                        "description": "Final message or summary to record for the completed workflow"
                    }
                },
                "required": ["output"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Take active workflow and create end log
        {
            let mut active_workflow_guard = self
                .active_workflow
                .write()
                .map_err(|e| lock_err(e, "write active_workflow"))?;

            match active_workflow_guard.take() {
                Some(active_wf) => {
                    // Add end workflow log
                    let mut indexer = self
                        .log_indexer
                        .write()
                        .map_err(|e| lock_err(e, "write log_indexer"))?;
                    indexer
                        .logs
                        .push(Arc::new(TenonLog::new(TenonLogData::Workflow(
                            TenonWorkflowLog {
                                id: active_wf.id.clone(),
                                content: "Workflow ended".to_string(),
                                step: None,
                            },
                        ))));
                }
                None => {
                    return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "No active workflow",
                    ))));
                }
            }
        };

        Ok(format!("workflow completed. output: {}", args.output))
    }
}
