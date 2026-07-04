use crate::chat::workflow::Workflow;
use crate::chat::{ActiveWorkflow, ChatLogIndexer, TenonLog, TenonLogData};
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
#[serde(deny_unknown_fields)]
pub struct StartWorkflowArgs {
    pub workflow_id: String,
}

#[derive(Clone)]
pub struct StartWorkflow {
    pub workflows: Vec<Arc<Workflow>>,
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub log_indexer: Arc<RwLock<ChatLogIndexer>>,
}

impl Tool for StartWorkflow {
    const NAME: &'static str = "start_workflow";
    type Error = ToolError;
    type Args = StartWorkflowArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        let candidate_workflow = self
            .workflows
            .iter()
            .map(|wf| format!("- {} — {}", wf.id, wf.description))
            .collect::<Vec<_>>()
            .join("\n");
        ToolDefinition {
            name: "start_workflow".to_string(),
            description: "Start workflow".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "workflow_id": {
                        "type": "string",
                        "description": format!(
                            "The workflow ID to start. Pick the workflow description that best matches the user's intent.\nID — description:\n{}", candidate_workflow
                        ),
                    }
                },
                "required": ["workflow_id"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Find the workflow by id from the agent's configured workflows
        let workflow = self
            .workflows
            .iter()
            .find(|wf| wf.id == args.workflow_id)
            .ok_or_else(|| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "Workflow '{}' is not available for this agent. Available workflows: {}",
                        args.workflow_id,
                        self.workflows
                            .iter()
                            .map(|wf| wf.id.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )))
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

        // Add workflow log for step 1
        {
            let mut indexer = self
                .log_indexer
                .write()
                .map_err(|e| lock_err(e, "write log_indexer"))?;
            if let Ok(workflow_log) = workflow.generate_log(1) {
                indexer
                    .log_window
                    .logs
                    .push(crate::chat::log::indexer::IndexedLog {
                        log: Arc::new(TenonLog::new(TenonLogData::Workflow(workflow_log))),
                        active: true,
                    });
            }
        }

        Ok(format!(
            "Workflow '{}' ({}): {}.",
            workflow.id, workflow.title, workflow.steps[0].title
        ))
    }
}
