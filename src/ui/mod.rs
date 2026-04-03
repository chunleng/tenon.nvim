mod components;
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

use crate::{
    chat::ChatProcess,
    ui::components::{
        FixedBufferVimWindow, FixedBufferVimWindowOption, SplitWindowOption, WindowOption,
    },
};

pub struct ChatWindow {
    output_window: Arc<Mutex<Option<FixedBufferVimWindow>>>,
    pub chat_process: ChatProcess,
}

impl ChatWindow {
    pub fn new() -> Self {
        let chat_process = ChatProcess::new();

        Self {
            output_window: Arc::new(Mutex::new(None)),
            chat_process,
        }
    }

    pub fn open(&mut self) -> OxiResult<()> {
        self.get_or_create_output_window()?;
        Ok(())
    }

    fn get_or_create_output_window(&mut self) -> OxiResult<FixedBufferVimWindow> {
        if let Ok(win) = self.output_window.lock()
            && let Some(win) = win.as_ref()
            && win.get_buffer().is_some()
            && win.get_window().is_some()
        {
            Ok(win.clone())
        } else {
            let win = FixedBufferVimWindow::new(FixedBufferVimWindowOption {
                window_option: WindowOption::Split(SplitWindowOption::Right { width: 0.4 }),
                modifiable: false,
                file_type: "markdown".to_string(),
                ..Default::default()
            })?;
            self.output_window = Arc::new(Mutex::new(Some(win.clone())));

            let chat_renderer_handle = AsyncHandle::new({
                let output_window = win.clone();
                let logs = self.chat_process.logs.clone();
                let usage_clone = self.chat_process.usage.clone();
                move || {
                    if let Ok(logs) = logs.read() {
                        let mut content = logs
                            .iter()
                            .flat_map(|x| x.as_chat_lines())
                            .collect::<Vec<_>>();

                        if let Ok(usage) = usage_clone.read()
                            && let Some(usage) = usage.as_ref()
                        {
                            content.push(format!(
                                "{} 󰕒 | {} 󰇚 | {}  | {} total",
                                usage.input_tokens,
                                usage.output_tokens,
                                usage.cached_input_tokens,
                                usage.total_tokens
                            ));
                        }

                        if let Some(mut buffer) = output_window.get_buffer()
                            && let Some(mut window) = output_window.get_window()
                        {
                            schedule({
                                move |_| {
                                    if let Ok(line_count) = buffer.line_count() {
                                        let mut follow_last_line = false;
                                        if let Ok((cursor_row, _)) = window.get_cursor()
                                            && let Ok(height) = window.get_height()
                                        {
                                            follow_last_line =
                                                cursor_row + height as usize >= line_count;
                                        };

                                        let buf_opts =
                                            OptionOpts::builder().buffer(buffer.clone()).build();
                                        let _ =
                                            api::set_option_value("modifiable", true, &buf_opts);
                                        let _ = buffer.set_lines(0.., false, content);
                                        let _ =
                                            api::set_option_value("modifiable", false, &buf_opts);
                                        let _ = api::set_option_value("modified", false, &buf_opts);

                                        if follow_last_line
                                            && let Ok(new_line_count) = buffer.line_count()
                                            && let Ok((cursor_row, cursor_col)) =
                                                window.get_cursor()
                                        {
                                            let _ = window.set_cursor(
                                                new_line_count - line_count + cursor_row,
                                                cursor_col,
                                            );
                                        }
                                    }
                                }
                            })
                        }
                    }
                }
            })?;

            spawn({
                let output_window = win.clone();
                move || {
                    loop {
                        if output_window.get_buffer().is_none() {
                            break;
                        }
                        sleep(Duration::from_millis(50));
                        let _ = chat_renderer_handle.send();
                    }
                }
            });
            Ok(win)
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
