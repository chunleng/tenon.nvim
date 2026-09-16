use crate::agent::worker::simple::SimpleTenonWorkerAgent;
use crate::get_application_config;
use crate::utils::format_yaml_block_scalars;
use futures::stream::{self, StreamExt};

use rig::tool::{Tool, ToolContext, ToolExecutionError};
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
    pub argv: Vec<String>,
    pub path: Option<String>,
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
) -> Result<(bool, Option<String>), ToolExecutionError> {
    let worker = SimpleTenonWorkerAgent::new(
        Some(model.clone()),
        r#"Judge command safety. Output YAML only.

Step 1 - Shell gate: if the command invokes a shell interpreter with inline code (bash -c, sh -c, zsh -c, dash -c, ...), deny immediately. Do not extract a subject or continue further.

Step 2 - Extract the subject:
- Default: the whole command.
- Inline code: command is an interpreter with an inline-code flag (python3 -c, node -e, ruby -e, perl -e, php -r, ...) → subject is the inline code only.
- Fallback: script files (python3 script.py), extra flags, or unrecognized forms → subject is the whole command.

Step 3 - Test the subject against the patterns. For code subjects, interpret patterns against code operations (open() read → read files; os.remove → delete; requests/urllib → network; eval/exec → code exec).

DENY patterns:
- Access to secrets: env vars (*KEY*, *SECRET*, *TOKEN*, *API*), files (.env, id_rsa, credentials, .pem)
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
- Pure utilities (no side effects): sleep, date, wc, sort, jq, stat, du, ps, basename, realpath, sha256sum, base64

Code subject gate: allow only if BOTH hold:
1. Fully traceable: you can enumerate every operation the code performs. Deny dynamic execution (eval, exec, getattr, compile, __import__), obfuscated payloads (base64/char-code), or code not straightforward to analyze
2. Stdlib imports only: no third-party imports (importing runs module-level code)

Judge by similarity to patterns above. Subject matching DENY patterns → deny. Subject matching ALLOW patterns → allow. When uncertain, deny.

Output (allow):
decision: allow

Output (deny):
decision: deny
reason: ..."#,
        None,
    )
    .map_err(|e| {
        ToolExecutionError::from_error(e)
    })?;

    let user_message = format!("Command: {}", command);

    let response = worker
        .chat(user_message)
        .await
        .map_err(|e| ToolExecutionError::other(format!("LLM safety check failed: {}", e)))?;

    // Parse YAML response
    let safety: CommandSafetyResponse = serde_yaml::from_str(&response).map_err(|e| {
        ToolExecutionError::other(format!(
            "Failed to parse LLM response as YAML: {} (response: {})",
            e, response
        ))
    })?;

    let allowed = safety.decision == "allow";
    Ok((allowed, safety.reason))
}

/// Check command safety using one LLM call per model in parallel.
/// All models must allow for the command to proceed.
/// Returns Ok(()) if allowed, or Err with the first denial reason.
async fn check_command_safety(command: &str) -> Result<(), ToolExecutionError> {
    let config = get_application_config();

    let models = &config.tools.run_command.check_models;
    if models.is_empty() {
        return Err(ToolExecutionError::permission_denied(
            "Command not in whitelist and no check_models configured for LLM safety check",
        ));
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
                    ToolExecutionError::other("LLM safety check failed after 3 attempts")
                }))
            }
        })
        .collect();

    let mut stream = stream::iter(checks).buffer_unordered(models.len());

    while let Some(result) = stream.next().await {
        match result {
            Ok((allowed, reason)) => {
                if !allowed {
                    return Err(ToolExecutionError::permission_denied(format!(
                        "Command denied by safety check: {}",
                        reason.unwrap_or_else(|| "Unknown reason".to_string())
                    )));
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
    type Error = ToolExecutionError;
    type Args = RunCommandArgs;
    type Output = String;

    fn description(&self) -> String {
        "Run command (exec form). Tool outputs yaml with both stdout and stderr\n
         Use filter, head, or tail (mutually exclusive, stdout only) to reduce output\n
         E.g.\n
         `git log` → argv=['git', 'log']\n
         `make 2>&1` → argv=['make'] (drop `2>&1`)\n
         `cat ./in.txt|grep foo` → argv=['cat', './in.txt'], filter='foo'\n
         `cargo build && cargo fmt` → Split into 2 sequential calls: `cargo build` first, then `cargo fmt` after it succeeds"
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "argv": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Exec-form argv. First element is the executable, rest are its args"
                },
                "path": {
                    "type": "string",
                    "description": "Working dir. cwd if omitted"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout (sec)",
                    "default": 30
                },
                "filter": {
                    "type": "string",
                    "description": "Keep stdout lines containing substring"
                },
                "head": {
                    "type": "integer",
                    "description": "Keep first N stdout lines"
                },
                "tail": {
                    "type": "integer",
                    "description": "Keep last N stdout lines"
                },
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Env vars"
                }
            },
            "required": ["argv"]
        })
    }

    async fn call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        // filter, head, and tail are mutually exclusive
        let set_count = args.filter.is_some() as usize
            + args.head.is_some() as usize
            + args.tail.is_some() as usize;
        if set_count > 1 {
            return Err(ToolExecutionError::invalid_args(
                "filter, head, and tail are mutually exclusive. Use only one.",
            ));
        }

        // argv is required and must contain the executable
        if args.argv.is_empty() || args.argv[0].trim().is_empty() {
            return Err(ToolExecutionError::invalid_args("Empty argv"));
        }

        let command_tokens = args.argv.clone();
        let full_command = args.argv.join(" ");

        // Check whitelist
        let config = get_application_config();
        let whitelist = &config.tools.run_command.whitelist;

        if !command_matches_whitelist(&command_tokens, whitelist) {
            // Whitelist doesn't match - use LLM to check if command is safe
            check_command_safety(&full_command).await?;
        }

        // Build the process
        let program = &args.argv[0];
        let program_args = &args.argv[1..];

        let timeout_secs = args.timeout.unwrap_or(30);

        let mut cmd = Command::new(program);
        cmd.args(program_args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(ref path) = args.path {
            cmd.current_dir(path);
        }

        if let Some(ref env) = args.env {
            cmd.envs(env);
        }

        let child = cmd.spawn().map_err(|e| {
            ToolExecutionError::other(format!("Failed to spawn '{}': {}", program, e))
        })?;

        // Run with timeout
        let result =
            tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output()).await;

        let output = match result {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return Err(ToolExecutionError::other(format!("Process error: {}", e)));
            }
            Err(_) => {
                // Timeout — try to get partial output by killing
                return Err(ToolExecutionError::timeout(format!(
                    "Command timed out after {}s: '{}'",
                    timeout_secs, full_command
                )));
            }
        };

        let exit_code = output.status.code().unwrap_or(-1);
        let raw_stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let raw_stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // Output filters apply to stdout only; stderr passes through raw
        let filtered_stdout =
            apply_output_filters(&raw_stdout, args.filter.as_deref(), args.head, args.tail);

        // Truncate each stream individually to OUTPUT_CAP
        let (truncated_stdout, stdout_was_truncated) = truncate_output(&filtered_stdout);
        let (truncated_stderr, stderr_was_truncated) = truncate_output(&raw_stderr);
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
    use crate::config::TenonConfig;
    use rig::tool::ToolErrorKind;

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
    fn test_command_matches_whitelist_with_argv() {
        // Test: argv (executable + args) should match whitelist patterns

        let whitelist = vec!["git log *".to_string(), "make".to_string()];

        // git log with args should match "git log *"
        let argv = vec![
            "git".to_string(),
            "log".to_string(),
            "--oneline".to_string(),
        ];
        assert!(command_matches_whitelist(&argv, &whitelist));

        // make without args should match "make"
        let argv = vec!["make".to_string()];
        assert!(command_matches_whitelist(&argv, &whitelist));
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

    /// Set global config with whitelist ["*"] so commands run without the LLM
    /// safety check. First caller wins (OnceLock); safe under parallel tests.
    fn setup_whitelist_all() {
        let mut config = TenonConfig::default();
        config.tools.run_command.whitelist = vec!["*".to_string()];
        let _ = crate::CONFIG.set(config);
    }

    #[tokio::test]
    async fn test_filter_head_tail_mutually_exclusive() {
        setup_whitelist_all();
        let tool = RunCommand;
        let mut context = ToolContext::new();

        // filter + head
        let args = RunCommandArgs {
            argv: vec!["echo".to_string()],
            path: None,
            timeout: None,
            filter: Some("a".to_string()),
            head: Some(1),
            tail: None,
            env: None,
        };
        let err = tool.call(&mut context, args).await.unwrap_err();
        assert_eq!(err.kind(), ToolErrorKind::InvalidArgs);

        // filter + tail
        let args = RunCommandArgs {
            argv: vec!["echo".to_string()],
            path: None,
            timeout: None,
            filter: Some("a".to_string()),
            head: None,
            tail: Some(1),
            env: None,
        };
        let err = tool.call(&mut context, args).await.unwrap_err();
        assert_eq!(err.kind(), ToolErrorKind::InvalidArgs);

        // all three
        let args = RunCommandArgs {
            argv: vec!["echo".to_string()],
            path: None,
            timeout: None,
            filter: Some("a".to_string()),
            head: Some(1),
            tail: Some(1),
            env: None,
        };
        let err = tool.call(&mut context, args).await.unwrap_err();
        assert_eq!(err.kind(), ToolErrorKind::InvalidArgs);
    }

    #[tokio::test]
    async fn test_filter_applies_to_stdout() {
        setup_whitelist_all();

        let tool = RunCommand;
        let mut context = ToolContext::new();
        let args = RunCommandArgs {
            argv: vec![
                "sh".to_string(),
                "-c".to_string(),
                "echo alpha-out; echo beta-err >&2".to_string(),
            ],
            path: None,
            timeout: None,
            filter: Some("alpha".to_string()),
            head: None,
            tail: None,
            env: None,
        };

        let output = tool.call(&mut context, args).await.unwrap();

        // stdout filtered: keeps matching line
        assert!(
            output.contains("alpha-out"),
            "stdout should keep 'alpha-out': {output}"
        );
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
