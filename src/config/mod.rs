use std::collections::HashMap;

use crate::{
    chat::TenonAgent,
    clients::{
        AnthropicProviderConfig, ApiKey, OllamaProviderConfig, ProviderConfig, SupportedModels,
    },
};

pub mod user;

#[derive(Debug, Clone)]
pub struct ToolsConfig {
    pub fetch_webpage: FetchWebpageConfig,
    pub analyze_image: AnalyzeImageConfig,
    pub run_command: RunCommandConfig,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            fetch_webpage: FetchWebpageConfig { model: None },
            analyze_image: AnalyzeImageConfig { model: None },
            run_command: RunCommandConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct FetchWebpageConfig {
    pub model: Option<SupportedModels>,
}

#[derive(Debug, Clone, Default)]
pub struct AnalyzeImageConfig {
    pub model: Option<SupportedModels>,
}

#[derive(Debug, Clone, Default)]
pub struct RunCommandConfig {
    pub whitelist: Vec<String>,
    pub check_models: Vec<SupportedModels>,
}

#[derive(Debug, Clone)]
pub struct HistoryConfig {
    pub directory: String,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            directory: ".tenon/history".to_string(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct TitleConfig {
    pub model: Option<SupportedModels>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TenonConfig {
    pub connectors: HashMap<String, ProviderConfig>,
    pub agents: HashMap<String, TenonAgent>,
    pub default_agent: String,
    pub models: Vec<SupportedModels>,
    pub tools: ToolsConfig,
    pub history: HistoryConfig,
    pub title: TitleConfig,
}

impl Default for TenonConfig {
    fn default() -> Self {
        let ollama_cloud_provider = ProviderConfig::Ollama(OllamaProviderConfig {
            base_url: "https://ollama.com".to_string(),
            ..Default::default()
        });
        let mut default_providers: HashMap<String, ProviderConfig> = HashMap::new();
        default_providers.insert("ollama_cloud".to_string(), ollama_cloud_provider.clone());
        default_providers.insert(
            "zai".to_string(),
            ProviderConfig::Anthropic(AnthropicProviderConfig {
                base_url: "https://api.z.ai/api/anthropic".to_string(),
                api_key: ApiKey::Env {
                    env: "ZAI_API_KEY".to_string(),
                },
            }),
        );
        let default_model = SupportedModels {
            connector_name: "ollama_cloud".to_string(),
            config: ollama_cloud_provider.clone(),
            model_name: "glm-5.1".to_string(),
            default_parameters: serde_json::Map::new(),
        };
        let mut default_agents: HashMap<String, TenonAgent> = HashMap::new();
        let default_agent_name = "default".to_string();
        default_agents.insert(
            default_agent_name.clone(),
            TenonAgent::new(
                default_model.clone(),
                vec![],
                &[
                    "create_file",
                    "edit_file",
                    "fetch_webpage",
                    "list_files",
                    "read_file",
                    "remove_path",
                    "run_command",
                    "search_text",
                    "web_search",
                    "record_thought",
                ],
                vec![],
            ),
        );
        TenonConfig {
            connectors: default_providers,
            agents: default_agents,
            default_agent: default_agent_name,
            models: vec![default_model],
            tools: ToolsConfig::default(),
            history: HistoryConfig::default(),
            title: TitleConfig::default(),
        }
    }
}
