use nvim_oxi::Result as OxiResult;
use nvim_oxi::mlua::LuaSerdeExt;
use rig::tool::{ToolContext, ToolExecutionError, ToolOutput};
use serde_json::Value;

use crate::tools::{ToolCore, ToolCoreCall};
use crate::utils::GLOBAL_EXECUTION_HANDLER;

/// An MCP tool exposed by a mcphub server, routed through `TenonTool`.
///
/// `Call = Self`: per-call state (`args`) is stored on a clone made in
/// `init_call`, so the same struct serves as both tool and call.
#[derive(Clone)]
pub struct McpClient {
    server_name: String,
    tool_name: String,
    description: String,
    input_schema: Value,
    args: Value,
}

impl McpClient {
    pub fn new(
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
            args: Value::Null,
        }
    }
}

impl ToolCore for McpClient {
    fn name(&self) -> String {
        format!("{}____{}", self.server_name, self.tool_name)
    }

    type Args = Value;
    type Output = ToolOutput;
    type Error = ToolExecutionError;
    type Call = McpClient;

    fn description(&self) -> String {
        self.description.clone()
    }

    fn parameters(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    async fn init_call(
        &self,
        _context: &mut ToolContext,
        args: Self::Args,
    ) -> Result<Self::Call, Self::Error> {
        Ok(Self {
            args,
            ..self.clone()
        })
    }
}

impl ToolCoreCall for McpClient {
    type Output = ToolOutput;
    type Error = ToolExecutionError;

    async fn result(self, _context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let args_json = serde_json::to_string(&self.args)
            .map_err(|e| {
                ToolExecutionError::invalid_args(format!("Failed to serialize args: {}", e))
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
            args_json, self.server_name, self.tool_name
        );

        let result = GLOBAL_EXECUTION_HANDLER
            .execute_rust_on_main_thread_async(move |resolver| {
                let lua = nvim_oxi::mlua::lua();
                let resolver_clone = resolver.clone();

                let result: OxiResult<()> = (|| {
                    let lua_clone = lua.clone();
                    let resolve_fn = lua.create_function(move |_, value: mlua::Value| {
                        let json_val = lua_clone
                            .from_value::<Value>(value)
                            .map_err(nvim_oxi::Error::Mlua);
                        resolver.resolve(json_val);
                        Ok(())
                    })?;

                    let wrapped = format!("return (function(resolve)\n{}\nend)(...)", lua_code);
                    lua.load(&wrapped).call::<()>(resolve_fn)?;

                    Ok(())
                })();

                if let Err(e) = result {
                    resolver_clone.resolve(Err(e));
                }
            })
            .await
            .map_err(|e| ToolExecutionError::other(format!("Failed to execute Lua code: {}", e)))?;

        if let Some(error) = result.get("error").and_then(|v| v.as_str()) {
            return Err(ToolExecutionError::other(format!(
                "MCP tool {}:{} failed: {}",
                self.server_name, self.tool_name, error
            )));
        }

        let response = result.get("response").unwrap_or(&Value::Null).clone();

        Ok(ToolOutput::text(
            serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{}".to_string()),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolCore;
    use rig::tool::ToolContext;
    use serde_json::json;

    fn test_client() -> McpClient {
        McpClient::new(
            "weather",
            "get_forecast",
            "Get weather forecast",
            json!({"type": "object"}),
        )
    }

    #[test]
    fn test_name_uses_server_tool_separator() {
        assert_eq!(test_client().name(), "weather____get_forecast");
    }

    #[test]
    fn test_description_and_parameters_passthrough() {
        let client = test_client();
        assert_eq!(client.description(), "Get weather forecast");
        assert_eq!(client.parameters(), json!({"type": "object"}));
    }

    #[tokio::test]
    async fn test_init_call_stores_args() {
        let client = test_client();
        let mut context = ToolContext::new();
        let call = client
            .init_call(&mut context, json!({"city": "Oslo"}))
            .await
            .unwrap();
        assert_eq!(call.args, json!({"city": "Oslo"}));
    }
}
