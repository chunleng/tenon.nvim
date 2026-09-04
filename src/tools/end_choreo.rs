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
pub struct EndChoreoArgs {
    pub move_artifact: Option<String>,
}

#[derive(Clone)]
pub struct EndChoreo {
    pub active_choreo: Arc<RwLock<Option<ActiveChoreo>>>,
}

impl Tool for EndChoreo {
    const NAME: &'static str = "end_choreo";
    type Error = ToolExecutionError;
    type Args = EndChoreoArgs;
    type Output = String;

    fn description(&self) -> String {
        "End choreo. Use when complete".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "move_artifact": {
                    "type": "string",
                    "description": "Artifact of choreo, according to \"Choreo Move Artifact\" section. If section is missing, this should be omitted"
                }
            }
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let active_choreo_guard = self
            .active_choreo
            .read()
            .map_err(|e| lock_err(e, "read active_choreo"))?;

        if active_choreo_guard.is_none() {
            return Err(ToolExecutionError::invalid_args("No active choreo"));
        }

        Ok(format!(
            "choreo completed. artifact: {}",
            args.move_artifact.as_deref().unwrap_or("")
        ))
    }
}
