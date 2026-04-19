use std::collections::HashMap;

use nvim_oxi::serde::DeserializeError;
use serde::Deserialize;

use crate::{
    chat::TenonAgent,
    clients::{ProviderConfig, SupportedModels},
    config::TenonConfig,
};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TenonUserConfig {
    pub connectors: Option<HashMap<String, ProviderConfig>>,
    pub agents: Option<HashMap<String, TenonAgentConfig>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TenonAgentConfig {
    model: ModelConfig,
    #[serde(default)]
    preamble: Option<String>,
    #[serde(default)]
    tool_names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ModelConfig {
    connector: String,
    name: String,
}

impl TryFrom<TenonUserConfig> for TenonConfig {
    type Error = nvim_oxi::Error;
    fn try_from(value: TenonUserConfig) -> Result<Self, Self::Error> {
        let mut conf = TenonConfig::default();

        if let Some(connectors) = value.connectors {
            conf.connectors = connectors;
        }
        if let Some(agents) = value.agents {
            conf.agents = agents
                .into_iter()
                .map(|(k, v)| -> Result<_, nvim_oxi::Error> {
                    let model_config: &ProviderConfig = conf
                        .connectors
                        .get(&v.model.connector)
                        .ok_or(nvim_oxi::Error::Deserialize(DeserializeError::Custom {
                            msg: format!("unknown connector: {}", v.model.connector),
                        }))?;
                    Ok((
                        k,
                        TenonAgent::new(
                            SupportedModels {
                                config: model_config.to_owned(),
                                model_name: v.model.name,
                            },
                            v.preamble,
                            &v.tool_names,
                        ),
                    ))
                })
                .collect::<Result<HashMap<_, _>, _>>()?;
        }

        Ok(conf)
    }
}
