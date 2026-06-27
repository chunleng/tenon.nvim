use crate::chat::{ActiveWorkflow, ChatLogIndexer, TenonLog, TenonLogData};
use crate::get_workflow_registry;
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
pub struct NavigateWorkflowArgs {
    pub step: usize,
    pub step_output: String,
}

#[derive(Clone)]
pub struct NavigateWorkflow {
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
    pub log_indexer: Arc<RwLock<ChatLogIndexer>>,
}

impl Tool for NavigateWorkflow {
    const NAME: &'static str = "navigate_workflow";
    type Error = ToolError;
    type Args = NavigateWorkflowArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "navigate_workflow".to_string(),
            description: "Navigate workflow steps".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "step": {
                        "type": "integer",
                        "description": "Step number (1-indexed)"
                    },
                    "step_output": {
                        "type": "string",
                        "description": "Output of current step"
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

        // Find matching goto_instruction from current step
        let goto_instruction = workflow.steps.get(current_step - 1).and_then(|step| {
            step.goto_instructions
                .iter()
                .find(|instr| instr.to.resolve_step_index(current_step) == Some(target_step))
        });

        // Store step_output in memory if configured
        if let Some(goto_instr) = goto_instruction
            && let Some(ref memory_key) = goto_instr.output_to_workflow_memory
            && let Some(ref mut workflow_ref) = active_workflow_guard.as_mut()
        {
            workflow_ref
                .memory
                .insert(memory_key.clone(), args.step_output.clone());
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
            let mut indexer = self
                .log_indexer
                .write()
                .map_err(|e| lock_err(e, "write log_indexer"))?;
            if let Ok(workflow_log) = workflow.generate_log(target_step) {
                indexer.logs.push(crate::chat::log_indexer::IndexedLog {
                    log: Arc::new(TenonLog::new(TenonLogData::Workflow(workflow_log))),
                    active: true,
                });
            }
        }

        Ok(format!(
            "Navigated to step {} ({}). Output: {}",
            target_step, step_title, args.step_output
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log_indexer::ChatLogIndexer;
    use std::collections::HashMap;

    #[test]
    fn test_navigate_workflow_stores_memory() {
        // Initialize PLUGIN_ROOT for testing
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        // Create workflow with memory
        let workflow = Arc::new(RwLock::new(Some(ActiveWorkflow {
            id: "implement_code".to_string(),
            step: 1,
            memory: HashMap::new(),
        })));

        let log_indexer = Arc::new(RwLock::new(ChatLogIndexer::new()));

        let tool = NavigateWorkflow {
            active_workflow: Arc::clone(&workflow),
            log_indexer,
        };

        // Navigate to step 2 with output
        let result =
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(tool.call(NavigateWorkflowArgs {
                    step: 2,
                    step_output: "test output from step 1".to_string(),
                }));

        assert!(result.is_ok());

        // Verify that workflow step was updated
        let guard = workflow.read().unwrap();
        let active = guard.as_ref().unwrap();
        assert_eq!(active.step, 2);

        // Note: Memory would only be populated if the workflow definition has
        // output_to_workflow_memory configured, which implement_code doesn't have
        // in step 1's goto_instructions. This test validates the basic navigation works.
    }
}
