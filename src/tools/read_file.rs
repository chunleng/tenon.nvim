use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
pub struct ReadFileArgs {
    pub filepath: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct ReadFile;

impl Tool for ReadFile {
    const NAME: &'static str = "read_file";
    type Error = ToolError;
    type Args = ReadFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "read_file".to_string(),
            description: "Read the contents of a file from the filesystem. Returns the file content as a string.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filepath": {
                        "type": "string",
                        "description": "The path to the file to read. Can be absolute or relative to the current working directory."
                    }
                },
                "required": ["filepath"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = Path::new(&args.filepath);

        match fs::read_to_string(path) {
            Ok(content) => Ok(format!(
                "<file path=\"{}\">{}</file>",
                path.to_str().unwrap_or_default().to_string(),
                content
            )),
            Err(e) => Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("Failed to read file '{}': {}", args.filepath, e),
            )))),
        }
    }
}
