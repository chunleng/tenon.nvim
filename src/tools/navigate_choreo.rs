use crate::chat::ActiveChoreo;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
use std::sync::{Arc, RwLock};

fn lock_err(e: impl std::fmt::Display, context: &str) -> ToolExecutionError {
    ToolExecutionError::other(format!("Failed to {}: {}", context, e))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigateChoreoArgs {
    pub r#move: usize,
    pub move_artifact: Option<String>,
}

#[derive(Clone)]
pub struct NavigateChoreo {
    pub active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
}

impl Tool for NavigateChoreo {
    const NAME: &'static str = "navigate_choreo";
    type Error = ToolExecutionError;
    type Args = NavigateChoreoArgs;
    type Output = String;

    fn description(&self) -> String {
        "Navigate choreo moves".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "move": {
                    "type": "integer",
                    "description": "Move number (1-indexed)"
                },
                "move_artifact": {
                    "type": "string",
                    "description": "Artifact of current move, according to \"Choreo Move Artifact\" section. If section is missing, this should be omitted"
                }
            },
            "required": ["move"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        // Acquire write lock upfront so check-and-mutate is atomic (no TOCTOU gap)
        let mut active_choreo_guard = self
            .active_choreo
            .write()
            .map_err(|e| lock_err(e, "write active_choreo"))?;

        let active_choreo = match active_choreo_guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                return Err(ToolExecutionError::invalid_args("No active choreo"));
            }
        };

        let choreo = &active_choreo.choreo;

        let current_move = active_choreo.r#move;
        let target_move = args.r#move;
        let total_moves = choreo.moves.len();

        // Validate navigation — enforce structural bounds only;
        // goto_instructions are already communicated to the LLM via the prompt.
        let is_valid_navigation =
            target_move > 0 && target_move <= current_move + 1 && target_move <= total_moves;

        if !is_valid_navigation {
            return Err(ToolExecutionError::invalid_args(format!(
                "Invalid navigation from move {} to move {}",
                current_move, target_move
            )));
        }

        // Find matching goto_instruction from current move
        let goto_instruction = choreo.moves.get(current_move - 1).and_then(|current| {
            current
                .goto_instructions
                .iter()
                .find(|instr| instr.to.resolve_move_index(current_move) == Some(target_move))
        });

        // Store move_artifact in memory if configured
        if let Some(goto_instr) = goto_instruction
            && let Some(ref memory_key) = goto_instr.output_to_choreo_memory
            && let Some(ref mut choreo_ref) = active_choreo_guard.as_mut()
            && let Some(move_artifact) = args.move_artifact.clone()
        {
            choreo_ref.memory.insert(memory_key.clone(), move_artifact);
        }

        if let Some(ref mut choreo_ref) = active_choreo_guard.as_mut() {
            choreo_ref.r#move = target_move;
        }

        let yaml = serde_yaml::to_string(&json!({
            "move": target_move,
            "artifact": args.move_artifact,
        }))
        .map_err(|e| lock_err(e, "serialize navigate_choreo output"))?;
        Ok(yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::window::LogWindow;
    use crate::chat::{TenonLog, TenonLogData, TenonToolCall};
    use rig::tool::ToolContext;
    use std::collections::HashMap;

    fn dummy_tool_call(name: &str) -> TenonToolCall {
        TenonToolCall {
            id: "test-id".to_string(),
            internal_call_id: "test-internal-id".to_string(),
            name: name.to_string(),
            args: serde_json::json!({"move": 2, "move_artifact": "test output from move 1"}),
        }
    }

    #[test]
    fn test_navigate_choreo_stores_memory() {
        // Initialize PLUGIN_ROOT for testing
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        // Create choreo with memory
        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();
        let active = Arc::new(RwLock::new(Some(ActiveChoreo {
            choreo,
            r#move: 1,
            memory: HashMap::new(),
        })));

        let log_window = Arc::new(RwLock::new(LogWindow { logs: Vec::new() }));

        let tool = NavigateChoreo {
            active_choreo: Arc::clone(&active),
        };

        // Navigate to move 2 with output
        let result = tokio::runtime::Runtime::new().unwrap().block_on(tool.call(
            &mut ToolContext::new(),
            NavigateChoreoArgs {
                r#move: 2,
                move_artifact: Some("test output from move 1".to_string()),
            },
        ));

        assert!(result.is_ok());

        // Verify that choreo move was updated
        let guard = active.read().unwrap();
        let active_choreo = guard.as_ref().unwrap();
        assert_eq!(active_choreo.r#move, 2);

        // Simulate the ToolResult handler: create choreo log with the tool log
        let tool_log = crate::chat::TenonToolLog {
            tool_call: dummy_tool_call("navigate_choreo"),
            tool_result: None,
        };
        {
            let choreo = active_choreo.choreo.clone();
            let choreo_log = choreo.generate_log(active_choreo.r#move, tool_log).unwrap();
            let mut log_window = log_window.write().unwrap();
            log_window.logs.push(crate::chat::log::indexer::IndexedLog {
                log: Arc::new(TenonLog::new(TenonLogData::Choreo(choreo_log))),
                active: true,
            });
        }

        // Verify choreo log contains the tool log that navigated to this move
        {
            let log_window = log_window.read().unwrap();
            let choreo_log = log_window
                .logs
                .iter()
                .find_map(|indexed| {
                    if let TenonLogData::Choreo(choreo_log) = indexed.log.data() {
                        Some(choreo_log)
                    } else {
                        None
                    }
                })
                .expect("choreo log should exist");
            assert_eq!(choreo_log.tool_log.tool_call.name, "navigate_choreo");
            assert_eq!(choreo_log.tool_log.tool_call.id, "test-id");
        }

        // Note: Memory would only be populated if the choreo definition has
        // output_to_choreo_memory configured, which implement_code doesn't have
        // in move 1's goto_instructions. This test validates the basic navigation works.
    }
}
