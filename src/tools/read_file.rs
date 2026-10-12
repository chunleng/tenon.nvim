use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::approval::{ApprovalResult, request_approval};
use crate::utils::{has_hidden_component, path_from_str};

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

#[derive(Deserialize, Serialize, Clone)]
pub struct ReadFile;

pub struct ReadFileCall {
    args: ReadFileArgs,
}

impl ToolCoreCall for ReadFileCall {
    type Error = ToolExecutionError;
    type Output = String;

    async fn result(self, context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args = self.args;
        let path = path_from_str(&args.filepath);

        if has_hidden_component(&path) {
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

        match fs::read_to_string(path) {
            Ok(content) => {
                let lines: Vec<&str> = content.lines().collect();
                let total_lines = lines.len();

                if total_lines == 0 {
                    return Ok(String::new());
                }

                let (start_line, end_line) = match (args.start_line, args.end_line) {
                    (Some(s), Some(e)) if e < s => (Some(e), Some(s)),
                    other => other,
                };

                let start = start_line.unwrap_or(1).saturating_sub(1);
                let end = end_line.unwrap_or(total_lines).min(total_lines);

                if start >= total_lines {
                    return Err(ToolExecutionError::invalid_args(format!(
                        "start_line {} > file_len {}",
                        start + 1,
                        total_lines
                    )));
                }

                let selected_lines = lines[start..end].join("\n");

                Ok(selected_lines)
            }
            Err(e) => Err(ToolExecutionError::other(format!(
                "read_file '{}': {}",
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
        "Read file contents. Supports line ranges (1-based, inclusive; default: full file). Empty string is returned if and only if the file exists and is empty. A missing file returns `Toolset error: ...`".to_string()
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

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(ReadFileCall { args })
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
        filepath: String,
    ) -> tokio::task::JoinHandle<Result<String, ToolExecutionError>> {
        tokio::spawn(async move {
            let mut context = ToolContext::new();
            if let Some(weak) = weak {
                context.insert(weak);
            }
            let call = ReadFile
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
        let call = ReadFile
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
        assert_eq!(output, "line2\nline3\nline4\nline5");
    }

    #[tokio::test]
    async fn test_equal_start_end_returns_single_line() {
        let content = "line1\nline2\nline3";
        let path = write_temp_file("equal", content);
        let filepath = path.to_string_lossy().to_string();

        let mut context = ToolContext::new();
        let call = ReadFile
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
        assert_eq!(output, "line2");
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
        let task = spawn_read(Some(weak.clone()), hidden_filepath.clone());
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
            "secret content"
        );
        channel.mark_done();

        // Reject: hidden path returns the prefixed deny message as an error
        let task = spawn_read(Some(weak.clone()), hidden_filepath.clone());
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
        let task = spawn_read(Some(weak), normal_filepath);
        let output = task.await.expect("task should not panic");
        assert_eq!(
            output.expect("non-hidden read should succeed"),
            "normal content"
        );
        assert!(
            channel.peek().is_none(),
            "non-hidden path must not push an approval"
        );

        // Missing channel: hidden path defaults to reject
        let task = spawn_read(None, hidden_filepath.clone());
        let err = task.await.expect("task should not panic").unwrap_err();
        assert!(
            err.to_string().contains(&hidden_filepath),
            "missing channel must default to reject, got: {err}"
        );

        std::fs::remove_file(&hidden_path).ok();
        std::fs::remove_file(&normal_path).ok();
    }
}
