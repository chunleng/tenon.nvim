use nvim_oxi::Result as OxiResult;
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde_json::Value;

use crate::utils::GLOBAL_EXECUTION_HANDLER;

#[derive(Clone)]
pub struct McpHubCaller {
    server_name: String,
    tool_name: String,
    description: String,
    input_schema: Value,
}

impl McpHubCaller {
    fn new(
        server_name: impl Into<String>,
        tool_name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            server_name: server_name.into(),
            tool_name: tool_name.into(),
            description: description.into(),
            input_schema,
        }
    }

    pub fn from_mcp_tools() -> OxiResult<Vec<Self>> {
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

        let result = GLOBAL_EXECUTION_HANDLER.execute_on_main_thread(lua_code)?;

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

            mcp_tools.push(McpHubCaller::new(
                server_name,
                tool_name,
                description,
                input_schema,
            ));
        }

        Ok(mcp_tools)
    }
}

impl Tool for McpHubCaller {
    const NAME: &'static str = "mcp_tool";
    type Error = ToolError;
    type Args = Value;
    type Output = String;

    fn name(&self) -> String {
        format!("{}.{}", self.server_name, self.tool_name)
    }

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: self.name(),
            description: self.description.clone(),
            parameters: self.input_schema.clone(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let server_name = self.server_name.clone();
        let tool_name = self.tool_name.clone();

        let args_json = serde_json::to_string(&args)
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("Failed to serialize args: {}", e),
                )))
            })?
            .replace("\\", "\\\\")
            .replace("\"", "\\\"");

        let lua_code = format!(
            r#"
local mcphub = require('mcphub').get_hub_instance()
if not mcphub then
    resolve({{error = "MCPHub instance not available"}})
    return
end
local args = vim.fn.json_decode("{}")
local shared = require("mcphub.extensions.shared")
local params = shared.parse_params({{server_name = "{}", tool_name = "{}", tool_input = args}}, "use_mcp_tool")
if not params.is_auto_approved_in_server then
    local args_str = vim.fn.json_encode(params.arguments)
    local choice = vim.fn.confirm("Run " .. params.server_name .. "." .. params.tool_name .. "?\nArgs: " .. args_str, "&Yes\n&No", 1)
    if choice ~= 1 then
        resolve({{error = "User denied the tool run"}})
        return
    end
end

local opts = {{parse_response = true, callback = function(response, err)
    if err and err ~= "" then
        resolve({{error = err}})
        return
    end
    resolve({{response = response}})
end}}
mcphub:call_tool(params.server_name, params.tool_name, params.arguments, opts)
"#,
            args_json, server_name, tool_name
        );

        let result = GLOBAL_EXECUTION_HANDLER
            .execute_on_main_thread_async(&lua_code)
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Failed to execute Lua code: {}", e),
                )))
            })?;

        if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
            return Err(ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("MCP tool {}:{} failed: {}", server_name, tool_name, error),
            ))));
        }

        let response = result.get("response").unwrap_or(&Value::Null).clone();

        Ok(serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{}".to_string()))
    }
}
