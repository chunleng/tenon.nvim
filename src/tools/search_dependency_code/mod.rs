mod rust;

use crate::chat::log::window::LogWindow;
use crate::chat::{TenonAssistantMessage, TenonAssistantMessageContent, TenonLog, TenonLogData};
use crate::tools::{ToolCore, ToolCoreCall, tool_display_summary};
use futures::stream::{BoxStream, StreamExt};
use rig::tool::{ToolContext, ToolExecutionError};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, RwLock};
use std::time::Duration;

#[derive(Deserialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum SupportedLanguages {
    Rust,
}

use crate::agent::worker::full::{GoalOrientedWorker, GoalResult};
use crate::get_application_config;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchDependencyCodeArgs {
    pub prompt: String,
    pub language: SupportedLanguages,
    pub dependency: String,
    pub version: Option<String>,
}

pub struct SearchDependencyCode;

impl ToolCore for SearchDependencyCode {
    fn name(&self) -> String {
        "search_dependency_code".to_string()
    }
    type Error = ToolExecutionError;
    type Args = SearchDependencyCodeArgs;
    type Output = String;
    type Call = SearchDependencyCodeCall;

    fn description(&self) -> String {
        "Search a project dependency's source code to understand how it works \
         or find implementation details."
            .to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "What to investigate in the dependency's code"
                },
                "language": {
                    "type": "string",
                    "enum": ["rust"],
                    "description": "Programming language"
                },
                "dependency": {
                    "type": "string",
                    "description": "Dependency name as it appears in the project's manifest"
                },
                "version": {
                    "type": "string",
                    "description": "Exact dependency version. Autoresolve when omitted. Required when multiple versions exist"
                }
            },
            "required": ["prompt", "language", "dependency"]
        })
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        // Validate project type, dependency existence, and version match.
        // Resolves exact version from Cargo.lock when not provided.
        let resolved_version = validate_dependency_source(&args)?;

        // cargo metadata resolves and downloads the dependency source path.
        let source_path = resolve_source_path(&args, &resolved_version)?;

        let config = get_application_config();
        let agent_config = config
            .agents
            .get(&config.default_agent)
            .ok_or_else(|| ToolExecutionError::other("No default agent configured"))?;
        let model = agent_config.model.clone();

        let tool_names = vec![
            "search_text".to_string(),
            "read_file".to_string(),
            "list_files".to_string(),
        ];

        let mut agent = GoalOrientedWorker::new(model, vec![], tool_names);

        let task = format!(
            "Investigate the following in the dependency source code located at:\n{}\n\n\
             {}\n\n\
             All paths must be within the dependency source directory above.",
            source_path.display(),
            args.prompt
        );

        let log_window = agent.log_window();
        let task: Pin<Box<dyn Future<Output = GoalResult> + Send>> =
            Box::pin(async move { agent.perform_task(&task).await });

        Ok(SearchDependencyCodeCall {
            log_window,
            task: Some(task),
            outcome: Arc::new(RwLock::new(None)),
        })
    }
}

pub struct SearchDependencyCodeCall {
    /// The sub-agent's log window, observed for live progress lines.
    log_window: Arc<RwLock<LogWindow>>,
    /// The sub-agent task. `stream()` takes it to drive while emitting
    /// progress; `result()` drives it directly when streaming never ran.
    task: Option<Pin<Box<dyn Future<Output = GoalResult> + Send>>>,
    /// Completion slot shared with the stream: the stream stores the
    /// sub-agent's result here, `result()` reads it.
    outcome: Arc<RwLock<Option<GoalResult>>>,
}

/// How often the stream checks the sub-agent's log window for new entries.
const PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// State of the sub-agent progress stream.
struct SubagentProgressState {
    log_window: Arc<RwLock<LogWindow>>,
    task: Option<Pin<Box<dyn Future<Output = GoalResult> + Send>>>,
    outcome: Arc<RwLock<Option<GoalResult>>>,
    seen: SeenLogs,
    /// Lines emitted by one log entry but not yet yielded (a settled
    /// assistant message emits all of its lines at once).
    pending: VecDeque<String>,
}

impl ToolCoreCall for SearchDependencyCodeCall {
    type Error = ToolExecutionError;
    type Output = String;

    fn stream(&mut self, _context: &mut ToolContext) -> Option<BoxStream<'_, String>> {
        let task = self.task.take()?;
        let state = SubagentProgressState {
            log_window: self.log_window.clone(),
            task: Some(task),
            outcome: self.outcome.clone(),
            seen: SeenLogs::default(),
            pending: VecDeque::new(),
        };
        // One step: emit the next settled log entry as progress lines, or
        // drive the sub-agent until it completes or the poll interval elapses.
        // The sub-agent runs at full speed; the interval only bounds how often
        // the log window is checked for new entries.
        Some(
            futures::stream::unfold(state, |mut state| async move {
                loop {
                    if let Some(line) = state.pending.pop_front() {
                        return Some((line, state));
                    }
                    let lines = next_progress_line(&state.log_window, &mut state.seen, false);
                    let mut lines = lines.into_iter();
                    if let Some(first) = lines.next() {
                        state.pending.extend(lines);
                        return Some((first, state));
                    }
                    let Some(mut task) = state.task.take() else {
                        // Task finished: drain remaining entries, including an
                        // assistant message held back while it was still growing
                        let mut lines =
                            next_progress_line(&state.log_window, &mut state.seen, true)
                                .into_iter();
                        if let Some(first) = lines.next() {
                            state.pending.extend(lines);
                            return Some((first, state));
                        }
                        return None;
                    };
                    match tokio::time::timeout(PROGRESS_POLL_INTERVAL, &mut task).await {
                        Ok(result) => {
                            if let Ok(mut guard) = state.outcome.write() {
                                *guard = Some(result);
                            }
                        }
                        Err(_) => state.task = Some(task),
                    }
                }
            })
            .boxed(),
        )
    }

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let Self { task, outcome, .. } = self;

        // Streaming never ran: drive the sub-agent here directly.
        if let Some(task) = task {
            let result = task.await;
            if let Ok(mut guard) = outcome.write() {
                *guard = Some(result);
            }
        } else if outcome.read().map(|g| g.is_none()).unwrap_or(true) {
            // stream() took the task but was dropped before completion: the
            // sub-agent future is gone, no answer can ever arrive.
            return Err(ToolExecutionError::other(
                "Sub-agent task was dropped before completion",
            ));
        }

        let result = loop {
            if let Some(result) = outcome.read().ok().and_then(|g| g.clone()) {
                break result;
            }
            tokio::time::sleep(PROGRESS_POLL_INTERVAL).await;
        };

        match result {
            GoalResult::Answer(answer) => Ok(answer),
            GoalResult::NoAnswer(Some(explanation)) => Err(ToolExecutionError::other(format!(
                "Agent could not complete the task: {}",
                explanation
            ))),
            GoalResult::NoAnswer(None) => Err(ToolExecutionError::other(
                "Agent timed out without producing an answer",
            )),
        }
    }
}

/// Tracks which sub-agent log entries have already been emitted as progress
/// lines. Holds Arc clones so entry addresses are never reused, keeping
/// pointer identity stable even when context truncation removes entries from
/// the window mid-run.
#[derive(Default)]
struct SeenLogs {
    seen: Vec<Arc<RwLock<TenonLog>>>,
}

impl SeenLogs {
    fn contains(&self, log: &Arc<RwLock<TenonLog>>) -> bool {
        self.seen.iter().any(|s| Arc::ptr_eq(s, log))
    }

    fn mark(&mut self, log: &Arc<RwLock<TenonLog>>) {
        self.seen.push(log.clone());
    }
}

/// Returns the progress lines for the next unseen sub-agent log entry.
///
/// - Tool calls emit immediately (their call info is complete on creation).
/// - Assistant messages are held back while they are the last entry: streaming
///   deltas append in place, so a message is only settled once a later entry
///   exists. `final_drain` (task finished) emits it regardless. A settled
///   assistant message emits all of its lines at once.
/// - Other entries emit nothing.
fn next_progress_line(
    log_window: &Arc<RwLock<LogWindow>>,
    seen: &mut SeenLogs,
    final_drain: bool,
) -> Vec<String> {
    let Some(window) = log_window.read().ok() else {
        return Vec::new();
    };
    let len = window.logs.len();
    for (i, indexed) in window.logs.iter().enumerate() {
        if seen.contains(&indexed.log) {
            continue;
        }
        let is_last = i + 1 == len;
        let Ok(log) = indexed.log.read() else {
            seen.mark(&indexed.log);
            continue;
        };
        match log.data() {
            TenonLogData::Tool(tool_log) => {
                seen.mark(&indexed.log);
                return vec![tool_call_line(
                    &tool_log.tool_call.name,
                    &tool_log.tool_call.args,
                )];
            }
            TenonLogData::Assistant(msg) => {
                if is_last && !final_drain {
                    // Still growing: hold back, re-check on the next poll
                    return Vec::new();
                }
                seen.mark(&indexed.log);
                let lines = assistant_lines(msg);
                if !lines.is_empty() {
                    return lines;
                }
            }
            _ => {
                seen.mark(&indexed.log);
            }
        }
    }
    Vec::new()
}

fn validate_dependency_source(
    args: &SearchDependencyCodeArgs,
) -> Result<String, ToolExecutionError> {
    match args.language {
        SupportedLanguages::Rust => {
            rust::validate_rust_dependency(&args.dependency, args.version.as_deref())
        }
    }
}

fn resolve_source_path(
    args: &SearchDependencyCodeArgs,
    resolved_version: &str,
) -> Result<PathBuf, ToolExecutionError> {
    match args.language {
        SupportedLanguages::Rust => {
            rust::resolve_rust_source_path(&args.dependency, resolved_version)
        }
    }
}

/// Formats a sub-agent tool call as a progress line mirroring the chat
/// display's tool rendering: `󰣖 <name> | <summary>`, or `󰣖 <name>` when the
/// tool has no display arg. No result yet at call time, so `result: None`.
fn tool_call_line(name: &str, args: &Value) -> String {
    match tool_display_summary(name, args, None) {
        Some(summary) => format!("󰣖 {} | {}", name, summary),
        None => format!("󰣖 {}", name),
    }
}

/// Progress lines for an assistant message, mirroring the chat display's
/// assistant rendering: the message's text content, one line per line; when
/// there is no text content, its reasoning lines instead.
fn assistant_lines(msg: &TenonAssistantMessage) -> Vec<String> {
    let text = msg
        .content
        .iter()
        .filter_map(|c| match c {
            TenonAssistantMessageContent::Text(text) => Some(text.as_str()),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let source = if text.trim().is_empty() {
        msg.reasoning.as_deref().unwrap_or("")
    } else {
        text.as_str()
    };
    source
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| format!("󰚩 {}", l))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_languages_deserializes_rust() {
        let json = r#"{"prompt":"test","language":"rust","dependency":"toml"}"#;
        let args: SearchDependencyCodeArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.language, SupportedLanguages::Rust);
        assert!(
            args.version.is_none(),
            "Version should be None when not provided"
        );
    }

    #[test]
    fn supported_languages_rejects_unsupported() {
        let json =
            r#"{"prompt":"test","language":"python","dependency":"toml","version":"0.8.23"}"#;
        let result: Result<SearchDependencyCodeArgs, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn tool_call_line_mirrors_chat_display_with_summary() {
        let args = json!({"pattern": "foo", "path": "src"});
        assert_eq!(
            tool_call_line("search_text", &args),
            "󰣖 search_text | pattern: foo"
        );
    }

    #[test]
    fn tool_call_line_name_only_when_no_display_arg() {
        // list_files has a core arg ("pattern") but it is missing from args
        assert_eq!(tool_call_line("list_files", &json!({})), "󰣖 list_files");
    }

    #[test]
    fn tool_call_line_run_command_special_case() {
        let args = json!({"argv": ["cargo", "build"]});
        assert_eq!(
            tool_call_line("run_command", &args),
            "󰣖 run_command | command: cargo build"
        );
    }

    #[test]
    fn tool_call_line_name_only_when_core_arg_not_a_string() {
        // navigate_choreo's core arg "move" is a number: summary is None
        let args = json!({"move": 2});
        assert_eq!(
            tool_call_line("navigate_choreo", &args),
            "󰣖 navigate_choreo"
        );
    }

    #[test]
    fn assistant_lines_emits_each_text_line() {
        let msg = TenonAssistantMessage {
            reasoning: None,
            content: vec![TenonAssistantMessageContent::Text(
                "\n  first line  \nsecond line".to_string(),
            )],
        };
        assert_eq!(assistant_lines(&msg), vec!["󰚩 first line", "󰚩 second line"]);
    }

    #[test]
    fn assistant_lines_skips_empty_content_parts() {
        let msg = TenonAssistantMessage {
            reasoning: None,
            content: vec![
                TenonAssistantMessageContent::Text("   ".to_string()),
                TenonAssistantMessageContent::Text("real text\nmore".to_string()),
            ],
        };
        assert_eq!(assistant_lines(&msg), vec!["󰚩 real text", "󰚩 more"]);
    }

    #[test]
    fn assistant_lines_falls_back_to_reasoning_when_content_empty() {
        // Mirrors the chat display: reasoning lines when there is no text content
        let msg = TenonAssistantMessage {
            reasoning: Some("thinking\nmore thoughts".to_string()),
            content: vec![],
        };
        assert_eq!(assistant_lines(&msg), vec!["󰚩 thinking", "󰚩 more thoughts"]);
    }

    use crate::chat::log::indexer::IndexedLog;
    use crate::chat::{TenonToolCall, TenonToolLog};

    fn tool_log(name: &str, args: Value) -> Arc<RwLock<TenonLog>> {
        Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
            TenonToolLog {
                tool_call: TenonToolCall {
                    id: "id".to_string(),
                    internal_call_id: "id".to_string(),
                    item_id: None,
                    name: name.to_string(),
                    args,
                },
                tool_result: None,
                progress: vec![],
            },
        ))))
    }

    fn assistant_log(text: &str) -> Arc<RwLock<TenonLog>> {
        Arc::new(RwLock::new(TenonLog::new(TenonLogData::Assistant(
            TenonAssistantMessage {
                reasoning: None,
                content: vec![TenonAssistantMessageContent::Text(text.to_string())],
            },
        ))))
    }

    fn reasoning_only_log() -> Arc<RwLock<TenonLog>> {
        Arc::new(RwLock::new(TenonLog::new(TenonLogData::Assistant(
            TenonAssistantMessage {
                reasoning: Some("thinking".to_string()),
                content: vec![],
            },
        ))))
    }

    fn window_with(logs: Vec<Arc<RwLock<TenonLog>>>) -> Arc<RwLock<LogWindow>> {
        Arc::new(RwLock::new(LogWindow {
            logs: logs
                .into_iter()
                .map(|log| IndexedLog { log, active: true })
                .collect(),
        }))
    }

    #[test]
    fn next_progress_line_emits_tool_and_assistant_lines_in_order() {
        let window = window_with(vec![
            tool_log("search_text", json!({"pattern": "foo"})),
            reasoning_only_log(),
            assistant_log("Investigating the parser\nmore text"),
            tool_log("read_file", json!({"filepath": "src/lib.rs"})),
        ]);
        let mut seen = SeenLogs::default();

        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰣖 search_text | pattern: foo".to_string()]
        );
        // Reasoning-only assistant emits its reasoning lines (mirrors the display)
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰚩 thinking".to_string()]
        );
        // Assistant message emits all of its lines at once
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec![
                "󰚩 Investigating the parser".to_string(),
                "󰚩 more text".to_string()
            ]
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰣖 read_file | filepath: src/lib.rs".to_string()]
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            Vec::<String>::new()
        );
    }

    #[test]
    fn next_progress_line_holds_back_last_assistant_until_settled() {
        let window = window_with(vec![
            tool_log("search_text", json!({"pattern": "foo"})),
            assistant_log("partial text"),
        ]);
        let mut seen = SeenLogs::default();

        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰣖 search_text | pattern: foo".to_string()]
        );
        // Last entry is a growing assistant message: held back
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            Vec::<String>::new()
        );

        // A later entry appears: the assistant message is settled
        window.write().unwrap().logs.push(IndexedLog {
            log: tool_log("read_file", json!({"filepath": "a.rs"})),
            active: true,
        });
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰚩 partial text".to_string()]
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰣖 read_file | filepath: a.rs".to_string()]
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            Vec::<String>::new()
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, true),
            Vec::<String>::new()
        );
    }

    #[test]
    fn next_progress_line_final_drain_releases_held_assistant() {
        let window = window_with(vec![assistant_log("final words")]);
        let mut seen = SeenLogs::default();

        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            Vec::<String>::new()
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, true),
            vec!["󰚩 final words".to_string()]
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, true),
            Vec::<String>::new()
        );
    }

    #[test]
    fn next_progress_line_survives_window_removal() {
        let window = window_with(vec![
            tool_log("search_text", json!({"pattern": "foo"})),
            assistant_log("found something"),
        ]);
        let mut seen = SeenLogs::default();
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰣖 search_text | pattern: foo".to_string()]
        );
        // Assistant is the last entry: held back until a later entry appears
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            Vec::<String>::new()
        );
        window.write().unwrap().logs.push(IndexedLog {
            log: tool_log("read_file", json!({"filepath": "a.rs"})),
            active: true,
        });
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰚩 found something".to_string()]
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰣖 read_file | filepath: a.rs".to_string()]
        );

        // Context truncation removes the first entry; a new one is appended
        window.write().unwrap().logs.remove(0);
        window.write().unwrap().logs.push(IndexedLog {
            log: tool_log("list_files", json!({"pattern": "*.rs"})),
            active: true,
        });

        // No re-emission of seen entries; the new entry still comes through
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            vec!["󰣖 list_files | pattern: *.rs".to_string()]
        );
        assert_eq!(
            next_progress_line(&window, &mut seen, false),
            Vec::<String>::new()
        );
    }
}
