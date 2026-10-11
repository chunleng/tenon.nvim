use std::sync::{Arc, RwLock};

use nvim_oxi::api::{self, opts::OptionOpts};

use crate::{
    get_application_config,
    tools::resolve_tool_names,
    ui::{
        nvim_primitives::window::NvimWindow,
        widget::chat_display::{ChatDisplayData, chat_metadata::MetadataValues},
    },
    utils::GLOBAL_EXECUTION_HANDLER,
};

/// Title line split into segments with truncation priority. When the winbar
/// is too narrow, segments drop right-to-left (meta, agent, queue) so the
/// title and chat index survive longest.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleSegments {
    /// Icon + chat title
    pub head: String,
    /// " {n} of {m}"
    pub index: String,
    /// ", queue: N" (empty when queue is empty)
    pub queue: String,
    /// ", agent: {name}"
    pub agent: String,
    /// Model/tool diff suffix (empty when no diff)
    pub meta: String,
}

#[derive(Clone)]
pub struct AgentDefaultsCache {
    pub default_model_display: String,
    pub resolved_tool_names: Vec<String>,
}

pub struct MetadataState {
    previous_values: Option<MetadataValues>,
    agent_defaults_cache: Option<AgentDefaultsCache>,
}

impl MetadataState {
    pub fn new() -> Self {
        Self {
            previous_values: None,
            agent_defaults_cache: None,
        }
    }

    /// Returns true if any metadata value changed. Gates the title winbar
    /// only; the session winbar is rebuilt every tick.
    pub fn should_render_title(&mut self, current: &MetadataValues) -> bool {
        let changed = match &self.previous_values {
            None => true,
            Some(prev) => {
                prev.title != current.title
                    || prev.chat_index != current.chat_index
                    || prev.total_count != current.total_count
                    || prev.agent_name != current.agent_name
                    || prev.model_display != current.model_display
                    || prev.current_tool_names != current.current_tool_names
                    || prev.input_tokens != current.input_tokens
                    || prev.output_tokens != current.output_tokens
                    || prev.cached_tokens != current.cached_tokens
                    || prev.total_tokens != current.total_tokens
                    || prev.context_tokens != current.context_tokens
                    || prev.work_queue_count != current.work_queue_count
                    || prev.input_tokens_delta != current.input_tokens_delta
                    || prev.output_tokens_delta != current.output_tokens_delta
                    || prev.cached_tokens_delta != current.cached_tokens_delta
                    || prev.total_tokens_delta != current.total_tokens_delta
            }
        };

        if changed {
            // Update cache if agent_name changed
            let agent_changed = match &self.previous_values {
                None => true,
                Some(prev) => prev.agent_name != current.agent_name,
            };

            if agent_changed {
                let config = get_application_config();
                self.agent_defaults_cache = config.agents.get(&current.agent_name).map(|a| {
                    let resolved_tool_names = resolve_tool_names(&a.tool_names);
                    AgentDefaultsCache {
                        default_model_display: a.model.display_name(),
                        resolved_tool_names,
                    }
                });
            }

            self.previous_values = Some(current.clone());
        }

        changed
    }

    /// Title line segments for the chat window winbar
    pub fn title_segments(&self, values: &MetadataValues) -> TitleSegments {
        // Use cached default model
        let default_model_display = self
            .agent_defaults_cache
            .as_ref()
            .map(|c| &c.default_model_display);

        // Check if model changed from default
        let model_changed = default_model_display
            .map(|d| d != &values.model_display)
            .unwrap_or(false);

        // Compute tool diff
        let current_resolved = resolve_tool_names(&values.current_tool_names);
        let default_resolved = self
            .agent_defaults_cache
            .as_ref()
            .map(|c| &c.resolved_tool_names);

        let (added, removed) = match default_resolved {
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

        let meta = match (model_changed, tool_suffix.is_empty()) {
            (true, true) => format!(" (󰚩 {})", values.model_display),
            (true, false) => format!(" (󰚩 {} | {})", values.model_display, tool_suffix),
            (false, true) => String::new(),
            (false, false) => format!(" ({})", tool_suffix),
        };

        let queue = if values.work_queue_count > 0 {
            format!(", queue: {}", values.work_queue_count)
        } else {
            String::new()
        };

        TitleSegments {
            head: values.title.clone().unwrap_or_default(),
            index: format!(" {} of {}", values.chat_index + 1, values.total_count),
            queue,
            agent: format!(", agent: {}", values.agent_name),
            meta,
        }
    }
}

/// Sets the title line as the winbar of the attached window, truncated to the
/// window width. The title is user-controlled, so `%` is escaped to keep it
/// from being parsed as a winbar format item. The title uses the WinBar
/// default highlight.
fn render_title_winbar(window: Arc<NvimWindow>, segments: TitleSegments) {
    let _ = GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread(move || {
        if let Some(window) = window.get_window() {
            // Reserve one column for the trailing space in the winbar format
            let max_width = window
                .get_width()
                .map(|w| w.saturating_sub(1) as usize)
                .unwrap_or(0);
            let title = truncate_title_segments(&segments, max_width);
            let winbar = format!("%={} %*", title.replace('%', "%%"));
            let win_opts = OptionOpts::builder().win(window.clone()).build();
            let _ = api::set_option_value("winbar", winbar, &win_opts);
        }
        Ok(())
    });
}

/// Owns the title rendering state and output. Created inside the renderer
/// thread; renders the title winbar whenever the metadata values changed.
pub struct MetadataTitleRenderer {
    state: MetadataState,
    window: Arc<NvimWindow>,
    chat_data: Arc<RwLock<ChatDisplayData>>,
}

impl MetadataTitleRenderer {
    pub(super) fn new(window: Arc<NvimWindow>, chat_data: Arc<RwLock<ChatDisplayData>>) -> Self {
        Self {
            state: MetadataState::new(),
            window,
            chat_data,
        }
    }

    /// Renders the title winbar if values changed
    pub(super) fn render(&mut self) {
        let values = MetadataValues::from(self.chat_data.clone());
        if self.state.should_render_title(&values) {
            let segments = self.state.title_segments(&values);
            render_title_winbar(self.window.clone(), segments);
        }
    }
}

/// Builds the winbar title within `max_width` display columns. Segments drop
/// right-to-left (meta, agent, queue) so the title and chat index survive
/// longest; if even head + index overflow, the title is truncated with an
/// ellipsis while the index suffix is kept.
pub fn truncate_title_segments(segments: &TitleSegments, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;

    let mut kept = vec![segments.head.as_str(), segments.index.as_str()];
    for segment in [&segments.queue, &segments.agent, &segments.meta] {
        if segment.is_empty() {
            continue;
        }
        let candidate = kept
            .iter()
            .copied()
            .chain([segment.as_str()])
            .collect::<String>();
        if candidate.width() <= max_width {
            kept.push(segment.as_str());
        } else {
            break;
        }
    }

    let result: String = kept.concat();
    if result.width() <= max_width {
        return result;
    }

    // Head + index alone overflow: truncate the head, keep the index
    let index_width = segments.index.width();
    if index_width >= max_width {
        return truncate_with_ellipsis(&segments.index, max_width);
    }
    let head = truncate_with_ellipsis(&segments.head, max_width - index_width);
    format!("{head}{}", segments.index)
}

/// Truncates `text` to at most `max_width` display columns, appending an
/// ellipsis when truncation occurred.
fn truncate_with_ellipsis(text: &str, max_width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

    if text.width() <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut width = 0;
    for ch in text.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if width + ch_width > max_width - 1 {
            break;
        }
        result.push(ch);
        width += ch_width;
    }
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::super::session::SessionLineParts;
    use super::*;
    use unicode_width::UnicodeWidthStr;

    impl TitleSegments {
        fn join(&self) -> String {
            format!(
                "{}{}{}{}{}",
                self.head, self.index, self.queue, self.agent, self.meta
            )
        }
    }

    #[test]
    fn test_metadata_should_render_on_first_call() {
        let mut state = MetadataState::new();
        let values = MetadataValues::test_default();
        assert!(state.should_render_title(&values));
    }

    #[test]
    fn test_metadata_should_render_when_title_changes() {
        let mut state = MetadataState::new();
        let values1 = MetadataValues::test_default();
        assert!(state.should_render_title(&values1));

        let values2 = MetadataValues {
            title: Some("New Title".to_string()),
            ..values1.clone()
        };
        assert!(state.should_render_title(&values2));
    }

    #[test]
    fn test_metadata_should_render_when_token_usage_changes() {
        let mut state = MetadataState::new();
        let values1 = MetadataValues {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            context_tokens: 200,
            ..MetadataValues::test_default()
        };
        assert!(state.should_render_title(&values1));

        let values2 = MetadataValues {
            input_tokens: 150,
            total_tokens: 200,
            ..values1.clone()
        };
        assert!(state.should_render_title(&values2));
    }

    #[test]
    fn test_metadata_should_render_when_work_queue_count_changes() {
        let mut state = MetadataState::new();
        let values1 = MetadataValues::test_default();
        assert!(state.should_render_title(&values1));

        let values2 = MetadataValues {
            work_queue_count: 2,
            ..values1.clone()
        };
        assert!(state.should_render_title(&values2));
    }

    #[test]
    fn test_metadata_should_not_render_when_unchanged() {
        let mut state = MetadataState::new();
        let values = MetadataValues::test_default();
        assert!(state.should_render_title(&values));
        assert!(!state.should_render_title(&values));
        assert!(!state.should_render_title(&values));
    }

    #[test]
    fn test_title_segments_model_and_tool_diff() {
        let mut state = MetadataState::new();
        let values = MetadataValues {
            title: Some("Test Chat".to_string()),
            chat_index: 2,
            total_count: 5,
            agent_name: "default".to_string(),
            model_display: "anthropic: claude-3-5-sonnet".to_string(),
            // 2 tools, both in default, so 7 removed from default's 9 tools
            current_tool_names: vec!["read_file".to_string(), "edit_file".to_string()],
            input_tokens: 100,
            output_tokens: 50,
            cached_tokens: 25,
            total_tokens: 175,
            context_tokens: 200,
            work_queue_count: 0,
            input_tokens_delta: 100,
            output_tokens_delta: 50,
            cached_tokens_delta: 25,
            total_tokens_delta: 175,
        };

        // Populate cache
        state.should_render_title(&values);

        // Show model diff and tool diff (model changed, 7 tools removed)
        assert_eq!(
            state.title_segments(&values).join(),
            "Test Chat 3 of 5, agent: default (󰚩 anthropic: claude-3-5-sonnet | 󰣖 -7)"
        );
        assert_eq!(
            SessionLineParts::from_values(&values).join(),
            "100 󰕒 (󰃨 25%), 50 󰇚, 175 (+175) total"
        );
    }

    #[test]
    fn test_title_segments_empty_title_tool_diff() {
        let mut state = MetadataState::new();
        let values = MetadataValues {
            title: None,
            chat_index: 0,
            total_count: 1,
            agent_name: "default".to_string(),
            model_display: "ollama_cloud: glm-5.1".to_string(),
            // 1 tool, 8 removed from default's 9 tools
            current_tool_names: vec!["read_file".to_string()],
            ..MetadataValues::test_default()
        };

        // Populate cache
        state.should_render_title(&values);

        // Show tool diff (model matches, 8 tools removed)
        assert_eq!(
            state.title_segments(&values).join(),
            " 1 of 1, agent: default (󰣖 -8)"
        );
        assert_eq!(
            SessionLineParts::from_values(&values).join(),
            "0 󰕒, 0 󰇚, 0 total"
        );
    }

    #[test]
    fn test_title_segments_no_diff() {
        let mut state = MetadataState::new();
        // Use all default tools - no diff
        let values = MetadataValues {
            title: Some("Test Chat".to_string()),
            chat_index: 0,
            total_count: 1,
            agent_name: "default".to_string(),
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

        // Populate cache
        state.should_render_title(&values);

        // No diff shown - delta omitted when 0
        assert_eq!(
            state.title_segments(&values).join(),
            "Test Chat 1 of 1, agent: default"
        );
        assert_eq!(
            SessionLineParts::from_values(&values).join(),
            "0 󰕒, 0 󰇚, 0 total"
        );
    }

    #[test]
    fn test_title_segments_with_queue() {
        let mut state = MetadataState::new();
        // Default model and tools - only the queue segment differs
        let values = MetadataValues {
            title: Some("Test Chat".to_string()),
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
            work_queue_count: 2,
            ..MetadataValues::test_default()
        };

        // Populate cache
        state.should_render_title(&values);

        assert_eq!(
            state.title_segments(&values).join(),
            "Test Chat 1 of 1, queue: 2, agent: default"
        );
    }

    #[test]
    fn test_title_segments_added_tools() {
        let mut state = MetadataState::new();
        // Add extra tools not in default
        let values = MetadataValues {
            title: None,
            chat_index: 0,
            total_count: 1,
            agent_name: "default".to_string(),
            model_display: "ollama_cloud: glm-5.1".to_string(),
            current_tool_names: vec![
                "read_file".to_string(),
                "edit_file".to_string(),
                "mcp_server____custom_tool".to_string(), // Extra tool
            ],
            ..MetadataValues::test_default()
        };

        // Populate cache
        state.should_render_title(&values);

        // 1 added (mcp_server____custom_tool), 7 removed (default's 9 minus read_file, edit_file)
        assert_eq!(
            state.title_segments(&values).join(),
            " 1 of 1, agent: default (󰣖 +1/-7)"
        );
    }

    #[test]
    fn test_truncate_title_segments_fits_unchanged() {
        let segments = TitleSegments {
            head: "Test".to_string(),
            index: " 1 of 2".to_string(),
            queue: ", queue: 3".to_string(),
            agent: ", agent: default".to_string(),
            meta: String::new(),
        };
        let full = segments.join();
        assert_eq!(truncate_title_segments(&segments, 200), full);
    }

    #[test]
    fn test_truncate_title_segments_drops_right_to_left() {
        let segments = TitleSegments {
            head: "Test".to_string(),
            index: " 1 of 2".to_string(),
            queue: ", queue: 3".to_string(),
            agent: ", agent: default".to_string(),
            meta: " (󰚩 model)".to_string(),
        };
        let full = segments.join();
        let full_width = full.width();

        // One column short: meta dropped first
        assert_eq!(
            truncate_title_segments(&segments, full_width - 1),
            "Test 1 of 2, queue: 3, agent: default"
        );

        // Agent no longer fits, queue (higher priority) still does
        assert_eq!(
            truncate_title_segments(&segments, "Test 1 of 2, agent: default".width()),
            "Test 1 of 2, queue: 3"
        );

        // Agent no longer fits
        assert_eq!(
            truncate_title_segments(&segments, "Test 1 of 2".width()),
            "Test 1 of 2"
        );
    }

    #[test]
    fn test_truncate_title_segments_truncates_head_keeps_index() {
        let segments = TitleSegments {
            head: "A very long chat title".to_string(),
            index: " 3 of 9".to_string(),
            queue: String::new(),
            agent: String::new(),
            meta: String::new(),
        };
        let index_width = segments.index.width();

        let truncated = truncate_title_segments(&segments, 10);
        assert!(truncated.ends_with(" 3 of 9"));
        assert!(truncated.contains('…'));
        assert_eq!(truncated.width(), 10);
        assert_eq!(index_width, 7);
    }
}
