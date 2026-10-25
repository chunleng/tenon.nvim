use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::approval::{ApprovalResult, request_approval};
use crate::utils::{has_hidden_component, is_within_directory, path_from_str};

use rig::tool::{ToolContext, ToolExecutionError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadFileArgs {
    pub filepath: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<usize>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct ReadFile {
    /// Base directory for resolving relative `filepath`s. `None` resolves against the process cwd.
    pub working_directory: Option<std::path::PathBuf>,
    /// When true, reject requests whose resolved path falls outside `working_directory`.
    pub restrict_to_working_directory: bool,
}

pub struct ReadFileCall {
    args: ReadFileArgs,
    working_directory: Option<std::path::PathBuf>,
    restrict_to_working_directory: bool,
}

impl ToolCoreCall for ReadFileCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
        let path = path_from_str(&args.filepath);
        let path = match &self.working_directory {
            Some(dir) if path.is_relative() => dir.join(path),
            _ => path,
        };
        if self.restrict_to_working_directory {
            // `None` restricts to the process cwd
            let base = match &self.working_directory {
                Some(dir) => dir.clone(),
                None => std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            };
            if !is_within_directory(&path, &base) {
                return Err(ToolExecutionError::invalid_args(format!(
                    "Path '{}' is outside the working directory '{}'",
                    path.display(),
                    base.display()
                )));
            }
        }

        // Use the raw `args.filepath` so that we check for hidden component based on whatever is
        // passed to the tool
        if has_hidden_component(&path_from_str(&args.filepath)) {
            let args_value = serde_json::to_value(&args)
                .map_err(|e| ToolExecutionError::other(format!("failed to serialize args: {e}")))?;

            if let ApprovalResult::Rejected(rejection) = request_approval(
                context,
                "read_file",
                args_value,
                format!("Attempt to read a hidden path: {}", args.filepath),
            )
            .await
            {
                return Err(ToolExecutionError::other(rejection));
            }
        }

        // Output contract: successful reads start with a `success: ...; content:` status line
        // followed by the content. Failures stay on the error channel (ToolExecutionError)
        // with an `error: ` prefix.
        match fs::read_to_string(path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();

                if total_lines == 0 {
                    return Ok("success: empty file".to_string());
                }

                let (start_line, end_line) = match (args.start_line, args.end_line) {
                    (Some(s), Some(e)) if e < s => (Some(e), Some(s)),
                    other => other,
                };

                let start = start_line.unwrap_or(1).saturating_sub(1);
                let end = end_line.unwrap_or(total_lines).min(total_lines);

                if start >= total_lines {
                    return Err(ToolExecutionError::invalid_args(format!(
                        "error: start_line {} > file_len {}",
                        start + 1,
                        total_lines
                    )));
                }

                let selected_lines = lines[start..end].join("\n");
                let status = if start + 1 == end {
                    format!("success: line {} read; content:", start + 1)
                } else {
                    format!("success: line {}-{} read; content:", start + 1, end)
                };

                Ok(format!("{status}\n{selected_lines}"))
            }
            Err(e) => Err(ToolExecutionError::other(format!(
                "error: read_file '{}': {}",
                args.filepath, e
            ))),
        }
    }
}

impl ToolCore for ReadFile {
    fn name(&self) -> String {
        "read_file".to_string()
    }
    type Error = ToolExecutionError;
    type Args = ReadFileArgs;
    type Output = String;
    type Call = ReadFileCall;

    fn description(&self) -> String {
        "Read file contents. Supports line ranges (1-based, inclusive; default: full file). \
         First line of the result is a status line (`success: ...; content:` for reads, \
         `error: ...` for failures); the second line onwards is the file content, verbatim."
            .to_string()
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
                    "description": "Start line (1-based)",
                    "default": 1
                },
                "end_line": {
                    "type": "number",
                    "description": "End line (1-based, inclusive). EOF if omitted"
                }
            },
            "required": ["filepath"]
        })
    }

    /// Read-only, no side effects
    fn properties(&self, _args: &ReadFileArgs) -> crate::chat::log::LogProperties {
        crate::chat::log::LogProperties {
            idempotent: true,
            important: false,
        }
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(ReadFileCall {
            args,
            working_directory: self.working_directory.clone(),
            restrict_to_working_directory: self.restrict_to_working_directory,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{EventChannel, PendingAction};
    use crate::utils::approval::{Approval, ApprovalResult, deny_message};
    use rig::tool::ToolContext;
    use std::sync::{Arc, Weak};

    fn write_temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("tenon_read_file_{}_{}", name, std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Like `write_temp_file`, but with a dot-prefixed filename so the path
    /// has a hidden component.
    fn write_hidden_temp_file(name: &str, content: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!(".tenon_read_file_{}_{}", name, std::process::id()));
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Spawns a read_file call with its own context bound to the channel,
    /// since the tool pushes the approval and awaits within a single call.
    fn spawn_read(
        weak: Option<Weak<EventChannel<PendingAction>>>,
        working_directory: Option<std::path::PathBuf>,
        filepath: String,
    ) -> tokio::task::JoinHandle<Result<String, ToolExecutionError>> {
        tokio::spawn(async move {
            let mut context = ToolContext::new();
            if let Some(weak) = weak {
                context = context.with_scope(Arc::new(weak));
            }
            let call = ReadFile {
                working_directory,
                ..Default::default()
            }
            .init_call(
                &mut context,
                ReadFileArgs {
                    filepath,
                    start_line: None,
                    end_line: None,
                },
            )
            .await
            .expect("init_call should succeed");
            call.result(&mut context).await
        })
    }

    async fn wait_for_approval(channel: &EventChannel<PendingAction>) -> PendingAction {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(action) = channel.peek() {
                    return action;
                }
                tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("approval action should be pushed")
    }

    /// Simulates the widget: on reject, sends the deny message computed from
    /// the action's own reject message, exactly as `send_result` does.
    fn respond(approval: Approval, approved: bool) {
        let tx = approval
            .response_tx
            .lock()
            .unwrap()
            .take()
            .expect("tx available");
        let result = if approved {
            ApprovalResult::Approved
        } else {
            ApprovalResult::Rejected(deny_message(&approval.reject_message))
        };
        tx.send(result).expect("send ok");
    }

    #[tokio::test]
    async fn test_swapped_start_end_returns_correct_range() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let path = write_temp_file("swapped", content);
        let filepath = path.to_string_lossy().to_string();

        let mut context = ToolContext::new();
        let call = ReadFile {
            working_directory: None,
            ..Default::default()
        }
        .init_call(
            &mut context,
            ReadFileArgs {
                filepath,
                start_line: Some(5),
                end_line: Some(2),
            },
        )
        .await
        .expect("init_call should succeed");
        let result = call.result(&mut context).await;

        std::fs::remove_file(&path).ok();
        let output = result.expect("should swap and return lines, not error");
        assert_eq!(
            output,
            "success: line 2-5 read; content:\nline2\nline3\nline4\nline5"
        );
    }

    #[tokio::test]
    async fn test_equal_start_end_returns_single_line() {
        let content = "line1\nline2\nline3";
        let path = write_temp_file("equal", content);
        let filepath = path.to_string_lossy().to_string();

        let mut context = ToolContext::new();
        let call = ReadFile {
            working_directory: None,
            ..Default::default()
        }
        .init_call(
            &mut context,
            ReadFileArgs {
                filepath,
                start_line: Some(2),
                end_line: Some(2),
            },
        )
        .await
        .expect("init_call should succeed");
        let result = call.result(&mut context).await;

        std::fs::remove_file(&path).ok();
        let output = result.expect("equal start/end is a valid single-line read");
        assert_eq!(output, "success: line 2 read; content:\nline2");
    }

    #[tokio::test]
    async fn test_empty_file_returns_success_marker() {
        let path = write_temp_file("empty", "");
        let filepath = path.to_string_lossy().to_string();

        let mut context = ToolContext::new();
        let call = ReadFile {
            working_directory: None,
            ..Default::default()
        }
        .init_call(
            &mut context,
            ReadFileArgs {
                filepath,
                start_line: None,
                end_line: None,
            },
        )
        .await
        .expect("init_call should succeed");
        let result = call.result(&mut context).await;

        std::fs::remove_file(&path).ok();
        let output = result.expect("empty file read should succeed");
        assert_eq!(output, "success: empty file");
    }

    #[tokio::test]
    async fn test_failures_return_tool_execution_error() {
        // Missing file: a real error, not in-band output
        let mut context = ToolContext::new();
        let call = ReadFile {
            working_directory: None,
            ..Default::default()
        }
        .init_call(
            &mut context,
            ReadFileArgs {
                filepath: "/nonexistent/tenon_read_file_missing".to_string(),
                start_line: None,
                end_line: None,
            },
        )
        .await
        .expect("init_call should succeed");
        let err = call
            .result(&mut context)
            .await
            .expect_err("missing file should return ToolExecutionError");
        assert!(
            err.to_string()
                .starts_with("error: read_file '/nonexistent/tenon_read_file_missing'"),
            "error should be prefixed with 'error: ' and name the filepath, got: {err}"
        );

        // Out-of-range start_line: same error contract
        let content = "line1\nline2\nline3";
        let path = write_temp_file("out_of_range", content);
        let filepath = path.to_string_lossy().to_string();
        let call = ReadFile {
            working_directory: None,
            ..Default::default()
        }
        .init_call(
            &mut context,
            ReadFileArgs {
                filepath,
                start_line: Some(10),
                end_line: None,
            },
        )
        .await
        .expect("init_call should succeed");
        let err = call
            .result(&mut context)
            .await
            .expect_err("out-of-range start_line should return ToolExecutionError");
        std::fs::remove_file(&path).ok();
        assert!(
            err.to_string().starts_with("error: "),
            "error should be prefixed with 'error: ', got: {err}"
        );
        assert!(
            err.to_string().contains("start_line"),
            "error should mention start_line, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_hidden_path_approval_gating() {
        let hidden_path = write_hidden_temp_file("gating", "secret content");
        let normal_path = write_temp_file("gating", "normal content");
        let hidden_filepath = hidden_path.to_string_lossy().to_string();
        let normal_filepath = normal_path.to_string_lossy().to_string();

        let channel = Arc::new(EventChannel::new());
        let weak = Arc::downgrade(&channel);

        // Approve: hidden path pushes an approval carrying the tool name,
        // args, and reject message, then returns the file content
        let task = spawn_read(Some(weak.clone()), None, hidden_filepath.clone());
        let PendingAction::Approval(approval) = wait_for_approval(&channel).await else {
            panic!("front pending action should be an approval");
        };
        assert_eq!(approval.tool, "read_file");
        assert!(
            approval.reject_message.contains(&hidden_filepath)
                && approval.reject_message.contains("hidden path"),
            "reject message should name the filepath and say it is a hidden path, got: {}",
            approval.reject_message
        );
        assert!(
            !approval.reject_message.starts_with("::tenon-denied::"),
            "reject message pushed to the widget must be plain, without the deny prefix"
        );
        respond(approval, true);
        let output = task.await.expect("task should not panic");
        assert_eq!(
            output.expect("approved read should succeed"),
            "success: line 1 read; content:\nsecret content"
        );
        channel.mark_done();

        // Reject: hidden path returns the prefixed deny message as an error
        let task = spawn_read(Some(weak.clone()), None, hidden_filepath.clone());
        let PendingAction::Approval(approval) = wait_for_approval(&channel).await else {
            panic!("front pending action should be an approval");
        };
        respond(approval, false);
        let err = task.await.expect("task should not panic").unwrap_err();
        assert!(
            err.to_string().starts_with("::tenon-denied::"),
            "widget denial must carry the ::tenon-denied:: prefix, got: {err}"
        );
        assert!(
            err.to_string().contains(&hidden_filepath),
            "rejection should surface the filepath, got: {err}"
        );
        channel.mark_done();

        // Non-hidden path: reads without pushing any approval
        let task = spawn_read(Some(weak), None, normal_filepath);
        let output = task.await.expect("task should not panic");
        assert_eq!(
            output.expect("non-hidden read should succeed"),
            "success: line 1 read; content:\nnormal content"
        );
        assert!(
            channel.peek().is_none(),
            "non-hidden path must not push an approval"
        );

        // Missing channel: hidden path defaults to reject
        let task = spawn_read(None, None, hidden_filepath.clone());
        let err = task.await.expect("task should not panic").unwrap_err();
        assert!(
            err.to_string().contains(&hidden_filepath),
            "missing channel must default to reject, got: {err}"
        );

        std::fs::remove_file(&hidden_path).ok();
        std::fs::remove_file(&normal_path).ok();
    }

    #[tokio::test]
    async fn test_relative_path_resolves_against_working_directory() {
        let dir = std::env::temp_dir().join(format!("tenon_read_file_wd_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("file.txt"), "wd content").unwrap();

        let mut context = ToolContext::new();
        let call = ReadFile {
            working_directory: Some(dir.clone()),
            ..Default::default()
        }
        .init_call(
            &mut context,
            ReadFileArgs {
                filepath: "file.txt".to_string(),
                start_line: None,
                end_line: None,
            },
        )
        .await
        .expect("init_call should succeed");
        let result = call.result(&mut context).await;

        std::fs::remove_dir_all(&dir).ok();
        let output = result.expect("relative path should resolve against working directory");
        assert_eq!(output, "success: line 1 read; content:\nwd content");
    }

    async fn run_read(
        working_directory: Option<std::path::PathBuf>,
        restrict_to_working_directory: bool,
        filepath: String,
    ) -> Result<String, ToolExecutionError> {
        let mut context = ToolContext::new();
        let call = ReadFile {
            working_directory,
            restrict_to_working_directory,
        }
        .init_call(
            &mut context,
            ReadFileArgs {
                filepath,
                start_line: None,
                end_line: None,
            },
        )
        .await
        .expect("init_call should succeed");
        call.result(&mut context).await
    }

    #[tokio::test]
    async fn test_restrict_to_working_directory() {
        let dir =
            std::env::temp_dir().join(format!("tenon_read_file_restrict_{}", std::process::id()));
        let other = std::env::temp_dir().join(format!(
            "tenon_read_file_restrict_other_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(dir.join("file.txt"), "inside content").unwrap();
        std::fs::write(other.join("other.txt"), "outside content").unwrap();

        // absolute path outside the working directory -> rejected
        let err = run_read(
            Some(dir.clone()),
            true,
            other.join("other.txt").to_string_lossy().to_string(),
        )
        .await
        .expect_err("absolute path outside working directory should be rejected");
        assert!(err.to_string().contains("outside the working directory"));

        // relative path escaping via `..` -> rejected
        let escape = format!(
            "../{}/other.txt",
            other.file_name().unwrap().to_string_lossy()
        );
        let err = run_read(Some(dir.clone()), true, escape)
            .await
            .expect_err("`..` escape should be rejected");
        assert!(err.to_string().contains("outside the working directory"));

        // relative path inside the working directory -> allowed
        let output = run_read(Some(dir.clone()), true, "file.txt".to_string())
            .await
            .expect("relative path inside working directory should be allowed");
        assert_eq!(output, "success: line 1 read; content:\ninside content");

        // no working directory configured -> restriction applies against cwd
        let err = run_read(
            None,
            true,
            other.join("other.txt").to_string_lossy().to_string(),
        )
        .await
        .expect_err("no working directory means restriction against cwd");
        assert!(err.to_string().contains("outside the working directory"));

        // path inside the cwd -> allowed
        let output = run_read(None, true, "src/lib.rs".to_string())
            .await
            .expect("path inside cwd should be allowed");
        assert!(output.starts_with("success:"), "got: {output}");

        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&other).ok();
    }

    #[tokio::test]
    async fn test_hidden_working_directory_allows_relative_path() {
        // Working directory itself is hidden; the raw relative filepath has no
        // hidden component, so no approval should be pushed.
        let dir =
            std::env::temp_dir().join(format!(".tenon_read_file_wd_hidden_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("file.txt"), "hidden wd content").unwrap();

        let channel = Arc::new(EventChannel::new());
        let weak = Arc::downgrade(&channel);
        let task = spawn_read(Some(weak), Some(dir.clone()), "file.txt".to_string());
        let output = task.await.expect("task should not panic");

        std::fs::remove_dir_all(&dir).ok();
        let output = output.expect("relative path in hidden working dir should read");
        assert_eq!(output, "success: line 1 read; content:\nhidden wd content");
        assert!(
            channel.peek().is_none(),
            "raw relative filepath without hidden component must not push an approval"
        );
    }
}
