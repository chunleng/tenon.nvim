use std::{
    sync::{Arc, Mutex},
    thread::{sleep, spawn},
    time::Duration,
};

use nvim_oxi::{
    Result as OxiResult,
    api::{self, opts::OptionOpts},
    libuv::AsyncHandle,
    schedule,
};
use rig::providers::ollama;

use crate::chat::ChatProcess;

pub struct ChatWindow {
    buffer: Arc<Mutex<api::Buffer>>,
    pub chat_process: ChatProcess,
}

impl ChatWindow {
    pub fn new() -> OxiResult<Self> {
        let buffer = Arc::new(Mutex::new(api::create_buf(false, true)?));

        let buf_opts = OptionOpts::builder().build();

        api::set_option_value("buftype", "nofile", &buf_opts)?;
        api::set_option_value("bufhidden", "hide", &buf_opts)?;
        api::set_option_value("swapfile", false, &buf_opts)?;
        api::set_option_value("filetype", "markdown", &buf_opts)?;
        api::set_option_value("modifiable", false, &buf_opts)?;

        api::command("botright vsplit")?;
        let mut window = api::get_current_win();

        let ui_width = api::get_option_value::<i64>("columns", &OptionOpts::default())?;
        let width = (ui_width as f32 * 0.4) as i64;
        api::command(&format!("vertical resize {}", width))?;

        if let Ok(buffer) = buffer.lock() {
            window.set_buf(&buffer)?;
        } else {
            todo!();
        }

        let win_opts = OptionOpts::builder().win(window.clone()).build();

        api::set_option_value("wrap", true, &win_opts)?;
        api::set_option_value("linebreak", true, &win_opts)?;

        let chat_process = ChatProcess::new();

        Ok(Self {
            buffer,
            chat_process,
        })
    }

    pub fn spawn_chat_renderer(&self) -> OxiResult<()> {
        let logs = self.chat_process.logs.clone();
        let buffer_clone = self.buffer.clone();

        let chat_renderer_handle = AsyncHandle::new(move || {
            if let Ok(logs) = logs.read() {
                let content = logs
                    .iter()
                    .flat_map(|x| x.as_chat_lines())
                    .collect::<Vec<_>>();
                let buffer_clone2 = buffer_clone.clone(); // Clone the Arc here

                schedule(move |_| {
                    if let Ok(mut buffer) = buffer_clone2.lock() {
                        let buf_opts = OptionOpts::builder().buffer(buffer.clone()).build();
                        api::set_option_value("modifiable", true, &buf_opts).unwrap();
                        buffer.set_lines(0.., false, content).unwrap();
                        api::set_option_value("modifiable", false, &buf_opts).unwrap();
                    }
                });
            }
        })?;

        spawn(move || {
            loop {
                sleep(Duration::from_millis(50));
                chat_renderer_handle.send().unwrap();
            }
        });

        Ok(())
    }
}

trait DisplayAsChat {
    fn as_chat_lines(&self) -> Vec<String> {
        match self.as_chat_string() {
            Some(chat_string) => chat_string.lines().map(|x| x.to_string()).collect(),
            None => {
                vec![]
            }
        }
    }
    fn as_chat_string(&self) -> Option<String>;
}

impl DisplayAsChat for ollama::Message {
    fn as_chat_string(&self) -> Option<String> {
        match self {
            ollama::Message::User { content, .. } => {
                Some(format!("# User\n\n{}\n\n---\n", content.to_string()))
            }
            ollama::Message::Assistant { content, .. } => {
                Some(format!("# Assistant\n\n{}\n\n---\n", content.to_string()))
            }
            ollama::Message::System { .. } => None,
            ollama::Message::ToolResult { .. } => None,
        }
    }
}
