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
    utils::GLOBAL_EXECUTION_HANDLER,
};

mod cache;

pub use cache::ChatLogCache;

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
                            && let Some(mut window) = window_clone.get_window()
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

                            let buf_opts = OptionOpts::builder().buffer(buffer.clone()).build();
                            let _ = nvim_oxi::api::set_option_value("modifiable", true, &buf_opts);
                            for update in updates {
                                let replace_start = update.replace_line_start;
                                let replace_end = update.replace_line_end;
                                let lines: Vec<&str> =
                                    update.lines.iter().map(|s| s.as_str()).collect();
                                let new_end = lines.len() + replace_start;
                                let _ = buffer.clear_namespace(ns, replace_start..replace_end);
                                let _ = buffer.set_lines(replace_start..replace_end, false, lines);

                                // Place sign extmark on first line of each log
                                if !update.sign.is_empty() {
                                    let opts = SetExtmarkOpts::builder()
                                        .sign_text(&update.sign)
                                        .sign_hl_group(update.sign_hl_group.as_str())
                                        .build();
                                    let _ = buffer.set_extmark(ns, replace_start, 0, &opts);
                                }

                                // Place line highlight extmark on all lines
                                if !update.line_hl_group.is_empty() {
                                    for line in replace_start..new_end {
                                        let opts = SetExtmarkOpts::builder()
                                            .end_row(line)
                                            .line_hl_group(update.line_hl_group.as_str())
                                            .hl_eol(true)
                                            .build();
                                        let _ = buffer.set_extmark(ns, line, 0, &opts);
                                    }
                                }
                            }

                            // Remove excess lines at end of buffer
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

                            // If cursor was at last line, move to new last line and scroll
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
                        Ok(())
                    });
                }

                sleep(Duration::from_millis(20));
            }
        })
    }
}
