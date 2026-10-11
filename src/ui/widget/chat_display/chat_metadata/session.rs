use std::sync::{Arc, Mutex, RwLock};

use nvim_oxi::api::{self, opts::OptionOpts};
use unicode_width::UnicodeWidthStr;

use crate::{
    chat::log::indexer::ChatLogIndexer,
    get_application_config,
    tools::resolve_tool_names,
    ui::{
        panels::swappable::SwappableBufferPanel,
        widget::chat_display::{ChatDisplayData, chat_metadata::MetadataValues},
    },
    utils::{GLOBAL_EXECUTION_HANDLER, format_token_count, format_token_with_delta},
};

/// Session line parts for the input window winbar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionLineParts {
    pub agent_name: String,
    /// Model display name when it differs from the agent default; empty
    /// otherwise
    pub model_display: String,
    /// Tool diff vs the agent default, e.g. "󰣖 +1/-7"; empty when no diff
    pub tool_suffix: String,
    pub input: String,
    pub output: String,
    /// Total with delta suffix, e.g. "175 (+175)"
    pub total: String,
    /// Cache hit rate as whole number percentage of input tokens; 0 hides the
    /// cache segment
    pub cache_pct: u64,
}

#[derive(Clone)]
pub struct AgentDefaultsCache {
    pub default_model_display: String,
    pub resolved_tool_names: Vec<String>,
}

impl SessionLineParts {
    pub fn from_values(values: &MetadataValues, defaults: Option<&AgentDefaultsCache>) -> Self {
        let model_display =
            if defaults.is_some_and(|c| c.default_model_display != values.model_display) {
                values.model_display.clone()
            } else {
                String::new()
            };

        let current_resolved = resolve_tool_names(&values.current_tool_names);
        let (added, removed) = match defaults.map(|c| &c.resolved_tool_names) {
            Some(default_tools) => {
                let added = current_resolved
                    .iter()
                    .filter(|t| !default_tools.contains(t))
                    .count();
                let removed = default_tools
                    .iter()
                    .filter(|t| !current_resolved.contains(t))
                    .count();
                (added, removed)
            }
            None => (0, 0),
        };

        let tool_suffix = match (added, removed) {
            (0, 0) => String::new(),
            (a, 0) => format!("󰣖 +{}", a),
            (0, r) => format!("󰣖 -{}", r),
            (a, r) => format!("󰣖 +{}/-{}", a, r),
        };

        // Only total shows delta; input/output are plain counts
        Self {
            agent_name: values.agent_name.clone(),
            model_display,
            tool_suffix,
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

    /// Left-aligned agent segment: "agent: {name}" plus the model/tool diff
    /// meta. `include_model` drops the model display, used when the winbar
    /// is too narrow.
    pub fn agent_segment(&self, include_model: bool) -> String {
        let model = if include_model {
            self.model_display.as_str()
        } else {
            ""
        };
        match (!model.is_empty(), !self.tool_suffix.is_empty()) {
            (true, true) => format!(
                "agent: {} (󰚩 {} | {})",
                self.agent_name, model, self.tool_suffix
            ),
            (true, false) => format!("agent: {} (󰚩 {})", self.agent_name, model),
            // Model dropped by truncation: robot sign and separator kept
            (false, true) if !self.model_display.is_empty() => {
                format!("agent: {} (󰚩 | {})", self.agent_name, self.tool_suffix)
            }
            // No model change: tool diff only, no robot sign
            (false, true) => format!("agent: {} ({})", self.agent_name, self.tool_suffix),
            // Model dropped by truncation: robot sign kept
            (false, false) if !self.model_display.is_empty() => {
                format!("agent: {} (󰚩 )", self.agent_name)
            }
            (false, false) => format!("agent: {}", self.agent_name),
        }
    }

    /// Plain-text session info without the agent segment or gauge bars
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

/// Owns the session line state: change detection and the agent defaults
/// cache used for the model/tool diff meta.
pub struct MetadataSessionState {
    previous_values: Option<MetadataValues>,
    agent_defaults_cache: Option<AgentDefaultsCache>,
    parts: SessionLineParts,
}

impl MetadataSessionState {
    pub fn new() -> Self {
        Self {
            previous_values: None,
            agent_defaults_cache: None,
            parts: SessionLineParts::default(),
        }
    }

    /// Session line parts, recomputed only when a value they depend on
    /// changed. Refreshes the agent defaults cache when the active agent
    /// changed.
    pub fn session_line_parts(&mut self, current: &MetadataValues) -> SessionLineParts {
        let changed = match &self.previous_values {
            None => true,
            Some(prev) => {
                prev.agent_name != current.agent_name
                    || prev.model_display != current.model_display
                    || prev.current_tool_names != current.current_tool_names
                    || prev.input_tokens != current.input_tokens
                    || prev.output_tokens != current.output_tokens
                    || prev.cached_tokens != current.cached_tokens
                    || prev.total_tokens != current.total_tokens
                    || prev.total_tokens_delta != current.total_tokens_delta
            }
        };

        if changed {
            let agent_changed = match &self.previous_values {
                None => true,
                Some(prev) => prev.agent_name != current.agent_name,
            };

            if agent_changed {
                let config = get_application_config();
                self.agent_defaults_cache =
                    config
                        .agents
                        .get(&current.agent_name)
                        .map(|a| AgentDefaultsCache {
                            default_model_display: a.model.display_name(),
                            resolved_tool_names: resolve_tool_names(&a.tool_names),
                        });
            }

            self.previous_values = Some(current.clone());
            self.parts = SessionLineParts::from_values(current, self.agent_defaults_cache.as_ref());
        }

        self.parts.clone()
    }
}

/// Builds the input window winbar: the agent segment on the left, gauge bars
/// and session info right-aligned via `%=`. When the full line exceeds
/// `max_width`, the model display drops from the agent segment first; if it
/// is still too narrow, the token count drops and the gauge bars stay.
/// Active gauge bars and session info use the WinBar default highlight
/// (`%*`); dim and critical bars use Tenon groups referenced inline via
/// `%#Group#`.
pub fn session_winbar_string(parts: &SessionLineParts, level: usize, max_width: usize) -> String {
    let active_hl = if level == 4 {
        "%#TenonGaugeBarCritical#"
    } else {
        "%*"
    };
    let end_col = ((level as f32 + 1.0) / 5.0 * GAUGE_BARS.len() as f32) as usize;
    let (active, dim) = GAUGE_BARS.split_at(end_col.min(GAUGE_BARS.len()));

    let session_info = parts.join();
    let gauge_width = GAUGE_BARS.width() + 1;
    let include_model =
        parts.agent_segment(true).width() + gauge_width + session_info.width() <= max_width;
    // Still too narrow without the model: drop the token count, keep the
    // gauge bars
    let include_session_info = include_model
        || parts.agent_segment(false).width() + gauge_width + session_info.width() <= max_width;

    let mut winbar = parts.agent_segment(include_model).replace('%', "%%");
    winbar.push_str("%=");
    winbar.push_str(active_hl);
    winbar.push_str(active);
    winbar.push_str("%#TenonGaugeBar#");
    winbar.push_str(dim);
    winbar.push_str("%*");
    if include_session_info {
        winbar.push(' ');
        winbar.push_str(&session_info);
    }
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

/// Sets the session line as the winbar of the input window, truncated to the
/// window width. The winbar is compared against the window's current value
/// and only written when it differs, so a recreated input window (empty
/// winbar) is re-set on the next tick without extra state tracking. Skipped
/// while no valid input window exists.
fn render_session_winbar(
    input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
    parts: SessionLineParts,
    level: usize,
) {
    let _ = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
        let window = input_window
            .lock()
            .ok()
            .and_then(|panel| panel.as_ref().map(|p| p.window.clone()))
            .and_then(|w| w.get_window());
        let Some(window) = window else {
            return Ok(());
        };
        let max_width = window.get_width().map(|w| w as usize).unwrap_or(0);
        let winbar = session_winbar_string(&parts, level, max_width);
        let win_opts = OptionOpts::builder().win(window.clone()).build();
        let current: String = api::get_option_value("winbar", &win_opts).unwrap_or_default();
        if current != winbar {
            let _ = api::set_option_value("winbar", winbar, &win_opts);
        }
        Ok(())
    });
}

pub struct MetadataSessionRenderer {
    state: MetadataSessionState,
    input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
    chat_data: Arc<RwLock<ChatDisplayData>>,
}

impl MetadataSessionRenderer {
    pub(super) fn new(
        input_window: Arc<Mutex<Option<SwappableBufferPanel>>>,
        chat_data: Arc<RwLock<ChatDisplayData>>,
    ) -> Self {
        Self {
            state: MetadataSessionState::new(),
            input_window,
            chat_data,
        }
    }

    /// Rebuilds the session winbar every tick from the cached parts; the
    /// parts are recomputed only when their values changed, and the
    /// compare-before-set in render_session_winbar skips redundant writes
    /// when unchanged
    pub(super) fn render(&mut self) {
        let values = MetadataValues::from(self.chat_data.clone());
        let parts = self.state.session_line_parts(&values);
        let level = gauge_level(values.context_tokens);
        render_session_winbar(self.input_window.clone(), parts, level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_parts(cache_pct: u64) -> SessionLineParts {
        SessionLineParts {
            agent_name: "default".to_string(),
            model_display: String::new(),
            tool_suffix: String::new(),
            input: "100".to_string(),
            output: "50".to_string(),
            total: "175 (+175)".to_string(),
            cache_pct,
        }
    }

    #[test]
    fn test_session_line_cache_segment_visibility() {
        let values = MetadataValues {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            ..MetadataValues::test_default()
        };

        // Input tokens exist but no cached tokens - cache segment hidden
        assert_eq!(
            SessionLineParts::from_values(&values, None).join(),
            "100 󰕒, 50 󰇚, 150 total"
        );

        // Cached tokens arrive - cache segment shown
        let values = MetadataValues {
            cached_tokens: 25,
            ..values
        };

        assert_eq!(
            SessionLineParts::from_values(&values, None).join(),
            "100 󰕒 (󰃨 25%), 50 󰇚, 150 total"
        );
    }

    #[test]
    fn test_session_line_agent_model_and_tool_diff() {
        let mut state = MetadataSessionState::new();
        // Model changed from default, 7 tools removed from default's 9 tools
        let values = MetadataValues {
            current_tool_names: vec!["read_file".to_string(), "edit_file".to_string()],
            input_tokens: 100,
            output_tokens: 50,
            cached_tokens: 25,
            total_tokens: 175,
            total_tokens_delta: 175,
            ..MetadataValues::test_default()
        };

        let parts = state.session_line_parts(&values);

        assert_eq!(
            parts.agent_segment(true),
            "agent: default (󰚩 claude-3-5-sonnet | 󰣖 -7)"
        );
        // Model dropped: robot sign and separator kept with the tool diff
        assert_eq!(parts.agent_segment(false), "agent: default (󰚩 | 󰣖 -7)");
        assert_eq!(parts.join(), "100 󰕒 (󰃨 25%), 50 󰇚, 175 (+175) total");
    }

    #[test]
    fn test_session_line_agent_tool_diff_only() {
        let mut state = MetadataSessionState::new();
        // Model matches default, 8 tools removed from default's 9 tools
        let values = MetadataValues {
            model_display: "ollama_cloud: glm-5.1".to_string(),
            current_tool_names: vec!["read_file".to_string()],
            ..MetadataValues::test_default()
        };

        let parts = state.session_line_parts(&values);

        // No model change: tool diff only, no robot sign
        assert_eq!(parts.agent_segment(true), "agent: default (󰣖 -8)");
        assert_eq!(parts.join(), "0 󰕒, 0 󰇚, 0 total");
    }

    #[test]
    fn test_session_line_agent_no_diff() {
        let mut state = MetadataSessionState::new();
        // Default model and all default tools - no meta
        let values = MetadataValues {
            model_display: "ollama_cloud: glm-5.1".to_string(),
            current_tool_names: vec![
                "edit_file".to_string(),
                "fetch_webpage".to_string(),
                "list_files".to_string(),
                "read_file".to_string(),
                "remove_path".to_string(),
                "run_command".to_string(),
                "search_text".to_string(),
                "web_search".to_string(),
                "record_thought".to_string(),
            ],
            ..MetadataValues::test_default()
        };

        let parts = state.session_line_parts(&values);

        assert_eq!(parts.agent_segment(true), "agent: default");
    }

    #[test]
    fn test_session_line_agent_added_tools() {
        let mut state = MetadataSessionState::new();
        // 1 added (mcp_server____custom_tool), 7 removed (default's 9 minus
        // read_file, edit_file)
        let values = MetadataValues {
            model_display: "ollama_cloud: glm-5.1".to_string(),
            current_tool_names: vec![
                "read_file".to_string(),
                "edit_file".to_string(),
                "mcp_server____custom_tool".to_string(),
            ],
            ..MetadataValues::test_default()
        };

        let parts = state.session_line_parts(&values);

        // No model change: tool diff only, no robot sign
        assert_eq!(parts.agent_segment(true), "agent: default (󰣖 +1/-7)");
    }

    #[test]
    fn test_session_winbar_string_with_cache() {
        let winbar = session_winbar_string(&session_parts(25), 2, 200);
        assert_eq!(
            winbar,
            "agent: default%=%*▮▮▮%#TenonGaugeBar#▮▮%* 100 󰕒 (󰃨 25%), 50 󰇚, 175 (+175) total%*"
        );
    }

    #[test]
    fn test_session_winbar_string_no_cache() {
        // Cache segment hidden at 0%
        let winbar = session_winbar_string(&session_parts(0), 0, 200);
        assert_eq!(
            winbar,
            "agent: default%=%*▮%#TenonGaugeBar#▮▮▮▮%* 100 󰕒, 50 󰇚, 175 (+175) total%*"
        );
    }

    #[test]
    fn test_session_winbar_string_critical_level() {
        // Level 4: all bars active with the critical highlight, no dim bars
        let winbar = session_winbar_string(&session_parts(0), 4, 200);
        assert_eq!(
            winbar,
            "agent: default%=%#TenonGaugeBarCritical#▮▮▮▮▮%#TenonGaugeBar#%* 100 󰕒, 50 󰇚, 175 (+175) total%*"
        );
    }

    #[test]
    fn test_session_line_agent_model_diff_only_truncated() {
        let mut parts = session_parts(0);
        parts.model_display = "claude-3-5-sonnet".to_string();

        // Model-only change, truncated: robot sign kept, no separator
        assert_eq!(parts.agent_segment(false), "agent: default (󰚩 )");
    }

    #[test]
    fn test_session_winbar_drops_model_when_too_long() {
        let mut parts = session_parts(0);
        parts.model_display = "claude-3-5-sonnet".to_string();
        parts.tool_suffix = "󰣖 -7".to_string();

        // Fits: full agent segment kept
        let winbar = session_winbar_string(&parts, 0, 200);
        assert!(winbar.starts_with("agent: default (󰚩 claude-3-5-sonnet | 󰣖 -7)%="));

        // Exactly fits without the model: model display dropped
        let right_width = GAUGE_BARS.width() + 1 + parts.join().width();
        let max_width = parts.agent_segment(false).width() + right_width;
        let winbar = session_winbar_string(&parts, 0, max_width);
        assert!(winbar.starts_with("agent: default (󰚩 | 󰣖 -7)%="));
        assert!(winbar.ends_with("100 󰕒, 50 󰇚, 175 (+175) total%*"));

        // One column short even without the model: token count dropped,
        // gauge bars kept
        let max_width = parts.agent_segment(false).width() + GAUGE_BARS.width();
        let winbar = session_winbar_string(&parts, 0, max_width);
        assert_eq!(
            winbar,
            "agent: default (󰚩 | 󰣖 -7)%=%*▮%#TenonGaugeBar#▮▮▮▮%*%*"
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
