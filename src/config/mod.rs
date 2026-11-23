use std::collections::HashMap;

use serde::Deserialize;

use crate::{
    chat::TenonAgent,
    clients::{ProviderConfig, SelectedModel, SupportedModels},
};

#[cfg(test)]
use crate::clients::{AnthropicProviderConfig, ApiKey, OllamaProviderConfig, ProviderKind};

pub mod user;

#[derive(Debug, Clone)]
pub enum WebSearchConfig {
    Brave { api_key: String },
    LangSearch { api_key: String },
    Tavily { api_key: String },
}

#[derive(Debug, Clone)]
pub struct ToolsConfig {
    pub fetch_webpage: FetchWebpageConfig,
    pub read_image: ReadImageConfig,
    pub run_command: RunCommandConfig,
    pub web_search: Option<WebSearchConfig>,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            fetch_webpage: FetchWebpageConfig { model: None },
            read_image: ReadImageConfig::default(),
            run_command: RunCommandConfig::default(),
            web_search: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FetchWebpageConfig {
    pub model: Option<SelectedModel>,
}

#[derive(Debug, Clone, Default)]
pub struct ReadImageConfig {
    /// Vision model used when the chat's model has no vision.
    pub fallback_model: Option<SelectedModel>,
}

/// How a whitelist entry matches a command's argv.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WhitelistMatch {
    /// Command argv equals the entry's argv.
    #[default]
    Exact,
    /// Command argv starts with the entry's argv; empty argv matches every command.
    Prefix,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhitelistEntry {
    pub argv: Vec<String>,
    #[serde(default, rename = "match")]
    pub match_kind: WhitelistMatch,
}

#[derive(Debug, Clone, Default)]
pub struct RunCommandConfig {
    pub whitelist: Vec<WhitelistEntry>,
    pub check_models: Vec<SelectedModel>,
}

#[derive(Debug, Clone, Default)]
pub struct TitleConfig {
    pub model: Option<SelectedModel>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TenonConfig {
    pub connectors: HashMap<String, ProviderConfig>,
    pub agents: HashMap<String, TenonAgent>,
    pub default_agent: String,
    pub models: HashMap<String, SupportedModels>,
    pub tools: ToolsConfig,
    pub directory: String,
    pub title: TitleConfig,
    pub hooks: Vec<crate::hooks::Hook>,
}

#[cfg(test)]
impl Default for TenonConfig {
    // Test-only scaffold; production code must not depend on a default config.
    fn default() -> Self {
        let ollama_cloud_provider = ProviderConfig {
            kind: ProviderKind::Ollama(OllamaProviderConfig {
                base_url: "https://ollama.com".to_string(),
                ..Default::default()
            }),
            parameters: HashMap::new(),
        };
        let mut test_providers: HashMap<String, ProviderConfig> = HashMap::new();
        test_providers.insert("ollama_cloud".to_string(), ollama_cloud_provider.clone());
        test_providers.insert(
            "zai".to_string(),
            ProviderConfig {
                kind: ProviderKind::Anthropic(AnthropicProviderConfig {
                    base_url: "https://api.z.ai/api/anthropic".to_string(),
                    api_key: ApiKey::Env {
                        env: "ZAI_API_KEY".to_string(),
                    },
                }),
                parameters: HashMap::new(),
            },
        );
        let test_model = SupportedModels {
            connector_name: "ollama_cloud".to_string(),
            config: ollama_cloud_provider.clone(),
            model_name: "glm-5.1".to_string(),
            parameters: serde_json::Map::new(),
            vision: false,
        };
        let mut test_agents: HashMap<String, TenonAgent> = HashMap::new();
        let test_agent_name = "default".to_string();
        test_agents.insert(
            test_agent_name.clone(),
            TenonAgent::new(
                SelectedModel::select("default", &test_model),
                vec![],
                &[
                    "edit_file",
                    "fetch_webpage",
                    "list_files",
                    "read_file",
                    "remove_path",
                    "run_command",
                    "search_text",
                    "web_search",
                ],
                vec![],
            ),
        );
        let mut test_models: HashMap<String, SupportedModels> = HashMap::new();
        test_models.insert("default".to_string(), test_model);
        TenonConfig {
            connectors: test_providers,
            agents: test_agents,
            default_agent: test_agent_name,
            models: test_models,
            tools: ToolsConfig::default(),
            directory: ".tenon".to_string(),
            title: TitleConfig::default(),
            hooks: vec![],
        }
    }
}
