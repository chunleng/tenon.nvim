use crate::agent::worker::SimpleTenonWorkerAgent;
use crate::get_application_config;
use crate::utils::format_yaml_block_scalars;
use futures::stream::{self, StreamExt};

use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Hard cap on combined stdout+stderr output size (bytes).
const OUTPUT_CAP: usize = 32 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommandArgs {
    pub command: String,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
    pub timeout: Option<u64>,
    pub filter: Option<String>,
    pub head: Option<usize>,
    pub tail: Option<usize>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct RunCommand;

#[derive(Serialize)]
struct RunCommandOutput {
    exit_code: i32,
    stdout: String,
    stderr: String,
    truncated: bool,
}

/// Arg allowance for a whitelist pattern.
enum ArgAllowance {
    /// No additional arguments beyond pattern tokens.
    Exact,
    /// Exactly one additional argument allowed.
    OneArg,
    /// Any number of additional arguments allowed.
    AnyArgs,
}

/// Parse a whitelist pattern into (command_tokens, arg_allowance).
///
/// - `"make"`       → `(["make"], Exact)`   — exact, no args
/// - `"make ?"`    → `(["make"], OneArg)`   — one arg only
/// - `"make *"`    → `(["make"], AnyArgs)`  — any number of args
/// - `"git log"`   → `(["git", "log"], Exact)` — exact subcommand
/// - `"git log *"` → `(["git", "log"], AnyArgs)` — subcommand with any args
fn parse_whitelist_pattern(pattern: &str) -> (Vec<String>, ArgAllowance) {
    let trimmed = pattern.trim();
    let tokens: Vec<String> = shlex::split(trimmed).unwrap_or_default();

    let allowance = if trimmed == "*" || trimmed.ends_with(" *") {
        ArgAllowance::AnyArgs
    } else if trimmed.ends_with(" ?") {
        ArgAllowance::OneArg
    } else {
        ArgAllowance::Exact
    };

    let mut cmd_tokens = tokens;
    // Strip the trailing wildcard token (* or ?) if present
    match cmd_tokens.last().map(|t| t.as_str()) {
        Some("*") | Some("?") => {
            cmd_tokens.pop();
        }
        _ => {}
    }

    (cmd_tokens, allowance)
}

/// Check if a parsed command matches any whitelist pattern.
fn command_matches_whitelist(command_tokens: &[String], whitelist: &[String]) -> bool {
    for pattern in whitelist {
        let (pattern_tokens, allowance) = parse_whitelist_pattern(pattern);

        // Command must have at least as many tokens as the pattern
        if command_tokens.len() < pattern_tokens.len() {
            continue;
        }

        // All leading tokens must match exactly
        let leading_match = command_tokens
            .iter()
            .zip(pattern_tokens.iter())
            .all(|(a, b)| a == b);

        if !leading_match {
            continue;
        }

        let extra_args = command_tokens.len() - pattern_tokens.len();

        let matches = match allowance {
            ArgAllowance::Exact => extra_args == 0,
            ArgAllowance::OneArg => extra_args == 1,
            ArgAllowance::AnyArgs => true,
        };

        if matches {
            return true;
        }
    }
    false
}

/// Response from LLM command safety check.
#[derive(Debug, Deserialize)]
struct CommandSafetyResponse {
    decision: String,
    reason: Option<String>,
}

/// Check if a command is safe to execute using LLM.
/// Returns Ok(true) if allowed, Ok(false) with reason if denied, or Err on failure.
async fn check_command_safety_with_llm(
    command: &str,
    model: &crate::clients::SupportedModels,
) -> Result<(bool, Option<String>), ToolError> {
    let worker = SimpleTenonWorkerAgent::new(
        Some(model.clone()),
        r#"Judge command safety. Output YAML only.

DENY patterns:
- Secrets: env vars (*KEY*, *SECRET*, *TOKEN*, *API*), files (.env, id_rsa, credentials, .pem)
- System modify: install packages, system config, services
- Delete: rm, rmdir, unlink, rmtree, shred
- Network: curl, wget, nc, netcat, http requests
- Code exec: eval, exec, source untrusted scripts
- Permissions: chmod, chown, setuid
- Process kill: kill, pkill, killall
- Sensitive paths: /etc/passwd, /etc/shadow, ~/.ssh, /root

ALLOW patterns:
- Read files: cat, head, tail, grep (non-sensitive paths only)
- List directory: ls, tree, find
- VCS read-only: git status, git log, git diff
- Build/test: make, cargo build, npm test
- Info: which, whereis, echo

Judge by similarity to patterns above. Commands matching DENY patterns → deny. Commands matching ALLOW patterns → allow. When uncertain, deny.

Output (allow):
decision: allow

Output (deny):
decision: deny
reason: ..."#,
        true,
    )
    .map_err(|e| {
        ToolError::ToolCallError(Box::new(e))
    })?;

    let user_message = format!("Command: {}", command);

    let response = worker.chat(user_message).await.map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::other(format!(
            "LLM safety check failed: {}",
            e
        ))))
    })?;

    // Parse YAML response
    let safety: CommandSafetyResponse = serde_yaml::from_str(&response).map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Failed to parse LLM response as YAML: {} (response: {})",
                e, response
            ),
        )))
    })?;

    let allowed = safety.decision == "allow";
    Ok((allowed, safety.reason))
}

/// Check command safety using one LLM call per model in parallel.
/// All models must allow for the command to proceed.
/// Returns Ok(()) if allowed, or Err with the first denial reason.
async fn check_command_safety(command: &str) -> Result<(), ToolError> {
    let config = get_application_config();

    let models = &config.tools.run_command.check_models;
    if models.is_empty() {
        return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "Command not in whitelist and no check_models configured for LLM safety check"
                .to_string(),
        ))));
    }

    // Run checks in parallel, process results as they arrive
    let checks: Vec<_> = models
        .iter()
        .map(|model| {
            let model = model.clone();
            let command = command.to_string();
            async move {
                let mut last_error = None;
                for _ in 0..3 {
                    match check_command_safety_with_llm(&command, &model).await {
                        Ok(result) => return Ok(result),
                        Err(e) => last_error = Some(e),
                    }
                }
                Err(last_error.unwrap_or_else(|| {
                    ToolError::ToolCallError(Box::new(std::io::Error::other(
                        "LLM safety check failed after 3 attempts",
                    )))
                }))
            }
        })
        .collect();

    let mut stream = stream::iter(checks).buffer_unordered(models.len());

    while let Some(result) = stream.next().await {
        match result {
            Ok((allowed, reason)) => {
                if !allowed {
                    return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        format!(
                            "Command denied by safety check: {}",
                            reason.unwrap_or_else(|| "Unknown reason".to_string())
                        ),
                    ))));
                }
                // allowed, continue checking other models
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

/// Apply filter, head, and tail to output lines.
fn apply_output_filters(
    output: &str,
    filter: Option<&str>,
    head: Option<usize>,
    tail: Option<usize>,
) -> String {
    let mut lines: Vec<&str> = output.lines().collect();

    if let Some(f) = filter {
        lines.retain(|line| line.contains(f));
    }

    if let Some(n) = head {
        lines.truncate(n);
    } else if let Some(n) = tail {
        // Keep last n lines
        let start = if lines.len() > n { lines.len() - n } else { 0 };
        lines = lines.drain(start..).collect();
    }

    lines.join("\n")
}

/// Truncate output to OUTPUT_CAP bytes.
/// Returns (truncated_output, was_truncated).
fn truncate_output(output: &str) -> (String, bool) {
    if output.len() <= OUTPUT_CAP {
        return (output.to_string(), false);
    }

    (output[..OUTPUT_CAP].to_string(), true)
}

impl Tool for RunCommand {
    const NAME: &'static str = "run_command";
    type Error = ToolError;
    type Args = RunCommandArgs;
    type Output = String;

    fn description(&self) -> String {
        "Run command (exec form). Pipes and redirects are not allowed (e.g. `2>&1`, `> out.txt`). Tool outputs yaml with both stdout and stderr.\nE.g.\n`git log` → command='git', args=['log'].\n`make 2>&1` → command='make' (drop `2>&1` as error is always output)\n`cat ./in.txt|grep foo` → Run for `cat` command and think of alternative for `grep`"
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Executable. E.g. 'git', 'make'."
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Args. E.g. ['log', '--oneline']."
                },
                "cwd": {
                    "type": "string",
                    "description": "Working dir. Default: cwd."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout (sec). Default: 30."
                },
                "filter": {
                    "type": "string",
                    "description": "Filter lines containing substring."
                },
                "head": {
                    "type": "integer",
                    "description": "Keep first N lines. Exclusive with tail."
                },
                "tail": {
                    "type": "integer",
                    "description": "Keep last N lines. Exclusive with head."
                },
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Env vars."
                }
            },
            "required": ["command"]
        })
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Validate mutual exclusivity of head and tail
        if args.head.is_some() && args.tail.is_some() {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Cannot set both 'head' and 'tail'. Use only one.",
            ))));
        }

        // Command is required and must not be empty
        if args.command.trim().is_empty() {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Empty command".to_string(),
            ))));
        }

        // Build command tokens for whitelist matching: [command, ...args]
        let mut command_tokens = vec![args.command.clone()];
        if let Some(ref cmd_args) = args.args {
            command_tokens.extend(cmd_args.clone());
        }

        // Check whitelist
        let config = get_application_config();
        let whitelist = &config.tools.run_command.whitelist;

        if !command_matches_whitelist(&command_tokens, whitelist) {
            // Whitelist doesn't match - use LLM to check if command is safe
            // Combine command + args for the check
            let full_command = if let Some(ref cmd_args) = args.args {
                format!("{} {}", args.command, cmd_args.join(" "))
            } else {
                args.command.clone()
            };
            check_command_safety(&full_command).await?;
        }

        // Build the process
        let program = &args.command;
        let program_args = args.args.as_deref().unwrap_or(&[]);

        let timeout_secs = args.timeout.unwrap_or(30);

        let mut cmd = Command::new(program);
        cmd.args(program_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(ref cwd) = args.cwd {
            cmd.current_dir(cwd);
        }

        if let Some(ref env) = args.env {
            cmd.envs(env);
        }

        let child = cmd.spawn().map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                e.kind(),
                format!("Failed to spawn '{}': {}", program, e),
            )))
        })?;

        // Run with timeout
        let result =
            tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await;

        let output = match result {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                    e.kind(),
                    format!("Process error: {}", e),
                ))));
            }
            Err(_) => {
                // Timeout — try to get partial output by killing
                return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!(
                        "Command timed out after {}s: '{}'",
                        timeout_secs, args.command
                    ),
                ))));
            }
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let raw_stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let raw_stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Apply output filters to both stdout and stderr
        let filtered_stdout =
            apply_output_filters(&raw_stdout, args.filter.as_deref(), args.head, args.tail);
        let filtered_stderr =
            apply_output_filters(&raw_stderr, args.filter.as_deref(), args.head, args.tail);

        // Truncate each stream individually to OUTPUT_CAP
        let (truncated_stdout, stdout_was_truncated) = truncate_output(&filtered_stdout);
        let (truncated_stderr, stderr_was_truncated) = truncate_output(&filtered_stderr);
        let truncated = stdout_was_truncated || stderr_was_truncated;

        let result = RunCommandOutput {
            exit_code,
            stdout: truncated_stdout,
            stderr: truncated_stderr,
            truncated,
        };

        Ok(format_yaml_block_scalars(
            &serde_yaml::to_string(&result).unwrap_or_else(|_| {
                "exit_code: -1\nstdout: \"\"\nstderr: \"\"\ntruncated: false\n".to_string()
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_whitelist_exact() {
        let (tokens, allowance) = parse_whitelist_pattern("make");
        assert_eq!(tokens, vec!["make"]);
        assert!(matches!(allowance, ArgAllowance::Exact));
    }

    #[test]
    fn test_parse_whitelist_one_arg() {
        let (tokens, allowance) = parse_whitelist_pattern("make ?");
        assert_eq!(tokens, vec!["make"]);
        assert!(matches!(allowance, ArgAllowance::OneArg));
    }

    #[test]
    fn test_parse_whitelist_any_args() {
        let (tokens, allowance) = parse_whitelist_pattern("git log *");
        assert_eq!(tokens, vec!["git", "log"]);
        assert!(matches!(allowance, ArgAllowance::AnyArgs));
    }

    #[test]
    fn test_command_matches_whitelist_with_separated_args() {
        // Test: command + args should match whitelist patterns
        // This test will FAIL before the change (function signature mismatch)
        // and PASS after implementing command + args separation

        let whitelist = vec!["git log *".to_string(), "make".to_string()];

        // git log with args should match "git log *"
        let command = "git".to_string();
        let args = vec!["log".to_string(), "--oneline".to_string()];
        let combined: Vec<String> = std::iter::once(command.clone())
            .chain(args.clone())
            .collect();
        assert!(command_matches_whitelist(&combined, &whitelist));

        // make without args should match "make"
        let command = "make".to_string();
        let args: Vec<String> = vec![];
        let combined: Vec<String> = std::iter::once(command.clone())
            .chain(args.clone())
            .collect();
        assert!(command_matches_whitelist(&combined, &whitelist));
    }

    #[test]
    fn test_head_keeps_first_n_lines() {
        let stdout = "line1\nline2\nline3\nline4\nline5";
        let result = apply_output_filters(stdout, None, Some(3), None);
        assert_eq!(result, "line1\nline2\nline3");
    }

    #[test]
    fn test_tail_keeps_last_n_lines() {
        let stdout = "line1\nline2\nline3\nline4\nline5";
        let result = apply_output_filters(stdout, None, None, Some(3));
        assert_eq!(result, "line3\nline4\nline5");
    }

    #[test]
    fn test_filter_with_head() {
        let stdout = "error line1\ninfo line2\nerror line3\ninfo line4\nerror line5";
        let result = apply_output_filters(stdout, Some("error"), Some(2), None);
        assert_eq!(result, "error line1\nerror line3");
    }

    #[test]
    fn test_filter_with_tail() {
        let stdout = "error line1\ninfo line2\nerror line3\ninfo line4\nerror line5";
        let result = apply_output_filters(stdout, Some("error"), None, Some(2));
        assert_eq!(result, "error line3\nerror line5");
    }

    #[test]
    fn test_truncate_output_under_cap() {
        let output = "line1\nline2\nline3";
        let (result, truncated) = truncate_output(output);
        assert_eq!(result, "line1\nline2\nline3");
        assert!(!truncated);
    }

    #[test]
    fn test_truncate_output_over_cap() {
        let output = "x".repeat(100_000);
        let (result, truncated) = truncate_output(&output);
        assert!(truncated);
        assert_eq!(result.len(), OUTPUT_CAP);
    }

    #[test]
    fn test_standalone_wildcard_allows_all_commands() {
        // Pattern "*" should match any command (allow all)
        let whitelist = vec!["*".to_string()];

        // Single command without args
        let combined = vec!["make".to_string()];
        assert!(
            command_matches_whitelist(&combined, &whitelist),
            "Pattern '*' should match 'make'"
        );

        // Command with args
        let combined = vec![
            "git".to_string(),
            "log".to_string(),
            "--oneline".to_string(),
        ];
        assert!(
            command_matches_whitelist(&combined, &whitelist),
            "Pattern '*' should match 'git log --oneline'"
        );

        // Any arbitrary command
        let combined = vec![
            "cargo".to_string(),
            "build".to_string(),
            "--release".to_string(),
        ];
        assert!(
            command_matches_whitelist(&combined, &whitelist),
            "Pattern '*' should match 'cargo build --release'"
        );
    }
}
