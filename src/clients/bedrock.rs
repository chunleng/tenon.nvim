use rig::{
    agent::Agent,
    client::{CompletionClient, ProviderClient},
    tool::ToolDyn,
};

pub fn get_bedrock_agent(
    model_name: String,
    preamble: Option<String>,
    tools: Vec<Box<dyn ToolDyn>>,
    thinking: bool,
) -> Agent<rig_bedrock::completion::CompletionModel> {
    // There's no config provider because bedrock is configured solely by env. Following are some
    // environment that you can override to provide the necessary configuration to bedrock (apart
    // from the standard env like AWS_REGION)
    // - AWS_ENDPOINT_URL_BEDROCK_RUNTIME
    // - AWS_BEARER_TOKEN_BEDROCK
    let client = rig_bedrock::client::Client::from_env()
        .expect("Failed to create Bedrock client from environment");
    let mut agent = client.agent(model_name.clone());
    if let Some(p) = preamble {
        agent = agent.preamble(&p);
    }
    if model_name.contains("anthropic.claude") {
        agent = agent.max_tokens(16000);
        // https://docs.aws.amazon.com/bedrock/latest/userguide/claude-messages-extended-thinking.html
        match model_name {
            x if x.contains("opus-4-5")
                || x.contains("sonnet-4-5")
                || x.contains("haiku-4-5")
                || x.contains("opus-4-6")
                || x.contains("sonnet-4-6") =>
            {
                if thinking {
                    agent = agent.additional_params(serde_json::json!({
                        "thinking": { "type": "enabled", "budget_tokens": 10000 }
                    }));
                }
            }
            _ => {}
        }
    }

    agent
        .tools(tools)
        .add_hook(crate::clients::InvalidToolCallHook)
        .add_hook(crate::clients::ToolErrorHook)
        .build()
}
