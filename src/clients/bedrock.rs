use rig::{
    agent::Agent,
    client::{AgentClientExt, ProviderClient},
    tool::DynamicTool,
};

pub fn get_bedrock_agent(
    model_name: String,
    preamble: Option<String>,
    tools: Vec<DynamicTool>,
    mut params: serde_json::Map<String, serde_json::Value>,
) -> Agent {
    // There's no config provider because bedrock is configured solely by env. Following are some
    // environment that you can override to provide the necessary configuration to bedrock (apart
    // from the standard env like AWS_REGION)
    // - AWS_ENDPOINT_URL_BEDROCK_RUNTIME
    // - AWS_BEARER_TOKEN_BEDROCK
    let client = rig_bedrock::client::Client::from_env()
        .expect("Failed to create Bedrock client from environment");
    let mut agent = client.agent(model_name);
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    if let Some(max_tokens) = params.remove("max_tokens").and_then(|v| v.as_u64()) {
        agent = agent.max_tokens(max_tokens);
    }
    if !params.is_empty() {
        agent = agent.additional_params(serde_json::Value::Object(params));
    }

    agent
        .dynamic_tools(tools)
        .add_hook(crate::clients::InvalidToolCallHook)
        .add_hook(crate::clients::ToolErrorHook)
        .build()
}
