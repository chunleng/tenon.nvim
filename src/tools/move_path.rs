use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::GLOBAL_EXECUTION_HANDLER;
use crate::utils::format_yaml_block_scalars;
use crate::utils::path::TenonPath;

use rig::tool::{ToolContext, ToolExecutionError};
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

pub struct MovePathCall {
    args: MovePathArgs,
}

impl ToolCoreCall for MovePathCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
        let source: PathBuf = TenonPath::with_cwd(&args.source).into();

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
        let dest: PathBuf = TenonPath::with_cwd(&args.destination).into();
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

        let _ = GLOBAL_EXECUTION_HANDLER
            .execute_rust_on_main_thread(|| Ok(nvim_oxi::api::command("checktime")?));

        Ok(output)
    }
}

impl ToolCore for MovePath {
    fn name(&self) -> String {
        "move_path".to_string()
    }
    type Error = ToolExecutionError;
    type Args = MovePathArgs;
    type Output = String;
    type Call = MovePathCall;

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

    fn properties(&self, _args: &MovePathArgs) -> crate::chat::log::LogProperties {
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
        Ok(MovePathCall { args })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Success paths call GLOBAL_EXECUTION_HANDLER, which panics outside
    // Neovim; only the pre-handler error paths are testable here
    async fn run_move(source: String, destination: String) -> Result<String, ToolExecutionError> {
        let mut context = ToolContext::new();
        let call = MovePath
            .init_call(
                &mut context,
                MovePathArgs {
                    source,
                    destination,
                },
            )
            .await
            .expect("init_call should succeed");
        call.result(&mut context).await
    }

    #[tokio::test]
    async fn test_move_path_error_contracts() {
        // Missing source: not-found error names the raw source argument
        let missing =
            std::env::temp_dir().join(format!("tenon_move_missing_{}", std::process::id()));
        let missing_str = missing.to_string_lossy().to_string();
        let expected = format!("not found: '{missing_str}'");
        let err = run_move(missing_str.clone(), missing_str)
            .await
            .expect_err("missing source should error");
        assert!(
            err.to_string().contains(&expected),
            "not-found error should name the raw source, got: {err}"
        );

        // Existing destination file: invalid-args error names the raw
        // destination
        let src = std::env::temp_dir().join(format!("tenon_move_src_{}", std::process::id()));
        std::fs::write(&src, "content").unwrap();
        let existing_dest =
            std::env::temp_dir().join(format!("tenon_move_dest_{}", std::process::id()));
        std::fs::write(&existing_dest, "occupied").unwrap();
        let err = run_move(
            src.to_string_lossy().to_string(),
            existing_dest.to_string_lossy().to_string(),
        )
        .await
        .expect_err("existing destination file should error");
        assert!(
            err.to_string().contains(&format!(
                "dest exists: '{}'",
                existing_dest.to_string_lossy()
            )),
            "dest-exists error should name the raw destination, got: {err}"
        );
        std::fs::remove_file(&src).ok();
        std::fs::remove_file(&existing_dest).ok();
    }
}
