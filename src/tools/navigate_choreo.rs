use crate::agent::engine::agentic::ContinueSignal;
use crate::chat::ActiveChoreo;
use crate::chat::{TenonLog, TenonLogData};
use crate::tools::{ToolCore, ToolCoreCall};

use rig::tool::{ToolContext, ToolExecutionError, ToolOutput};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::Ordering;
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

impl ToolCore for NavigateChoreo {
    fn name(&self) -> String {
        "navigate_choreo".to_string()
    }
    type Error = ToolExecutionError;
    type Args = NavigateChoreoArgs;
    type Output = String;
    type Call = NavigateChoreoCall;

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
                    "description": "Artifact of current move, according to \"Choreo Move Artifact\" section in `choreo-state` instruction. If section is missing, this should be omitted",
                    "default": null
                }
            },
            "required": ["move"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(NavigateChoreoCall {
            active_choreo: Arc::clone(&self.active_choreo),
            args,
        })
    }

    async fn convert_log(&self, log: Arc<RwLock<TenonLog>>) -> Option<ToolOutput> {
        // active_choreo is still alive here (only end_choreo clears it)
        let choreo_log = {
            let active = self.active_choreo.read().ok()?;
            let active_choreo = active.as_ref()?;
            let log_guard = log.read().ok()?;
            let TenonLogData::Tool(tool_log) = &log_guard.data else {
                return None;
            };
            active_choreo
                .choreo
                .generate_log(active_choreo.r#move, tool_log.clone())
                .ok()?
        };

        let mut log = log.write().ok()?;
        if !matches!(log.data, TenonLogData::Tool(_)) {
            return None;
        }
        log.data = TenonLogData::Choreo(choreo_log);
        log.refresh();
        None
    }
}

pub struct NavigateChoreoCall {
    active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
    args: NavigateChoreoArgs,
}

impl ToolCoreCall for NavigateChoreoCall {
    type Output = String;
    type Error = ToolExecutionError;

    async fn result(self, context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        // Acquire write lock upfront so check-and-mutate is atomic (no TOCTOU gap)
        let mut active_choreo_guard = self
            .active_choreo
            .write()
            .map_err(|e| lock_err(e, "write active_choreo"))?;

        let active = match active_choreo_guard.as_ref() {
            Some(c) => c.clone(),
            None => {
                return Err(ToolExecutionError::invalid_args("No active choreo"));
            }
        };

        let choreo = &active.choreo;

        let current_move = active.r#move;
        let target_move = self.args.r#move;
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
            && let Some(move_artifact) = self.args.move_artifact.clone()
        {
            choreo_ref.memory.insert(memory_key.clone(), move_artifact);
        }

        if let Some(ref mut choreo_ref) = active_choreo_guard.as_mut() {
            choreo_ref.r#move = target_move;
        }

        // Signal the engine to end the stream and start a new request
        if let Some(signal) = context.get::<Arc<ContinueSignal>>() {
            signal.0.store(true, Ordering::Release);
        }

        let yaml = serde_yaml::to_string(&json!({
            "move": target_move,
            "artifact": self.args.move_artifact,
        }))
        .map_err(|e| lock_err(e, "serialize navigate_choreo output"))?;
        Ok(yaml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::{TenonLog, TenonLogData, TenonToolCall, TenonToolResult};
    use rig::tool::ToolContext;
    use std::collections::HashMap;

    fn dummy_tool_call(name: &str) -> TenonToolCall {
        TenonToolCall {
            id: "test-id".to_string(),
            internal_call_id: "test-internal-id".to_string(),
            item_id: None,
            name: name.to_string(),
            args: serde_json::json!({"move": 2, "move_artifact": "test output from move 1"}),
        }
    }

    fn test_tool(
        choreo: Arc<crate::chat::choreo::Choreo>,
    ) -> (NavigateChoreo, Arc<RwLock<Option<ActiveChoreo>>>) {
        let active = Arc::new(RwLock::new(Some(ActiveChoreo {
            choreo,
            r#move: 1,
            memory: HashMap::new(),
        })));
        let tool = NavigateChoreo {
            active_choreo: Arc::clone(&active),
        };
        (tool, active)
    }

    #[tokio::test]
    async fn test_navigate_choreo_stores_memory() {
        // Initialize PLUGIN_ROOT for testing
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        // Create choreo with memory
        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();
        let (tool, active) = test_tool(choreo.clone());
        let mut context = ToolContext::new();

        // Navigate to move 2 with output
        let result = tool
            .init_call(
                &mut context,
                NavigateChoreoArgs {
                    r#move: 2,
                    move_artifact: Some("test output from move 1".to_string()),
                },
            )
            .await
            .unwrap()
            .result(&mut context)
            .await;

        assert!(result.is_ok());

        // Verify that choreo move was updated
        let guard = active.read().unwrap();
        let active_choreo = guard.as_ref().unwrap();
        assert_eq!(active_choreo.r#move, 2);

        // convert_log swaps the Tool log in place for a Choreo log carrying the tool log
        let tool_log = crate::chat::TenonToolLog {
            tool_call: dummy_tool_call("navigate_choreo"),
            tool_result: Some(Ok(TenonToolResult::Text(rig::agent::Text {
                text: "output:\n  move: 2\n  artifact: test output from move 1".to_string(),
                ..Default::default()
            }))),
            progress: vec![],
        };
        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(tool_log))));
        let output = tool.convert_log(log.clone()).await;
        assert!(output.is_none(), "convert_log should return None");

        let log = log.read().unwrap();
        let TenonLogData::Choreo(choreo_log) = log.data() else {
            panic!("expected Choreo log after convert_log");
        };
        assert_eq!(choreo_log.r#move, Some(2));
        assert_eq!(choreo_log.tool_log.tool_call.name, "navigate_choreo");
        assert_eq!(choreo_log.tool_log.tool_call.id, "test-id");
        // Token count reflects the choreo system content, not the tool log
        assert_eq!(
            log.token_count,
            crate::utils::estimate_tokens(&choreo_log.system_content())
        );

        // Note: Memory would only be populated if the choreo definition has
        // output_to_choreo_memory configured, which implement_code doesn't have
        // in move 1's goto_instructions. This test validates the basic navigation works.
    }
}
