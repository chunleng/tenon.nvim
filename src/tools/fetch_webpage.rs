use crate::clients::{BehaviorSource, get_agent};
use crate::get_application_config;
use html_to_markdown_rs::{ConversionOptions, PreprocessingOptions, PreprocessingPreset};
use rig::completion::ToolDefinition;
use rig::tool::{Tool, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize)]
pub struct FetchWebpageArgs {
    pub url: String,
    pub prompt: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct FetchWebpage;

impl Tool for FetchWebpage {
    const NAME: &'static str = "fetch_webpage";
    type Error = ToolError;
    type Args = FetchWebpageArgs;
    type Output = String;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: "fetch_webpage".to_string(),
            description: "Fetch webpage → readable text. With prompt: answer based on content. Without prompt: full markdown.".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "URL"
                    },
                    "prompt": {
                        "type": "string",
                        "description": "What to extract/answer. Returns only the answer. Scalar: fact/yes-no. Structured: table/steps/kvpairs. Compressed: summary/takeaways/translation. Filtered: return portion of document."
                    }
                },
                "required": ["url"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let html = reqwest::get(&args.url)
            .await
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Fetch failed: '{}' → {}", args.url, e),
                )))
            })?
            .text()
            .await
            .map_err(|e| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!("Read body failed: {}", e),
                )))
            })?;

        let markdown = html_to_markdown_rs::convert(
            &html,
            Some(ConversionOptions {
                preprocessing: PreprocessingOptions {
                    enabled: true,
                    preset: PreprocessingPreset::Aggressive,
                    remove_navigation: true,
                    remove_forms: true,
                },
                ..Default::default()
            }),
        )
        .map_err(|e| {
            ToolError::ToolCallError(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("HTML→markdown failed: {}", e),
            )))
        })?;

        match args.prompt {
            Some(prompt) => answer_with_prompt(&markdown, &prompt).await,
            None => Ok(markdown),
        }
    }
}

async fn answer_with_prompt(markdown: &str, prompt: &str) -> Result<String, ToolError> {
    let config = get_application_config();
    let model = match &config.tools.fetch_webpage.model {
        Some(m) => m.clone(),
        None => {
            let agent_config = config.agents.get(&config.default_agent).ok_or_else(|| {
                ToolError::ToolCallError(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "No default agent",
                )))
            })?;
            agent_config.model.clone()
        }
    };

    let behavior = BehaviorSource::Text {
        value: "Webpage content only. No preamble/hedge/commentary/source refs. Preserve format: code→code blocks, steps→numbered lists, comparisons→tables, items→bullets. Caveman mode".to_string(),
    };

    let caveman_mode_behavior = BehaviorSource::Text {
        value: "Caveman mode. Short sentences. Drop filler (the/a/an/is/are). Symbols > words (→/=/vs). No politeness. Max meaning/token".to_string()
    };

    let agent = get_agent(model, vec![behavior, caveman_mode_behavior], vec![]);

    let user_message = format!("{}\n\nWebpage content:\n\n{}", prompt, markdown);

    let response = agent.chat(user_message).await.map_err(|e| {
        ToolError::ToolCallError(Box::new(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Extraction agent failed: {}", e),
        )))
    })?;

    Ok(response)
}
