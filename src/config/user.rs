use std::{collections::HashMap, path::PathBuf};

use nvim_oxi::serde::DeserializeError;
use serde::Deserialize;

use crate::{
    chat::TenonAgent,
    clients::{ProviderConfig, SupportedModels},
    config::TenonConfig,
    directive::{Directive, DirectiveSource},
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenonUserConfig {
    pub connectors: Option<HashMap<String, ProviderConfig>>,
    pub agents: Option<HashMap<String, TenonAgentConfig>>,
    pub models: Option<Vec<ModelConfig>>,
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
    pub model: Option<ModelConfig>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolsUserConfig {
    pub fetch_webpage: Option<FetchWebpageUserConfig>,
    pub analyze_image: Option<AnalyzeImageUserConfig>,
    pub run_command: Option<RunCommandUserConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommandUserConfig {
    pub whitelist: Vec<String>,

    #[serde(default)]
    pub check_models: Vec<ModelConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetchWebpageUserConfig {
    pub model: Option<ModelConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeImageUserConfig {
    pub model: Option<ModelConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TenonAgentConfig {
    model: ModelConfig,
    #[serde(default)]
    directive: Vec<DirectiveConfig>,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    workflows: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    connector: String,
    name: String,
}

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
        if let Some(agents) = value.agents {
            if agents.is_empty() {
                return Err(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                    msg: "agents cannot be empty".to_string(),
                }));
            }
            conf.agents = agents
                .into_iter()
                .map(|(k, v)| -> Result<_, nvim_oxi::Error> {
                    let model_config: &ProviderConfig = conf
                        .connectors
                        .get(&v.model.connector)
                        .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                            msg: format!("unknown connector: {}", v.model.connector),
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
                    let workflows: Vec<std::sync::Arc<crate::chat::workflow::Workflow>> = v
                        .workflows
                        .into_iter()
                        .filter_map(|id| crate::get_workflow_registry().get(&id).cloned())
                        .collect();
                    Ok((
                        k,
                        TenonAgent::new(
                            SupportedModels {
                                connector_name: v.model.connector.clone(),
                                config: model_config.to_owned(),
                                model_name: v.model.name,
                            },
                            directives,
                            &v.tool_names,
                            workflows,
                        ),
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

        if let Some(models) = value.models {
            conf.models = models
                .into_iter()
                .map(|m| -> Result<SupportedModels, nvim_oxi::Error> {
                    let provider_config: &ProviderConfig = conf
                        .connectors
                        .get(&m.connector)
                        .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                            msg: format!("unknown connector for model: {}", m.connector),
                        }))?;
                    Ok(SupportedModels {
                        connector_name: m.connector.clone(),
                        config: provider_config.to_owned(),
                        model_name: m.name,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
        }

        if let Some(tools) = value.tools {
            if let Some(fetch_webpage) = tools.fetch_webpage
                && let Some(model) = fetch_webpage.model
            {
                let provider_config: &ProviderConfig = conf
                    .connectors
                    .get(&model.connector)
                    .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                        msg: format!(
                            "unknown connector for fetch_webpage model: {}",
                            model.connector
                        ),
                    }))?;
                conf.tools.fetch_webpage.model = Some(SupportedModels {
                    connector_name: model.connector.clone(),
                    config: provider_config.to_owned(),
                    model_name: model.name,
                });
            }
            if let Some(analyze_image) = tools.analyze_image
                && let Some(model) = analyze_image.model
            {
                let provider_config: &ProviderConfig = conf
                    .connectors
                    .get(&model.connector)
                    .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                        msg: format!(
                            "unknown connector for analyze_image model: {}",
                            model.connector
                        ),
                    }))?;
                conf.tools.analyze_image.model = Some(SupportedModels {
                    connector_name: model.connector.clone(),
                    config: provider_config.to_owned(),
                    model_name: model.name,
                });
            }
            if let Some(run) = tools.run_command {
                conf.tools.run_command.whitelist = run.whitelist;
                conf.tools.run_command.check_models = run
                    .check_models
                    .into_iter()
                    .map(|m| -> Result<_, nvim_oxi::Error> {
                        let provider_config: &ProviderConfig = conf
                            .connectors
                            .get(&m.connector)
                            .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                                msg: format!(
                                    "unknown connector for run check model: {}",
                                    m.connector
                                ),
                            }))?;
                        Ok(SupportedModels {
                            connector_name: m.connector.clone(),
                            config: provider_config.to_owned(),
                            model_name: m.name,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            }
        }

        if let Some(history) = value.history {
            conf.history.directory = history.directory;
        }

        if let Some(title) = value.title {
            if let Some(model) = title.model {
                let provider_config: &ProviderConfig = conf
                    .connectors
                    .get(&model.connector)
                    .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                        msg: format!("unknown connector for title model: {}", model.connector),
                    }))?;
                conf.title.model = Some(SupportedModels {
                    connector_name: model.connector.clone(),
                    config: provider_config.to_owned(),
                    model_name: model.name,
                });
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
        assert!(matches!(&directive.source, DirectiveSource::File { .. }));
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
