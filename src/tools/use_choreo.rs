use crate::chat::ActiveChoreo;
use crate::chat::choreo::Choreo;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::json;
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

impl Tool for UseChoreo {
    const NAME: &'static str = "use_choreo";
    type Error = ToolExecutionError;
    type Args = UseChoreoArgs;
    type Output = String;

    fn description(&self) -> String {
        let candidate_choreo = self
            .choreos
            .iter()
            .map(|c| format!("- {} — {}", c.id, c.description))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "Start a Tenon choreo. Check the choreos below: use if any description matches the task
             \n\nAvailable Choreo ID - description:
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

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
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

        // Set active choreo to move 1
        {
            let mut active_choreo_guard = self
                .active_choreo
                .write()
                .map_err(|e| lock_err(e, "write active_choreo"))?;
            *active_choreo_guard = Some(ActiveChoreo::new(choreo.clone(), 1));
        }

        Ok(format!(
            "Choreo '{}' ({}): {}.",
            choreo.id, choreo.title, choreo.moves[0].title
        ))
    }
}
