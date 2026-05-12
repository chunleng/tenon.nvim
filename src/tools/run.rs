use crate::clients::get_agent;
use crate::directive::{Directive, DirectiveSource};
use crate::get_application_config;
use futures::stream::{self, StreamExt};
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

/// Shell metacharacters that indicate shell features (pipes, redirects, etc.)
const SHELL_METACHARACTERS: &[&str] = &["|", "&&", ";", ">", "<", "$("];

/// Hard cap on combined stdout+stderr output size (bytes).
const OUTPUT_CAP: usize = 64 * 1024;

#[derive(Deserialize)]
pub struct RunArgs {
    pub command: String,
    pub cwd: Option<String>,
    pub timeout: Option<u64>,
    pub filter: Option<String>,
    pub direction: Option<String>,
    pub limit: Option<usize>,
    pub env: Option<HashMap<String, String>>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Run;

#[derive(Serialize)]
struct RunOutput {
    exit_code: i32,
    stdout: String,
    stderr: String,
    truncated: bool,
}

/// Check a command string for shell metacharacters.
/// Returns the first metacharacter found, or None if clean.
fn find_shell_metacharacter(command: &str) -> Option<&'static str> {
    for &mc in SHELL_METACHARACTERS {
        if command.contains(mc) {
            return Some(mc);
        }
    }
    None
}

/// Check if command starts with env var prefix (VAR=value).
/// Returns true if the first token matches VAR=pattern.
fn has_env_var_prefix(command: &str) -> bool {
    command
        .split_whitespace()
        .next()
        .map(|first| first.contains('='))
        .unwrap_or(false)
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

    let allowance = if trimmed.ends_with(" *") {
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
    let safety_checker_directive = Directive {
        condition: None,
        source: DirectiveSource::Text {
            value: r#"Judge command safety. Output JSON only.

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

Judge by similarity to patterns above. Commands matching DENY patterns → deny. Commands matching ALLOW patterns → allow. Similar safe read-only operations → allow.

Output:
{"decision": "allow"}
{"decision": "deny", "reason": "..."}"#
                .to_string(),
        },
    };

    let agent = get_agent(model.clone(), vec![safety_checker_directive], vec![]);

    let user_message = format!("Command: {}", command);

    let response = agent.chat(user_message).await.map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("LLM safety check failed: {}", e),
        )))
    })?;

    // Parse JSON response
    let safety: CommandSafetyResponse = serde_json::from_str(&response).map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "Failed to parse LLM response as JSON: {} (response: {})",
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

    let models = &config.tools.run.check_models;
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
            async move { check_command_safety_with_llm(&command, &model).await }
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

/// Apply filter, direction, and limit to stdout lines.
fn apply_output_filters(
    stdout: &str,
    filter: Option<&str>,
    direction: Option<&str>,
    limit: Option<usize>,
) -> String {
    let mut lines: Vec<&str> = stdout.lines().collect();

    if let Some(f) = filter {
        lines.retain(|line| line.contains(f));
    }

    if let Some(dir) = direction {
        if dir == "head" {
            // keep from the start
        } else {
            // "tail" (default) — keep from the end
            lines.reverse();
        }
    }

    if let Some(n) = limit {
        lines.truncate(n);
    }

    // If we reversed for tail, reverse back
    if let Some(dir) = direction {
        if dir != "head" {
            lines.reverse();
        }
    }

    lines.join("\n")
}

impl Tool for Run {
    const NAME: &'static str = "run";
    type Error = ToolError;
    type Args = RunArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "run".to_string(),
            description: "Execute allowed commands. Output: stdout (filtered) + all stderr."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Command to execute. Runs directly without shell. Example: to run \"VAR=1 cargo build 2>&1 | grep error\", instead set env: {\"VAR\": \"1\"}, command: \"cargo build\", filter: \"error\". No need for 2>&1 - stderr always included."
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory. Default: cwd."
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Seconds before kill. Default: 30."
                    },
                    "filter": {
                        "type": "string",
                        "description": "Only stdout lines containing this substring."
                    },
                    "direction": {
                        "type": "string",
                        "description": "\"head\"|\"tail\". Which end to keep. Default: tail."
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max stdout lines. null = no limit."
                    },
                    "env": {
                        "type": "object",
                        "additionalProperties": {"type": "string"},
                        "description": "Env vars to set."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        // Check for shell metacharacters
        if let Some(mc) = find_shell_metacharacter(&args.command) {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "Shell metacharacter '{}' not allowed. No shell features (pipes, &&, redirects, $()). Use filter/limit/direction.",
                    mc
                ),
            ))));
        }

        // Check for env var prefix (VAR=value cmd)
        if has_env_var_prefix(&args.command) {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Env var prefix not allowed. No shell features (VAR=value cmd). Use env field.",
            ))));
        }

        // Parse the command
        let command_tokens = shlex::split(&args.command).ok_or_else(|| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Failed to parse command: '{}'", args.command),
            )))
        })?;

        if command_tokens.is_empty() {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Empty command".to_string(),
            ))));
        }

        // Check whitelist
        let config = get_application_config();
        let whitelist = &config.tools.run.whitelist;

        if !command_matches_whitelist(&command_tokens, whitelist) {
            // Whitelist doesn't match - use LLM to check if command is safe
            check_command_safety(&args.command).await?;
        }

        // Build the process
        let program = &command_tokens[0];
        let program_args = &command_tokens[1..];

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

        // Apply output filters to stdout only
        let filtered_stdout = apply_output_filters(
            &raw_stdout,
            args.filter.as_deref(),
            args.direction.as_deref(),
            args.limit,
        );

        // Check truncation on combined filtered output + stderr
        let combined_len = filtered_stdout.len() + raw_stderr.len();
        let truncated = combined_len > OUTPUT_CAP;

        let result = RunOutput {
            exit_code,
            stdout: filtered_stdout,
            stderr: raw_stderr,
            truncated,
        };

        Ok(serde_json::to_string(&result).unwrap_or_else(|_| {
            r#"{"exit_code":-1,"stdout":"","stderr":"","truncated":false}"#.to_string()
        }))
    }
}
