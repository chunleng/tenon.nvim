use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::GLOBAL_EXECUTION_HANDLER;
use crate::utils::path::TenonPath;

use rig::tool::{ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovePathArgs {
    pub filepath: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RemovePath;

pub struct RemovePathCall {
    args: RemovePathArgs,
}

impl ToolCoreCall for RemovePathCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
        let path: std::path::PathBuf = TenonPath::with_cwd(&args.filepath).into();

        if !path.exists() {
            return Err(ToolExecutionError::not_found(format!(
                "not found: '{}'",
                args.filepath
            )));
        }

        let result = if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };

        match result {
            Ok(()) => {
                let _ = GLOBAL_EXECUTION_HANDLER
                    .execute_rust_on_main_thread(|| Ok(nvim_oxi::api::command("checktime")?));
                Ok(format!("removed '{}'", args.filepath))
            }
            Err(e) => Err(ToolExecutionError::other(format!(
                "remove fail '{}': {}",
                args.filepath, e
            ))),
        }
    }
}

impl ToolCore for RemovePath {
    fn name(&self) -> String {
        "remove_path".to_string()
    }
    type Error = ToolExecutionError;
    type Args = RemovePathArgs;
    type Output = String;
    type Call = RemovePathCall;

    fn description(&self) -> String {
        "Delete file/dir. Error if missing.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "filepath": {
                    "type": "string",
                    "description": "Path"
                }
            },
            "required": ["filepath"]
        })
    }

    fn properties(&self, _args: &RemovePathArgs) -> crate::chat::log::LogProperties {
        crate::chat::log::LogProperties {
            idempotent: false,
            important: false,
        }
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(RemovePathCall { args })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_remove_path_not_found_error() {
        // Success paths call GLOBAL_EXECUTION_HANDLER, which panics outside
        // Neovim; the pre-handler not-found path is the testable contract
        let missing =
            std::env::temp_dir().join(format!("tenon_remove_missing_{}", std::process::id()));
        let mut context = ToolContext::new();
        let call = RemovePath
            .init_call(
                &mut context,
                RemovePathArgs {
                    filepath: missing.to_string_lossy().to_string(),
                },
            )
            .await
            .expect("init_call should succeed");
        let err = call
            .result(&mut context)
            .await
            .expect_err("missing path should error");
        assert!(
            err.to_string()
                .contains(&format!("not found: '{}'", missing.to_string_lossy())),
            "not-found error should name the raw filepath, got: {err}"
        );
    }
}
