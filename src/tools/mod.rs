pub mod analyze_image;
pub mod ask_question;
pub mod edit_file;
pub mod end_workflow;
pub mod fetch_webpage;
pub mod list_files;
pub mod move_path;
pub mod navigate_workflow;
pub mod read_file;
pub mod record_thought;
pub mod remove_path;
pub mod run_command;
pub mod search_dependency_code;
pub mod search_text;
pub mod start_workflow;
pub mod web_search;

use crate::tools::web_search::{LangSearch, Tavily};
use crate::{config::WebSearchConfig, mcp::McpHubCaller, tools::web_search::Brave};
pub use analyze_image::AnalyzeImage;
pub use ask_question::AskQuestion;
pub use edit_file::EditFile;
pub use fetch_webpage::FetchWebpage;
pub use list_files::ListFiles;
pub use move_path::MovePath;
pub use read_file::ReadFile;
pub use record_thought::RecordThought;
pub use remove_path::RemovePath;
use rig::tool::{DynamicTool, IntoToolOutput, Tool, ToolExecutionError};
pub use run_command::RunCommand;
pub use search_dependency_code::SearchDependencyCode;
pub use search_text::SearchText;
pub use web_search::WebSearch;

use serde_json::Value;

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

    /// Tenon system tools for workflow management.
    System,

    /// Tools with unknown classification (e.g., MCP tools).
    Unknown,
}

/// Returns the classification for a tool by its name.
///
/// Built-in tools have fixed classifications:
/// - Idempotent: read_file, list_files, search_text
/// - NonMutating: web_search, fetch_webpage, record_thought
/// - Mutating: edit_file, move_path, remove_path, run_command
/// - System: start_workflow, navigate_workflow, end_workflow
///
/// Unknown tool names (including MCP tools) return `ToolClassification::Unknown`.
pub fn get_tool_classification(name: &str) -> ToolClassification {
    match name {
        // Idempotent tools: read-only, reproducible results
        "read_file" | "list_files" | "search_text" | "search_dependency_code" => {
            ToolClassification::Idempotent
        }

        // Non-mutating tools: read-only, may produce different results
        "web_search" | "fetch_webpage" | "record_thought" | "analyze_image" | "ask_question" => {
            ToolClassification::NonMutating
        }

        // Mutating tools: modify state when run
        "edit_file" | "move_path" | "remove_path" | "run_command" => ToolClassification::Mutating,

        // System tools: Tenon workflow management
        "start_workflow" | "navigate_workflow" | "end_workflow" => ToolClassification::System,

        // Unknown: MCP tools or unrecognized names
        _ => ToolClassification::Unknown,
    }
}

/// Returns a short human-readable summary of what a tool call is doing,
/// by extracting the core parameter from its args JSON.
///
/// Returns `None` for tools with no useful display arg (e.g. "record_thought", MCP tools).
pub fn tool_display_summary(name: &str, args: &Value) -> Option<String> {
    // Special case for "run_command": combine command + args for display
    if name == "run_command" {
        let command = args.get("command").and_then(|v| v.as_str())?;
        let display = if let Some(args_list) = args.get("args").and_then(|v| v.as_array()) {
            let args_str = args_list
                .iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if args_str.is_empty() {
                command.to_string()
            } else {
                format!("{} {}", command, args_str)
            }
        } else {
            command.to_string()
        };
        let display = display.lines().collect::<Vec<_>>().join("↵");
        return Some(format!("command: {}", display));
    }

    let core_arg: &str = match name {
        "web_search" => "query",
        "read_file" | "edit_file" | "remove_path" => "filepath",
        "move_path" => "source",
        "list_files" | "search_text" => "pattern",
        "search_dependency_code" => "dependency",
        "fetch_webpage" => "url",
        "analyze_image" => "image",
        "ask_question" => "question",
        "navigate_workflow" => "step",
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
            names.push(tool.tool_name());
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

/// Wrap a built-in `Tool` implementation into a `DynamicTool` for runtime
/// registration.
pub(crate) fn into_dynamic_tool<T: Tool + 'static>(tool: T) -> DynamicTool {
    let name = T::NAME.to_string();
    let description = tool.description();
    let parameters = tool.parameters();
    let tool = std::sync::Arc::new(tool);

    DynamicTool::new(name, description, parameters, move |context, args| {
        let tool = std::sync::Arc::clone(&tool);
        Box::pin(async move {
            let args: T::Args = serde_json::from_value(args).map_err(|e| {
                ToolExecutionError::invalid_args(format!("Failed to deserialize args: {}", e))
            })?;
            let output = tool
                .call(context, args)
                .await
                .map_err(|e| tool.map_error(e))?;
            output.into_tool_output()
        })
    })
}

/// Build the list of built-in tools (excluding MCP tools).
fn builtin_tools() -> Vec<Option<(String, DynamicTool)>> {
    let mut all_tools: Vec<Option<(String, DynamicTool)>> = vec![
        Some(("edit_file".to_string(), into_dynamic_tool(EditFile))),
        Some(("fetch_webpage".to_string(), into_dynamic_tool(FetchWebpage))),
        Some(("analyze_image".to_string(), into_dynamic_tool(AnalyzeImage))),
        Some(("list_files".to_string(), into_dynamic_tool(ListFiles))),
        Some(("move_path".to_string(), into_dynamic_tool(MovePath))),
        Some(("read_file".to_string(), into_dynamic_tool(ReadFile))),
        Some(("remove_path".to_string(), into_dynamic_tool(RemovePath))),
        Some(("run_command".to_string(), into_dynamic_tool(RunCommand))),
        Some((
            "search_dependency_code".to_string(),
            into_dynamic_tool(SearchDependencyCode),
        )),
        Some(("search_text".to_string(), into_dynamic_tool(SearchText))),
    ];

    if let Some(web_search_config) = &crate::get_application_config().tools.web_search {
        let provider: Box<dyn web_search::SearchProvider> = match web_search_config {
            WebSearchConfig::Brave { api_key } => Box::new(Brave {
                api_key: api_key.clone(),
            }),
            WebSearchConfig::LangSearch { api_key } => Box::new(LangSearch {
                api_key: api_key.clone(),
            }),
            WebSearchConfig::Tavily { api_key } => Box::new(Tavily {
                api_key: api_key.clone(),
            }),
        };
        all_tools.push(Some((
            "web_search".to_string(),
            into_dynamic_tool(WebSearch { provider }),
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
pub fn resolve_tools(names: &[impl AsRef<str>]) -> Vec<DynamicTool> {
    let name_refs: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();

    let mut all_tools = builtin_tools();

    if let Ok(mcp_tools) = McpHubCaller::from_mcp_tools() {
        for tool in mcp_tools {
            all_tools.push(Some((tool.tool_name(), tool.into_dynamic_tool())));
        }
    }

    select_in_order(all_tools, &name_refs)
}

// Test mock: resolves built-in tools only, skipping MCP tools.
// The real implementation calls McpHubCaller::from_mcp_tools() which requires
// a Neovim context (GLOBAL_EXECUTION_HANDLER), causing panics in unit tests.
#[cfg(test)]
pub fn resolve_tools(names: &[impl AsRef<str>]) -> Vec<DynamicTool> {
    let name_refs: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();
    select_in_order(builtin_tools(), &name_refs)
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
}
