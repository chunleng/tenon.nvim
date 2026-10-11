use std::sync::{Arc, Mutex, RwLock};

use nvim_oxi::api::{self, opts::OptionOpts};

use crate::{
    chat::log::indexer::ChatLogIndexer,
    ui::{
        panels::swappable::SwappableBufferPanel,
        widget::chat_display::{ChatDisplayData, chat_metadata::MetadataValues},
    },
    utils::{GLOBAL_EXECUTION_HANDLER, format_token_count, format_token_with_delta},
};

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

pub struct SpinnerState {
    frame: usize,
}

impl SpinnerState {
    pub fn new() -> Self {
        Self { frame: 0 }
    }

    /// Get current spinner character
    pub fn get_char(&self) -> &'static str {
        SPINNER_CHARS[self.frame % SPINNER_CHARS.len()]
    }

    /// Advance to next frame
    pub fn advance(&mut self) {
        self.frame = (self.frame + 1) % SPINNER_CHARS.len();
    }
}

/// Session line parts for the input window winbar.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionLineParts {
    pub input: String,
    pub output: String,
    /// Total with delta suffix, e.g. "175 (+175)"
    pub total: String,
    /// Cache hit rate as whole number percentage of input tokens; 0 hides the
    /// cache segment
    pub cache_pct: u64,
}

impl SessionLineParts {
    pub fn from_values(values: &MetadataValues) -> Self {
        // Only total shows delta; input/output are plain counts
        Self {
            input: format_token_count(values.input_tokens),
            output: format_token_count(values.output_tokens),
            total: format_token_with_delta(values.total_tokens, values.total_tokens_delta),
            cache_pct: values
                .cached_tokens
                .checked_mul(100)
                .and_then(|v| v.checked_div(values.input_tokens))
                .unwrap_or(0),
        }
    }

    /// Plain-text session line without the gauge bars
    pub fn join(&self) -> String {
        if self.cache_pct > 0 {
            format!(
                "{} 󰕒 (󰃨 {}%), {} 󰇚, {} total",
                self.input, self.cache_pct, self.output, self.total
            )
        } else {
            format!("{} 󰕒, {} 󰇚, {} total", self.input, self.output, self.total)
        }
    }
}

/// Builds the input window winbar: spinner pinned to the left edge (while
/// processing), gauge bars and session info right-aligned via `%=`. Active
/// gauge bars and session info use the WinBar default highlight (`%*`); dim
/// and critical bars use Tenon groups referenced inline via `%#Group#`.
pub fn session_winbar_string(
    parts: &SessionLineParts,
    level: usize,
    spinner: Option<&str>,
) -> String {
    let active_hl = if level == 4 {
        "%#TenonGaugeBarCritical#"
    } else {
        "%*"
    };
    let end_col = ((level as f32 + 1.0) / 5.0 * GAUGE_BARS.len() as f32) as usize;
    let (active, dim) = GAUGE_BARS.split_at(end_col.min(GAUGE_BARS.len()));

    let mut winbar = String::new();
    if let Some(spinner) = spinner {
        winbar.push_str("%#TenonSignProcessing#");
        winbar.push_str(spinner);
        winbar.push_str("%* ");
    }
    winbar.push_str("%=");
    winbar.push_str(active_hl);
    winbar.push_str(active);
    winbar.push_str("%#TenonGaugeBar#");
    winbar.push_str(dim);
    winbar.push_str("%* ");
    winbar.push_str(&parts.join());
    winbar.push_str("%*");
    winbar
}

/// 5 identical bars always shown; the active one gets a gauge highlight.
pub const GAUGE_BARS: &str = "▮▮▮▮▮";

/// Active bar index (0-based) for the given context token count.
/// Test build: MAX = 10, HARD = 20, so bars split at 5/10/15/20.
pub fn gauge_level(context_tokens: u64) -> usize {
    let max = ChatLogIndexer::MAX_ACTIVE_CONTEXT_TOKENS as u64;
    let hard = ChatLogIndexer::HARD_LIMIT_ACTIVE_CONTEXT_TOKENS as u64;
    let mid_hard = max + (hard - max) / 2;

    if context_tokens > hard {
        4
    } else if context_tokens > mid_hard {
        3
    } else if context_tokens > max {
        2
    } else if context_tokens > max / 2 {
        1
    } else {
        0
    }
}

/// Sets the session info line as the winbar of the input window. The winbar
/// is compared against the window's current value and only written when it
/// differs, so a recreated input window (empty winbar) is re-set on the next
/// tick without extra state tracking. Skipped while no valid input window
/// exists.
fn render_session_winbar(input_window: Arc<Mutex<Option<SwappableBufferPanel>>>, winbar: String) {
    let _ = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
        let window = input_window
            .lock()
            .ok()
            .and_then(|panel| panel.as_ref().map(|p| p.window.clone()))
            .and_then(|w| w.get_window());
        let Some(window) = window else {
            return Ok(());
        };
        let win_opts = OptionOpts::builder().win(window.clone()).build();
        let current: String = api::get_option_value("winbar", &win_opts).unwrap_or_default();
        if current != winbar {
            let _ = api::set_option_value("winbar", winbar, &win_opts);
        }
        Ok(())
    });
}

pub struct MetadataSessionRenderer {
    spinner: SpinnerState,
    input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
    chat_data: Arc<RwLock<ChatDisplayData>>,
}

impl MetadataSessionRenderer {
    pub(super) fn new(
        input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
        chat_data: Arc<RwLock<ChatDisplayData>>,
    ) -> Self {
        Self {
            spinner: SpinnerState::new(),
            input_window,
            chat_data,
        }
    }

    /// Rebuilds the session winbar every tick: the spinner frame changes
    /// while processing, and the compare-before-set in render_session_winbar
    /// skips redundant writes when idle
    pub(super) fn render(&mut self) {
        let values = MetadataValues::from(self.chat_data.clone());
        let is_processing = if let Ok(data) = self.chat_data.read()
            && let Ok(session) = data.chat_session.read()
        {
            session.is_processing()
        } else {
            false
        };

        let spinner_char = is_processing.then(|| self.spinner.get_char());
        if is_processing {
            self.spinner.advance();
        }
        let parts = SessionLineParts::from_values(&values);
        let winbar =
            session_winbar_string(&parts, gauge_level(values.context_tokens), spinner_char);
        render_session_winbar(self.input_window.clone(), winbar);
    }
}

#[cfg(test)]
mod tests {
    use super::super::title::MetadataState;
    use super::*;

    fn session_parts(cache_pct: u64) -> SessionLineParts {
        SessionLineParts {
            input: "100".to_string(),
            output: "50".to_string(),
            total: "175 (+175)".to_string(),
            cache_pct,
        }
    }

    #[test]
    fn test_session_line_cache_segment_visibility() {
        let mut state = MetadataState::new();
        // Input tokens exist but no cached tokens - cache segment hidden
        let values = MetadataValues {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            ..MetadataValues::test_default()
        };

        // Populate cache
        state.should_render_title(&values);

        assert_eq!(
            SessionLineParts::from_values(&values).join(),
            "100 󰕒, 50 󰇚, 150 total"
        );

        // Cached tokens arrive - cache segment shown
        let values = MetadataValues {
            cached_tokens: 25,
            ..values
        };
        state.should_render_title(&values);

        assert_eq!(
            SessionLineParts::from_values(&values).join(),
            "100 󰕒 (󰃨 25%), 50 󰇚, 150 total"
        );
    }

    #[test]
    fn test_session_winbar_string_with_spinner_and_cache() {
        let winbar = session_winbar_string(&session_parts(25), 2, Some("⠄⠠⠠"));
        assert_eq!(
            winbar,
            "%#TenonSignProcessing#⠄⠠⠠%* %=%*▮▮▮%#TenonGaugeBar#▮▮%* 100 󰕒 (󰃨 25%), 50 󰇚, 175 (+175) total%*"
        );
    }

    #[test]
    fn test_session_winbar_string_idle_no_cache() {
        // No spinner while idle; cache segment hidden at 0%
        let winbar = session_winbar_string(&session_parts(0), 0, None);
        assert_eq!(
            winbar,
            "%=%*▮%#TenonGaugeBar#▮▮▮▮%* 100 󰕒, 50 󰇚, 175 (+175) total%*"
        );
    }

    #[test]
    fn test_session_winbar_string_critical_level() {
        // Level 4: all bars active with the critical highlight, no dim bars
        let winbar = session_winbar_string(&session_parts(0), 4, None);
        assert_eq!(
            winbar,
            "%=%#TenonGaugeBarCritical#▮▮▮▮▮%#TenonGaugeBar#%* 100 󰕒, 50 󰇚, 175 (+175) total%*"
        );
    }

    // Test build constants: MAX_ACTIVE_CONTEXT_TOKENS = 10, HARD_LIMIT_ACTIVE_CONTEXT_TOKENS = 20
    // Bars: 1: <=5, 2: 6..=10, 3: 11..=15, 4: 16..=20, 5: >20
    #[test]
    fn test_gauge_level_boundaries() {
        assert_eq!(gauge_level(0), 0);
        assert_eq!(gauge_level(5), 0);

        assert_eq!(gauge_level(6), 1);
        assert_eq!(gauge_level(10), 1);

        assert_eq!(gauge_level(11), 2);
        assert_eq!(gauge_level(15), 2);

        assert_eq!(gauge_level(16), 3);
        assert_eq!(gauge_level(20), 3);

        assert_eq!(gauge_level(21), 4);
    }
}
