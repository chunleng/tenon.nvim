use nvim_oxi::Result as OxiResult;
use nvim_oxi::mlua::lua;

use serde_json::Value;

use crate::tools::McpClient;
use crate::utils::GLOBAL_EXECUTION_HANDLER;

/// Fetches MCP tools from the mcphub Lua runtime and wraps them as `McpClient`s.
pub struct McpHubCaller;

impl McpHubCaller {
    pub fn from_mcp_tools() -> OxiResult<Vec<McpClient>> {
        let lua_code = r#"local mcphub = require('mcphub').get_hub_instance()
if not mcphub then
    return {}
end

local tools = mcphub:get_tools()
local result = {}
for _, tool in ipairs(tools) do
    table.insert(result, {
        server_name = tool.server_name,
        name = tool.name,
        description = tool.description,
        inputSchema = tool.inputSchema,
    })
end
return result"#;

        let result: Value = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
            let val = lua()
                .load(lua_code)
                .eval::<mlua::Value>()
                .map_err(nvim_oxi::Error::Mlua)?;
            serde_json::to_value(&val)
                .map_err(|e| nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(e.to_string())))
        })?;

        let tools_array =
            result
                .as_array()
                .ok_or(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(
                    "Tools is not an array".into(),
                )))?;

        let mut mcp_tools = Vec::new();
        for tool in tools_array {
            let server_name =
                tool.get("server_name")
                    .and_then(|v| v.as_str())
                    .ok_or(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(
                        "Missing server_name".into(),
                    )))?;
            let tool_name =
                tool.get("name")
                    .and_then(|v| v.as_str())
                    .ok_or(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(
                        "Missing name".into(),
                    )))?;
            let description = tool
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let input_schema = tool
                .get("inputSchema")
                .ok_or(nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(
                    "Missing inputSchema".into(),
                )))?
                .clone();

            mcp_tools.push(McpClient::new(
                server_name,
                tool_name,
                description,
                input_schema,
            ));
        }

        Ok(mcp_tools)
    }
}
