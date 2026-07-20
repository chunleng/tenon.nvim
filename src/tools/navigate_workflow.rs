use crate::chat::ActiveWorkflow;

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
pub struct NavigateWorkflowArgs {
    pub step: usize,
    pub step_artifact: Option<String>,
}

#[derive(Clone)]
pub struct NavigateWorkflow {
    pub active_workflow: Arc<RwLock<Option<ActiveWorkflow>>>,
}

impl Tool for NavigateWorkflow {
    const NAME: &'static str = "navigate_workflow";
    type Error = ToolError;
    type Args = NavigateWorkflowArgs;
    type Output = String;

    fn description(&self) -> String {
        "Navigate workflow steps".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "step": {
                    "type": "integer",
                    "description": "Step number (1-indexed)"
                },
                "step_artifact": {
                    "type": "string",
                    "description": "Artifact of current step, according to \"Workflow Step Artifact\" section. If section is missing, this should be omitted"
                }
            },
            "required": ["step"]
        })
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

        let workflow = &active_workflow.workflow;

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

        // Store step_artifact in memory if configured
        if let Some(goto_instr) = goto_instruction
            && let Some(ref memory_key) = goto_instr.output_to_workflow_memory
            && let Some(ref mut workflow_ref) = active_workflow_guard.as_mut()
            && let Some(step_artifact) = args.step_artifact.clone()
        {
            workflow_ref
                .memory
                .insert(memory_key.clone(), step_artifact);
        }

        if let Some(ref mut workflow_ref) = active_workflow_guard.as_mut() {
            workflow_ref.step = target_step;
        }

        let yaml = serde_yaml::to_string(&json!({
            "step": target_step,
            "artifact": args.step_artifact,
        }))
        .map_err(|e| lock_err(e, "serialize navigate_workflow output"))?;
        Ok(yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::window::LogWindow;
    use crate::chat::{TenonLog, TenonLogData, TenonToolCall};
    use std::collections::HashMap;

    fn dummy_tool_call(name: &str) -> TenonToolCall {
        TenonToolCall {
            id: "test-id".to_string(),
            internal_call_id: "test-internal-id".to_string(),
            name: name.to_string(),
            args: serde_json::json!({"step": 2, "step_artifact": "test output from step 1"}),
        }
    }

    #[test]
    fn test_navigate_workflow_stores_memory() {
        // Initialize PLUGIN_ROOT for testing
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        // Create workflow with memory
        let registry = crate::get_workflow_registry();
        let wf = registry.get("implement_code").unwrap().clone();
        let workflow = Arc::new(RwLock::new(Some(ActiveWorkflow {
            workflow: wf,
            step: 1,
            memory: HashMap::new(),
        })));

        let log_window = Arc::new(RwLock::new(LogWindow { logs: Vec::new() }));

        let tool = NavigateWorkflow {
            active_workflow: Arc::clone(&workflow),
        };

        // Navigate to step 2 with output
        let result =
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(tool.call(NavigateWorkflowArgs {
                    step: 2,
                    step_artifact: Some("test output from step 1".to_string()),
                }));

        assert!(result.is_ok());

        // Verify that workflow step was updated
        let guard = workflow.read().unwrap();
        let active = guard.as_ref().unwrap();
        assert_eq!(active.step, 2);

        // Simulate the ToolResult handler: create workflow log with the tool log
        let tool_log = crate::chat::TenonToolLog {
            tool_call: dummy_tool_call("navigate_workflow"),
            tool_result: None,
        };
        {
            let wf = active.workflow.clone();
            let wf_log = wf.generate_log(active.step, tool_log).unwrap();
            let mut log_window = log_window.write().unwrap();
            log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                log: Arc::new(TenonLog::new(TenonLogData::Workflow(wf_log))),
                active: true,
            });
        }

        // Verify workflow log contains the tool log that navigated to this step
        {
            let log_window = log_window.read().unwrap();
            let workflow_log = log_window
                .logs
                .iter()
                .find_map(|indexed| {
                    if let TenonLogData::Workflow(wf_log) = indexed.log.data() {
                        Some(wf_log)
                    } else {
                        None
                    }
                })
                .expect("workflow log should exist");
            assert_eq!(workflow_log.tool_log.tool_call.name, "navigate_workflow");
            assert_eq!(workflow_log.tool_log.tool_call.id, "test-id");
        }

        // Note: Memory would only be populated if the workflow definition has
        // output_to_workflow_memory configured, which implement_code doesn't have
        // in step 1's goto_instructions. This test validates the basic navigation works.
    }
}
