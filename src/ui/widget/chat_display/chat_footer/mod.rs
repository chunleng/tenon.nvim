mod footer_state;
mod spinner;

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

use nvim_oxi::api::{
    self,
    opts::{OptionOpts, SetExtmarkOpts},
};

use crate::{ui::nvim_primitives::buffer::NvimBuffer, utils::GLOBAL_EXECUTION_HANDLER};

use super::ChatDisplayData;
use footer_state::{FooterState, FooterValues};
pub use spinner::SpinnerState;

fn render_footer(buffer: Arc<NvimBuffer>, title_line: String, token_line: String) {
    let ns_footer = api::create_namespace("TenonChatFooter");
    let _ = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
        if let Some(mut buffer) = buffer.get_buffer() {
            let buf_opts = OptionOpts::builder().buffer(buffer.clone()).build();
            let _ = nvim_oxi::api::set_option_value("modifiable", true, &buf_opts);

            // Get line count and set footer on last 2 lines
            if let Ok(line_count) = buffer.line_count() {
                let footer_start = line_count.saturating_sub(2);
                let _ = buffer.set_lines(footer_start.., false, vec![title_line, token_line]);

                // Apply TenonLineChatMeta highlight to footer lines
                for line in footer_start..line_count {
                    let opts = SetExtmarkOpts::builder()
                        .end_row(line)
                        .line_hl_group("TenonLineChatMeta")
                        .hl_eol(true)
                        .build();
                    let _ = buffer.set_extmark(ns_footer, line, 0, &opts);
                }
            }

            let _ = nvim_oxi::api::set_option_value("modifiable", false, &buf_opts);
        }
        Ok(())
    });
}

pub struct ChatFooterRenderer {
    pub attached_buffer: Arc<NvimBuffer>,
    chat_data: Arc<RwLock<ChatDisplayData>>,
}

impl ChatFooterRenderer {
    pub fn new(attached_buffer: Arc<NvimBuffer>, chat_data: Arc<RwLock<ChatDisplayData>>) -> Self {
        Self {
            attached_buffer,
            chat_data,
        }
    }

    pub fn spawn_renderer_thread(&self, stop_signal: Arc<AtomicBool>) -> JoinHandle<()> {
        let stop_signal_clone = stop_signal.clone();
        let attached_buffer = self.attached_buffer.clone();
        let chat_data = self.chat_data.clone();
        let ns_spinner = api::create_namespace("TenonChatFooterSpinner");
        let mut spinner = SpinnerState::new();
        let mut footer_state = FooterState::new();

        std::thread::spawn(move || {
            // Initialize footer space on startup
            let buffer_clone = attached_buffer.clone();
            let _ = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
                if let Some(mut buffer) = buffer_clone.get_buffer() {
                    let buf_opts = OptionOpts::builder().buffer(buffer.clone()).build();
                    let _ = nvim_oxi::api::set_option_value("modifiable", true, &buf_opts);
                    let _ = buffer.set_lines(0.., false, vec!["", ""]);
                    let _ = nvim_oxi::api::set_option_value("modifiable", false, &buf_opts);
                }
                Ok(())
            });

            loop {
                if stop_signal_clone.load(Ordering::SeqCst) {
                    break;
                }

                // Render footer if values changed
                let values = FooterValues::from(chat_data.clone());
                if footer_state.should_render(&values) {
                    let (title_line, token_line) = footer_state.get_footer_lines(&values);
                    render_footer(attached_buffer.clone(), title_line, token_line);
                }

                let is_processing = if let Ok(data) = chat_data.read()
                    && let Ok(session) = data.chat_session.read()
                {
                    session.is_processing()
                } else {
                    false
                };

                // Render if processing OR if state just changed from true to false (to clear spinner)
                if spinner.should_render(is_processing) {
                    let buffer_clone = attached_buffer.clone();
                    let ns_spinner_clone = ns_spinner;
                    let spinner_char = spinner.get_char();

                    let _ = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
                        if let Some(mut buffer) = buffer_clone.get_buffer() {
                            // Clear previous extmarks in this namespace
                            let _ = buffer.clear_namespace(ns_spinner_clone, 0..);

                            // Place spinner on second last line if still processing
                            if is_processing && let Ok(line_count) = buffer.line_count() {
                                let second_last_line = line_count.saturating_sub(2);
                                let opts = SetExtmarkOpts::builder()
                                    .sign_text(spinner_char)
                                    .sign_hl_group("TenonSignProcessing")
                                    .build();
                                let _ = buffer.set_extmark(
                                    ns_spinner_clone,
                                    second_last_line,
                                    0,
                                    &opts,
                                );
                            }
                        }
                        Ok(())
                    });

                    if is_processing {
                        spinner.advance();
                    }
                }

                std::thread::sleep(Duration::from_millis(100));
            }
        })
    }
}
