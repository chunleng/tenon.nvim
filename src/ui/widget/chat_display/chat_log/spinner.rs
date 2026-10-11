use std::sync::{Arc, RwLock};

use nvim_oxi::api::opts::SetExtmarkOpts;

use super::ChatLogCache;
use crate::{ui::nvim_primitives::buffer::NvimBuffer, utils::GLOBAL_EXECUTION_HANDLER};

/// Braille spinner frames; blank frames at both ends pause between cycles.
/// Leading spaces position each frame within the animation area.
const SPINNER_CHARS: [&str; 43] = [
    "",
    "",
    "",
    "⠄",
    "⠠",
    " ⠄",
    " ⠠",
    "⠄ ⠄",
    "⠠ ⠠",
    " ⠄ ⠄",
    "⠄⠠ ⠠",
    "⠠ ⠄ ⠄",
    "⠄⠄⠠ ⠠",
    "⠠⠠ ⠄⠠",
    " ⠄⠄⠠⠠",
    " ⠠⠠ ⠤",
    "  ⠄⠄⠤",
    "  ⠠⠠⠤",
    "   ⠤⠤",
    "   ⠤⠤",
    "   ⠤⠤",
    "   ⠤⠤",
    "   ⠤⠄⠄",
    "   ⠤⠄⠠",
    "   ⠤⠄ ⠄",
    "   ⠤⠄ ⠠",
    "   ⠤⠠  ⠄",
    "   ⠤ ⠄ ⠠",
    "   ⠤ ⠠",
    "   ⠄⠄ ⠄",
    "   ⠄⠠ ⠠",
    "   ⠠ ⠄ ⠄",
    "    ⠄⠠ ⠠",
    "    ⠠ ⠄",
    "     ⠄⠠",
    "     ⠠ ⠄",
    "      ⠄⠠",
    "      ⠠",
    "       ⠄",
    "       ⠠",
    "",
    "",
    "",
];

struct SpinnerState {
    frame: usize,
}

impl SpinnerState {
    fn new() -> Self {
        Self { frame: 0 }
    }

    /// Get current spinner character
    fn get_char(&self) -> &'static str {
        SPINNER_CHARS[self.frame % SPINNER_CHARS.len()]
    }

    /// Advance to next frame
    fn advance(&mut self) {
        self.frame = (self.frame + 1) % SPINNER_CHARS.len();
    }

    /// Reset to the first frame
    fn reset(&mut self) {
        self.frame = 0;
    }
}

/// Renders the processing spinner as a virt_text extmark on the last line of
/// the chat log buffer. Owns the animation frame and the extmark id.
pub struct SpinnerRenderer {
    state: SpinnerState,
    chat_log_cache: Arc<RwLock<ChatLogCache>>,
    buffer: Arc<NvimBuffer>,
    // Extmark id of the spinner, allocated on first placement. Re-set
    // with the same id every tick, so a mark wiped by update_buffer's
    // range-based clear_namespace self-heals on the next tick.
    mark_id: Option<u32>,
    // Ticks between frame advances; higher = slower animation
    frame_interval: usize,
    ticks_since_advance: usize,
}

impl SpinnerRenderer {
    pub fn new(
        chat_log_cache: Arc<RwLock<ChatLogCache>>,
        buffer: Arc<NvimBuffer>,
        frame_interval: usize,
    ) -> Self {
        Self {
            state: SpinnerState::new(),
            chat_log_cache,
            buffer,
            mark_id: None,
            frame_interval,
            ticks_since_advance: 0,
        }
    }

    /// Place/update the spinner extmark on the last buffer line while
    /// processing, remove it once idle. Called every render tick; dispatches
    /// to the main thread.
    pub fn tick(&mut self, ns: u32) {
        let is_processing = self
            .chat_log_cache
            .read()
            .map(|chat_log| {
                chat_log
                    .chat_session
                    .read()
                    .map(|session| session.is_processing())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if is_processing {
            // The extmark is still re-set every tick (self-heal), but the
            // frame only advances every `frame_interval` ticks
            self.ticks_since_advance += 1;
            if self.ticks_since_advance >= self.frame_interval {
                self.ticks_since_advance = 0;
                self.state.advance();
            }
            let spinner_char = self.state.get_char().to_string();
            let existing_mark = self.mark_id;
            let buffer = self.buffer.clone();
            let mark = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
                let Some(mut buffer) = buffer.get_buffer() else {
                    return Ok(None);
                };
                let last_line = buffer.line_count().unwrap_or(1).saturating_sub(1);
                let mut builder = SetExtmarkOpts::builder();
                // virt_text_win_col pins to the first text column: with
                // 'list' on, the listchars eol char occupies the first
                // cell of the empty last line and eol virt_text would
                // otherwise render one cell past it
                let mut opts = builder
                    .virt_text([(spinner_char, "TenonSignProcessing")])
                    .virt_text_win_col(0);
                if let Some(id) = existing_mark {
                    opts = opts.id(id);
                }
                let id = buffer.set_extmark(ns, last_line, 0, &opts.build())?;
                Ok(Some(id))
            });
            self.mark_id = mark.ok().flatten().or(existing_mark);
        } else if let Some(mark_id) = self.mark_id.take() {
            // Restart the animation from the first frame next time
            self.state.reset();
            self.ticks_since_advance = 0;
            let buffer = self.buffer.clone();
            let _ = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
                if let Some(mut buffer) = buffer.get_buffer() {
                    let _ = buffer.del_extmark(ns, mark_id);
                }
                Ok(())
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spinner_state_cycles_through_all_frames() {
        let mut spinner = SpinnerState::new();
        let total = SPINNER_CHARS.len();

        // First frames are blank (pause between cycles)
        assert_eq!(spinner.get_char(), "");

        // Walk every frame exactly once; must return to the start frame
        for i in 0..total {
            assert_eq!(spinner.get_char(), SPINNER_CHARS[i]);
            spinner.advance();
        }
        assert_eq!(spinner.get_char(), SPINNER_CHARS[0]);
    }
}
