use std::io;

use crate::agent::engine::SingleTextResponseEngine;
use crate::clients::SelectedModel;
use crate::directive::{Directive, DirectiveSource, PresetContent};
use crate::get_application_config;
use rig::message::Message;

pub struct SimpleTenonWorkerAgent {
    engine: SingleTextResponseEngine,
}

impl SimpleTenonWorkerAgent {
    pub fn new(model: Option<SelectedModel>, directive_text: &str) -> io::Result<Self> {
        let selected = match model {
            Some(selected) => selected,
            None => {
                let config = get_application_config();
                let agent_config = config
                    .agents
                    .get(&config.default_agent)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No default agent"))?;
                agent_config.model.clone()
            }
        };

        let directive = Directive {
            condition: None,
            source: DirectiveSource::Preset {
                id: "Simple Worker".into(),
                content: PresetContent::Text(directive_text.to_string()),
            },
        };

        let engine = SingleTextResponseEngine::new(selected, vec![directive]);
        Ok(Self { engine })
    }

    #[allow(clippy::result_large_err)]
    pub async fn chat(
        &self,
        message: impl Into<Message> + Send,
    ) -> Result<String, rig::completion::PromptError> {
        self.engine
            .chat(message)
            .await
            .map(|x| x.trim().to_string())
    }
}
