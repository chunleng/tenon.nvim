mod chat_log;
mod chat_metadata;
mod format;

use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

use nvim_oxi::{Result as OxiResult, api::opts::OptionOpts};

use crate::{
    chat::{ChatSession, TenonLog},
    ui::{
        nvim_primitives::{buffer::NvimBuffer, window::NvimWindow},
        panels::swappable::SwappableBufferPanel,
        widget::Widget,
    },
};

use chat_log::{ChatLogCache, ChatLogRenderer};
use chat_metadata::ChatMetadataRenderer;

#[derive(Clone)]
pub struct ChatDisplayData {
    pub chat_session: Arc<RwLock<ChatSession>>,
    pub chat_index: usize,
}

struct SharedState {
    running_threads: Vec<Arc<JoinHandle<()>>>,
    stop_signal: Arc<AtomicBool>,
    chat_log_cache: Option<Arc<RwLock<ChatLogCache>>>,
}

#[derive(Clone)]
pub struct ChatDisplay {
    pub inner: Arc<NvimBuffer>,
    attached_window: Option<Arc<NvimWindow>>,
    input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
    attached_chat: Arc<RwLock<ChatDisplayData>>,
    shared: Arc<RwLock<SharedState>>,
}

impl ChatDisplay {
    pub fn new(
        buffer: NvimBuffer,
        chat: ChatDisplayData,
        input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
    ) -> Self {
        Self {
            inner: Arc::new(buffer),
            attached_window: None,
            input_window,
            attached_chat: Arc::new(RwLock::new(chat)),
            shared: Arc::new(RwLock::new(SharedState {
                running_threads: Vec::new(),
                stop_signal: Arc::new(AtomicBool::new(false)),
                chat_log_cache: None,
            })),
        }
    }

    pub fn switch_chat(&mut self, chat: ChatDisplayData) -> OxiResult<()> {
        if let Ok(mut current_chat) = self.attached_chat.write() {
            *current_chat = chat;
        }
        self.reset_thread()
    }

    /// Returns the log entry at the cursor position in the attached window, or
    /// `None` if the cursor is not on a rendered log line.
    pub fn get_log_at_cursor(&self) -> Option<Arc<RwLock<TenonLog>>> {
        let window_arc = self.attached_window.as_ref()?;
        let window = window_arc.get_window()?;
        let Ok((cursor_row, _)) = window.get_cursor() else {
            return None;
        };

        let line = cursor_row.saturating_sub(1);

        let Ok(shared) = self.shared.read() else {
            return None;
        };
        let cache = shared.chat_log_cache.as_ref()?;
        let Ok(cache) = cache.read() else {
            return None;
        };
        cache.get_log_at_line(line)
    }

    fn start_thread(&mut self) -> OxiResult<()> {
        {
            let shared = self.shared.read().unwrap();
            if !shared.running_threads.is_empty() {
                return Ok(());
            }
        }

        let Some(attached_window) = self.attached_window.as_ref() else {
            return Ok(());
        };

        if let Ok(chat) = self.attached_chat.read() {
            let chat_log_cache =
                Arc::new(RwLock::new(ChatLogCache::new(chat.chat_session.clone())));
            {
                let mut shared = self.shared.write().unwrap();
                shared.chat_log_cache = Some(chat_log_cache.clone());
            }

            let log_renderer =
                ChatLogRenderer::new(self.inner.clone(), attached_window.clone(), chat_log_cache);

            let metadata_renderer = ChatMetadataRenderer::new(
                attached_window.clone(),
                self.input_window.clone(),
                self.attached_chat.clone(),
            );

            let stop_signal = {
                let shared = self.shared.read().unwrap();
                shared.stop_signal.clone()
            };
            let log_handle = log_renderer.spawn_renderer_thread(stop_signal.clone());
            let metadata_handle = metadata_renderer.spawn_renderer_thread(stop_signal.clone());
            {
                let mut shared = self.shared.write().unwrap();
                shared.running_threads.push(Arc::new(log_handle));
                shared.running_threads.push(Arc::new(metadata_handle));
            }
        }

        Ok(())
    }

    fn reset_thread(&mut self) -> OxiResult<()> {
        if let Ok(mut shared) = self.shared.write() {
            shared.stop_signal.store(true, Ordering::SeqCst);
            shared.running_threads.clear();
            shared.stop_signal = Arc::new(AtomicBool::new(false));
        }
        // TODO we might want to instead switch buffer in the future
        // Clear buffer before starting new thread (switching chats, resetting)
        if let Some(mut buffer) = self.inner.get_buffer() {
            let buf_opts = OptionOpts::builder().buf(buffer.clone()).build();
            nvim_oxi::api::set_option_value("modifiable", true, &buf_opts)?;
            let line_count = buffer.line_count()?;
            let _ = buffer.set_lines(0..line_count, false, Vec::<&str>::new());
            // Clear signs and highlights placed by renderer
            let ns = nvim_oxi::api::create_namespace("TenonChatLog");
            let _ = buffer.clear_namespace(ns, 0..line_count);
            nvim_oxi::api::set_option_value("modifiable", false, &buf_opts)?;
        }

        self.start_thread()
    }
}

impl Widget for ChatDisplay {
    fn render(&mut self) -> OxiResult<()> {
        self.start_thread()
    }

    fn buffer(&self) -> &NvimBuffer {
        &self.inner
    }

    fn set_window(&mut self, window: NvimWindow) {
        self.attached_window = Some(Arc::new(window));
    }
}
