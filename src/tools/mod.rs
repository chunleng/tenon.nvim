pub mod create_file;
pub mod edit_file;
pub mod fetch_webpage;
pub mod list_file;
pub mod read_file;

use crate::mcp::McpHubCaller;
pub use create_file::CreateFile;
pub use edit_file::EditFile;
pub use fetch_webpage::FetchWebpage;
pub use list_file::ListFile;
pub use read_file::ReadFile;
use rig::{tool::ToolDyn, tools::ThinkTool};

/// Resolve a list of tool name strings into concrete `Box<dyn ToolDyn>` instances.
///
/// Built-in names: `"create_file"`, `"edit_file"`, `"fetch_webpage"`,
/// `"list_file"`, `"read_file"`, `"think"`.
/// MCP tool names follow the `"server_name.tool_name"` format.
/// Unknown names are silently skipped.
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
            "list_file".to_string(),
            Box::new(ListFile) as Box<dyn ToolDyn>,
        ),
        (
            "read_file".to_string(),
            Box::new(ReadFile) as Box<dyn ToolDyn>,
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
        .filter(|(name, _)| name_refs.contains(&name.as_str()))
        .map(|(_, tool)| tool)
        .collect()
}
