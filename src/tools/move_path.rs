use crate::utils::GLOBAL_EXECUTION_HANDLER;
use crate::utils::format_yaml_block_scalars;
use crate::utils::path_from_str;

use rig::tool::{Tool, ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_MOVED_ENTRIES: usize = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovePathArgs {
    pub source: String,
    pub destination: String,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct MovePath;

#[derive(Serialize)]
struct MovedEntry {
    from: PathBuf,
    to: PathBuf,
}

#[derive(Serialize)]
struct MoveResult {
    moved: Vec<MovedEntry>,
    count: usize,
    truncated: bool,
}

fn collect_files(source: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    if source.is_dir() {
        collect_files_recursive(source, source, &mut files)?;
    } else {
        files.push(source.to_path_buf());
    }
    Ok(files)
}

fn collect_files_recursive(
    _root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), std::io::Error> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(_root, &path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

impl Tool for MovePath {
    const NAME: &'static str = "move_path";
    type Error = ToolExecutionError;
    type Args = MovePathArgs;
    type Output = String;

    fn description(&self) -> String {
        "Move/rename file or dir. Dir dest → move into. Error if dest file exists.".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Source path"
                },
                "destination": {
                    "type": "string",
                    "description": "Destination path"
                }
            },
            "required": ["source", "destination"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let source = path_from_str(&args.source);

        // 1. Source must exist
        if !source.exists() {
            return Err(ToolExecutionError::not_found(format!(
                "not found: '{}'",
                args.source
            )));
        }

        let source_abs = source
            .canonicalize()
            .map_err(|e| ToolExecutionError::other(format!("resolve '{}': {}", args.source, e)))?;

        // 2. Resolve destination
        let dest = path_from_str(&args.destination);
        let actual_dest = if dest.exists() && dest.is_dir() {
            // Move into directory — append source filename
            dest.join(source.file_name().unwrap_or_default())
        } else if dest.exists() {
            return Err(ToolExecutionError::invalid_args(format!(
                "dest exists: '{}'",
                args.destination
            )));
        } else {
            dest.to_path_buf()
        };

        // 3. Guard source == destination
        if source_abs == actual_dest
            || source_abs
                .to_str()
                .zip(actual_dest.to_str())
                .is_some_and(|(s, d)| s == d)
        {
            return Err(ToolExecutionError::invalid_args(format!(
                "same path: '{}'",
                args.source
            )));
        }

        // 4. Create destination parent dirs if missing
        if let Some(parent) = actual_dest.parent()
            && !parent.as_os_str().is_empty()
            && let Err(e) = fs::create_dir_all(parent)
        {
            return Err(ToolExecutionError::other(format!(
                "mkdir fail: {} {}",
                args.destination, e
            )));
        }

        // 5. Collect files before move
        let source_files = collect_files(&source_abs)
            .map_err(|e| ToolExecutionError::other(format!("walk '{}': {}", args.source, e)))?;

        // 6. Attempt rename
        if let Err(e) = fs::rename(source, &actual_dest) {
            let msg = if e.kind() == std::io::ErrorKind::CrossesDevices {
                "cross-device".to_string()
            } else {
                format!("move fail: {} {}", args.source, e)
            };
            return Err(ToolExecutionError::other(msg));
        }

        // 7. Build structured output
        let dest_abs = actual_dest
            .canonicalize()
            .unwrap_or_else(|_| actual_dest.clone());
        let rel_prefix = if source_abs.is_dir() {
            source_abs.clone()
        } else {
            source_abs.parent().unwrap_or(Path::new("")).to_path_buf()
        };

        let mut moved_entries = Vec::new();
        for src_path in &source_files {
            let rel = src_path.strip_prefix(&rel_prefix).unwrap_or(src_path);
            let dest_path = if source_abs.is_dir() {
                dest_abs.join(rel)
            } else {
                dest_abs.clone()
            };
            moved_entries.push(MovedEntry {
                from: src_path.clone(),
                to: dest_path,
            });
        }

        let count = moved_entries.len();
        let truncated = count > MAX_MOVED_ENTRIES;
        moved_entries.truncate(MAX_MOVED_ENTRIES);

        let result = MoveResult {
            moved: moved_entries,
            count,
            truncated,
        };

        let output = format_yaml_block_scalars(
            &serde_yaml::to_string(&result).unwrap_or_else(|_| format!("moved {}", args.source)),
        );

        let _ = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread("vim.cmd('checktime')");

        Ok(output)
    }
}
