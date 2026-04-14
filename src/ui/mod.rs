mod components;
use std::{
    sync::{Arc, Mutex},
    thread::{sleep, spawn},
    time::Duration,
};

use nvim_oxi::{
    Result as OxiResult,
    api::{
        self,
        opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts, SetKeymapOpts},
        types::{LogLevel, Mode},
    },
    libuv::AsyncHandle,
    schedule,
};

use crate::{
    chat::{
        ChatProcess, TenonAssistantMessage, TenonAssistantTextMessage, TenonLog, TenonToolLog,
        TenonUserMessage, TenonUserTextMessage,
    },
    ui::components::{
        FixedBufferVimWindow, FixedBufferVimWindowOption, Keymap, SplitWindowOption, WindowOption,
    },
    utils::notify,
};

pub struct ChatWindow {
    output_window: Arc<Mutex<Option<FixedBufferVimWindow>>>,
    input_window: Arc<Mutex<Option<FixedBufferVimWindow>>>,
    pub chat_process: ChatProcess,
}

impl ChatWindow {
    pub fn new() -> Self {
        let chat_process = ChatProcess::new();

        Self {
            output_window: Arc::new(Mutex::new(None)),
            input_window: Arc::new(Mutex::new(None)),
            chat_process,
        }
    }

    pub fn open(&mut self) -> OxiResult<()> {
        self.get_or_create_output_window()?;
        self.get_or_create_input_window()?;
        Ok(())
    }

    pub fn send(&mut self) -> OxiResult<()> {
        if let Some(mut input_win_buffer) = self.get_or_create_input_window()?.get_buffer() {
            let lines = input_win_buffer.get_lines(0.., false)?;
            let message = lines
                .map(|x| x.to_string())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            if message.is_empty() {
                notify("please enter your message before sending", LogLevel::Error);
            } else {
                self.chat_process.send_message(message);
                let _ = input_win_buffer.set_lines(0.., false, Vec::<String>::new());
            }
        }

        Ok(())
    }

    pub fn close(&mut self) -> OxiResult<()> {
        // We just need to close one of the input/output windows as the windows are linked.
        if let Ok(input_win) = self.input_window.lock()
            && let Some(input_win) = input_win.as_ref()
            && let Some(win) = input_win.get_window()
        {
            win.close(true)?;
        }

        Ok(())
    }

    fn get_or_create_input_window(&mut self) -> OxiResult<FixedBufferVimWindow> {
        if let Ok(win) = self.input_window.lock()
            && let Some(win) = win.as_ref()
            && win.get_buffer().is_some()
            && win.get_window().is_some()
        {
            Ok(win.clone())
        } else {
            // TODO use error to handle unwrap in this function
            let output_window = self.get_or_create_output_window()?.get_window().unwrap();
            api::set_current_win(&output_window)?;

            let input_win = FixedBufferVimWindow::new(FixedBufferVimWindowOption {
                window_option: WindowOption::Split {
                    direction: SplitWindowOption::Bottom,
                    ratio_wh: 0.3,
                    edge: false,
                },
                file_type: "markdown".to_string(),
                buf_keymaps: vec![
                    Keymap {
                        modes: vec![Mode::Insert, Mode::Normal],
                        lhs: "<c-cr>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.send()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "q".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.close()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                ],
                ..Default::default()
            })?;

            let augroup = api::create_augroup(
                "TenonInOutLinkedWindows",
                &CreateAugroupOpts::builder().clear(true).build(),
            )?;
            api::create_autocmd(
                ["WinClosed"],
                &CreateAutocmdOpts::builder()
                    .group(augroup)
                    .patterns([input_win
                        .get_window()
                        .unwrap()
                        .handle()
                        .to_string()
                        .as_str()])
                    .callback({
                        let output_window = output_window.clone();
                        move |_| {
                            print!("A");
                            let output_window = output_window.clone();
                            if output_window.is_valid() {
                                let _ = output_window.close(true);
                            }
                            false
                        }
                    })
                    .build(),
            )?;
            api::create_autocmd(
                ["WinClosed"],
                &CreateAutocmdOpts::builder()
                    .group(augroup)
                    .patterns([output_window.handle().to_string().as_str()])
                    .callback({
                        let input_win = input_win.clone();
                        move |_| {
                            print!("B");
                            if let Some(win) = input_win.get_window()
                                && win.is_valid()
                            {
                                let _ = win.close(true);
                            }
                            false
                        }
                    })
                    .build(),
            )?;
            self.input_window = Arc::new(Mutex::new(Some(input_win.clone())));
            Ok(input_win)
        }
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
                window_option: WindowOption::Split {
                    direction: SplitWindowOption::Right,
                    ratio_wh: 0.4,
                    edge: true,
                },
                modifiable: false,
                file_type: "markdown".to_string(),
                buf_keymaps: vec![Keymap {
                    modes: vec![Mode::Normal],
                    lhs: "q".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.close()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                }],
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

impl DisplayAsChat for TenonLog {
    fn as_chat_string(&self) -> Option<String> {
        match self {
            TenonLog::User(TenonUserMessage::Text(TenonUserTextMessage(msg))) => {
                Some(format!("# User\n\n{}\n\n---\n", msg))
            }
            TenonLog::Assistant(TenonAssistantMessage::Text(TenonAssistantTextMessage(msg))) => {
                Some(format!("# Assistant\n\n{}\n\n---\n", msg))
            }
            TenonLog::Tool(TenonToolLog {
                tool_call,
                tool_result,
            }) => Some(format!(
                "[{}] id: {} ({})\n\n---\n",
                tool_call.name,
                tool_call.id,
                if tool_result.is_some() {
                    "Done"
                } else {
                    "Running"
                }
            )),
        }
    }
}
