mod components;
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{sleep, spawn},
    time::Duration,
};

use nvim_oxi::{
    Result as OxiResult,
    api::{
        self,
        opts::{CreateAugroupOpts, CreateAutocmdOpts, OptionOpts, SetExtmarkOpts, SetKeymapOpts},
        types::{LogLevel, Mode},
    },
    libuv::AsyncHandle,
    schedule,
};

use crate::{
    chat::{
        ChatProcess, TenonAssistantMessageContent, TenonLog, TenonToolLog, TenonUserMessage,
        TenonUserTextMessage, chat_process_count, get_or_create_chat_process, remove_chat_process,
    },
    ui::components::{
        FixedBufferVimWindow, FixedBufferVimWindowOption, Keymap, SplitWindowOption, WindowOption,
    },
    utils::notify,
};

#[derive(Debug, Default)]
struct RenderState {
    /// Index of the first log entry that needs (re-)rendering.
    /// All entries before this index are frozen (their buffer lines won't change).
    next_render_from: usize,
    /// Number of buffer lines occupied by frozen entries (0..next_render_from).
    frozen_line_count: usize,
}

pub struct ChatWindow {
    output_window: Arc<Mutex<Option<FixedBufferVimWindow>>>,
    input_window: Arc<Mutex<Option<FixedBufferVimWindow>>>,
    /// Shared reference to the currently loaded chat process.
    /// The outer `RwLock` allows swapping the inner `Arc` when loading a different chat,
    /// so the renderer thread always reads from the current chat without closing windows.
    pub loaded_chat_process: Arc<RwLock<Arc<RwLock<ChatProcess>>>>,
    pub loaded_chat_index: Arc<AtomicUsize>,
    render_state: Arc<RwLock<RenderState>>,
    force_rerender: Arc<AtomicBool>,
    spinner_frame: Arc<AtomicUsize>,
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
            render_state: Arc::new(RwLock::new(RenderState::default())),
            force_rerender: Arc::new(AtomicBool::new(false)),
            spinner_frame: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn open(&mut self) -> OxiResult<()> {
        self.get_or_create_output_window()?;
        self.get_or_create_input_window()?;
        Ok(())
    }

    pub fn scroll_output_to_bottom(&mut self) -> OxiResult<()> {
        if let Some(mut output_win_window) = self.get_or_create_output_window()?.get_window()
            && let Ok(line_count) = output_win_window.get_buf().and_then(|b| b.line_count())
        {
            output_win_window.set_cursor(line_count, 0)?;
        }
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

    /// Loads the chat process at `index`, keeping windows open.
    pub fn load_chat(&mut self, index: usize) -> OxiResult<()> {
        self.loaded_chat_index.store(index, Ordering::SeqCst);
        if let Ok(mut loaded) = self.loaded_chat_process.write() {
            *loaded = get_or_create_chat_process(index);
        }
        // Reset render state so the buffer is fully redrawn for the new chat
        if let Ok(mut state) = self.render_state.write() {
            *state = RenderState::default();
        }
        self.force_rerender.store(true, Ordering::SeqCst);
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
        self.load_chat(new_index)
    }

    /// Dismisses the current chat. If it was the last one, closes the window
    /// and prepares a fresh chat for the next open.
    pub fn dismiss_chat(&mut self) -> OxiResult<()> {
        remove_chat_process(self.loaded_chat_index.load(Ordering::SeqCst));

        if chat_process_count() == 0 {
            self.close()?;
            self.loaded_chat_index.store(0, Ordering::SeqCst);
            if let Ok(mut loaded) = self.loaded_chat_process.write() {
                *loaded = get_or_create_chat_process(0);
            }
            if let Ok(mut state) = self.render_state.write() {
                *state = RenderState::default();
            }
        } else {
            let current = self.loaded_chat_index.load(Ordering::SeqCst);
            let new_index = current.min(chat_process_count() - 1);
            self.load_chat(new_index)?;
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
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-n>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.next_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-p>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.prev_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "<leader>n".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.new_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-q>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.dismiss_chat()<cr>".to_string(),
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
                buf_keymaps: vec![
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "q".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.close()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-n>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.next_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-p>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.prev_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "<leader>n".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.new_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                    Keymap {
                        modes: vec![Mode::Normal],
                        lhs: "<c-q>".to_string(),
                        rhs: "<cmd>lua require('tenon').keymap.dismiss_chat()<cr>".to_string(),
                        opts: SetKeymapOpts::default(),
                    },
                ],
                undo_levels: -1,
                sign_column: "yes:3".to_string(),
                number: false,
                relative_number: false,
                ..Default::default()
            })?;
            self.output_window = Arc::new(Mutex::new(Some(win.clone())));

            let (tx, rx) = mpsc::channel();
            let ns_id = api::create_namespace("TenonSigns");
            let render_state = self.render_state.clone();
            let spinner_frame = self.spinner_frame.clone();
            let chat_renderer_handle = AsyncHandle::new({
                let output_window = win.clone();
                let loaded_chat_process = self.loaded_chat_process.clone();
                let loaded_chat_index = self.loaded_chat_index.clone();
                let render_state_clone = render_state.clone();
                let spinner_frame_clone = spinner_frame.clone();
                move || {
                    let (logs, usage_clone) = {
                        if let Ok(loaded) = loaded_chat_process.read() {
                            if let Ok(chat_process) = loaded.read() {
                                (chat_process.logs.clone(), chat_process.usage.clone())
                            } else {
                                return;
                            }
                        } else {
                            return;
                        }
                    };
                    if let Ok(logs) = logs.read() {
                        let log_count = logs.len();

                        let (start_idx, frozen_line_count) = {
                            let state = render_state_clone.read().ok();
                            let next_render_from = state.as_ref().map_or(0, |s| s.next_render_from);
                            let frozen = state.as_ref().map_or(0, |s| s.frozen_line_count);
                            let clamped_start = if log_count == 0 {
                                0
                            } else {
                                next_render_from.min(log_count - 1)
                            };
                            (clamped_start, if log_count == 0 { 0 } else { frozen })
                        };

                        let logs_vec: Vec<_> = logs
                            .iter()
                            .skip(start_idx)
                            .enumerate()
                            .map(|(i, x)| x.as_chat_lines_with_sign(i == log_count - start_idx - 1))
                            .collect();

                        // Collect entries to render (from start_idx onwards)
                        let entry_lines: Vec<(Vec<String>, SignIcon)> = logs_vec
                            .iter()
                            .cloned()
                            .enumerate()
                            .map(|(i, current)| {
                                let next = logs_vec.get(i + 1);
                                if let Some(&(_, icon)) = next
                                    && current.1 == icon
                                {
                                    current
                                } else {
                                    let (mut text, icon) = current;
                                    text.push("".to_string());
                                    (text, icon)
                                }
                            })
                            .collect();

                        let mut content: Vec<String> = entry_lines
                            .iter()
                            .map(|(l, _)| l)
                            .flatten()
                            .cloned()
                            .collect();

                        // Spinner line
                        const SPINNER_CHARS: [&str; 7] = ["⣀⣤", "⣶⣿", "⣿⣿", "⣿⣶", "⣶⣤", "⣤⣀", "⣀⣀"];
                        let spinner_idx =
                            spinner_frame_clone.load(Ordering::SeqCst) % SPINNER_CHARS.len();
                        let is_currently_processing = if let Ok(loaded) = loaded_chat_process.read()
                        {
                            loaded.read().map_or(false, |cp| cp.is_processing())
                        } else {
                            false
                        };
                        let chat_index_display = {
                            let idx = loaded_chat_index.load(Ordering::SeqCst);
                            let total = chat_process_count();
                            format!("{} of {}", idx + 1, total)
                        };
                        let model_display = if let Ok(loaded) = loaded_chat_process.read() {
                            loaded
                                .read()
                                .map_or_else(|_| String::new(), |cp| cp.model.display_name())
                        } else {
                            String::new()
                        };
                        content.push(format!("󰭹  {}, {}", chat_index_display, model_display));
                        let spinner_buf_line = frozen_line_count + content.len() - 1;

                        let usage_buf_line;
                        {
                            let (input, output, cached, total) = if let Ok(usage) =
                                usage_clone.read()
                                && let Some(usage) = usage.as_ref()
                            {
                                (
                                    usage.input_tokens,
                                    usage.output_tokens,
                                    usage.cached_input_tokens,
                                    usage.total_tokens,
                                )
                            } else {
                                (0, 0, 0, 0)
                            };
                            content.push(format!(
                                "{} 󰕒 | {} 󰇚 | {}  | {} total",
                                input, output, cached, total
                            ));
                            usage_buf_line = Some(frozen_line_count + content.len() - 1);
                        }

                        // Collect sign placements: (buf_line_idx, SignIcon)
                        // Collect line highlight placements: (buf_line_idx, hl_group)
                        let mut signs: Vec<(usize, SignIcon)> = Vec::new();
                        let mut line_highlights: Vec<(usize, &str)> = Vec::new();
                        let mut buf_line = frozen_line_count;
                        for (lines, sign) in &entry_lines {
                            signs.push((buf_line, *sign));
                            if let Some(hl) = sign.line_hl_group() {
                                for offset in 0..lines.len() {
                                    line_highlights.push((buf_line + offset, hl));
                                }
                            }
                            buf_line += lines.len();
                        }
                        if let Some(ul) = usage_buf_line {
                            line_highlights.push((ul, "TenonLineChatMeta"));
                        }
                        line_highlights.push((spinner_buf_line, "TenonLineChatMeta"));

                        // Compute new render state for after buffer update
                        let (new_next_render_from, new_frozen_line_count) = if log_count == 0 {
                            (0, 0)
                        } else {
                            // Entries that become frozen: start_idx..log_count-1 (exclusive of last)
                            let newly_frozen_count = log_count - 1 - start_idx;
                            let newly_frozen_lines: usize = entry_lines[..newly_frozen_count]
                                .iter()
                                .map(|(l, _)| l.len())
                                .sum();
                            (log_count - 1, frozen_line_count + newly_frozen_lines)
                        };

                        if let Some(mut buffer) = output_window.get_buffer()
                            && let Some(mut window) = output_window.get_window()
                        {
                            let tx_clone = tx.clone();
                            let render_state_clone_2 = render_state_clone.clone();
                            schedule({
                                move |_| {
                                    if let Ok(line_count) = buffer.line_count() {
                                        let mut follow_last_line = false;
                                        if let Ok((cursor_row, _)) = window.get_cursor() {
                                            follow_last_line = cursor_row == line_count;
                                        };

                                        let buf_opts =
                                            OptionOpts::builder().buffer(buffer.clone()).build();
                                        let _ =
                                            api::set_option_value("modifiable", true, &buf_opts);
                                        let _ =
                                            buffer.set_lines(frozen_line_count.., false, content);
                                        let _ =
                                            api::set_option_value("modifiable", false, &buf_opts);
                                        let _ = api::set_option_value("modified", false, &buf_opts);

                                        // Place sign extmarks
                                        buffer
                                            .clear_namespace(ns_id, frozen_line_count..line_count)
                                            .ok();
                                        for (line, icon) in &signs {
                                            let opts = SetExtmarkOpts::builder()
                                                .sign_text(icon.text())
                                                .sign_hl_group(icon.hl_group())
                                                .build();
                                            buffer.set_extmark(ns_id, *line, 0, &opts).ok();
                                        }
                                        let spinner_sign = if is_currently_processing {
                                            SPINNER_CHARS[spinner_idx]
                                        } else {
                                            ""
                                        };
                                        if !spinner_sign.is_empty() {
                                            let opts = SetExtmarkOpts::builder()
                                                .sign_text(spinner_sign)
                                                .sign_hl_group("TenonSignProcessing")
                                                .build();
                                            buffer
                                                .set_extmark(ns_id, spinner_buf_line, 0, &opts)
                                                .ok();
                                        }

                                        // Place line highlight extmarks
                                        for (line, hl) in &line_highlights {
                                            let opts = SetExtmarkOpts::builder()
                                                .end_line((line + 1).try_into().unwrap())
                                                .hl_group(*hl)
                                                .hl_eol(true)
                                                .build();
                                            buffer.set_extmark(ns_id, *line, 0, &opts).ok();
                                        }

                                        if follow_last_line
                                            && let Ok(new_line_count) = buffer.line_count()
                                            && let Ok((_, cursor_col)) = window.get_cursor()
                                        {
                                            let _ = window.set_cursor(new_line_count, cursor_col);
                                        }
                                    }

                                    if let Ok(mut state) = render_state_clone_2.write() {
                                        state.next_render_from = new_next_render_from;
                                        state.frozen_line_count = new_frozen_line_count;
                                    }

                                    let _ = tx_clone.send(());
                                }
                            })
                        }
                    }
                }
            })?;

            spawn({
                let output_window = win.clone();
                let loaded_chat_process = self.loaded_chat_process.clone();
                let force_rerender = self.force_rerender.clone();
                let spinner_frame = self.spinner_frame.clone();
                move || {
                    // Set true so that the first run will alway try to redraw
                    let mut previous_processing_state = true;
                    let mut tick: u32 = 0;
                    loop {
                        if output_window.get_buffer().is_none() {
                            break;
                        }
                        let is_processing = if let Ok(loaded) = loaded_chat_process.read() {
                            if let Ok(chat_process) = loaded.read() {
                                let current_state = chat_process.is_processing();
                                // Check both current and previous state, we want to render one more
                                // time after the state change
                                let r = current_state || previous_processing_state;
                                previous_processing_state = current_state;
                                r
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if is_processing || force_rerender.swap(false, Ordering::SeqCst) {
                            if is_processing && tick % 3 == 0 {
                                spinner_frame.fetch_add(1, Ordering::SeqCst);
                            }
                            tick = tick.wrapping_add(1);
                            let _ = chat_renderer_handle.send();
                            let _ = rx.recv();
                        }
                        sleep(Duration::from_millis(20))
                    }
                }
            });
            Ok(win)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SignIcon {
    User,
    AssistantReasoning,
    AssistantTalk,
    Tool,
}

impl SignIcon {
    fn text(&self) -> &str {
        match self {
            SignIcon::User => " ",
            SignIcon::AssistantReasoning => " ",
            SignIcon::AssistantTalk => "󰚩 ",
            SignIcon::Tool => "󰣖 ",
        }
    }
    fn hl_group(&self) -> &str {
        match self {
            SignIcon::User => "TenonSignUser",
            SignIcon::AssistantReasoning => "TenonSignAssistantReasoning",
            SignIcon::AssistantTalk => "TenonSignAssistantTalk",
            SignIcon::Tool => "TenonSignTool",
        }
    }
    fn line_hl_group(&self) -> Option<&'static str> {
        match self {
            SignIcon::AssistantReasoning => Some("TenonLineAssistantReasoning"),
            SignIcon::Tool => Some("TenonLineTool"),
            _ => None,
        }
    }
}

trait DisplayAsChat {
    fn as_chat_lines_with_sign(&self, is_processing: bool) -> (Vec<String>, SignIcon);
}

impl DisplayAsChat for TenonLog {
    fn as_chat_lines_with_sign(&self, is_processing: bool) -> (Vec<String>, SignIcon) {
        match self {
            TenonLog::User(TenonUserMessage::Text(TenonUserTextMessage(msg))) => {
                (msg.lines().map(|x| x.to_string()).collect(), SignIcon::User)
            }
            TenonLog::Assistant(msg) => {
                if msg.content.is_empty() {
                    let display_last_x = if is_processing { 3 } else { 1 };
                    let reasoning_text = msg.reasoning.clone().unwrap_or("[thoughts]".to_string());
                    let lines = reasoning_text.lines().collect::<Vec<_>>();
                    let mut displayed_lines = lines
                        .iter()
                        .skip(lines.len().saturating_sub(display_last_x))
                        .map(|y| y.to_string())
                        .collect::<Vec<_>>();
                    if lines.len() > display_last_x {
                        displayed_lines
                            .get_mut(0)
                            .map(|x| *x = format!("... {}", x));
                    }
                    (displayed_lines, SignIcon::AssistantReasoning)
                } else {
                    (
                        msg.content
                            .clone()
                            .into_iter()
                            .flat_map(|x| match x {
                                TenonAssistantMessageContent::Text(s) => {
                                    s.lines().map(|x| x.to_string()).collect::<Vec<_>>()
                                }
                            })
                            .map(|x| x.to_string())
                            .collect::<Vec<_>>(),
                        SignIcon::AssistantTalk,
                    )
                }
            }
            TenonLog::Tool(TenonToolLog {
                tool_call,
                tool_result,
            }) => (
                vec![format!(
                    "[{}] {}",
                    tool_call.name,
                    if tool_result.is_some() {
                        "Done!"
                    } else {
                        "Running.."
                    }
                )],
                SignIcon::Tool,
            ),
        }
    }
}
