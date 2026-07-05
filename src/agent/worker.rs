use std::io;

use crate::clients::{ChatAgent, SupportedModels, get_agent};
use crate::directive::{Directive, DirectiveSource};
use crate::get_application_config;
use rig::message::Message;

pub struct SimpleTenonWorkerAgent {
    agent: ChatAgent,
}

impl SimpleTenonWorkerAgent {
    pub fn new(
        model: Option<SupportedModels>,
        directive_text: &str,
        thinking: bool,
    ) -> io::Result<Self> {
        let config = get_application_config();
        let model = match model {
            Some(m) => m,
            None => {
                let agent_config = config
                    .agents
                    .get(&config.default_agent)
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No default agent"))?;
                agent_config.model.clone()
            }
        };

        let directive = Directive {
            condition: None,
            source: DirectiveSource::Text {
                value: directive_text.to_string(),
            },
        };

        let agent = get_agent(model, vec![directive], vec![], thinking);
        Ok(Self { agent })
    }

    pub async fn chat(
        &self,
        message: impl Into<Message> + Send,
    ) -> Result<String, rig::agent::StreamingError> {
        self.agent.chat(message).await.map(|x| x.trim().to_string())
    }
}
