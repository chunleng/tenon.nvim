use crate::utils::GLOBAL_EXECUTION_HANDLER;
use crate::utils::path_from_str;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFileArgs {
    pub filepath: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct CreateFile;

impl Tool for CreateFile {
    const NAME: &'static str = "create_file";
    type Error = ToolExecutionError;
    type Args = CreateFileArgs;
    type Output = String;

    fn description(&self) -> String {
        "Create empty file. Error if exists. Auto-creates parent dirs.".to_string()
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

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let path = path_from_str(&args.filepath);

        if path.exists() {
            return Err(ToolExecutionError::invalid_args(format!(
                "exists: '{}'",
                args.filepath
            )));
        }

        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && let Err(e) = fs::create_dir_all(parent)
        {
            return Err(ToolExecutionError::other(format!(
                "mkdir fail '{}': {}",
                args.filepath, e
            )));
        }

        match fs::File::create_new(path) {
            Ok(_) => {
                let _ = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread("vim.cmd('checktime')");
                Ok(format!("created '{}'", args.filepath))
            }
            Err(e) => Err(ToolExecutionError::other(format!(
                "create fail '{}': {}",
                args.filepath, e
            ))),
        }
    }
}
