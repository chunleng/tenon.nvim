use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
pub struct ReadFileArgs {
    pub filepath: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
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
            description: "Read file contents. Supports line ranges.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filepath": {
                        "type": "string",
                        "description": "Path to file (absolute or relative)"
                    },
                    "start_line": {
                        "type": "number",
                        "description": "Start line (1-based). Default: 1"
                    },
                    "end_line": {
                        "type": "number",
                        "description": "End line (1-based, inclusive). Default: EOF"
                    }
                },
                "required": ["filepath"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = Path::new(&args.filepath);

        match fs::read_to_string(path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();

                let start = args.start_line.unwrap_or(1).saturating_sub(1);
                let end = args.end_line.unwrap_or(total_lines).min(total_lines);

                if start >= total_lines {
                    return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!(
                            "start_line {} exceeds file length of {} lines",
                            start + 1,
                            total_lines
                        ),
                    ))));
                }

                let selected_lines = lines[start..end].join("\n");

                Ok(format!(
                    "<file path=\"{}\">{}</file>",
                    path.to_str().unwrap_or_default().to_string(),
                    selected_lines
                ))
            }
            Err(e) => Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("Failed to read file '{}': {}", args.filepath, e),
            )))),
        }
    }
}
