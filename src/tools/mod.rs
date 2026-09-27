pub mod analyze_image;
pub mod ask_question;
pub mod edit_file;
pub mod end_choreo;
pub mod fetch_webpage;
pub mod list_files;
pub mod mcp_client;
pub mod move_path;
pub mod navigate_choreo;
pub mod pop_task;
pub mod push_tasks;
pub mod read_file;
pub mod record_thought;
pub mod remove_path;
pub mod run_command;
pub mod search_dependency_code;
pub mod search_text;
pub mod use_choreo;
pub mod web_search;

use crate::tools::web_search::{LangSearch, Tavily};
use crate::{config::WebSearchConfig, mcp::McpHubCaller, tools::web_search::Brave};
pub use analyze_image::AnalyzeImage;
pub use ask_question::AskQuestion;
pub use edit_file::EditFile;
pub use fetch_webpage::FetchWebpage;
pub use list_files::ListFiles;
pub use mcp_client::McpClient;
pub use move_path::MovePath;
pub use pop_task::PopTask;
pub use push_tasks::PushTasks;
pub use read_file::ReadFile;
pub use record_thought::RecordThought;
pub use remove_path::RemovePath;
use rig::message::ToolResultContent;
use rig::tool::{DynamicTool, IntoToolOutput, ToolContext, ToolExecutionError, ToolOutput};
use std::sync::{Arc, RwLock};

use crate::chat::log::indexer::IndexedLog;
use crate::chat::log::window::LogWindow;
use crate::chat::{
    TenonLog, TenonLogData, TenonToolCall, TenonToolError, TenonToolLog, TenonToolResult,
};
pub use run_command::RunCommand;
pub use search_dependency_code::SearchDependencyCode;
pub use search_text::SearchText;
pub use web_search::WebSearch;

use serde_json::Value;

use futures::stream::{BoxStream, StreamExt};
use serde::de::DeserializeOwned;
use std::future::Future;

/// Core tool logic
pub trait ToolCore: Send + Sync {
    /// Unique registration and provider-facing name.
    fn name(&self) -> String;
    /// Typed JSON arguments.
    type Args: DeserializeOwned + Send + Sync;
    /// Output convertible into Rig's canonical model presentation.
    type Output: IntoToolOutput;
    /// Typed error returned by direct calls to this tool.
    type Error: std::error::Error + Send + Sync + 'static;
    /// Per-call execution instance.
    type Call: ToolCoreCall<Output = Self::Output, Error = Self::Error>;

    /// Model-facing description.
    fn description(&self) -> String;

    /// JSON Schema for arguments.
    fn parameters(&self) -> serde_json::Value;

    /// Initialize the tool calling struct. A good place to perform validation and kick off work.
    fn init_call(
        &self,
        context: &mut ToolContext,
        args: Self::Args,
    ) -> impl Future<Output = Result<Self::Call, Self::Error>> + Send;

    /// Converts the tool's log entry after a successful result and returns the replacement output
    /// to return to the model. `None` keeps the original output. The log's tool result is already
    /// written when this runs. Errored results never reach this. Default: no conversion.
    fn convert_log(
        &self,
        _log: Arc<RwLock<TenonLog>>,
    ) -> impl Future<Output = Option<ToolOutput>> + Send {
        std::future::ready(None)
    }
}

pub trait ToolCoreCall: Send {
    /// Output convertible into Rig's canonical model presentation.
    type Output: IntoToolOutput;
    /// Typed error returned by this call.
    type Error: std::error::Error + Send + Sync + 'static;

    /// Stream progress content until the end.
    ///
    /// Default is `None` (nothing to stream). Items are plain strings for live visibility
    fn stream(&mut self, _context: &mut ToolContext) -> Option<BoxStream<'_, String>> {
        None
    }

    /// Get the result, used for the chat log.
    fn result(
        self,
        context: &mut ToolContext,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send;
}

/// `ToolCore` wrapper that implements `rig::tool::Tool`.
///
/// Used for tool registration in Tenon, providing features necessary for Tenon.
pub struct TenonTool<T> {
    inner: T,
    log_window: Arc<RwLock<LogWindow>>,
}

impl<T> TenonTool<T> {
    pub fn new(inner: T, log_window: Arc<RwLock<LogWindow>>) -> Self {
        Self { inner, log_window }
    }
}

impl<T: ToolCore> TenonTool<T> {
    /// Creates the log entry and returns the log handle for processing
    fn create_tool_log(
        &self,
        args: &serde_json::Value,
    ) -> Result<Arc<RwLock<TenonLog>>, ToolExecutionError> {
        let id = rig::id::generate();
        let log = Arc::new(RwLock::new(TenonLog::new(TenonLogData::Tool(
            TenonToolLog {
                tool_call: TenonToolCall {
                    // Same value for both: TenonTool mints the id, so the
                    // engine's internal_call_id matching finds this log entry.
                    id: id.clone(),
                    internal_call_id: id,
                    item_id: None,
                    name: self.inner.name(),
                    args: args.clone(),
                },
                tool_result: None,
                progress: vec![],
            },
        ))));
        let mut log_window = self
            .log_window
            .write()
            .map_err(|_| ToolExecutionError::other("Failed to lock log window"))?;
        log_window.logs.push(IndexedLog {
            log: log.clone(),
            active: true,
        });
        Ok(log)
    }
}

/// Maps a tool output into the log's result representation, mirroring the mapping in the engine's
/// ToolResult handler.
fn tenon_result_from_output(output: &ToolOutput) -> TenonToolResult {
    match output.as_content().first() {
        Some(ToolResultContent::Text(text)) => TenonToolResult::Text(text.clone()),
        Some(ToolResultContent::Image(img)) => TenonToolResult::Image(img.clone()),
        Some(ToolResultContent::Json { value }) => TenonToolResult::Text(rig::agent::Text {
            text: value.to_string(),
            ..Default::default()
        }),
        None => TenonToolResult::Text(rig::agent::Text::default()),
    }
}

impl<T: ToolCore + 'static> From<TenonTool<T>> for DynamicTool {
    fn from(tool: TenonTool<T>) -> Self {
        let name = tool.inner.name();
        let description = tool.inner.description();
        let parameters = tool.inner.parameters();
        let tool = Arc::new(tool);

        DynamicTool::new(name, description, parameters, move |context, args| {
            let tool = Arc::clone(&tool);
            Box::pin(async move {
                let log = tool.create_tool_log(&args)?;

                // Every failure path below must record its error in the log
                // before returning, so the UI always shows the outcome.
                let result = async {
                    let typed_args: T::Args = serde_json::from_value(args).map_err(|e| {
                        ToolExecutionError::invalid_args(format!(
                            "Failed to deserialize args: {}",
                            e
                        ))
                    })?;
                    let mut call = tool
                        .inner
                        .init_call(context, typed_args)
                        .await
                        .map_err(ToolExecutionError::from_error)?;
                    if let Some(mut stream) = call.stream(context) {
                        while let Some(item) = stream.next().await {
                            if let Ok(mut log) = log.write() {
                                log.append_tool_progress(&item);
                            }
                        }
                    }
                    call.result(context)
                        .await
                        .map_err(ToolExecutionError::from_error)?
                        .into_tool_output()
                }
                .await;
                let log_result = match &result {
                    Ok(output) => Ok(tenon_result_from_output(output)),
                    Err(e) => Err(TenonToolError(e.to_string())),
                };
                if let Ok(mut log) = log.write() {
                    log.set_tool_result(Some(log_result));
                }

                if result.is_ok()
                    && let Some(new_output) = tool.inner.convert_log(log.clone()).await
                {
                    return Ok(new_output);
                }
                result
            })
        })
    }
}

/// Classification of tools based on their behavior when rerun.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolClassification {
    /// Read-only tools that produce reproducible results.
    /// Rerunning with same inputs yields the same output.
    Idempotent,

    /// Read-only tools that may produce different results on rerun,
    /// but don't mutate any state.
    NonMutating,

    /// Tools that mutate state when run.
    /// Rerunning may have different effects or cause errors.
    Mutating,

    /// Tenon system tools for choreo management.
    System,

    /// Tools with unknown classification (e.g., MCP tools).
    Unknown,
}

/// Returns the classification for a tool by its name.
///
/// Built-in tools have fixed classifications:
/// - Idempotent: read_file, list_files, search_text
/// - NonMutating: web_search, fetch_webpage, record_thought, pop_task
/// - Mutating: edit_file, move_path, remove_path, run_command
/// - System: use_choreo, navigate_choreo, end_choreo, push_tasks
///
/// Unknown tool names (including MCP tools) return `ToolClassification::Unknown`.
pub fn get_tool_classification(name: &str) -> ToolClassification {
    match name {
        // Idempotent tools: read-only, reproducible results
        "read_file" | "list_files" | "search_text" | "search_dependency_code" => {
            ToolClassification::Idempotent
        }

        // Non-mutating tools: read-only, may produce different results
        // pop_task is NonMutating (not System) so its result stays in the
        // chat history for reference
        "web_search" | "fetch_webpage" | "record_thought" | "analyze_image" | "ask_question"
        | "pop_task" => ToolClassification::NonMutating,

        // Mutating tools: modify state when run
        "edit_file" | "move_path" | "remove_path" | "run_command" => ToolClassification::Mutating,

        // System tools: Tenon choreo management and work queue
        "use_choreo" | "navigate_choreo" | "end_choreo" | "push_tasks" => {
            ToolClassification::System
        }

        // Unknown: MCP tools or unrecognized names
        _ => ToolClassification::Unknown,
    }
}

/// Returns a short human-readable summary of what a tool call is doing,
/// by extracting the core parameter from its args JSON (or, for tools whose
/// meaningful info is in the output, from the tool result).
///
/// Returns `None` for tools with no useful display arg (e.g. "record_thought", MCP tools).
pub fn tool_display_summary(
    name: &str,
    args: &Value,
    result: Option<&Result<crate::chat::TenonToolResult, crate::chat::TenonToolError>>,
) -> Option<String> {
    // Special case for "run_command": join argv elements for display
    if name == "run_command" {
        let argv = args.get("argv").and_then(|v| v.as_array())?;
        let display = argv
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let display = display.lines().collect::<Vec<_>>().join("↵");
        return Some(format!("command: {}", display));
    }

    // Special case for "search_text": mirror the tool's lenient
    // literal/pattern resolution (prefer non-empty pattern, fall back to literal)
    if name == "search_text" {
        let pattern = args.get("pattern").and_then(|v| v.as_str());
        let literal = args.get("literal").and_then(|v| v.as_str());
        let (core_arg, text) = match (pattern, literal) {
            (Some(p), _) if !p.is_empty() => ("pattern", p),
            (_, Some(l)) => ("literal", l),
            (Some(p), None) => ("pattern", p),
            (None, None) => return None,
        };
        return Some(format!("{}: {}", core_arg, text));
    }

    // Special case for "pop_task": the meaningful info (task id) is in the
    // output YAML, not in the args (which only contain the group).
    if name == "pop_task" {
        let text = match result? {
            Ok(crate::chat::TenonToolResult::Text(t)) => &t.text,
            _ => return None,
        };
        let yaml: serde_yaml::Value = serde_yaml::from_str(text).ok()?;
        let id = yaml.get("id")?.as_str()?;
        return Some(format!("id: {}", id));
    }

    let core_arg: &str = match name {
        "web_search" => "query",
        "read_file" | "edit_file" | "remove_path" => "filepath",
        "move_path" => "source",
        "list_files" => "pattern",
        "search_dependency_code" => "dependency",
        "fetch_webpage" => "url",
        "analyze_image" => "image",
        "ask_question" => "question",
        "navigate_choreo" => "move",
        _ => return None,
    };
    args.get(core_arg).and_then(|v| v.as_str()).map(|x| {
        let display = if core_arg == "filepath" || core_arg == "source" {
            std::env::current_dir()
                .ok()
                .and_then(|cwd| {
                    let cwd_str = cwd.to_string_lossy();
                    x.strip_prefix(cwd_str.as_ref())
                        .map(|rest| format!("./{}", rest.trim_start_matches('/')))
                })
                .unwrap_or_else(|| x.to_string())
        } else {
            x.to_string()
        };
        let display = display.lines().collect::<Vec<_>>().join("↵");
        format!("{}: {}", core_arg, display)
    })
}

/// Returns the names of all selectable tools (built-in + MCP - system tools).
pub fn all_tool_names() -> Vec<String> {
    let mut names: Vec<String> = vec![
        "edit_file".into(),
        "fetch_webpage".into(),
        "list_files".into(),
        "move_path".into(),
        "read_file".into(),
        "remove_path".into(),
        "run_command".into(),
        "search_dependency_code".into(),
        "search_text".into(),
        "analyze_image".into(),
    ];

    if crate::get_application_config().tools.web_search.is_some() {
        names.push("web_search".into());
    }

    if let Ok(mcp_tools) = McpHubCaller::from_mcp_tools() {
        for tool in mcp_tools {
            names.push(tool.name());
        }
    }

    names
}

/// Check whether a concrete tool `name` matches any of the given `selectors`.
///
/// - Selectors containing `.` → exact string match (e.g. `"server.tool_a"`).
/// - Selectors without `.` → exact match for built-ins, or prefix match for
///   MCP tools (e.g. `"server"` matches `"server.tool_a"`).
pub fn tool_matches_selectors(name: &str, selectors: &[&str]) -> bool {
    selectors.iter().any(|&r| {
        // TODO refactor to have a constant for the separator
        // Remove the use of . or : because GPT doesn't allow `:` and Bedrock doesn't allow `.`
        if r.contains("____") {
            r == name
        } else {
            r == name || name.starts_with(&format!("{}____", r))
        }
    })
}

/// Resolve a list of tool name strings into their expanded concrete names.
///
/// Applies the same matching rules as [`resolve_tools`] but returns just the
/// names, without instantiating tool objects. Useful for comparison / display.
#[cfg(not(test))]
pub fn resolve_tool_names(names: &[impl AsRef<str>]) -> Vec<String> {
    let selectors: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();
    all_tool_names()
        .into_iter()
        .filter(|name| tool_matches_selectors(name, &selectors))
        .collect()
}

// Test mock: returns tool names as-is without resolving MCP tools.
// The real implementation calls McpHubCaller::from_mcp_tools() which requires
// a Neovim context (GLOBAL_EXECUTION_HANDLER), causing panics in unit tests.
#[cfg(test)]
pub fn resolve_tool_names(names: &[impl AsRef<str>]) -> Vec<String> {
    names.iter().map(|x| x.as_ref().to_string()).collect()
}

/// Select tools matching `selectors`, returned in selector order.
///
/// Each selector contributes its matches consecutively, at the selector's
/// position. A tool is taken at most once (first matching selector wins).
fn select_in_order<T>(mut tools: Vec<Option<(String, T)>>, selectors: &[&str]) -> Vec<T> {
    let mut result = Vec::new();
    for selector in selectors {
        for entry in tools.iter_mut() {
            if entry.is_none() {
                continue;
            }
            if tool_matches_selectors(&entry.as_ref().unwrap().0, std::slice::from_ref(selector)) {
                result.push(entry.take().unwrap().1);
            }
        }
    }
    result
}

/// Build the list of built-in tools (excluding MCP tools).
fn builtin_tools(log_window: Arc<RwLock<LogWindow>>) -> Vec<Option<(String, DynamicTool)>> {
    let mut all_tools: Vec<Option<(String, DynamicTool)>> = vec![
        Some((
            "edit_file".to_string(),
            TenonTool::new(EditFile, log_window.clone()).into(),
        )),
        Some((
            "fetch_webpage".to_string(),
            TenonTool::new(FetchWebpage, log_window.clone()).into(),
        )),
        Some((
            "analyze_image".to_string(),
            TenonTool::new(AnalyzeImage, log_window.clone()).into(),
        )),
        Some((
            "list_files".to_string(),
            TenonTool::new(ListFiles, log_window.clone()).into(),
        )),
        Some((
            "move_path".to_string(),
            TenonTool::new(MovePath, log_window.clone()).into(),
        )),
        Some((
            "read_file".to_string(),
            TenonTool::new(ReadFile, log_window.clone()).into(),
        )),
        Some((
            "record_thought".to_string(),
            TenonTool::new(RecordThought, log_window.clone()).into(),
        )),
        Some((
            "remove_path".to_string(),
            TenonTool::new(RemovePath, log_window.clone()).into(),
        )),
        Some((
            "run_command".to_string(),
            TenonTool::new(RunCommand, log_window.clone()).into(),
        )),
        Some((
            "search_dependency_code".to_string(),
            TenonTool::new(SearchDependencyCode, log_window.clone()).into(),
        )),
        Some((
            "search_text".to_string(),
            TenonTool::new(SearchText, log_window.clone()).into(),
        )),
    ];

    if let Some(web_search_config) = &crate::get_application_config().tools.web_search {
        let provider: Arc<dyn web_search::SearchProvider> = match web_search_config {
            WebSearchConfig::Brave { api_key } => Arc::new(Brave {
                api_key: api_key.clone(),
            }),
            WebSearchConfig::LangSearch { api_key } => Arc::new(LangSearch {
                api_key: api_key.clone(),
            }),
            WebSearchConfig::Tavily { api_key } => Arc::new(Tavily {
                api_key: api_key.clone(),
            }),
        };
        all_tools.push(Some((
            "web_search".to_string(),
            TenonTool::new(WebSearch { provider }, log_window).into(),
        )));
    }

    all_tools
}

/// Resolve a list of tool name strings into concrete `DynamicTool` instances.
///
/// Built-in names: "edit_file", "fetch_webpage",
/// "list_files", "move_path", "read_file", "remove_path", "run_command", "search_text", "web_search", "record_thought".
/// MCP tool names: "server_name.tool_name" for a specific tool,
/// or "server_name" to include all tools from that server.
#[cfg(not(test))]
pub fn resolve_tools(
    names: &[impl AsRef<str>],
    log_window: Arc<RwLock<LogWindow>>,
) -> Vec<DynamicTool> {
    let name_refs: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();

    let mut all_tools = builtin_tools(log_window.clone());

    if let Ok(mcp_tools) = McpHubCaller::from_mcp_tools() {
        for tool in mcp_tools {
            all_tools.push(Some((
                tool.name(),
                TenonTool::new(tool, log_window.clone()).into(),
            )));
        }
    }

    select_in_order(all_tools, &name_refs)
}

// Test mock: resolves built-in tools only, skipping MCP tools.
// The real implementation calls McpHubCaller::from_mcp_tools() which requires
// a Neovim context (GLOBAL_EXECUTION_HANDLER), causing panics in unit tests.
#[cfg(test)]
pub fn resolve_tools(
    names: &[impl AsRef<str>],
    log_window: Arc<RwLock<LogWindow>>,
) -> Vec<DynamicTool> {
    let name_refs: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();
    select_in_order(builtin_tools(log_window), &name_refs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_in_order_preserves_selector_order() {
        let tools = vec![
            Some(("a".to_string(), 1)),
            Some(("b".to_string(), 2)),
            Some(("c".to_string(), 3)),
        ];
        let selectors = ["c", "a", "b"];
        let result: Vec<i32> = select_in_order(tools, &selectors);
        assert_eq!(result, vec![3, 1, 2]);
    }

    #[test]
    fn select_in_order_groups_prefix_selector_matches() {
        let tools = vec![
            Some(("srv____tool1".to_string(), 1)),
            Some(("srv____tool2".to_string(), 2)),
            Some(("other".to_string(), 3)),
        ];
        let selectors = ["srv", "other"];
        let result: Vec<i32> = select_in_order(tools, &selectors);
        assert_eq!(result, vec![1, 2, 3]);
    }

    #[test]
    fn pop_task_summary_extracts_id_from_output() {
        use crate::chat::TenonToolResult;
        let result = Ok(TenonToolResult::Text(rig::agent::Text {
            text: "group: bugs\nid: fix crash\ndetails: crash details\n".to_string(),
            ..Default::default()
        }));
        let summary = tool_display_summary(
            "pop_task",
            &serde_json::json!({"group": "bugs"}),
            Some(&result),
        );
        assert_eq!(summary, Some("id: fix crash".to_string()));
    }

    #[test]
    fn pop_task_is_non_mutating_not_system() {
        assert_eq!(
            get_tool_classification("pop_task"),
            ToolClassification::NonMutating
        );
        assert_eq!(
            get_tool_classification("push_tasks"),
            ToolClassification::System
        );
    }

    mod tenon_tool_tests {
        use super::*;
        use crate::chat::log::TenonThoughtLog;
        use crate::chat::log::indexer::IndexedLog;
        use crate::chat::log::window::LogWindow;
        use crate::chat::{TenonLogData, TenonToolLog, TenonToolResult};
        use rig::tool::{ToolContext, ToolSet};
        use std::sync::{Arc, RwLock};

        /// Executes a TenonTool through its DynamicTool registration, the same
        /// path the engine uses after the `From<TenonTool<T>>` conversion.
        async fn execute_tenon_tool<T: ToolCore + 'static>(
            tool: TenonTool<T>,
            args: &str,
        ) -> rig::tool::ToolResult {
            let name = tool.inner.name();
            let dynamic: DynamicTool = tool.into();
            let set = ToolSet::from_dynamic_tools(vec![dynamic]);
            set.execute(&name, args, &mut ToolContext::new()).await
        }

        fn test_log_window() -> Arc<RwLock<LogWindow>> {
            Arc::new(RwLock::new(LogWindow { logs: vec![] }))
        }

        fn tool_logs(log_window: &Arc<RwLock<LogWindow>>) -> Vec<TenonToolLog> {
            log_window
                .read()
                .unwrap()
                .logs
                .iter()
                .filter_map(
                    |indexed: &IndexedLog| match indexed.log.read().unwrap().data() {
                        TenonLogData::Tool(tool_log) => Some(tool_log.clone()),
                        _ => None,
                    },
                )
                .collect()
        }

        #[derive(Debug)]
        struct MockError;

        impl std::fmt::Display for MockError {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "mock error")
            }
        }

        impl std::error::Error for MockError {}

        #[derive(Debug, serde::Deserialize)]
        struct MockArgs {
            input: String,
        }

        /// Streams two items then succeeds.
        struct MockStreamTool;

        struct MockStreamCall {
            items: Vec<String>,
        }

        impl ToolCore for MockStreamTool {
            fn name(&self) -> String {
                "mock_stream_tool".to_string()
            }
            type Args = MockArgs;
            type Output = String;
            type Error = MockError;
            type Call = MockStreamCall;

            fn description(&self) -> String {
                "mock stream tool".to_string()
            }

            fn parameters(&self) -> serde_json::Value {
                serde_json::json!({})
            }

            async fn init_call(
                &self,
                _context: &mut ToolContext,
                args: MockArgs,
            ) -> Result<MockStreamCall, MockError> {
                Ok(MockStreamCall {
                    items: vec![args.input, "world".to_string()],
                })
            }
        }

        impl ToolCoreCall for MockStreamCall {
            type Output = String;
            type Error = MockError;

            fn stream(&mut self, _context: &mut ToolContext) -> Option<BoxStream<'_, String>> {
                let items = std::mem::take(&mut self.items);
                Some(futures::stream::iter(items).boxed())
            }

            async fn result(self, _context: &mut ToolContext) -> Result<String, MockError> {
                Ok("done".to_string())
            }
        }

        /// Converts its log to a Thought log with a replacement result.
        struct MockConvertTool;

        impl ToolCore for MockConvertTool {
            fn name(&self) -> String {
                "mock_convert_tool".to_string()
            }
            type Args = MockArgs;
            type Output = String;
            type Error = MockError;
            type Call = MockStreamCall;

            fn description(&self) -> String {
                "mock convert tool".to_string()
            }

            fn parameters(&self) -> serde_json::Value {
                serde_json::json!({})
            }

            async fn init_call(
                &self,
                _context: &mut ToolContext,
                _args: MockArgs,
            ) -> Result<MockStreamCall, MockError> {
                Ok(MockStreamCall { items: vec![] })
            }

            async fn convert_log(&self, log: Arc<RwLock<TenonLog>>) -> Option<ToolOutput> {
                let mut log = log.write().ok()?;
                let TenonLogData::Tool(tool_log) = &log.data else {
                    return None;
                };
                let tool_log = tool_log.clone();
                log.data = TenonLogData::Thought(TenonThoughtLog {
                    summary: None,
                    tool_log,
                });
                log.refresh();
                Some(ToolOutput::text("converted"))
            }
        }

        /// Fails in init_call.
        struct MockFailingTool;

        impl ToolCore for MockFailingTool {
            fn name(&self) -> String {
                "mock_failing_tool".to_string()
            }
            type Args = MockArgs;
            type Output = String;
            type Error = MockError;
            type Call = MockStreamCall;

            fn description(&self) -> String {
                "mock failing tool".to_string()
            }

            fn parameters(&self) -> serde_json::Value {
                serde_json::json!({})
            }

            async fn init_call(
                &self,
                _context: &mut ToolContext,
                _args: MockArgs,
            ) -> Result<MockStreamCall, MockError> {
                Err(MockError)
            }
        }

        #[tokio::test]
        async fn test_tenon_tool_creates_log_with_minted_id_and_result() {
            let log_window = test_log_window();
            let tool = TenonTool::new(MockStreamTool, log_window.clone());

            let result = execute_tenon_tool(tool, r#"{"input": "hello"}"#).await;
            assert!(result.is_success());
            assert_eq!(result.output().as_text(), Some("done"));

            let logs = tool_logs(&log_window);
            assert_eq!(logs.len(), 1);
            assert!(!logs[0].tool_call.id.is_empty());
            assert_eq!(logs[0].tool_call.name, "mock_stream_tool");
            assert_eq!(
                logs[0].tool_call.args,
                serde_json::json!({"input": "hello"})
            );
            let result = logs[0].tool_result.as_ref().unwrap().as_ref().unwrap();
            match result {
                TenonToolResult::Text(text) => assert_eq!(text.text, "done"),
                _ => panic!("expected text result"),
            }
        }

        #[tokio::test]
        async fn test_tenon_tool_streams_into_progress() {
            let log_window = test_log_window();
            let tool = TenonTool::new(MockStreamTool, log_window.clone());

            let result = execute_tenon_tool(tool, r#"{"input": "hello"}"#).await;
            assert!(result.is_success());

            let logs = tool_logs(&log_window);
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].progress, vec!["hello", "world"]);
        }

        /// The handle returned by create_tool_log mutates the log directly and
        /// the mutation is visible through the window, without touching the
        /// window lock.
        #[test]
        fn test_create_tool_log_handle_mutates_in_place() {
            let log_window = test_log_window();
            let tool = TenonTool::new(MockStreamTool, log_window.clone());

            let handle = tool
                .create_tool_log(&serde_json::json!({"input": "hello"}))
                .unwrap();

            {
                let mut log = handle.write().unwrap();
                log.append_tool_progress("chunk");
                log.append_tool_progress("multi\nline\ntext");
            }

            let logs = tool_logs(&log_window);
            assert_eq!(logs.len(), 1);
            assert!(!logs[0].tool_call.id.is_empty());
            assert_eq!(logs[0].progress, vec!["chunk", "multi", "line", "text"]);
        }

        #[tokio::test]
        async fn test_tenon_tool_returns_converted_result() {
            let log_window = test_log_window();
            let tool = TenonTool::new(MockConvertTool, log_window.clone());

            let result = execute_tenon_tool(tool, r#"{"input": "hello"}"#).await;
            assert!(result.is_success());
            // The returned output is the converted result, not the original
            assert_eq!(result.output().as_text(), Some("converted"));

            // The log was swapped to a Thought log; its embedded result is the
            // original one, written before the conversion
            let window = log_window.read().unwrap();
            let log = window.logs[0].log.read().unwrap();
            let TenonLogData::Thought(thought_log) = log.data() else {
                panic!("expected Thought log after convert_log");
            };
            let Some(Ok(TenonToolResult::Text(text))) = &thought_log.tool_log.tool_result else {
                panic!("expected embedded Text tool result");
            };
            assert_eq!(text.text, "done");
        }

        #[tokio::test]
        async fn test_tenon_tool_logs_error_result() {
            let log_window = test_log_window();
            let tool = TenonTool::new(MockFailingTool, log_window.clone());

            let result = execute_tenon_tool(tool, r#"{"input": "x"}"#).await;
            assert!(!result.is_success());

            let logs = tool_logs(&log_window);
            assert_eq!(logs.len(), 1);
            let err = logs[0].tool_result.as_ref().unwrap().as_ref().unwrap_err();
            assert!(!err.0.is_empty());
        }

        #[tokio::test]
        async fn test_tenon_tool_invalid_args_creates_log_with_error() {
            let log_window = test_log_window();
            let tool = TenonTool::new(MockStreamTool, log_window.clone());

            // Missing "input" field: deserialization fails inside the
            // DynamicTool callback
            let result = execute_tenon_tool(tool, r#"{"wrong": 1}"#).await;
            assert!(!result.is_success());

            let logs = tool_logs(&log_window);
            assert_eq!(logs.len(), 1);
            assert!(logs[0].tool_result.as_ref().unwrap().is_err());
        }
    }
}
