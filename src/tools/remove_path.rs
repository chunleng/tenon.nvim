use crate::utils::GLOBAL_EXECUTION_HANDLER;
use crate::utils::path_from_str;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
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

impl Tool for RemovePath {
    const NAME: &'static str = "remove_path";
    type Error = ToolError;
    type Args = RemovePathArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "remove_path".to_string(),
            description: "Delete file/dir. Error if missing.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filepath": {
                        "type": "string",
                        "description": "Path"
                    }
                },
                "required": ["filepath"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = path_from_str(&args.filepath);

        if !path.exists() {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("not found: '{}'", args.filepath),
            ))));
        }

        let result = if path.is_dir() {
            fs::remove_dir_all(path)
        } else {
            fs::remove_file(path)
        };

        match result {
            Ok(()) => {
                let _ = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread("vim.cmd('checktime')");
                Ok(format!("removed '{}'", args.filepath))
            }
            Err(e) => Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("remove fail '{}': {}", args.filepath, e),
            )))),
        }
    }
}
