use rig::message::Message;

use crate::agent::provider::{ChatStream, StreamItem, get_agent};
use crate::clients::SupportedModels;
use crate::directive::Directive;
use rig::agent::Agent;

/// Non-streaming engine: collects all text from a single-turn chat.
/// Creates an agent with no tools - intended for lightweight sub-agent use
/// (e.g. summarization, image analysis).
pub struct SingleTextResponseEngine {
    agent: Agent,
}

impl SingleTextResponseEngine {
    pub fn new(
        model: SupportedModels,
        directive: Vec<Directive>,
        override_params: Option<serde_json::Map<String, serde_json::Value>>,
    ) -> Self {
        let agent = get_agent(model, directive, vec![], override_params);
        Self { agent }
    }

    pub async fn chat(
        &self,
        message: impl Into<Message> + Send,
    ) -> Result<String, rig::agent::StreamingError> {
        let mut stream = ChatStream::new(&self.agent, message, vec![], 100).await;
        let mut full_text = String::new();
        let mut was_text = false;
        while let Some(result) = stream.next().await {
            match result {
                Ok(StreamItem::Text { text }) => {
                    if !was_text {
                        was_text = true;
                        full_text = String::new();
                    }
                    full_text.push_str(&text);
                }
                Ok(_) => was_text = false,
                Err(e) => return Err(e),
            }
        }
        Ok(full_text)
    }
}
