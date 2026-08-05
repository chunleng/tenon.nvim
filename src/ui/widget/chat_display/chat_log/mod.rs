use std::{
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    thread::{JoinHandle, sleep, spawn},
    time::Duration,
};

use nvim_oxi::api::{
    self,
    opts::{OptionOpts, SetExtmarkOpts},
};

use crate::{
    ui::nvim_primitives::{buffer::NvimBuffer, window::NvimWindow},
    ui::widget::chat_display::format::DisplayChatFormatter,
    utils::GLOBAL_EXECUTION_HANDLER,
};

mod cache;

pub use cache::{ChatLogCache, RenderType};

pub struct ChatLogRenderer {
    pub attached_buffer: Arc<NvimBuffer>,
    pub attached_window: Arc<NvimWindow>,
    chat_log_cache: Arc<RwLock<ChatLogCache>>,
}

impl ChatLogRenderer {
    pub fn new(
        attached_buffer: Arc<NvimBuffer>,
        attached_window: Arc<NvimWindow>,
        chat_log_cache: Arc<RwLock<ChatLogCache>>,
    ) -> Self {
        Self {
            attached_buffer,
            attached_window,
            chat_log_cache,
        }
    }
    pub fn spawn_renderer_thread(&self, stop_signal: Arc<AtomicBool>) -> JoinHandle<()> {
        let attached_buffer = self.attached_buffer.clone();
        let attached_window = self.attached_window.clone();
        let chat_log_cache = self.chat_log_cache.clone();
        let ns = api::create_namespace("TenonChatLog");

        spawn(move || {
            loop {
                if stop_signal.load(Ordering::SeqCst) || attached_buffer.get_buffer().is_none() {
                    break;
                }

                let buffer_clone = attached_buffer.clone();
                let window_clone = attached_window.clone();
                let (updates, current_line) = {
                    if let Ok(mut chat_log) = chat_log_cache.write() {
                        chat_log.poll_render_update()
                    } else {
                        (Vec::new(), 0)
                    }
                };
                if !updates.is_empty() {
                    let _ = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
                        if let Some(mut buffer) = buffer_clone.get_buffer()
                            && let Some(window) = window_clone.get_window()
                        {
                            // Check if cursor is at last line before update (tail-line behavior)
                            let follow_last_line = if let Ok(line_count) = buffer.line_count() {
                                if let Ok((cursor_row, _)) = window.get_cursor() {
                                    cursor_row == line_count
                                } else {
                                    false
                                }
                            } else {
                                false
                            };

                            let buf_opts = OptionOpts::builder().buf(buffer.clone()).build();
                            let _ = nvim_oxi::api::set_option_value("modifiable", true, &buf_opts);
                            for update in updates {
                                let mut lines: Vec<String> =
                                    update.target_log.data.lines().into_iter().collect();
                                if let RenderType::Tail(x) = update.render_type
                                    && lines.len() > x
                                {
                                    let skip = lines.len() - x;
                                    lines = lines.into_iter().skip(skip).collect();
                                    if let Some(first) = lines.first_mut() {
                                        *first = format!("... {}", first);
                                    }
                                }
                                let mut lines: Vec<&str> =
                                    lines.iter().map(|s| s.as_str()).collect();
                                if update.line_separator_after {
                                    lines.push("");
                                }
                                update_buffer(
                                    &mut buffer,
                                    ns,
                                    update.replace_line_start,
                                    update.replace_line_end,
                                    &update.sign,
                                    &update.sign_hl_group,
                                    &update.line_hl_group,
                                    &lines,
                                );
                            }

                            // Remove excess lines at end of buffer
                            // This can happen when we remove logs from the session (i.e. from
                            // `prune_incomplete_messages`)
                            // buffer_line_count - 2 (footer) - current_line = excess lines to remove
                            let line_count = buffer.line_count().unwrap_or(0) as i64;
                            let excess_lines = line_count - 2 - current_line as i64;
                            if excess_lines > 0 {
                                let remove_start = current_line;
                                let remove_end = current_line + excess_lines as usize;
                                let _ = buffer.set_lines(
                                    remove_start..remove_end,
                                    false,
                                    Vec::<&str>::new(),
                                );
                            }

                            let _ = nvim_oxi::api::set_option_value("modifiable", false, &buf_opts);

                            // If cursor was at last line, snap last line to bottom via winrestview.
                            // winrestview is not affected by scrolloff or smoothscroll, unlike zb.
                            if follow_last_line
                                && let Ok(new_line_count) = buffer.line_count()
                                && let Ok(win_height) = window.get_height()
                            {
                                let topline = new_line_count
                                    .saturating_sub(win_height as usize)
                                    .saturating_add(1);
                                let lnum = new_line_count;
                                let _ = window.call(move |()| {
                                    let _ = api::command(&format!("call cursor({lnum}, 1)"));
                                    let winline: i64 = api::eval("winline()").unwrap_or(0);
                                    let winheight: i64 = api::eval("winheight(0)").unwrap_or(0);
                                    if winline < winheight {
                                        let _ = api::command(&format!(
                                            "lua vim.fn.winrestview({{topline = {topline}, lnum = {lnum}}})"
                                        ));
                                    }
                                });
                            }
                        }
                        Ok(())
                    });
                }

                sleep(Duration::from_millis(20));
            }
        })
    }
}

fn update_buffer(
    buffer: &mut api::Buffer,
    ns: u32,
    replace_start: usize,
    replace_end: usize,
    sign: &str,
    sign_hl_group: &str,
    line_hl_group: &str,
    lines: &[&str],
) {
    let new_end = lines.len() + replace_start;

    // Compare existing buffer lines with new lines to skip unchanged prefix.
    // During streaming, earlier lines are often unchanged, so we only write
    // from the first diverging line onward to reduce redundant writes.
    let existing_lines: Vec<String> = buffer
        .get_lines(replace_start..replace_end, false)
        .map(|lines| lines.map(|s| s.to_string()).collect())
        .unwrap_or_default();

    let diverge_at = existing_lines
        .iter()
        .zip(lines.iter())
        .position(|(existing, new)| existing.as_str() != *new)
        .unwrap_or_else(|| existing_lines.len().min(lines.len()));

    // All match
    if diverge_at == lines.len() && existing_lines.len() == lines.len() {
        return;
    }

    let write_start = replace_start + diverge_at;
    let _ = buffer.clear_namespace(ns, write_start..replace_end);
    let _ = buffer.set_lines(
        write_start..replace_end,
        false,
        lines[diverge_at..].iter().copied(),
    );

    // Place sign extmark on first line of each log.
    // Only re-place if the first line was in the cleared range (diverge_at == 0);
    // otherwise the existing sign extmark at replace_start is preserved.
    if !sign.is_empty() && diverge_at == 0 {
        let opts = SetExtmarkOpts::builder()
            .sign_text(sign)
            .sign_hl_group(sign_hl_group)
            .build();
        let _ = buffer.set_extmark(ns, replace_start, 0, &opts);
    }

    // Place line highlight extmark on changed lines only
    if !line_hl_group.is_empty() {
        for line in write_start..new_end {
            let opts = SetExtmarkOpts::builder()
                .end_row(line)
                .line_hl_group(line_hl_group)
                .hl_eol(true)
                .build();
            let _ = buffer.set_extmark(ns, line, 0, &opts);
        }
    }
}
