use crate::agent::engine::agentic::ContinueSignal;
use crate::chat::ActiveChoreo;
use crate::chat::choreo::Choreo;
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
pub struct UseChoreoArgs {
    pub choreo_id: String,
}

#[derive(Clone)]
pub struct UseChoreo {
    pub choreos: Vec<Arc<Choreo>>,
    pub active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
}

impl ToolCore for UseChoreo {
    fn name(&self) -> String {
        "use_choreo".to_string()
    }
    type Error = ToolExecutionError;
    type Args = UseChoreoArgs;
    type Output = String;
    type Call = UseChoreoCall;

    fn description(&self) -> String {
        let candidate_choreo = self
            .choreos
            .iter()
            .map(|c| format!("- {}: {}", c.id, c.description))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Start a Tenon choreo. Check the choreos below: use if any description matches the task
             \n\n**Available Choreo ID: description**
             \n{}",
            candidate_choreo
        )
    }

    fn parameters(&self) -> serde_json::Value {
        let choreo_ids = self
            .choreos
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        json!({
            "type": "object",
            "properties": {
                "choreo_id": {
                    "type": "string",
                    "enum": choreo_ids,
                    "description": "Choreo ID to use",
                }
            },
            "required": ["choreo_id"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        // Find the choreo by id from the agent's configured choreos
        let choreo = self
            .choreos
            .iter()
            .find(|c| c.id == args.choreo_id)
            .ok_or_else(|| {
                ToolExecutionError::invalid_args(format!(
                    "Choreo '{}' is not available for this agent. Available choreos: {}",
                    args.choreo_id,
                    self.choreos
                        .iter()
                        .map(|c| c.id.clone())
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?
            .clone();

        Ok(UseChoreoCall {
            choreo,
            active_choreo: Arc::clone(&self.active_choreo),
        })
    }

    async fn convert_log(&self, log: Arc<RwLock<TenonLog>>) -> Option<ToolOutput> {
        let choreo_log = {
            let active = self.active_choreo.read().ok()?;
            let active_choreo = active.as_ref()?;
            let TenonLogData::Tool(tool_log) = &log.read().ok()?.data else {
                return None;
            };
            active_choreo
                .choreo
                .generate_log(1, tool_log.clone())
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

pub struct UseChoreoCall {
    choreo: Arc<Choreo>,
    active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
}

impl ToolCoreCall for UseChoreoCall {
    type Output = String;
    type Error = ToolExecutionError;

    async fn result(self, context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        // Set active choreo to move 1. The Arc is shared with the engine, so
        // this mutation is visible outside the tool call.
        {
            let mut active_choreo_guard = self
                .active_choreo
                .write()
                .map_err(|e| lock_err(e, "write active_choreo"))?;
            *active_choreo_guard = Some(ActiveChoreo::new(self.choreo, 1));
        }

        // Signal the engine to end the stream and start a new request
        if let Some(signal) = context.get::<Arc<ContinueSignal>>() {
            signal.0.store(true, Ordering::Release);
        }

        Ok("done".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::{TenonLog, TenonLogData, TenonToolCall, TenonToolLog, TenonToolResult};
    use rig::agent::Text;

    fn test_tool() -> (UseChoreo, Arc<RwLock<Option<ActiveChoreo>>>) {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();
        let registry = crate::get_choreo_registry();
        let active_choreo: Arc<RwLock<Option<ActiveChoreo>>> = Arc::new(RwLock::new(None));
        let tool = UseChoreo {
            choreos: vec![registry.get("implement_code").unwrap().clone()],
            active_choreo: Arc::clone(&active_choreo),
        };
        (tool, active_choreo)
    }

    fn use_choreo_tool_log() -> TenonToolLog {
        TenonToolLog {
            tool_call: TenonToolCall {
                id: "call-1".to_string(),
                internal_call_id: "call-1".to_string(),
                item_id: None,
                name: "use_choreo".to_string(),
                args: serde_json::json!({"choreo_id": "implement_code"}),
            },
            tool_result: Some(Ok(TenonToolResult::Text(Text {
                text: "done".to_string(),
                ..Default::default()
            }))),
            progress: vec![],
        }
    }

    #[tokio::test]
    async fn test_use_choreo_sets_active_choreo() {
        let (tool, active) = test_tool();
        let mut context = ToolContext::new();

        let result = tool
            .init_call(
                &mut context,
                UseChoreoArgs {
                    choreo_id: "implement_code".to_string(),
                },
            )
            .await
            .unwrap()
            .result(&mut context)
            .await
            .unwrap();

        assert_eq!(result, "done");
        let guard = active.read().unwrap();
        let active_choreo = guard.as_ref().expect("active choreo should be set");
        assert_eq!(active_choreo.choreo.id, "implement_code");
        assert_eq!(active_choreo.r#move, 1);
    }

    #[tokio::test]
    async fn test_use_choreo_convert_log_converts_to_choreo_log() {
        let (tool, _active) = test_tool();
        let mut context = ToolContext::new();

        // Run the call first so active_choreo is set, as TenonTool::call would
        tool.init_call(
            &mut context,
            UseChoreoArgs {
                choreo_id: "implement_code".to_string(),
            },
        )
        .await
        .unwrap()
        .result(&mut context)
        .await
        .unwrap();

        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
            use_choreo_tool_log(),
        ))));

        // None keeps the original output; the conversion is silent
        let output = tool.convert_log(log.clone()).await;
        assert!(output.is_none(), "convert_log should return None");

        let log = log.read().unwrap();
        let TenonLogData::Choreo(choreo_log) = log.data() else {
            panic!("expected Choreo log after convert_log");
        };
        assert_eq!(choreo_log.id, "implement_code");
        assert_eq!(choreo_log.r#move, Some(1));
        assert_eq!(choreo_log.tool_log.tool_call.name, "use_choreo");
        // Token count reflects the choreo system content, not the tool log
        assert_eq!(
            log.token_count,
            crate::utils::estimate_tokens(&choreo_log.system_content())
        );
    }
}
