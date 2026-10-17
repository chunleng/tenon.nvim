use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use nvim_oxi::{
    Function, Result as OxiResult,
    api::{
        self,
        opts::{CreateAutocmdOpts, SetExtmarkOpts, SetKeymapOpts},
        types::{AutocmdCallbackArgs, LogLevel, Mode},
    },
};

use crate::ui::{
    nvim_primitives::buffer::{NvimBuffer, NvimBufferOption, NvimKeymap},
    nvim_primitives::window::NvimWindow,
    widget::Widget,
};
use crate::utils::notify;

/// Options for building a `FreeTextWidget`.
#[derive(Clone)]
pub struct FreeTextWidgetOption<'a> {
    /// Text placed in the input region when the widget opens.
    pub default_text: Option<&'a str>,
    /// Reject submissions whose text is blank after trimming whitespace
    pub reject_empty_submit: bool,
    /// Trim leading and trailing whitespace from the submitted text before passing it to the submit
    /// handler.
    pub trim_answer: bool,
}

impl Default for FreeTextWidgetOption<'_> {
    fn default() -> Self {
        Self {
            default_text: None,
            reject_empty_submit: true,
            trim_answer: true,
        }
    }
}

/// A free-text input widget: a fully editable buffer with a title.
///
/// The title is text rendered at the top of the buffer, followed by a blank separator line; every
/// line below is user input.
///
/// The title region is protected by two buffer-local guards:
/// - CursorMovedI and InsertEnter guard that bounces the cursor off the title region in insert
///   mode; normal mode may rest on the title
/// - TextChanged/I guard that restores the title region if it is modified or deleted and re-applies
///   the highlight
#[derive(Clone)]
pub struct FreeTextWidget {
    inner: NvimBuffer,
    window: Option<NvimWindow>,
}

impl FreeTextWidget {
    pub fn new(
        title: &str,
        option: FreeTextWidgetOption<'_>,
        // Using FnMut, instead of FnOnce, for allowing reusability in case of multi-layer
        // input requiring current input to be reused again
        on_submit: Option<Box<dyn FnMut(String) + Send + Sync>>,
        on_cancel: Option<Box<dyn FnOnce() + Send + Sync>>,
        base_keymaps: Vec<NvimKeymap>,
    ) -> OxiResult<Self> {
        let mut buffer = NvimBuffer::new(NvimBufferOption {
            file_type: "markdown".to_string(),
            buf_keymaps: base_keymaps,
            ..Default::default()
        })?;

        let top_lines = Self::top_lines(title);
        let top_len = top_lines.len();
        let mut initial_lines = top_lines.clone();
        match option.default_text {
            Some(text) => {
                // Normalize CRLF (and a trailing lone CR) to LF before splitting. split (unlike
                // lines) preserves a trailing empty line.
                let mut normalized = text.replace("\r\n", "\n");
                if normalized.ends_with('\r') {
                    normalized.pop();
                }
                initial_lines.extend(normalized.split('\n').map(|s| s.to_string()));
            }
            None => initial_lines.push(String::new()),
        }
        buffer.inner.set_lines(0.., false, initial_lines)?;
        Self::set_title_highlight(&mut buffer.inner, &top_lines)?;

        Self::register_cursor_guard(&buffer.inner, &top_lines)?;
        Self::register_text_guard(&buffer.inner, &top_lines)?;

        if on_submit.is_some() || on_cancel.is_some() {
            let on_submit = Arc::new(Mutex::new(on_submit));
            let on_cancel = Arc::new(Mutex::new(on_cancel));

            // Each registration needs its own Function (own LuaRef): Function::clone shares the
            // ref, and registering it twice means Neovim unrefs it twice on buffer delete,
            // corrupting the registry freelist.
            let submit_buf = buffer.inner.clone();
            let reject_empty = option.reject_empty_submit;
            let trim_answer = option.trim_answer;
            let make_submit = || {
                let buf = submit_buf.clone();
                let handler = on_submit.clone();
                Function::from_fn(move |()| {
                    // The title region is not user input.
                    let text = buf
                        .get_lines(top_len.., false)
                        .map(|lines| lines.map(|l| l.to_string()).collect::<Vec<_>>().join("\n"))
                        .unwrap_or_default();
                    if reject_empty && text.trim().is_empty() {
                        notify("Cannot submit an empty answer", LogLevel::Error);
                        return;
                    }
                    let text = if trim_answer {
                        text.trim().to_string()
                    } else {
                        text
                    };
                    if let Some(mut guard) = handler.lock().ok()
                        && let Some(f) = guard.as_mut()
                    {
                        f(text);
                    }
                })
            };
            buffer.inner.set_keymap(
                Mode::Insert,
                "<c-cr>",
                "",
                &SetKeymapOpts::builder().callback(make_submit()).build(),
            )?;
            buffer.inner.set_keymap(
                Mode::Normal,
                "<c-cr>",
                "",
                &SetKeymapOpts::builder().callback(make_submit()).build(),
            )?;

            let make_cancel = || {
                let handler = on_cancel.clone();
                Function::from_fn(move |()| {
                    if let Some(mut guard) = handler.lock().ok()
                        && let Some(f) = guard.take()
                    {
                        f();
                    }
                })
            };
            buffer.inner.set_keymap(
                Mode::Insert,
                "<c-c>",
                "",
                &SetKeymapOpts::builder().callback(make_cancel()).build(),
            )?;
            buffer.inner.set_keymap(
                Mode::Normal,
                "<c-c>",
                "",
                &SetKeymapOpts::builder().callback(make_cancel()).build(),
            )?;
        }

        Ok(Self {
            inner: buffer,
            window: None,
        })
    }

    fn top_lines(title: &str) -> Vec<String> {
        let mut lines: Vec<String> = title.lines().map(str::to_string).collect();
        lines.push(String::new());
        lines
    }

    /// Registers the CursorMovedI/InsertEnter guard that bounces the cursor off the title region in
    /// insert mode
    fn register_cursor_guard(buffer: &api::Buffer, top_lines: &[String]) -> OxiResult<()> {
        let top_len = top_lines.len();
        let cursor_buf = buffer.clone();
        let expected_top = top_lines.to_vec();
        let opts = CreateAutocmdOpts::builder()
            .buf(cursor_buf.clone())
            .callback(move |args: AutocmdCallbackArgs| {
                let entering_insert = args.event == "InsertEnter";
                let mut win = api::get_current_win();

                if let Ok((line, col)) = win.get_cursor()
                    && line <= top_len
                {
                    let damaged = cursor_buf
                        .get_lines(line - 1..line, false)
                        .map(|mut lines| match lines.next() {
                            Some(l) => l != expected_top[line - 1],
                            None => true,
                        })
                        .unwrap_or(true);
                    if !damaged {
                        // Neovim restores the cursor to its pre-InsertEnter position after the
                        // autocmd unless v:char is set to a non-empty string, so set it here or the
                        // bounce is undone (breaking `i` on the title).
                        if entering_insert {
                            let _ = api::set_vvar("char", " ");
                        }

                        let _ = win.set_cursor(top_len + 1, col);
                    }
                }
                false
            })
            .build();
        Ok(api::create_autocmd(["CursorMovedI", "InsertEnter"], &opts).map(|_| ())?)
    }

    /// Registers the TextChanged/TextChangedI guard that restores the title region when it is
    /// modified or deleted
    fn register_text_guard(buffer: &api::Buffer, top_lines: &[String]) -> OxiResult<()> {
        let text_buf = buffer.clone();
        let expected_top = top_lines.to_vec();
        // set_lines fires TextChanged re-entrantly; this flag makes events
        // from our own restore batch no-ops so the batch cannot double-apply.
        let restoring = Arc::new(AtomicBool::new(false));
        let opts = CreateAutocmdOpts::builder()
            .buf(text_buf.clone())
            .callback(move |_| {
                if restoring.load(Ordering::SeqCst) {
                    return false;
                }
                let mut buf = text_buf.clone();
                let top_len = expected_top.len();
                let title_len = top_len - 1;
                let mut touched = false;

                restoring.store(true, Ordering::SeqCst);

                // 1. Pad buffers shorter than the minimum line count (title lines + separator + 1
                //    input line) with blank lines until they reach the minimum.
                let len = buf.line_count().unwrap_or(0);
                if len < top_len + 1 {
                    let pad = top_len + 1 - len;
                    let cur_line = api::get_current_win()
                        .get_cursor()
                        .map(|(line, _)| line)
                        .unwrap_or(1);
                    let at = cur_line.saturating_sub(1).min(len);
                    let _ = buf.set_lines(at..at, false, vec![String::new(); pad]);
                    touched = true;
                }

                // 2. Restore the title lines when they differ.
                let title: Vec<String> = buf
                    .get_lines(0..title_len, false)
                    .map(|lines| lines.map(|l| l.to_string()).collect())
                    .unwrap_or_default();
                if title.as_slice() != &expected_top[..title_len] {
                    let _ = buf.set_lines(0..title_len, false, expected_top[..title_len].to_vec());
                    touched = true;
                }

                // 3. The separator line must be blank; otherwise (e.g. a backspace join) insert a
                //    fresh blank, pushing the joined content down.
                let sep_blank = buf
                    .get_lines(title_len..title_len + 1, false)
                    .map(|mut lines| lines.next().is_some_and(|l| l.is_empty()))
                    .unwrap_or(false);
                if !sep_blank {
                    let _ = buf.set_lines(title_len..title_len, false, vec![String::new()]);
                    touched = true;
                }

                if touched {
                    let _ = Self::set_title_highlight(&mut buf, &expected_top);
                    // Buffer edits can leave the cursor inside the title region without a
                    // CursorMovedI event (e.g. a backspace join restored above), so bounce it here
                    // too. Normal mode may rest on the title.
                    let mode = api::get_mode().mode.to_string_lossy().into_owned();
                    if mode == "i" || mode == "R" {
                        let mut win = api::get_current_win();
                        if let Ok((line, col)) = win.get_cursor()
                            && line <= expected_top.len()
                        {
                            let _ = win.set_cursor(expected_top.len() + 1, col);
                        }
                    }
                }
                restoring.store(false, Ordering::SeqCst);

                false
            })
            .build();
        Ok(api::create_autocmd(["TextChanged", "TextChangedI"], &opts).map(|_| ())?)
    }

    /// Highlights the title lines (everything above the separator) with a `TenonLineInputTitle`
    /// extmark.
    fn set_title_highlight(buffer: &mut api::Buffer, top_lines: &[String]) -> OxiResult<()> {
        let ns = api::create_namespace("TenonFreeTextTitle");
        let _ = buffer.clear_namespace(ns, 0..);
        let title_len = top_lines.len().saturating_sub(1);
        if title_len == 0 {
            return Ok(());
        }
        let end_col = top_lines[title_len - 1].chars().count();
        let opts = SetExtmarkOpts::builder()
            .hl_group("TenonLineInputTitle")
            .end_row(title_len - 1)
            .end_col(end_col)
            .build();
        let _ = buffer.set_extmark(ns, 0, 0, &opts);
        Ok(())
    }
}

impl Widget for FreeTextWidget {
    fn render(&mut self) -> OxiResult<()> {
        api::command("startinsert")?;
        Ok(())
    }

    fn buffer(&self) -> &NvimBuffer {
        &self.inner
    }

    fn set_window(&mut self, window: NvimWindow) {
        self.window = Some(window);
    }
}
