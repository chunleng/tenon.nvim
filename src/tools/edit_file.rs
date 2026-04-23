use crate::utils::GLOBAL_EXECUTION_HANDLER;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::Path;

#[derive(Deserialize)]
pub struct EditFileArgs {
    pub filepath: String,
    pub search: String,
    pub replace: String,
    pub replace_mode: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct EditFile;

impl Tool for EditFile {
    const NAME: &'static str = "edit_file";
    type Error = ToolError;
    type Args = EditFileArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "edit_file".to_string(),
            description: "Find text → replace. Error if 'one' mode hits multiple matches."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "filepath": {
                        "type": "string",
                        "description": "Path"
                    },
                    "search": {
                        "type": "string",
                        "description": "Search text"
                    },
                    "replace": {
                        "type": "string",
                        "description": "Replace with"
                    },
                    "replace_mode": {
                        "type": "string",
                        "enum": ["one", "all"],
                        "description": "one = first match (error if >1 found). all = every match"
                    }
                },
                "required": ["filepath", "search", "replace"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let replace_mode = args.replace_mode.unwrap_or_else(|| "one".to_string());
        let path = Path::new(&args.filepath);

        let content = fs::read_to_string(path).map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("Read fail '{}': {}", args.filepath, e),
            )))
        })?;

        let match_count = content.matches(&args.search).count();

        if match_count == 0 {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No match in '{}'", args.filepath),
            ))));
        }

        if replace_mode == "one" && match_count > 1 {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("{} matches. Use 'all' or narrow search", match_count),
            ))));
        }

        let new_content = match replace_mode.as_str() {
            "one" => content.replacen(&args.search, &args.replace, 1),
            "all" => content.replace(&args.search, &args.replace),
            _ => {
                return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Bad mode '{}'. Use 'one' or 'all'", replace_mode),
                ))));
            }
        };

        fs::write(path, &new_content).map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("Write fail '{}': {}", args.filepath, e),
            )))
        })?;

        let _ = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread("vim.cmd('checktime')");

        Ok(format!(
            "Replaced {} in '{}'",
            if replace_mode == "one" {
                1
            } else {
                match_count
            },
            args.filepath
        ))
    }
}
