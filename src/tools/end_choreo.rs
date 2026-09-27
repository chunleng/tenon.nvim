use crate::agent::engine::agentic::ContinueSignal;
use crate::chat::ActiveChoreo;
use crate::chat::{TenonChoreoLog, TenonLog, TenonLogData, TenonToolResult};
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
pub struct EndChoreoArgs {
    pub move_artifact: Option<String>,
}

#[derive(Clone)]
pub struct EndChoreo {
    pub active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
}

impl ToolCore for EndChoreo {
    const NAME: &'static str = "end_choreo";
    type Error = ToolExecutionError;
    type Args = EndChoreoArgs;
    type Output = String;
    type Call = EndChoreoCall;

    fn description(&self) -> String {
        "End choreo. Use when complete".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "move_artifact": {
                    "type": "string",
                    "description": "Artifact of choreo, according to \"Choreo Move Artifact\" section in `choreo-state` instruction. If section is missing, this should be omitted",
                    "default": null
                }
            }
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        let active_choreo_guard = self
            .active_choreo
            .read()
            .map_err(|e| lock_err(e, "read active_choreo"))?;

        if active_choreo_guard.is_none() {
            return Err(ToolExecutionError::invalid_args("No active choreo"));
        }

        Ok(EndChoreoCall {
            active_choreo: Arc::clone(&self.active_choreo),
            move_artifact: args.move_artifact,
        })
    }

    async fn convert_log(&self, log: Arc<RwLock<TenonLog>>) -> Option<ToolOutput> {
        let (choreo_log, artifact) = {
            let TenonLogData::Tool(tool_log) = &log.read().ok()?.data else {
                return None;
            };
            // The result carries the choreo id as JSON; active_choreo is already cleared by
            // result()
            let result_text = match &tool_log.tool_result {
                Some(Ok(TenonToolResult::Text(text))) => &text.text,
                _ => return None,
            };
            let value: serde_json::Value = serde_json::from_str(result_text).ok()?;
            let id = value.get("ended_choreo")?.as_str()?.to_string();
            let artifact = value
                .get("artifact")
                .and_then(|a| a.as_str())
                .map(|s| s.to_string());
            (
                TenonChoreoLog::new(id, "Choreo ended", None, tool_log.clone()),
                artifact,
            )
        };

        let mut log = log.write().ok()?;
        if !matches!(log.data, TenonLogData::Tool(_)) {
            return None;
        }

        log.data = TenonLogData::Choreo(choreo_log);
        log.refresh();

        // Convert the output back to the friendly text for the model
        Some(ToolOutput::text(format!(
            "choreo completed. artifact: {}",
            artifact.as_deref().unwrap_or("")
        )))
    }
}

pub struct EndChoreoCall {
    active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
    move_artifact: Option<String>,
}

impl ToolCoreCall for EndChoreoCall {
    type Output = String;
    type Error = ToolExecutionError;

    async fn result(self, context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        // Clear active_choreo;  read the id needed for the ChoreoLog conversion
        let id = {
            let mut active = self
                .active_choreo
                .write()
                .map_err(|e| lock_err(e, "write active_choreo"))?;
            let id = active
                .as_ref()
                .map(|c| c.choreo.id.clone())
                .unwrap_or_default();
            *active = None;
            id
        };

        // Signal the engine to end the stream and start a new request
        if let Some(signal) = context.get::<Arc<ContinueSignal>>() {
            signal.0.store(true, Ordering::Release);
        }

        Ok(serde_json::json!({
            "ended_choreo": id,
            "artifact": self.move_artifact,
        })
        .to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::{TenonLog, TenonLogData, TenonToolCall, TenonToolLog, TenonToolResult};
    use rig::agent::Text;
    use std::collections::HashMap;

    fn test_tool() -> (EndChoreo, Arc<RwLock<Option<ActiveChoreo>>>) {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();
        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();
        let active = Arc::new(RwLock::new(Some(ActiveChoreo {
            choreo,
            r#move: 1,
            memory: HashMap::new(),
        })));
        let tool = EndChoreo {
            active_choreo: Arc::clone(&active),
        };
        (tool, active)
    }

    fn end_choreo_tool_log() -> TenonToolLog {
        TenonToolLog {
            tool_call: TenonToolCall {
                id: "call-1".to_string(),
                internal_call_id: "call-1".to_string(),
                item_id: None,
                name: "end_choreo".to_string(),
                args: serde_json::json!({"move_artifact": "final summary of work"}),
            },
            tool_result: Some(Ok(TenonToolResult::Text(Text {
                text: serde_json::json!({
                    "ended_choreo": "implement_code",
                    "artifact": "final summary of work",
                })
                .to_string(),
                ..Default::default()
            }))),
            progress: vec![],
        }
    }

    #[tokio::test]
    async fn test_end_choreo_succeeds_with_active_choreo() {
        let (tool, active) = test_tool();
        let mut context = ToolContext::new();

        let result = tool
            .init_call(
                &mut context,
                EndChoreoArgs {
                    move_artifact: Some("final summary of work".to_string()),
                },
            )
            .await
            .unwrap()
            .result(&mut context)
            .await
            .unwrap();

        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "ended_choreo": "implement_code",
                "artifact": "final summary of work",
            })
        );
        // result clears active_choreo after reading the id
        assert!(active.read().unwrap().is_none());
    }

    #[tokio::test]
    async fn test_end_choreo_convert_log_converts_to_choreo_log() {
        let (tool, active) = test_tool();
        let expected_id = active.read().unwrap().as_ref().unwrap().choreo.id.clone();

        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
            end_choreo_tool_log(),
        ))));

        // The output is converted back to the friendly text for the model
        let output = tool
            .convert_log(log.clone())
            .await
            .expect("convert_log should convert the output");
        assert_eq!(
            output.as_text(),
            Some("choreo completed. artifact: final summary of work")
        );

        let log = log.read().unwrap();
        let TenonLogData::Choreo(choreo_log) = log.data() else {
            panic!("expected Choreo log after convert_log");
        };
        assert_eq!(choreo_log.id, expected_id);
        assert_eq!(choreo_log.content, "Choreo ended");
        assert_eq!(choreo_log.r#move, None);
        assert_eq!(choreo_log.tool_log.tool_call.name, "end_choreo");
        // Token count reflects the choreo system content, not the tool log
        assert_eq!(
            log.token_count,
            crate::utils::estimate_tokens(&choreo_log.system_content())
        );
    }
}
