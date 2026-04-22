pub mod create_file;
pub mod edit_file;
pub mod fetch_webpage;
pub mod list_files;
pub mod read_file;
pub mod remove_path;
pub mod search_text;
pub mod web_search;

use crate::mcp::McpHubCaller;
pub use create_file::CreateFile;
pub use edit_file::EditFile;
pub use fetch_webpage::FetchWebpage;
pub use list_files::ListFiles;
pub use read_file::ReadFile;
pub use remove_path::RemovePath;
use rig::{tool::ToolDyn, tools::ThinkTool};
pub use search_text::SearchText;
pub use web_search::WebSearch;

/// Returns the names of all available tools (built-in + MCP).
///
/// Built-in names: "create_file", "edit_file", "fetch_webpage",
/// "list_files", "read_file", "remove_file", "search_text", "web_search", "think".
/// MCP tool names: "server_name.tool_name".
pub fn all_tool_names() -> Vec<String> {
    let mut names: Vec<String> = vec![
        "create_file".into(),
        "edit_file".into(),
        "fetch_webpage".into(),
        "list_files".into(),
        "read_file".into(),
        "remove_path".into(),
        "search_text".into(),
        "web_search".into(),
        "think".into(),
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
        if r.contains('.') {
            r == name
        } else {
            r == name || name.starts_with(&format!("{}.", r))
        }
    })
}

/// Resolve a list of tool name strings into their expanded concrete names.
///
/// Applies the same matching rules as [`resolve_tools`] but returns just the
/// names, without instantiating tool objects. Useful for comparison / display.
pub fn resolve_tool_names(names: &[impl AsRef<str>]) -> Vec<String> {
    let selectors: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();
    all_tool_names()
        .into_iter()
        .filter(|name| tool_matches_selectors(name, &selectors))
        .collect()
}

/// Resolve a list of tool name strings into concrete `Box<dyn ToolDyn>` instances.
///
/// Built-in names: "create_file", "edit_file", "fetch_webpage",
/// "list_files", "read_file", "remove_path", "web_search", "think".
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
            "list_files".to_string(),
            Box::new(ListFiles) as Box<dyn ToolDyn>,
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
