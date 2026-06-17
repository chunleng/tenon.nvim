use std::collections::HashMap;

use nvim_oxi::serde::DeserializeError;
use serde::Deserialize;

use crate::{
    chat::TenonAgent,
    clients::{ProviderConfig, SupportedModels},
    config::TenonConfig,
    directive::Directive,
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
    pub run: Option<RunUserConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunUserConfig {
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
pub struct WorkflowConfig {
    pub id: String,
    #[serde(default)]
    pub condition: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TenonAgentConfig {
    model: ModelConfig,
    #[serde(default)]
    directive: Vec<Directive>,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    workflows: Vec<WorkflowConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelConfig {
    connector: String,
    name: String,
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
                    Ok((
                        k,
                        TenonAgent::new(
                            SupportedModels {
                                connector_name: v.model.connector.clone(),
                                config: model_config.to_owned(),
                                model_name: v.model.name,
                            },
                            v.directive,
                            &v.tool_names,
                            v.workflows,
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
            if let Some(run) = tools.run {
                conf.tools.run.whitelist = run.whitelist;
                conf.tools.run.check_models = run
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
