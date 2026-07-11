pub mod analyze_image;
pub mod create_file;
pub mod edit_file;
pub mod end_workflow;
pub mod fetch_webpage;
pub mod list_files;
pub mod move_path;
pub mod navigate_workflow;
pub mod read_file;
pub mod remove_path;
pub mod run_command;
pub mod search_text;
pub mod start_workflow;
pub mod web_search;

use crate::mcp::McpHubCaller;
pub use analyze_image::AnalyzeImage;
pub use create_file::CreateFile;
pub use edit_file::EditFile;
pub use fetch_webpage::FetchWebpage;
pub use list_files::ListFiles;
pub use move_path::MovePath;
pub use read_file::ReadFile;
pub use remove_path::RemovePath;
use rig::{tool::ToolDyn, tools::ThinkTool};
pub use run_command::RunCommand;
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
/// - NonMutating: web_search, fetch_webpage, think
/// - Mutating: create_file, edit_file, move_path, remove_path, run_command
/// - System: start_workflow, navigate_workflow, end_workflow
///
/// Unknown tool names (including MCP tools) return `ToolClassification::Unknown`.
pub fn get_tool_classification(name: &str) -> ToolClassification {
    match name {
        // Idempotent tools: read-only, reproducible results
        "read_file" | "list_files" | "search_text" => ToolClassification::Idempotent,

        // Non-mutating tools: read-only, may produce different results
        "web_search" | "fetch_webpage" | "think" | "analyze_image" => {
            ToolClassification::NonMutating
        }

        // Mutating tools: modify state when run
        "create_file" | "edit_file" | "move_path" | "remove_path" | "run_command" => {
            ToolClassification::Mutating
        }

        // System tools: Tenon workflow management
        "start_workflow" | "navigate_workflow" | "end_workflow" => ToolClassification::System,

        // Unknown: MCP tools or unrecognized names
        _ => ToolClassification::Unknown,
    }
}

/// Returns a short human-readable summary of what a tool call is doing,
/// by extracting the core parameter from its args JSON.
///
/// Returns `None` for tools with no useful display arg (e.g. "think", MCP tools).
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
        "read_file" | "edit_file" | "create_file" | "remove_path" => "filepath",
        "move_path" => "source",
        "list_files" | "search_text" => "pattern",
        "fetch_webpage" => "url",
        "analyze_image" => "image",
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

/// Returns the names of all available tools (built-in + MCP).
///
/// Built-in names: "create_file", "edit_file", "fetch_webpage",
/// "list_files", "move_path", "read_file", "remove_path", "run_command", "search_text", "web_search", "think".
/// MCP tool names: "server_name.tool_name".
pub fn all_tool_names() -> Vec<String> {
    let mut names: Vec<String> = vec![
        "create_file".into(),
        "edit_file".into(),
        "fetch_webpage".into(),
        "list_files".into(),
        "move_path".into(),
        "read_file".into(),
        "remove_path".into(),
        "run_command".into(),
        "search_text".into(),
        "web_search".into(),
        "think".into(),
        "analyze_image".into(),
    ];

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

/// Resolve a list of tool name strings into concrete `Box<dyn ToolDyn>` instances.
///
/// Built-in names: "create_file", "edit_file", "fetch_webpage",
/// "list_files", "move_path", "read_file", "remove_path", "run_command", "search_text", "web_search", "think".
/// MCP tool names: "server_name.tool_name" for a specific tool,
/// or "server_name" to include all tools from that server.
pub fn resolve_tools(names: &[impl AsRef<str>]) -> Vec<Box<dyn ToolDyn>> {
    let name_refs: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();

    let mut all_tools: Vec<(String, Box<dyn ToolDyn>)> = vec![
        (
            "create_file".to_string(),
            Box::new(CreateFile) as Box<dyn ToolDyn>,
        ),
        (
            "edit_file".to_string(),
            Box::new(EditFile) as Box<dyn ToolDyn>,
        ),
        (
            "fetch_webpage".to_string(),
            Box::new(FetchWebpage) as Box<dyn ToolDyn>,
        ),
        (
            "analyze_image".to_string(),
            Box::new(AnalyzeImage) as Box<dyn ToolDyn>,
        ),
        (
            "list_files".to_string(),
            Box::new(ListFiles) as Box<dyn ToolDyn>,
        ),
        (
            "move_path".to_string(),
            Box::new(MovePath) as Box<dyn ToolDyn>,
        ),
        (
            "read_file".to_string(),
            Box::new(ReadFile) as Box<dyn ToolDyn>,
        ),
        (
            "remove_path".to_string(),
            Box::new(RemovePath) as Box<dyn ToolDyn>,
        ),
        (
            "run_command".to_string(),
            Box::new(RunCommand) as Box<dyn ToolDyn>,
        ),
        (
            "search_text".to_string(),
            Box::new(SearchText) as Box<dyn ToolDyn>,
        ),
        (
            "web_search".to_string(),
            Box::new(WebSearch) as Box<dyn ToolDyn>,
        ),
        ("think".to_string(), Box::new(ThinkTool) as Box<dyn ToolDyn>),
    ];

    if let Ok(mcp_tools) = McpHubCaller::from_mcp_tools() {
        for tool in mcp_tools {
            all_tools.push((tool.name(), Box::new(tool) as Box<dyn ToolDyn>));
        }
    }

    all_tools
        .into_iter()
        .filter(|(name, _)| tool_matches_selectors(name, &name_refs))
        .map(|(_, tool)| tool)
        .collect()
}
