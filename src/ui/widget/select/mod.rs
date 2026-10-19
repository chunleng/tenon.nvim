use std::sync::{Arc, Mutex};

use nvim_oxi::{
    Function, Result as OxiResult,
    api::{
        self,
        opts::{CreateAutocmdOpts, SetExtmarkOpts, SetKeymapOpts},
        types::Mode,
    },
};

use crate::tools::ask_question::AskQuestionOption;
use crate::ui::{
    nvim_primitives::buffer::{NvimBuffer, NvimBufferOption, NvimKeymap},
    widget::Widget,
};

pub mod approval;
pub mod question;

/// A selection widget that displays a title followed by a list of options.
/// Each option gets a bullet sign; recommended options additionally get a
/// star sign in the sign column.
/// Supports hover highlighting and selection via `<cr>` / `<c-c>` callbacks.
#[derive(Clone)]
pub struct SelectWidget {
    inner: NvimBuffer,
}

impl SelectWidget {
    pub fn new(
        title: &str,
        options: &[AskQuestionOption],
        // Using FnMut, instead of FnOnce, for allowing reusability in case of multi-layer
        // input requiring current input to be reused again
        on_select: Option<Box<dyn FnMut(usize) + Send + Sync>>,
        on_cancel: Option<Box<dyn FnOnce() + Send + Sync>>,
        base_keymaps: Vec<NvimKeymap>,
    ) -> OxiResult<Self> {
        let title_lines: Vec<String> = title.lines().map(String::from).collect();
        let title_line_count = title_lines.len();

        let mut lines = title_lines;
        lines.push(String::new());

        // Track 1-indexed line ranges for each option so selection can return full text.
        let mut option_ranges = Vec::new();
        let mut current_line = title_line_count + 2; // 1-indexed: title lines + blank
        for option in options {
            let option_line_count = option.text.lines().count().max(1);
            let start = current_line;
            let end = current_line + option_line_count - 1;
            option_ranges.push((start, end, option.text.clone()));
            current_line = end + 1;
            lines.extend(option.text.lines().map(String::from));
        }

        let mut buffer = NvimBuffer::new(NvimBufferOption {
            file_type: "markdown".to_string(),
            modifiable: true,
            ..Default::default()
        })?;
        buffer.inner.set_lines(0.., false, lines)?;

        let buf_opts = api::opts::OptionOpts::builder()
            .buf(buffer.inner.clone())
            .build();
        api::set_option_value("modifiable", false, &buf_opts)?;

        // Every option gets a bullet sign; recommended options additionally get
        // a star sign. Signs on a line render highest-priority first (leftmost),
        // so the bullet outranks the star to appear left of it. Own namespace so
        // the hover autocmd below, which clears its namespace on CursorMoved,
        // never removes them.
        let sign_ns = api::create_namespace("TenonSelectSign");
        for ((start, _, _), option) in option_ranges.iter().zip(options) {
            let opts = SetExtmarkOpts::builder()
                .sign_text("⁃")
                .sign_hl_group("TenonSignSelectBullet")
                .priority(20)
                .build();
            let _ = buffer.inner.set_extmark(sign_ns, start - 1, 0, &opts);
            if option.recommended {
                let opts = SetExtmarkOpts::builder()
                    .sign_text("*")
                    .sign_hl_group("TenonSignSelectRecommended")
                    .priority(10)
                    .build();
                let _ = buffer.inner.set_extmark(sign_ns, start - 1, 0, &opts);
            }
        }

        // Highlight the title lines. Own namespace so the hover autocmd below,
        // which clears its namespace on CursorMoved, never removes it.
        if title_line_count > 0 {
            let title_ns = api::create_namespace("TenonSelectTitle");
            let opts = SetExtmarkOpts::builder()
                .end_row(title_line_count - 1)
                .line_hl_group("TenonLineInputTitle")
                .hl_eol(true)
                .build();
            let _ = buffer.inner.set_extmark(title_ns, 0, 0, &opts);
        }

        // Apply base keymaps first; SelectWidget's own <cr>/<c-c> below override on conflict.
        for keymap in base_keymaps {
            for mode in &keymap.modes {
                buffer
                    .inner
                    .set_keymap(*mode, &keymap.lhs, &keymap.rhs, &keymap.opts)?;
            }
        }

        // Hover highlight: highlight the option line under the cursor.
        let ns = api::create_namespace("TenonSelectHover");
        let hover_buf = buffer.inner.clone();
        let hover_ranges = option_ranges.clone();
        api::create_autocmd(
            ["CursorMoved"],
            &CreateAutocmdOpts::builder()
                .buffer(hover_buf.clone())
                .callback(move |_| {
                    let mut buf = hover_buf.clone();
                    let win = api::get_current_win();
                    if let Ok((row, _)) = win.get_cursor() {
                        let _ = buf.clear_namespace(ns, 0..);
                        if let Some((start, end, _)) = hover_ranges
                            .iter()
                            .find(|(start, end, _)| row >= *start && row <= *end)
                        {
                            let opts = SetExtmarkOpts::builder()
                                .end_row(end - 1)
                                .line_hl_group("Visual")
                                .hl_eol(true)
                                .build();
                            let _ = buf.set_extmark(ns, start - 1, 0, &opts);
                        }
                    }
                    false
                })
                .build(),
        )?;

        if on_select.is_some() || on_cancel.is_some() {
            let on_select = Arc::new(Mutex::new(on_select));
            let on_cancel = Arc::new(Mutex::new(on_cancel));
            let ranges_for_cr = Arc::new(option_ranges.clone());

            let cr_on_select = Arc::clone(&on_select);
            let cr_callback = Function::from_fn(move |()| {
                let row = api::get_current_win()
                    .get_cursor()
                    .map(|(row, _)| row)
                    .unwrap_or(0);
                let idx = ranges_for_cr
                    .iter()
                    .position(|(start, end, _)| row >= *start && row <= *end);
                if let Some(idx) = idx
                    && let Some(mut guard) = cr_on_select.lock().ok()
                    && let Some(handler) = guard.as_mut()
                {
                    handler(idx);
                }
            });
            let cr_opts = SetKeymapOpts::builder().callback(cr_callback).build();
            buffer
                .inner
                .set_keymap(Mode::Normal, "<cr>", "", &cr_opts)?;

            let cc_on_cancel = Arc::clone(&on_cancel);
            let cc_callback = Function::from_fn(move |()| {
                if let Some(mut guard) = cc_on_cancel.lock().ok()
                    && let Some(handler) = guard.take()
                {
                    handler();
                }
            });
            let cc_opts = SetKeymapOpts::builder().callback(cc_callback).build();
            buffer
                .inner
                .set_keymap(Mode::Normal, "<c-c>", "", &cc_opts)?;
        }

        Ok(Self { inner: buffer })
    }
}

impl Widget for SelectWidget {
    fn render(&mut self) -> OxiResult<()> {
        Ok(())
    }

    fn buffer(&self) -> &NvimBuffer {
        &self.inner
    }
}
