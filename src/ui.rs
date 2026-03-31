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
    buffer: Arc<Mutex<Option<api::Buffer>>>,
    window: Arc<Mutex<Option<api::Window>>>,
    pub chat_process: ChatProcess,
}

impl ChatWindow {
    pub fn new() -> Self {
        let chat_process = ChatProcess::new();

        Self {
            buffer: Arc::new(Mutex::new(None)),
            window: Arc::new(Mutex::new(None)),
            chat_process,
        }
    }

    pub fn open(&mut self) -> OxiResult<()> {
        // get_or_create_window opens a new window if window does not exists
        self.get_or_create_window()?;
        Ok(())
    }

    fn get_or_create_buffer(&mut self) -> OxiResult<api::Buffer> {
        if let Ok(buffer) = self.buffer.lock()
            && let Some(buffer) = buffer.as_ref()
            && buffer.is_valid()
        {
            return Ok(buffer.clone());
        }

        let buffer = Arc::new(Mutex::new(Some(api::create_buf(false, true)?)));

        let buf_opts = OptionOpts::builder().build();

        api::set_option_value("buftype", "nofile", &buf_opts)?;
        api::set_option_value("buflisted", false, &buf_opts)?;
        api::set_option_value("bufhidden", "wipe", &buf_opts)?;
        api::set_option_value("swapfile", false, &buf_opts)?;
        api::set_option_value("filetype", "markdown", &buf_opts)?;
        api::set_option_value("modifiable", false, &buf_opts)?;

        let logs = self.chat_process.logs.clone();

        let chat_renderer_handle = AsyncHandle::new({
            let buffer_clone = buffer.clone();
            move || {
                if let Ok(logs) = logs.read() {
                    let content = logs
                        .iter()
                        .flat_map(|x| x.as_chat_lines())
                        .collect::<Vec<_>>();
                    let buffer_clone2 = buffer_clone.clone(); // Clone the Arc here

                    schedule(move |_| {
                        if let Ok(mut buffer) = buffer_clone2.lock()
                            && let Some(buffer) = buffer.as_mut()
                        {
                            let buf_opts = OptionOpts::builder().buffer(buffer.clone()).build();
                            let _ = api::set_option_value("modifiable", true, &buf_opts);
                            let _ = buffer.set_lines(0.., false, content);
                            let _ = api::set_option_value("modifiable", false, &buf_opts);
                            let _ = api::set_option_value("modified", false, &buf_opts);
                        }
                    });
                }
            }
        })?;

        spawn({
            let buffer_clone = buffer.clone();
            move || {
                loop {
                    if let Ok(mut buffer) = buffer_clone.lock()
                        && let Some(buffer) = buffer.as_mut()
                        && !buffer.is_valid()
                    {
                        break;
                    }
                    sleep(Duration::from_millis(50));
                    let _ = chat_renderer_handle.send();
                }
            }
        });

        self.buffer = buffer;
        let mut window = self.get_or_create_window()?;
        if let Ok(buffer) = self.buffer.lock()
            && let Some(buffer) = buffer.as_ref()
            && buffer.is_valid()
        {
            window.set_buf(&buffer)?;
            Ok(buffer.clone())
        } else {
            todo!("fix after error is introduced")
        }
    }

    fn get_or_create_window(&mut self) -> OxiResult<api::Window> {
        let buffer = self.get_or_create_buffer()?;
        if let Ok(mut win) = self.window.lock()
            && let Some(win) = win.as_mut()
            && win.is_valid()
        {
            win.set_buf(&buffer)?;
            return Ok(win.clone());
        }

        api::command("botright vsplit")?;
        let window = Arc::new(Mutex::new(Some(api::get_current_win())));

        let ui_width = api::get_option_value::<i64>("columns", &OptionOpts::default())?;
        let width = (ui_width as f32 * 0.4) as i64;
        api::command(&format!("vertical resize {}", width))?;

        self.window = window;
        if let Ok(mut win) = self.window.lock()
            && let Some(win) = win.as_mut()
            && win.is_valid()
        {
            let win_opts = OptionOpts::builder().win(win.clone()).build();
            api::set_option_value("wrap", true, &win_opts)?;
            api::set_option_value("linebreak", true, &win_opts)?;

            win.set_buf(&buffer)?;
            Ok(win.clone())
        } else {
            todo!("fix after error is introduced")
        }
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
