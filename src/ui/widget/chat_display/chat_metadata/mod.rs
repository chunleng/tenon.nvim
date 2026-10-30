mod session;
mod title;

use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::ui::{
    nvim_primitives::window::NvimWindow, panels::swappable::SwappableBufferPanel,
    widget::chat_display::chat_metadata::session::MetadataSessionRenderer,
    widget::chat_display::chat_metadata::title::MetadataTitleRenderer,
};

use super::ChatDisplayData;
use crate::chat::chat_session_count;

#[derive(Clone)]
pub struct MetadataValues {
    pub title: Option<String>,
    pub chat_index: usize,
    pub total_count: usize,
    pub agent_name: String,
    pub model_display: String,
    /// Current parameter map of the chat's selected model
    pub model_parameters: serde_json::Map<String, serde_json::Value>,
    pub current_tool_names: Vec<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub total_tokens: u64,
    pub context_tokens: u64,
    pub work_queue_count: usize,
    // Delta values for last exchange
    pub total_tokens_delta: u64,
    /// (choreo title, 1-indexed move) when a choreo is active
    pub choreo: Option<(String, usize)>,
}

impl From<Arc<RwLock<ChatDisplayData>>> for MetadataValues {
    fn from(data: Arc<RwLock<ChatDisplayData>>) -> Self {
        if let Ok(data) = data.read()
            && let Ok(session) = data.chat_session.read()
        {
            let title = session.title_handler.title();
            let chat_index = data.chat_index;
            let total_count = chat_session_count();
            let agent_name = session.active_agent_name.clone();
            let model_display = session.engine.model.display_name();
            let model_parameters = session.engine.model.parameters.clone();
            let current_tool_names: Vec<String> = session
                .engine
                .tools
                .iter()
                .map(|t| t.name().to_string())
                .collect();

            let (input_tokens, output_tokens, cached_tokens, total_tokens, total_tokens_delta) =
                if let Ok(usage_lock) = session.usage.read() {
                    let session_usage = &*usage_lock;
                    (
                        session_usage.accumulated.input_tokens.unwrap_or(0),
                        session_usage.accumulated.output_tokens.unwrap_or(0),
                        session_usage.accumulated.cached_input_tokens.unwrap_or(0),
                        session_usage.accumulated.total_tokens.unwrap_or(0),
                        session_usage.last_exchange.total_tokens.unwrap_or(0),
                    )
                } else {
                    (0, 0, 0, 0, 0)
                };

            let context_tokens = session
                .engine
                .log_handler
                .log_window
                .read()
                .ok()
                .map(|log_window| log_window.active_context_token_count())
                .unwrap_or(0);

            let work_queue_count = session
                .work_queue
                .read()
                .map(|queue| queue.entries.len())
                .unwrap_or(0);

            let choreo = session
                .engine
                .active_choreo
                .read()
                .ok()
                .and_then(|active| active.as_ref().map(|c| (c.choreo.title.clone(), c.r#move)));

            return Self {
                title,
                chat_index,
                total_count,
                agent_name,
                model_display,
                model_parameters,
                current_tool_names,
                input_tokens,
                output_tokens,
                cached_tokens,
                total_tokens,
                context_tokens: context_tokens as u64,
                work_queue_count,
                total_tokens_delta,
                choreo,
            };
        }

        Self {
            title: None,
            chat_index: 0,
            total_count: 0,
            agent_name: String::new(),
            model_display: String::new(),
            model_parameters: serde_json::Map::new(),
            current_tool_names: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            total_tokens: 0,
            context_tokens: 0,
            work_queue_count: 0,
            total_tokens_delta: 0,
            choreo: None,
        }
    }
}

#[cfg(test)]
impl MetadataValues {
    pub(crate) fn test_default() -> Self {
        Self {
            title: None,
            chat_index: 0,
            total_count: 1,
            agent_name: "default".to_string(),
            model_display: "claude-3-5-sonnet".to_string(),
            model_parameters: serde_json::Map::new(),
            current_tool_names: vec!["read_file".to_string()],
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            total_tokens: 0,
            context_tokens: 0,
            work_queue_count: 0,
            total_tokens_delta: 0,
            choreo: None,
        }
    }
}

pub struct ChatMetadataRenderer {
    attached_window: Arc<NvimWindow>,
    input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
    chat_data: Arc<RwLock<ChatDisplayData>>,
}

impl ChatMetadataRenderer {
    pub fn new(
        attached_window: Arc<NvimWindow>,
        input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
        chat_data: Arc<RwLock<ChatDisplayData>>,
    ) -> Self {
        Self {
            attached_window,
            input_window,
            chat_data,
        }
    }

    pub fn spawn_renderer_thread(&self, stop_signal: Arc<AtomicBool>) -> JoinHandle<()> {
        let stop_signal_clone = stop_signal.clone();
        let attached_window = self.attached_window.clone();
        let input_window = self.input_window.clone();
        let chat_data = self.chat_data.clone();

        std::thread::spawn(move || {
            let mut title_renderer = MetadataTitleRenderer::new(attached_window, chat_data.clone());
            let mut session_renderer = MetadataSessionRenderer::new(input_window, chat_data);

            loop {
                if stop_signal_clone.load(Ordering::SeqCst) {
                    break;
                }

                title_renderer.render();
                session_renderer.render();

                std::thread::sleep(Duration::from_millis(100));
            }
        })
    }
}
