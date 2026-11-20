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

    /// MCP tool semantics are unknown: take the least important role
    fn properties(&self, _args: &Value) -> crate::chat::log::LogProperties {
        crate::chat::log::LogProperties::UNKNOWN
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

    async fn result(self, context: &mut ToolContext) -> Result<Self::Output, Self::Error> {
        let (server, tool, args_json, auto_approved): (String, String, String, bool) =
            GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
                let lua = nvim_oxi::mlua::lua();
                Ok(lua.load(r#"
local server, tool, input = ...
assert(require('mcphub').get_hub_instance(), "MCPHub instance not available")
local params = require('mcphub.extensions.shared').parse_params({
    server_name = server, tool_name = tool, tool_input = vim.fn.json_decode(input)
}, "use_mcp_tool")
return params.server_name, params.tool_name, vim.fn.json_encode(params.arguments), not not params.is_auto_approved_in_server
"#).call((self.server_name, self.tool_name, self.args.to_string()))?)
            }).map_err(|e| ToolExecutionError::other(format!("Failed to prepare MCP call: {e}")))?;

        let name = format!("{server}.{tool}");
        check_approval(context, &name, &args_json, auto_approved).await?;

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

                    lua.load(
                        r#"
local server, tool, args, resolve = ...
local mcphub = assert(require('mcphub').get_hub_instance(), "MCPHub instance not available")
local opts = {parse_response = true, callback = function(response, err)
    if err and err ~= "" then
        resolve({error = err})
        return
    end
    resolve({response = response})
end}
mcphub:call_tool(server, tool, vim.fn.json_decode(args), opts)
"#,
                    )
                    .call::<()>((server, tool, args_json, resolve_fn))?;

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
                "MCP tool {name} failed: {error}"
            )));
        }

        let response = result.get("response").unwrap_or(&Value::Null).clone();

        Ok(ToolOutput::text(format_response(&response)))
    }
}

async fn check_approval(
    context: &mut ToolContext,
    name: &str,
    args_json: &str,
    auto_approved: bool,
) -> Result<(), ToolExecutionError> {
    use crate::utils::approval::{ApprovalResult, request_approval};

    if auto_approved {
        return Ok(());
    }
    let args = serde_json::from_str(args_json)
        .map_err(|e| ToolExecutionError::invalid_args(format!("Invalid MCP arguments: {e}")))?;
    match request_approval(
        context,
        name,
        args,
        format!("MCP call not approved: {name}"),
    )
    .await
    {
        ApprovalResult::Approved => Ok(()),
        ApprovalResult::Rejected(message) => Err(ToolExecutionError::other(message)),
    }
}

fn format_response(response: &Value) -> String {
    if let Some(object) = response.as_object()
        && object.len() == 4
        && let Some(text) = object.get("text").and_then(Value::as_str)
        && ["blobs", "images", "audios"]
            .iter()
            .all(|key| matches!(object.get(*key), Some(Value::Object(items)) if items.is_empty()))
    {
        return text.to_string();
    }

    serde_json::to_string_pretty(response).unwrap_or_else(|_| "{}".to_string())
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

    #[tokio::test]
    async fn only_server_exempt_calls_pass_without_an_approval_channel() {
        let mut context = ToolContext::new();
        let name = "weather.get_forecast";
        assert!(check_approval(&mut context, name, "{}", true).await.is_ok());
        assert!(
            check_approval(&mut context, name, "{}", false)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn approval_shows_resolved_call_and_requires_affirmative_response() {
        use crate::chat::{EventChannel, PendingAction};
        use crate::utils::approval::ApprovalResult;
        use std::sync::Arc;

        for verdict in [
            Some(ApprovalResult::Approved),
            Some(ApprovalResult::Rejected("denied".into())),
            None,
        ] {
            let approved = verdict == Some(ApprovalResult::Approved);
            let channel = Arc::new(EventChannel::<PendingAction>::new());
            let mut context = ToolContext::new().with_scope(Arc::new(Arc::downgrade(&channel)));
            let mut gate = Box::pin(check_approval(
                &mut context,
                "weather.get_forecast",
                r#"{"city":"Oslo"}"#,
                false,
            ));
            assert!(futures::poll!(&mut gate).is_pending());
            let Some(PendingAction::Approval(action)) = channel.peek() else {
                panic!("expected MCP approval");
            };
            assert_eq!(action.tool, "weather.get_forecast");
            assert_eq!(action.args, json!({"city":"Oslo"}));
            let tx = action.response_tx.lock().unwrap().take().unwrap();
            match verdict {
                Some(result) => tx.send(result).unwrap(),
                None => drop(tx),
            }
            assert_eq!(gate.await.is_ok(), approved);
        }
    }

    #[test]
    fn test_text_only_response_returns_raw_text() {
        for text in ["<RESULT>\n\"quoted\"", ""] {
            let response = json!({
                "blobs": {},
                "images": {},
                "text": text,
                "audios": {},
            });
            assert_eq!(format_response(&response), text);
        }
    }

    #[test]
    fn test_media_and_nonmatching_responses_remain_structured() {
        let text_only = json!({"blobs": {}, "images": {}, "text": "<RESULT>", "audios": {}});
        let mut responses = vec![
            json!(null),
            json!("plain text"),
            json!({"text": "<RESULT>"}),
        ];
        for key in ["blobs", "images", "audios"] {
            for media in [json!({"data": "media"}), json!(["media"]), json!(null)] {
                let mut response = text_only.clone();
                response[key] = media;
                responses.push(response);
            }
            let mut response = text_only.clone();
            response.as_object_mut().unwrap().remove(key);
            responses.push(response);
        }
        let mut non_string_text = text_only.clone();
        non_string_text["text"] = json!(["<RESULT>"]);
        responses.push(non_string_text);
        let mut extra_field = text_only;
        extra_field["metadata"] = json!({"important": true});
        responses.push(extra_field);

        for response in responses {
            let formatted = format_response(&response);
            assert_eq!(serde_json::from_str::<Value>(&formatted).unwrap(), response);
            assert_eq!(formatted, serde_json::to_string_pretty(&response).unwrap());
        }
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
