use crate::utils::path_from_str;

use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
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

    fn description(&self) -> String {
        "Read file contents. Supports line ranges. Output starting with `Toolset error:` indicates error".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
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
        })
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let path = path_from_str(&args.filepath);

        match fs::read_to_string(path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();

                if total_lines == 0 {
                    return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("read_file '{}': file empty", args.filepath),
                    ))));
                }

                let (start_line, end_line) = match (args.start_line, args.end_line) {
                    (Some(s), Some(e)) if e < s => (Some(e), Some(s)),
                    other => other,
                };

                let start = start_line.unwrap_or(1).saturating_sub(1);
                let end = end_line.unwrap_or(total_lines).min(total_lines);

                if start >= total_lines {
                    return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("start_line {} > file_len {}", start + 1, total_lines),
                    ))));
                }

                let selected_lines = lines[start..end].join("\n");

                Ok(selected_lines)
            }
            Err(e) => Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("read_file '{}': {}", args.filepath, e),
            )))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("tenon_read_file_{}_{}", name, std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[tokio::test]
    async fn test_swapped_start_end_returns_correct_range() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let path = write_temp_file("swapped", content);
        let filepath = path.to_string_lossy().to_string();

        let result = ReadFile
            .call(ReadFileArgs {
                filepath,
                start_line: Some(5),
                end_line: Some(2),
            })
            .await;

        std::fs::remove_file(&path).ok();
        let output = result.expect("should swap and return lines, not error");
        assert_eq!(output, "line2\nline3\nline4\nline5");
    }

    #[tokio::test]
    async fn test_equal_start_end_returns_single_line() {
        let content = "line1\nline2\nline3";
        let path = write_temp_file("equal", content);
        let filepath = path.to_string_lossy().to_string();

        let result = ReadFile
            .call(ReadFileArgs {
                filepath,
                start_line: Some(2),
                end_line: Some(2),
            })
            .await;

        std::fs::remove_file(&path).ok();
        let output = result.expect("equal start/end is a valid single-line read");
        assert_eq!(output, "line2");
    }
}
