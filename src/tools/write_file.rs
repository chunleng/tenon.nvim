use crate::utils::GLOBAL_EXECUTION_HANDLER;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
pub struct WriteFileArgs {
    pub filepath: String,
    pub content: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct WriteFile;

impl Tool for WriteFile {
    const NAME: &'static str = "write_file";
    type Error = ToolError;
    type Args = WriteFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".to_string(),
            description: "Write content to file. Creates if missing, overwrites if exists."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filepath": {
                        "type": "string",
                        "description": "File path (absolute or relative)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write"
                    }
                },
                "required": ["filepath", "content"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = Path::new(&args.filepath);

        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                if let Err(e) = fs::create_dir_all(parent) {
                    return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                        e.kind(),
                        format!("mkdir failed '{}': {}", args.filepath, e),
                    ))));
                }
            }
        }

        match fs::write(path, &args.content) {
            Ok(_) => {
                let byte_count = args.content.len();
                let _ = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread("vim.cmd('checktime')");
                Ok(format!("wrote {}B → '{}'", byte_count, args.filepath))
            }
            Err(e) => Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("write failed '{}': {}", args.filepath, e),
            )))),
        }
    }
}
