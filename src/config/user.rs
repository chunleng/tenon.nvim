use std::{collections::HashMap, path::PathBuf};

use nvim_oxi::{api::types::LogLevel, serde::DeserializeError};
use serde::Deserialize;

use crate::{
    chat::TenonAgent,
    clients::{ApiKey, ProviderConfig, SupportedModels},
    config::{TenonConfig, WebSearchConfig},
    directive::{Directive, DirectiveSource},
    utils::notify,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenonUserConfig {
    pub connectors: Option<HashMap<String, ProviderConfig>>,
    pub agents: Option<HashMap<String, TenonAgentConfig>>,
    pub models: Option<HashMap<String, ModelConfig>>,
    pub tools: Option<ToolsUserConfig>,
    pub history: Option<HistoryUserConfig>,
    pub title: Option<TitleUserConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryUserConfig {
    pub directory: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TitleUserConfig {
    pub model: Option<Model>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase", deny_unknown_fields)]
pub enum WebSearchProviderConfig {
    Brave { api_key: ApiKey },
    LangSearch { api_key: ApiKey },
    Tavily { api_key: ApiKey },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsUserConfig {
    pub fetch_webpage: Option<FetchWebpageUserConfig>,
    pub analyze_image: Option<AnalyzeImageUserConfig>,
    pub run_command: Option<RunCommandUserConfig>,
    pub web_search: Option<WebSearchProviderConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommandUserConfig {
    pub whitelist: Vec<String>,

    #[serde(default)]
    pub check_models: Vec<Model>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchWebpageUserConfig {
    pub model: Option<Model>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeImageUserConfig {
    pub model: Option<Model>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TenonAgentConfig {
    model: Model,
    #[serde(default)]
    directive: Vec<DirectiveConfig>,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    choreos: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    connector: String,
    name: String,
    #[serde(default)]
    default_parameters: serde_json::Map<String, serde_json::Value>,
}

/// A named reference to a model in the models registry.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct Model(pub String);

/// Describes a directive source as provided in user configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum DirectiveSourceConfig {
    /// An inline string.
    Text { value: String },

    /// File paths.
    File { paths: Vec<PathBuf> },

    /// Reference to a system directive by name.
    System { name: String },
}

/// A directive as provided in user configuration, with an optional condition.
#[derive(Debug, Clone, Deserialize)]
pub struct DirectiveConfig {
    /// Optional condition for conditional directive application.
    #[serde(default)]
    pub condition: Option<String>,
    /// The source of the directive content.
    #[serde(flatten)]
    pub source: DirectiveSourceConfig,
}

impl TryFrom<DirectiveConfig> for Directive {
    type Error = std::io::Error;

    fn try_from(config: DirectiveConfig) -> Result<Self, Self::Error> {
        let source = match config.source {
            DirectiveSourceConfig::Text { value } => DirectiveSource::Text { value },
            DirectiveSourceConfig::File { paths } => DirectiveSource::File { paths },
            DirectiveSourceConfig::System { name } => {
                let registry = crate::get_directive_registry();
                registry
                    .get(&name)
                    .map(|d| d.source.clone())
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("System directive '{}' not found", name),
                        )
                    })?
            }
        };
        Ok(Directive {
            condition: config.condition,
            source,
        })
    }
}

impl TryFrom<TenonUserConfig> for TenonConfig {
    type Error = nvim_oxi::Error;
    fn try_from(value: TenonUserConfig) -> Result<Self, Self::Error> {
        let mut conf = TenonConfig::default();
        let mut default_agent = None;

        if let Some(connectors) = value.connectors {
            conf.connectors = connectors;
        }
        if let Some(models) = value.models {
            conf.models = models
                .into_iter()
                .map(
                    |(name, m)| -> Result<(String, SupportedModels), nvim_oxi::Error> {
                        let provider_config: &ProviderConfig = conf
                            .connectors
                            .get(&m.connector)
                            .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                                msg: format!("unknown connector for model: {}", m.connector),
                            }))?;
                        Ok((
                            name,
                            SupportedModels {
                                connector_name: m.connector.clone(),
                                config: provider_config.to_owned(),
                                model_name: m.name,
                                default_parameters: m.default_parameters,
                            },
                        ))
                    },
                )
                .collect::<Result<HashMap<_, _>, _>>()?;
        }

        if let Some(agents) = value.agents {
            if agents.is_empty() {
                return Err(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                    msg: "agents cannot be empty".to_string(),
                }));
            }
            conf.agents = agents
                .into_iter()
                .map(|(k, v)| -> Result<_, nvim_oxi::Error> {
                    let model = conf
                        .models
                        .get(&v.model.0)
                        .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                            msg: format!("unknown model: {}", v.model.0),
                        }))?;
                    if v.default {
                        match &default_agent {
                            Some(agent) => {
                                return Err(nvim_oxi::Error::Deserialize(
                                    DeserializeError::Custom {
                                        msg: format!(
                                            "more than one default agents found: {} and {}",
                                            agent, &k
                                        ),
                                    },
                                ));
                            }
                            None => {
                                default_agent = Some(k.to_string());
                            }
                        }
                    }
                    let directives = v
                        .directive
                        .into_iter()
                        .map(Directive::try_from)
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| {
                            nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                                msg: e.to_string(),
                            })
                        })?;
                    let choreos: Vec<std::sync::Arc<crate::chat::choreo::Choreo>> = v
                        .choreos
                        .into_iter()
                        .filter_map(|id| crate::get_choreo_registry().get(&id).cloned())
                        .collect();
                    Ok((
                        k,
                        TenonAgent::new(model.clone(), directives, &v.tool_names, choreos),
                    ))
                })
                .collect::<Result<HashMap<_, _>, _>>()?;
            match default_agent {
                Some(x) => conf.default_agent = x,
                None => {
                    return Err(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                        msg: "at least one agent needs to be set as default".to_string(),
                    }));
                }
            }
        }

        if let Some(tools) = value.tools {
            if let Some(fetch_webpage) = tools.fetch_webpage
                && let Some(model) = fetch_webpage.model
            {
                conf.tools.fetch_webpage.model = Some(conf.models.get(&model.0).cloned().ok_or(
                    nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                        msg: format!("unknown model for fetch_webpage: {}", model.0),
                    }),
                )?);
            }
            if let Some(analyze_image) = tools.analyze_image
                && let Some(model) = analyze_image.model
            {
                conf.tools.analyze_image.model = Some(conf.models.get(&model.0).cloned().ok_or(
                    nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                        msg: format!("unknown model for analyze_image: {}", model.0),
                    }),
                )?);
            }
            if let Some(run) = tools.run_command {
                conf.tools.run_command.whitelist = run.whitelist;
                conf.tools.run_command.check_models = run
                    .check_models
                    .into_iter()
                    .map(|m| -> Result<_, nvim_oxi::Error> {
                        conf.models
                            .get(&m.0)
                            .cloned()
                            .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                                msg: format!("unknown model for run_command check: {}", m.0),
                            }))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            }
            conf.tools.web_search = match tools.web_search {
                Some(WebSearchProviderConfig::Brave { api_key }) => match api_key.resolve() {
                    Ok(key) => Some(WebSearchConfig::Brave { api_key: key }),
                    Err(e) => {
                        notify(format!("[tenon] web_search: {}", e), LogLevel::Warn);
                        None
                    }
                },
                Some(WebSearchProviderConfig::LangSearch { api_key }) => match api_key.resolve() {
                    Ok(key) => Some(WebSearchConfig::LangSearch { api_key: key }),
                    Err(e) => {
                        notify(format!("[tenon] web_search: {}", e), LogLevel::Warn);
                        None
                    }
                },
                Some(WebSearchProviderConfig::Tavily { api_key }) => match api_key.resolve() {
                    Ok(key) => Some(WebSearchConfig::Tavily { api_key: key }),
                    Err(e) => {
                        notify(format!("[tenon] web_search: {}", e), LogLevel::Warn);
                        None
                    }
                },
                None => None,
            };
        }

        if let Some(history) = value.history {
            conf.history.directory = history.directory;
        }

        if let Some(title) = value.title {
            if let Some(model) = title.model {
                conf.title.model = Some(conf.models.get(&model.0).cloned().ok_or(
                    nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                        msg: format!("unknown model for title: {}", model.0),
                    }),
                )?);
            }
            if let Some(prompt) = title.prompt {
                conf.title.prompt = Some(prompt);
            }
        }

        Ok(conf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_from_models_builds_hashmap_with_names() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));
        let mut connectors = HashMap::new();
        connectors.insert(
            "ollama".to_string(),
            ProviderConfig::Ollama(Default::default()),
        );

        let mut models = HashMap::new();
        let mut params = serde_json::Map::new();
        params.insert("temperature".to_string(), serde_json::json!(0.7));
        models.insert(
            "fast".to_string(),
            ModelConfig {
                connector: "ollama".to_string(),
                name: "llama3".to_string(),
                default_parameters: params,
            },
        );
        models.insert(
            "smart".to_string(),
            ModelConfig {
                connector: "ollama".to_string(),
                name: "gpt-4".to_string(),
                default_parameters: serde_json::Map::new(),
            },
        );

        let config = TenonUserConfig {
            connectors: Some(connectors),
            agents: None,
            models: Some(models),
            tools: None,
            history: None,
            title: None,
        };

        let result = TenonConfig::try_from(config).unwrap();
        assert_eq!(result.models.len(), 2);

        let fast = result.models.get("fast").unwrap();
        assert_eq!(fast.connector_name, "ollama");
        assert_eq!(fast.model_name, "llama3");
        assert_eq!(
            fast.default_parameters.get("temperature").unwrap(),
            &serde_json::json!(0.7)
        );

        let smart = result.models.get("smart").unwrap();
        assert_eq!(smart.model_name, "gpt-4");
    }

    #[test]
    fn try_from_models_error_on_unknown_connector() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));

        let mut models = HashMap::new();
        models.insert(
            "bad".to_string(),
            ModelConfig {
                connector: "nonexistent".to_string(),
                name: "model".to_string(),
                default_parameters: serde_json::Map::new(),
            },
        );

        let config = TenonUserConfig {
            connectors: None,
            agents: None,
            models: Some(models),
            tools: None,
            history: None,
            title: None,
        };

        let result = TenonConfig::try_from(config);
        assert!(result.is_err());
    }

    #[test]
    fn choreos_config_key_deserializes() {
        // The user-facing Lua config key for per-agent choreos is "choreos".
        let config: TenonUserConfig = serde_json::from_str(
            r#"{"agents":{"main":{"model":"fast","choreos":["implement_code_together"]}}}"#,
        )
        .unwrap();
        let agent = config.agents.as_ref().unwrap().get("main").unwrap();
        assert_eq!(agent.choreos, vec!["implement_code_together".to_string()]);
    }

    #[test]
    fn try_from_agent_resolves_model_from_registry() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));

        let mut connectors = HashMap::new();
        connectors.insert(
            "ollama".to_string(),
            ProviderConfig::Ollama(Default::default()),
        );

        let mut models = HashMap::new();
        models.insert(
            "fast".to_string(),
            ModelConfig {
                connector: "ollama".to_string(),
                name: "llama3".to_string(),
                default_parameters: serde_json::Map::new(),
            },
        );

        let mut agents = HashMap::new();
        agents.insert(
            "main".to_string(),
            TenonAgentConfig {
                model: Model("fast".to_string()),
                directive: vec![],
                tool_names: vec![],
                default: true,
                choreos: vec![],
            },
        );

        let config = TenonUserConfig {
            connectors: Some(connectors),
            agents: Some(agents),
            models: Some(models),
            tools: None,
            history: None,
            title: None,
        };

        let result = TenonConfig::try_from(config).unwrap();
        let agent = result.agents.get("main").unwrap();
        assert_eq!(agent.model.connector_name, "ollama");
        assert_eq!(agent.model.model_name, "llama3");
    }

    #[test]
    fn try_from_agent_error_on_unknown_model() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));

        let mut agents = HashMap::new();
        agents.insert(
            "main".to_string(),
            TenonAgentConfig {
                model: Model("nonexistent".to_string()),
                directive: vec![],
                tool_names: vec![],
                default: true,
                choreos: vec![],
            },
        );

        let config = TenonUserConfig {
            connectors: None,
            agents: Some(agents),
            models: None,
            tools: None,
            history: None,
            title: None,
        };

        let result = TenonConfig::try_from(config);
        assert!(result.is_err());
    }

    #[test]
    fn try_from_tools_resolves_model_from_registry() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));
        let mut connectors = HashMap::new();
        connectors.insert(
            "ollama".to_string(),
            ProviderConfig::Ollama(Default::default()),
        );

        let mut models = HashMap::new();
        models.insert(
            "fast".to_string(),
            ModelConfig {
                connector: "ollama".to_string(),
                name: "llama3".to_string(),
                default_parameters: serde_json::Map::new(),
            },
        );

        let tools = ToolsUserConfig {
            fetch_webpage: Some(FetchWebpageUserConfig {
                model: Some(Model("fast".to_string())),
            }),
            analyze_image: Some(AnalyzeImageUserConfig {
                model: Some(Model("fast".to_string())),
            }),
            run_command: Some(RunCommandUserConfig {
                whitelist: vec![],
                check_models: vec![Model("fast".to_string())],
            }),
            web_search: None,
        };

        let title = TitleUserConfig {
            model: Some(Model("fast".to_string())),
            prompt: None,
        };

        let config = TenonUserConfig {
            connectors: Some(connectors),
            agents: None,
            models: Some(models),
            tools: Some(tools),
            history: None,
            title: Some(title),
        };

        let result = TenonConfig::try_from(config).unwrap();

        let fw = result.tools.fetch_webpage.model.unwrap();
        assert_eq!(fw.model_name, "llama3");

        let ai = result.tools.analyze_image.model.unwrap();
        assert_eq!(ai.model_name, "llama3");

        let rc = &result.tools.run_command.check_models;
        assert_eq!(rc.len(), 1);
        assert_eq!(rc[0].model_name, "llama3");

        let title_model = result.title.model.unwrap();
        assert_eq!(title_model.model_name, "llama3");
    }

    #[test]
    fn try_from_tools_error_on_unknown_model() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));

        let tools = ToolsUserConfig {
            fetch_webpage: Some(FetchWebpageUserConfig {
                model: Some(Model("nonexistent".to_string())),
            }),
            analyze_image: None,
            run_command: None,
            web_search: None,
        };

        let config = TenonUserConfig {
            connectors: None,
            agents: None,
            models: None,
            tools: Some(tools),
            history: None,
            title: None,
        };

        let result = TenonConfig::try_from(config);
        assert!(result.is_err());
    }

    #[test]
    fn try_from_tools_web_search_passes_through() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));

        let tools = ToolsUserConfig {
            fetch_webpage: None,
            analyze_image: None,
            run_command: None,
            web_search: Some(WebSearchProviderConfig::Brave {
                api_key: ApiKey::Value("test-key".into()),
            }),
        };

        let config = TenonUserConfig {
            connectors: None,
            agents: None,
            models: None,
            tools: Some(tools),
            history: None,
            title: None,
        };

        let result = TenonConfig::try_from(config).unwrap();
        match &result.tools.web_search {
            Some(WebSearchConfig::Brave { api_key }) => {
                assert_eq!(api_key, "test-key");
            }
            other => panic!("expected Brave, got {other:?}"),
        }
    }

    #[test]
    fn try_from_text_config_preserves_condition() {
        let config = DirectiveConfig {
            condition: Some("when coding".into()),
            source: DirectiveSourceConfig::Text {
                value: "be careful".into(),
            },
        };
        let directive = Directive::try_from(config).unwrap();
        assert_eq!(directive.condition.as_deref(), Some("when coding"));
        assert!(matches!(
            &directive.source,
            DirectiveSource::Text { value } if value == "be careful"
        ));
    }

    #[test]
    fn try_from_file_config_preserves_paths() {
        let config = DirectiveConfig {
            condition: None,
            source: DirectiveSourceConfig::File {
                paths: vec![PathBuf::from("./example.md")],
            },
        };
        let directive = Directive::try_from(config).unwrap();
        assert!(directive.condition.is_none());
        assert!(matches!(
            &directive.source,
            DirectiveSource::File { paths } if paths.len() == 1
        ));
    }

    #[test]
    fn try_from_system_config_expands_from_registry() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));
        let config = DirectiveConfig {
            condition: Some("when making code changes".into()),
            source: DirectiveSourceConfig::System {
                name: "YAGNI Attitude".into(),
            },
        };
        let directive = Directive::try_from(config).unwrap();
        assert_eq!(
            directive.condition.as_deref(),
            Some("when making code changes")
        );
        assert!(matches!(
            &directive.source,
            DirectiveSource::Preset { id, .. } if id == "YAGNI Attitude"
        ));
    }

    #[test]
    fn try_from_system_config_unknown_name_returns_not_found() {
        let _ = crate::utils::PLUGIN_ROOT.set(PathBuf::from("."));
        let config = DirectiveConfig {
            condition: None,
            source: DirectiveSourceConfig::System {
                name: "Does Not Exist".into(),
            },
        };
        let result = Directive::try_from(config);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotFound);
    }
}
