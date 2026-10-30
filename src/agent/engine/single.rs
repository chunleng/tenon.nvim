use rig::message::Message;

use crate::agent::provider::{ChatStream, StreamItem, get_agent};
use crate::clients::SelectedModel;
use crate::directive::Directive;
use rig::agent::Agent;
use rig::tool::ToolContext;

/// Non-streaming engine: collects all text from a single-turn chat.
/// Creates an agent with no tools - intended for lightweight sub-agent use
/// (e.g. summarization, image analysis).
pub struct SingleTextResponseEngine {
    agent: Agent,
}

impl SingleTextResponseEngine {
    pub fn new(model: SelectedModel, directive: Vec<Directive>) -> Self {
        let agent = get_agent(&model, directive, vec![]);
        Self { agent }
    }

    // rig's PromptError is a large non_exhaustive enum; boxing it here would
    // ripple through every caller for no behavioral gain.
    #[allow(clippy::result_large_err)]
    pub async fn chat(
        &self,
        message: impl Into<Message> + Send,
    ) -> Result<String, rig::completion::PromptError> {
        let mut stream = ChatStream::new(&self.agent, message, vec![], 100, ToolContext::new());
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
