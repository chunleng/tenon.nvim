use crate::utils::GLOBAL_EXECUTION_HANDLER;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
pub struct CreateFileArgs {
    pub filepath: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct CreateFile;

impl Tool for CreateFile {
    const NAME: &'static str = "create_file";
    type Error = ToolError;
    type Args = CreateFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "create_file".to_string(),
            description: "Create empty file. Error if exists.".to_string(),
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
        let path = Path::new(&args.filepath);

        if path.exists() {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("exists: '{}'", args.filepath),
            ))));
        }

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                        e.kind(),
                        format!("mkdir fail '{}': {}", args.filepath, e),
                    ))));
                }
            }
        }

        match fs::File::create_new(path) {
            Ok(_) => {
                let _ = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread("vim.cmd('checktime')");
                Ok(format!("created '{}'", args.filepath))
            }
            Err(e) => Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("create fail '{}': {}", args.filepath, e),
            )))),
        }
    }
}
