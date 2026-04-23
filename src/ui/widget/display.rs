use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::{JoinHandle, sleep, spawn},
    time::Duration,
};

use nvim_oxi::{
    Result as OxiResult,
    api::{
        self,
        opts::{OptionOpts, SetExtmarkOpts},
    },
    libuv::AsyncHandle,
    schedule,
};

use crate::{
    chat::{
        ChatProcess, TenonAssistantMessageContent, TenonLog, TenonToolLog, TenonUserMessage,
        TenonUserTextMessage, chat_process_count,
    },
    get_application_config,
    tools::{resolve_tool_names, tool_display_summary},
    ui::{
        nvim_primitives::{buffer::NvimBuffer, window::NvimWindow},
        widget::Widget,
    },
};

#[derive(Clone)]
pub struct ChatDisplayData {
    pub chat_process: Arc<RwLock<ChatProcess>>,
    pub chat_index: usize,
}

#[derive(Debug, Default)]
struct RenderState {
    /// Index of the first log entry that needs (re-)rendering.
    /// All entries before this index are frozen (their buffer lines won't change).
    next_render_from: usize,
    /// Number of buffer lines occupied by frozen entries (0..next_render_from).
    frozen_line_count: usize,
}

#[derive(Clone)]
pub struct ChatDisplay {
    pub inner: Arc<NvimBuffer>,
    attached_window: Option<Arc<NvimWindow>>,
    attached_chat: Arc<RwLock<ChatDisplayData>>,
    render_state: Arc<RwLock<RenderState>>,
    force_rerender: Arc<AtomicBool>,
    spinner_frame: Arc<AtomicUsize>,
    tool_added: Arc<AtomicUsize>,
    tool_removed: Arc<AtomicUsize>,
    running_thread: Option<Arc<JoinHandle<()>>>,
}

impl ChatDisplay {
    pub fn new(buffer: NvimBuffer, chat: ChatDisplayData) -> Self {
        Self {
            inner: Arc::new(buffer),
            attached_window: None,
            attached_chat: Arc::new(RwLock::new(chat)),
            render_state: Arc::new(RwLock::new(RenderState::default())),
            force_rerender: Arc::new(AtomicBool::new(false)),
            spinner_frame: Arc::new(AtomicUsize::new(0)),
            tool_added: Arc::new(AtomicUsize::new(0)),
            tool_removed: Arc::new(AtomicUsize::new(0)),
            running_thread: None,
        }
    }

    pub fn switch_chat(&mut self, chat: ChatDisplayData) -> OxiResult<()> {
        if let Ok(mut current_chat) = self.attached_chat.write() {
            *current_chat = chat;
        }
        Ok(())
    }

    fn spawn_refresh_display_thread(&mut self) -> OxiResult<()> {
        let (tx, rx) = mpsc::channel();
        let ns_id = api::create_namespace("TenonSigns");
        let render_state = self.render_state.clone();
        let spinner_frame = self.spinner_frame.clone();
        let tool_added = self.tool_added.clone();
        let tool_removed = self.tool_removed.clone();
        let chat_renderer_handle = AsyncHandle::new({
            let inner = self.inner.clone();
            let attached_window = self.attached_window.clone();
            let attached_chat = self.attached_chat.clone();
            let render_state_clone = render_state.clone();
            let spinner_frame_clone = spinner_frame.clone();
            let tool_added_clone = tool_added.clone();
            let tool_removed_clone = tool_removed.clone();
            move || {
                let (logs, usage_clone) = {
                    if let Ok(chat) = attached_chat.read()
                        && let Ok(chat_process) = chat.chat_process.read()
                    {
                        (chat_process.logs.clone(), chat_process.usage.clone())
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
                    const SPINNER_CHARS: [&str; 8] =
                        ["⣀⣤", "⣤⣶", "⣶⣿", "⣿⣿", "⣿⣶", "⣶⣤", "⣤⣀", "⣀⣀"];
                    let spinner_idx =
                        spinner_frame_clone.load(Ordering::SeqCst) % SPINNER_CHARS.len();
                    let chat = attached_chat
                        .read()
                        .unwrap_or_else(|poison| poison.into_inner());
                    let chat_process = chat.chat_process.read().unwrap_or_else(|x| x.into_inner());
                    let is_currently_processing = chat_process.is_processing();
                    let chat_index_display = {
                        let idx = chat.chat_index;
                        let total = chat_process_count();
                        format!("{} of {}", idx + 1, total)
                    };
                    let agent_name = chat_process.active_agent.name.clone();
                    let model_display = chat_process.active_agent.inner.model.display_name();
                    drop(chat_process);
                    drop(chat);
                    let added = tool_added_clone.load(Ordering::SeqCst);
                    let removed = tool_removed_clone.load(Ordering::SeqCst);
                    let default_model_display = {
                        let config = get_application_config();
                        config
                            .agents
                            .get(&agent_name)
                            .map(|a| a.model.display_name())
                    };
                    let model_changed = default_model_display.as_ref() != Some(&model_display);
                    let tool_suffix = match (added, removed) {
                        (0, 0) => String::new(),
                        (a, 0) => format!("󰣖 +{}", a),
                        (0, r) => format!("󰣖 -{}", r),
                        (a, r) => format!("󰣖 +{}/-{}", a, r),
                    };
                    let meta_suffix = match (model_changed, tool_suffix.is_empty()) {
                        (true, true) => format!(" (󰚩 {})", model_display),
                        (true, false) => format!(" (󰚩 {} | {})", model_display, tool_suffix),
                        (false, true) => String::new(),
                        (false, false) => format!(" ({})", tool_suffix),
                    };
                    content.push(format!(
                        "󰭹  {}, agent: {}{}",
                        chat_index_display, agent_name, meta_suffix
                    ));
                    let spinner_buf_line = frozen_line_count + content.len() - 1;

                    let usage_buf_line;
                    {
                        let (input, output, cached, total) = if let Ok(usage) = usage_clone.read()
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

                    if let Some(mut buffer) = inner.get_buffer()
                        && let Some(ref aw) = attached_window
                        && let Some(mut window) = aw.get_window()
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
                                    let _ = api::set_option_value("modifiable", true, &buf_opts);
                                    let _ = buffer.set_lines(frozen_line_count.., false, content);
                                    let _ = api::set_option_value("modifiable", false, &buf_opts);
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
                                        buffer.set_extmark(ns_id, spinner_buf_line, 0, &opts).ok();
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
                                        let _ = window.call(|()| {
                                            _ = api::command("normal! zb");
                                        });
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
        self.running_thread = Some(Arc::new(spawn({
            let inner = self.inner.clone();
            let chat = self.attached_chat.clone();
            let force_rerender = self.force_rerender.clone();
            let spinner_frame = self.spinner_frame.clone();
            let tool_added = self.tool_added.clone();
            let tool_removed = self.tool_removed.clone();
            move || {
                // Set true so that the first run will alway try to redraw
                let mut previous_processing_state = true;
                let mut tick: u32 = 0;
                loop {
                    if inner.get_buffer().is_none() {
                        break;
                    }

                    let is_processing = if let Ok(chat_process) = chat
                        .read()
                        .unwrap_or_else(|x| x.into_inner())
                        .chat_process
                        .read()
                    {
                        let current_state = chat_process.is_processing();
                        // Check both current and previous state, we want to render one more
                        // time after the state change
                        let r = current_state || previous_processing_state;
                        previous_processing_state = current_state;
                        r
                    } else {
                        false
                    };

                    if is_processing || force_rerender.swap(false, Ordering::SeqCst) {
                        // Compute tool diff off-thread (safe to call resolve_tool_names here)
                        // TODO make better performance by managing tools mcp server tool lifecycle
                        {
                            let (agent_name, current_tools) = {
                                let chat_data = chat.read().unwrap_or_else(|x| x.into_inner());
                                let chat_process = chat_data
                                    .chat_process
                                    .read()
                                    .unwrap_or_else(|x| x.into_inner());
                                (
                                    chat_process.active_agent.name.clone(),
                                    chat_process.active_agent.tool_names.clone(),
                                )
                            };
                            let config_tools = get_application_config()
                                .agents
                                .get(&agent_name)
                                .map(|a| a.tool_names.clone())
                                .unwrap_or_default();
                            let current_resolved = resolve_tool_names(&current_tools);
                            let config_resolved = resolve_tool_names(&config_tools);
                            let added = current_resolved
                                .iter()
                                .filter(|t| !config_resolved.contains(t))
                                .count();
                            let removed = config_resolved
                                .iter()
                                .filter(|t| !current_resolved.contains(t))
                                .count();
                            tool_added.store(added, Ordering::SeqCst);
                            tool_removed.store(removed, Ordering::SeqCst);
                        }

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
        })));
        Ok(())
    }
}

impl Widget for ChatDisplay {
    fn render(&mut self) -> OxiResult<()> {
        if self.running_thread.is_some() {
            if let Ok(mut state) = self.render_state.write() {
                *state = RenderState::default();
            }
            self.force_rerender.store(true, Ordering::SeqCst);
            return Ok(());
        }

        self.spawn_refresh_display_thread()?;

        Ok(())
    }

    fn buffer(&self) -> &NvimBuffer {
        &self.inner
    }

    fn set_window(&mut self, window: NvimWindow) {
        self.attached_window = Some(Arc::new(window));
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
            }) => {
                let (status, extra_lines) = match tool_result {
                    None => (" ", Vec::new()),
                    Some(Ok(_)) => (" ", Vec::new()),
                    Some(Err(err)) => (
                        " ",
                        err.display_message()
                            .lines()
                            .map(|x| format!("   > {}", x))
                            .collect::<Vec<_>>(),
                    ),
                };
                let summary = tool_display_summary(&tool_call.name, &tool_call.args);
                let line = match summary {
                    Some(s) => format!("{} {} | {}", status, tool_call.name, s),
                    None => format!("{} {}", status, tool_call.name),
                };
                let mut lines = vec![line];
                lines.extend(extra_lines);
                (lines, SignIcon::Tool)
            }
        }
    }
}
