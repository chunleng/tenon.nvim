mod nvim_primitives;
mod panels;
mod widget;

use std::sync::{
    Arc, Mutex, RwLock,
    atomic::{AtomicUsize, Ordering},
};

use nvim_oxi::{
    Result as OxiResult,
    api::{
        self,
        opts::{CreateAugroupOpts, CreateAutocmdOpts, SetKeymapOpts},
        types::{GotMode, LogLevel, Mode},
    },
};

use crate::{
    chat::chat_process_count,
    ui::widget::{BasicWidget, Widget},
};
use crate::{
    chat::{ChatProcess, get_or_create_chat_process, remove_chat_process},
    ui::{
        nvim_primitives::{
            buffer::{NvimBuffer, NvimBufferOption, NvimKeymap},
            window::{NvimSplitWindowType, NvimWindowType},
        },
        panels::fixed::{FixedBufferPanel, FixedBufferPanelOption},
        panels::swappable::{SwappableBufferPanel, SwappablePanelOption},
        widget::display::{ChatDisplay, ChatDisplayData},
    },
    utils::notify,
};

pub struct ChatWindow {
    output_window: Arc<Mutex<Option<FixedBufferPanel<ChatDisplay>>>>,
    input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
    /// Shared reference to the currently loaded chat process.
    /// The outer `RwLock` allows swapping the inner `Arc` when loading a different chat,
    /// so the renderer thread always reads from the current chat without closing windows.
    pub loaded_chat_process: Arc<RwLock<Arc<RwLock<ChatProcess>>>>,
    pub loaded_chat_index: Arc<AtomicUsize>,
}

impl ChatWindow {
    pub fn new() -> Self {
        let loaded_chat_index = 0;
        let loaded_chat_process =
            Arc::new(RwLock::new(get_or_create_chat_process(loaded_chat_index)));

        Self {
            output_window: Arc::new(Mutex::new(None)),
            input_window: Arc::new(Mutex::new(None)),
            loaded_chat_process,
            loaded_chat_index: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn focus_input_window(&self) -> OxiResult<()> {
        if let Ok(input_win) = self.input_window.lock()
            && let Some(input_win) = input_win.as_ref()
            && let Some(win) = input_win.window.get_window()
        {
            api::set_current_win(&win)?;
            let GotMode { mode, .. } = api::get_mode()?;
            if mode != "i" {
                api::feedkeys(c"i", c"n", false);
            }
        }
        Ok(())
    }

    pub fn toggle_focus(&mut self) -> OxiResult<()> {
        // Determine target first, then drop locks before switching
        // to avoid deadlock (focus methods also acquire the same mutexes).
        let target_win = {
            let current_win = api::get_current_win();
            let Ok(input_win) = self.input_window.lock() else {
                return Ok(());
            };
            if let Some(input_win) = input_win.as_ref()
                && let Some(win) = input_win.window.get_window()
                && current_win == win
            {
                // In input window → switch to output window
                let Ok(output_win) = self.output_window.lock() else {
                    return Ok(());
                };
                output_win.as_ref().and_then(|w| w.window.get_window())
            } else {
                // In output window (or elsewhere) → switch to input window
                input_win.as_ref().and_then(|w| w.window.get_window())
            }
        };
        if let Some(win) = target_win {
            api::set_current_win(&win)?;
        }
        Ok(())
    }

    pub fn open(&mut self) -> OxiResult<()> {
        self.get_or_create_output_window()?;
        self.get_or_create_input_window()?;
        self.focus_input_window()?;
        Ok(())
    }

    pub fn scroll_output_to_bottom(&mut self) -> OxiResult<()> {
        if let Some(mut output_win_window) = self.get_or_create_output_window()?.window.get_window()
            && let Ok(line_count) = output_win_window.get_buf().and_then(|b| b.line_count())
        {
            output_win_window.set_cursor(line_count, 0)?;
        }
        Ok(())
    }

    pub fn send(&mut self) -> OxiResult<()> {
        if let Some(mut input_win_buffer) = self
            .get_or_create_input_window()?
            .active_widget()
            .buffer()
            .get_buffer()
        {
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
                self.scroll_output_to_bottom()?;
                if let Ok(loaded) = self.loaded_chat_process.read() {
                    if let Ok(mut chat_process) = loaded.write() {
                        chat_process.send_message(message);
                        let _ = input_win_buffer.set_lines(0.., false, Vec::<String>::new());
                    }
                }
            }
        }

        Ok(())
    }

    pub fn stop_streaming(&mut self) -> OxiResult<()> {
        if let Ok(loaded) = self.loaded_chat_process.read() {
            if let Ok(mut chat_process) = loaded.write() {
                chat_process.cancel();
            }
        }
        Ok(())
    }

    fn is_open(&self) -> bool {
        if let Ok(output_win) = self.output_window.lock()
            && let Some(output_win) = output_win.as_ref()
            && output_win.window.get_window().is_some()
        {
            true
        } else {
            false
        }
    }

    fn is_focused(&self) -> bool {
        if let (Ok(output_win), Ok(input_win)) =
            (self.output_window.lock(), self.input_window.lock())
        {
            let current_win = api::get_current_win();
            if let Some(output_win) = output_win.as_ref()
                && let Some(win) = output_win.window.get_window()
                && current_win == win
            {
                return true;
            }
            if let Some(input_win) = input_win.as_ref()
                && let Some(win) = input_win.window.get_window()
                && current_win == win
            {
                return true;
            }
        }
        false
    }

    pub fn toggle(&mut self) -> OxiResult<()> {
        if !self.is_open() {
            return self.open();
        }

        if self.is_focused() {
            self.close()
        } else {
            // Cursor is outside — focus input window
            self.focus_input_window()
        }
    }

    pub fn close(&mut self) -> OxiResult<()> {
        // We just need to close one of the input/output windows as the windows are linked.
        if let Ok(input_win) = self.input_window.lock()
            && let Some(input_win) = input_win.as_ref()
            && let Some(win) = input_win.window.get_window()
        {
            win.close(true)?;
        }

        Ok(())
    }

    /// Loads the chat process at `index`, keeping windows open.
    pub fn load_chat(&mut self, index: usize) -> OxiResult<()> {
        self.loaded_chat_index.store(index, Ordering::SeqCst);
        if let Ok(mut loaded) = self.loaded_chat_process.write() {
            *loaded = get_or_create_chat_process(index);
        }
        let mut panel = self.get_or_create_output_window()?;
        panel.widget.switch_chat(ChatDisplayData {
            chat_process: self
                .loaded_chat_process
                .read()
                .map_err(|_| {
                    // TODO fix error handling
                    nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(
                        "chat can't be read".to_string(),
                    ))
                })?
                .clone(),
            chat_index: self.loaded_chat_index.load(Ordering::SeqCst),
        })?;
        panel.widget.render()?;

        // Swap the input window buffer to the one for this chat process.
        if let Ok(mut input_panel) = self.input_window.lock() {
            if let Some(panel) = input_panel.as_mut() {
                let chat_key = Self::chat_key(
                    &self
                        .loaded_chat_process
                        .read()
                        .map_err(|_| {
                            nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(
                                "chat can't be read".to_string(),
                            ))
                        })?
                        .clone(),
                );
                if !panel.widget_keys().any(|k| k == &chat_key) {
                    let buffer = self.create_input_buffer()?;
                    let widget = BasicWidget::new(buffer);
                    panel.add_widget(&chat_key, Box::new(widget))?;
                }
                panel.swap_to(chat_key)?;
            }
        }

        Ok(())
    }

    /// Loads the next chat in the list (no-op if already at the last).
    pub fn load_next_chat(&mut self) -> OxiResult<()> {
        let count = chat_process_count();
        let current = self.loaded_chat_index.load(Ordering::SeqCst);
        if current + 1 < count {
            self.load_chat(current + 1)?;
        }
        Ok(())
    }

    /// Loads the previous chat in the list (no-op if already at the first).
    pub fn load_prev_chat(&mut self) -> OxiResult<()> {
        let current = self.loaded_chat_index.load(Ordering::SeqCst);
        if current > 0 {
            self.load_chat(current - 1)?;
        }
        Ok(())
    }

    /// Creates a new chat and loads it.
    pub fn new_chat(&mut self) -> OxiResult<()> {
        let new_index = chat_process_count();
        self.load_chat(new_index)?;
        self.focus_input_window()?;
        Ok(())
    }

    /// Dismisses the current chat. If it was the last one, creates a new
    /// chat and loads it so the window stays open.
    pub fn dismiss_chat(&mut self) -> OxiResult<()> {
        // Remove the dismissed chat's input buffer from the panel.
        if let Ok(loaded) = self.loaded_chat_process.read() {
            let old_key = Self::chat_key(&loaded);
            if let Ok(mut input_panel) = self.input_window.lock() {
                if let Some(panel) = input_panel.as_mut() {
                    panel.remove_widget(old_key);
                }
            }
        }

        remove_chat_process(self.loaded_chat_index.load(Ordering::SeqCst));

        if chat_process_count() == 0 {
            self.new_chat()?;
        } else {
            let current = self.loaded_chat_index.load(Ordering::SeqCst);
            let new_index = current.min(chat_process_count() - 1);
            self.load_chat(new_index)?;
        }

        Ok(())
    }

    /// Returns a stable key for a chat process, based on its Arc pointer address.
    /// This remains valid even when chat indices shift due to dismiss.
    fn chat_key(process: &Arc<RwLock<ChatProcess>>) -> String {
        format!("{:p}", Arc::as_ptr(process))
    }

    /// Creates a new input buffer with the standard keymaps and filetype.
    fn create_input_buffer(&self) -> OxiResult<NvimBuffer> {
        NvimBuffer::new(NvimBufferOption {
            file_type: "markdown".to_string(),
            buf_keymaps: vec![
                NvimKeymap {
                    modes: vec![Mode::Insert, Mode::Normal],
                    lhs: "<c-cr>".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.send()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                },
                NvimKeymap {
                    modes: vec![Mode::Insert, Mode::Normal],
                    lhs: "<c-c>".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.stop_streaming()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                },
                NvimKeymap {
                    modes: vec![Mode::Normal],
                    lhs: "q".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.close()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                },
                NvimKeymap {
                    modes: vec![Mode::Normal],
                    lhs: "<c-n>".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.next_chat()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                },
                NvimKeymap {
                    modes: vec![Mode::Normal],
                    lhs: "<c-p>".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.prev_chat()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                },
                NvimKeymap {
                    modes: vec![Mode::Normal],
                    lhs: "<leader>n".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.new_chat()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                },
                NvimKeymap {
                    modes: vec![Mode::Normal],
                    lhs: "<c-q>".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.dismiss_chat()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                },
                NvimKeymap {
                    modes: vec![Mode::Normal],
                    lhs: "<tab>".to_string(),
                    rhs: "<cmd>lua require('tenon').keymap.toggle_focus()<cr>".to_string(),
                    opts: SetKeymapOpts::default(),
                },
            ],
            ..Default::default()
        })
    }

    fn get_or_create_input_window(&mut self) -> OxiResult<SwappableBufferPanel> {
        if let Ok(win) = self.input_window.lock()
            && let Some(win) = win.as_ref()
            && win.active_widget().buffer().get_buffer().is_some()
            && win.window.get_window().is_some()
        {
            Ok(win.clone())
        } else {
            // TODO use error to handle unwrap in this function
            let output_window = self
                .get_or_create_output_window()?
                .window
                .get_window()
                .unwrap();
            api::set_current_win(&output_window)?;

            let buffer = self.create_input_buffer()?;
            let widget = BasicWidget::new(buffer);
            let panel_option = SwappablePanelOption {
                window_option: NvimWindowType::Split {
                    direction: NvimSplitWindowType::Bottom,
                    ratio_wh: 0.3,
                    edge: false,
                },
                ..Default::default()
            };
            let chat_key = Self::chat_key(
                &self
                    .loaded_chat_process
                    .read()
                    .unwrap_or_else(|x| x.into_inner()),
            );
            let input_win = SwappableBufferPanel::new(&panel_option, &chat_key, Box::new(widget))?;

            let augroup = api::create_augroup(
                "TenonInOutLinkedWindows",
                &CreateAugroupOpts::builder().clear(true).build(),
            )?;
            api::create_autocmd(
                ["WinClosed"],
                &CreateAutocmdOpts::builder()
                    .group(augroup)
                    .patterns([input_win
                        .window
                        .get_window()
                        .unwrap()
                        .handle()
                        .to_string()
                        .as_str()])
                    .callback({
                        let output_window = output_window.clone();
                        move |_| {
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
                            if let Some(win) = input_win.window.get_window()
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

    fn get_or_create_output_window(&mut self) -> OxiResult<FixedBufferPanel<ChatDisplay>> {
        if let Ok(win) = self.output_window.lock()
            && let Some(win) = win.as_ref()
            && win.widget.buffer().get_buffer().is_some()
            && win.window.get_window().is_some()
        {
            Ok(win.clone())
        } else {
            let option = FixedBufferPanelOption {
                window_option: NvimWindowType::Split {
                    direction: NvimSplitWindowType::Right,
                    ratio_wh: 0.4,
                    edge: true,
                },
                modifiable: false,
                file_type: "markdown".to_string(),
                buf_keymaps: vec![
                    NvimKeymap {
                        modes: vec![Mode::Normal],
                        lhs: "q".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.close()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    NvimKeymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-n>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.next_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    NvimKeymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-p>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.prev_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    NvimKeymap {
                        modes: vec![Mode::Normal],
                        lhs: "<leader>n".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.new_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    NvimKeymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-q>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.dismiss_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    NvimKeymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-c>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.stop_streaming()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    NvimKeymap {
                        modes: vec![Mode::Normal],
                        lhs: "<tab>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.toggle_focus()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                ],
                undo_levels: -1,
                sign_column: "yes:3".to_string(),
                number: false,
                relative_number: false,
                ..Default::default()
            };
            let buffer = NvimBuffer::try_from(&option)?;
            let widget = ChatDisplay::new(
                buffer,
                ChatDisplayData {
                    chat_process: self
                        .loaded_chat_process
                        .read()
                        .unwrap_or_else(|x| x.into_inner())
                        .clone(),
                    chat_index: self.loaded_chat_index.load(Ordering::SeqCst),
                },
            );
            let mut win = FixedBufferPanel::new(&option, widget)?;
            win.widget.render()?;
            self.output_window = Arc::new(Mutex::new(Some(win.clone())));

            Ok(win)
        }
    }
}
