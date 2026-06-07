use std::sync::{Arc, RwLock};

use crate::chat::chat_session_count;
use crate::get_application_config;
use crate::ui::widget::chat_display::ChatDisplayData;
use crate::utils::format_token_with_delta;

#[cfg(not(test))]
use crate::tools::resolve_tool_names;

// Test mock: returns tool names as-is without resolving MCP tools.
// The real implementation calls McpHubCaller::from_mcp_tools() which requires
// a Neovim context (GLOBAL_EXECUTION_HANDLER), causing panics in unit tests.
#[cfg(test)]
fn resolve_tool_names(names: &[impl AsRef<str>]) -> Vec<String> {
    names.iter().map(|x| x.as_ref().to_string()).collect()
}

#[derive(Clone)]
pub struct FooterValues {
    pub title: Option<String>,
    pub chat_index: usize,
    pub total_count: usize,
    pub agent_name: String,
    pub model_display: String,
    pub current_tool_names: Vec<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    pub total_tokens: u64,
    pub context_tokens: u64,
    // Delta values for last exchange
    pub input_tokens_delta: u64,
    pub output_tokens_delta: u64,
    pub cached_tokens_delta: u64,
    pub total_tokens_delta: u64,
}

impl From<Arc<RwLock<ChatDisplayData>>> for FooterValues {
    fn from(data: Arc<RwLock<ChatDisplayData>>) -> Self {
        if let Ok(data) = data.read()
            && let Ok(session) = data.chat_session.read()
        {
            let title = session.title.read().ok().and_then(|t| t.clone());
            let chat_index = data.chat_index;
            let total_count = chat_session_count();
            let agent_name = session.active_agent.name.clone();
            let model_display = session.active_agent.inner.model.display_name();
            let current_tool_names = session.active_agent.tool_names.clone();

            let (
                input_tokens,
                output_tokens,
                cached_tokens,
                total_tokens,
                input_tokens_delta,
                output_tokens_delta,
                cached_tokens_delta,
                total_tokens_delta,
            ) = if let Ok(usage_lock) = session.usage.read() {
                let session_usage = &*usage_lock;
                (
                    session_usage.accumulated.input_tokens,
                    session_usage.accumulated.output_tokens,
                    session_usage.accumulated.cached_input_tokens,
                    session_usage.accumulated.total_tokens,
                    session_usage.last_exchange.input_tokens,
                    session_usage.last_exchange.output_tokens,
                    session_usage.last_exchange.cached_input_tokens,
                    session_usage.last_exchange.total_tokens,
                )
            } else {
                (0, 0, 0, 0, 0, 0, 0, 0)
            };

            let context_tokens = session
                .log_indexer
                .read()
                .ok()
                .map(|idx| idx.active_context_token_count())
                .unwrap_or(0);

            return Self {
                title,
                chat_index,
                total_count,
                agent_name,
                model_display,
                current_tool_names,
                input_tokens,
                output_tokens,
                cached_tokens,
                total_tokens,
                context_tokens: context_tokens as u64,
                input_tokens_delta,
                output_tokens_delta,
                cached_tokens_delta,
                total_tokens_delta,
            };
        }

        Self {
            title: None,
            chat_index: 0,
            total_count: 0,
            agent_name: String::new(),
            model_display: String::new(),
            current_tool_names: Vec::new(),
            input_tokens: 0,
            output_tokens: 0,
            cached_tokens: 0,
            total_tokens: 0,
            context_tokens: 0,
            input_tokens_delta: 0,
            output_tokens_delta: 0,
            cached_tokens_delta: 0,
            total_tokens_delta: 0,
        }
    }
}

#[derive(Clone)]
pub struct AgentDefaultsCache {
    pub default_model_display: String,
    pub resolved_tool_names: Vec<String>,
}

pub struct FooterState {
    previous_values: Option<FooterValues>,
    agent_defaults_cache: Option<AgentDefaultsCache>,
}

impl FooterState {
    pub fn new() -> Self {
        Self {
            previous_values: None,
            agent_defaults_cache: None,
        }
    }

    /// Returns true if any footer value changed
    pub fn should_render(&mut self, current: &FooterValues) -> bool {
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

    pub fn get_footer_lines(&self, values: &FooterValues) -> (String, String) {
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

        let meta_suffix = match (model_changed, tool_suffix.is_empty()) {
            (true, true) => format!(" (󰚩 {})", values.model_display),
            (true, false) => format!(" (󰚩 {} | {})", values.model_display, tool_suffix),
            (false, true) => String::new(),
            (false, false) => format!(" ({})", tool_suffix),
        };

        // Build title line
        let title_line = format!(
            "󰭹 {} {} of {}, agent: {}{}",
            values.title.clone().unwrap_or_default(),
            values.chat_index + 1,
            values.total_count,
            values.agent_name,
            meta_suffix
        );

        // Format token counts with K/M/B suffixes
        let input = format_token_with_delta(values.input_tokens, values.input_tokens_delta);
        let output = format_token_with_delta(values.output_tokens, values.output_tokens_delta);
        let cached = format_token_with_delta(values.cached_tokens, values.cached_tokens_delta);
        let total = format_token_with_delta(values.total_tokens, values.total_tokens_delta);

        let token_line = format!(
            "tokens: {}~ | usage: {} 󰕒 + {} 󰇚 + {}  = {} total",
            values.context_tokens, input, output, cached, total
        );

        (title_line, token_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl FooterValues {
        fn test_default() -> Self {
            Self {
                title: None,
                chat_index: 0,
                total_count: 1,
                agent_name: "default".to_string(),
                model_display: "claude-3-5-sonnet".to_string(),
                current_tool_names: vec!["read_file".to_string()],
                input_tokens: 0,
                output_tokens: 0,
                cached_tokens: 0,
                total_tokens: 0,
                context_tokens: 0,
                input_tokens_delta: 0,
                output_tokens_delta: 0,
                cached_tokens_delta: 0,
                total_tokens_delta: 0,
            }
        }
    }

    #[test]
    fn test_footer_should_render_on_first_call() {
        let mut state = FooterState::new();
        let values = FooterValues::test_default();
        assert!(state.should_render(&values));
    }

    #[test]
    fn test_footer_should_render_when_title_changes() {
        let mut state = FooterState::new();
        let values1 = FooterValues::test_default();
        assert!(state.should_render(&values1));

        let values2 = FooterValues {
            title: Some("New Title".to_string()),
            ..values1.clone()
        };
        assert!(state.should_render(&values2));
    }

    #[test]
    fn test_footer_should_render_when_token_usage_changes() {
        let mut state = FooterState::new();
        let values1 = FooterValues {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            context_tokens: 200,
            ..FooterValues::test_default()
        };
        assert!(state.should_render(&values1));

        let values2 = FooterValues {
            input_tokens: 150,
            total_tokens: 200,
            ..values1.clone()
        };
        assert!(state.should_render(&values2));
    }

    #[test]
    fn test_footer_should_not_render_when_unchanged() {
        let mut state = FooterState::new();
        let values = FooterValues::test_default();
        assert!(state.should_render(&values));
        assert!(!state.should_render(&values));
        assert!(!state.should_render(&values));
    }

    #[test]
    fn test_footer_state_get_footer_lines() {
        let mut state = FooterState::new();
        let values = FooterValues {
            title: Some("Test Chat".to_string()),
            chat_index: 2,
            total_count: 5,
            agent_name: "default".to_string(),
            model_display: "anthropic: claude-3-5-sonnet".to_string(),
            // 2 tools, both in default, so 8 removed from default's 10 tools
            current_tool_names: vec!["read_file".to_string(), "edit_file".to_string()],
            input_tokens: 100,
            output_tokens: 50,
            cached_tokens: 25,
            total_tokens: 175,
            context_tokens: 200,
            input_tokens_delta: 100,
            output_tokens_delta: 50,
            cached_tokens_delta: 25,
            total_tokens_delta: 175,
        };

        // Populate cache
        state.should_render(&values);

        let (title_line, token_line) = state.get_footer_lines(&values);

        // Show model diff and tool diff (model changed, 8 tools removed)
        assert_eq!(
            title_line,
            "󰭹 Test Chat 3 of 5, agent: default (󰚩 anthropic: claude-3-5-sonnet | 󰣖 -8)"
        );
        assert_eq!(
            token_line,
            "tokens: 200~ | usage: 100 (+100) 󰕒 + 50 (+50) 󰇚 + 25 (+25)  = 175 (+175) total"
        );
    }

    #[test]
    fn test_footer_state_get_footer_lines_empty_title() {
        let mut state = FooterState::new();
        let values = FooterValues {
            title: None,
            chat_index: 0,
            total_count: 1,
            agent_name: "default".to_string(),
            model_display: "ollama_cloud: glm-5.1".to_string(),
            // 1 tool, 9 removed from default's 10 tools
            current_tool_names: vec!["read_file".to_string()],
            ..FooterValues::test_default()
        };

        // Populate cache
        state.should_render(&values);

        let (title_line, token_line) = state.get_footer_lines(&values);

        // Show tool diff (model matches, 9 tools removed)
        assert_eq!(title_line, "󰭹  1 of 1, agent: default (󰣖 -9)");
        assert_eq!(token_line, "tokens: 0~ | usage: 0 󰕒 + 0 󰇚 + 0  = 0 total");
    }

    #[test]
    fn test_footer_state_get_footer_lines_no_diff() {
        let mut state = FooterState::new();
        // Use all default tools - no diff
        let values = FooterValues {
            title: Some("Test Chat".to_string()),
            chat_index: 0,
            total_count: 1,
            agent_name: "default".to_string(),
            model_display: "ollama_cloud: glm-5.1".to_string(),
            current_tool_names: vec![
                "create_file".to_string(),
                "edit_file".to_string(),
                "fetch_webpage".to_string(),
                "list_files".to_string(),
                "read_file".to_string(),
                "remove_path".to_string(),
                "run".to_string(),
                "search_text".to_string(),
                "web_search".to_string(),
                "think".to_string(),
            ],
            ..FooterValues::test_default()
        };

        // Populate cache
        state.should_render(&values);

        let (title_line, token_line) = state.get_footer_lines(&values);

        // No diff shown - delta omitted when 0
        assert_eq!(title_line, "󰭹 Test Chat 1 of 1, agent: default");
        assert_eq!(token_line, "tokens: 0~ | usage: 0 󰕒 + 0 󰇚 + 0  = 0 total");
    }

    #[test]
    fn test_footer_state_get_footer_lines_added_tools() {
        let mut state = FooterState::new();
        // Add extra tools not in default
        let values = FooterValues {
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
            ..FooterValues::test_default()
        };

        // Populate cache
        state.should_render(&values);

        let (title_line, _) = state.get_footer_lines(&values);

        // 1 added (mcp_server____custom_tool), 8 removed (default's 10 minus read_file, edit_file)
        assert_eq!(title_line, "󰭹  1 of 1, agent: default (󰣖 +1/-8)");
    }
}
