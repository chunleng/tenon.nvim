use std::sync::{Arc, RwLock};

use nvim_oxi::api::{self, opts::OptionOpts};

use crate::{
    ui::{
        nvim_primitives::window::NvimWindow,
        widget::chat_display::{ChatDisplayData, chat_metadata::MetadataValues},
    },
    utils::GLOBAL_EXECUTION_HANDLER,
};

/// Title line split into segments with truncation priority. When the winbar
/// is too narrow, the choreo segment drops first, then the queue segment, so
/// the title and chat index survive longest.
#[derive(Clone, Debug, PartialEq)]
pub struct TitleSegments {
    /// "{choreo name}, Move {n}" (empty when no choreo is active); rendered
    /// left of the right-aligned title
    pub choreo: String,
    /// Icon + chat title
    pub head: String,
    /// " {n} of {m}"
    pub index: String,
    /// ", queue: N" (empty when queue is empty)
    pub queue: String,
}

pub struct MetadataState {
    previous_values: Option<MetadataValues>,
}

impl MetadataState {
    pub fn new() -> Self {
        Self {
            previous_values: None,
        }
    }

    /// Returns true if any value shown in the title changed. Gates the title
    /// winbar only; the session winbar is rebuilt every tick.
    pub fn should_render_title(&mut self, current: &MetadataValues) -> bool {
        let changed = match &self.previous_values {
            None => true,
            Some(prev) => {
                prev.title != current.title
                    || prev.chat_index != current.chat_index
                    || prev.total_count != current.total_count
                    || prev.work_queue_count != current.work_queue_count
                    || prev.choreo != current.choreo
            }
        };

        if changed {
            self.previous_values = Some(current.clone());
        }

        changed
    }

    /// Title line segments for the chat window winbar
    pub fn title_segments(&self, values: &MetadataValues) -> TitleSegments {
        let queue = if values.work_queue_count > 0 {
            format!(", queue: {}", values.work_queue_count)
        } else {
            String::new()
        };

        let choreo = values
            .choreo
            .as_ref()
            .map(|(name, r#move)| format!("\u{ef0d}  {name} \u{f292} {move}"))
            .unwrap_or_default();

        TitleSegments {
            choreo,
            head: values.title.clone().unwrap_or_default(),
            index: format!(" {} of {}", values.chat_index + 1, values.total_count),
            queue,
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
            // The truncated title embeds a `%=` marker between the left
            // (choreo) and right (title) halves; escape `%` in each half so
            // user text is not parsed as winbar format items
            let (left, right) = title.split_once("%=").unwrap_or(("", &title));
            let winbar = format!(
                "{}%={} %*",
                left.replace('%', "%%"),
                right.replace('%', "%%")
            );
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

/// Builds the winbar title within `max_width` display columns, returned as
/// `{left}%={right}` where `%=` right-aligns `right`. The choreo segment
/// (left of the marker) drops first, then the queue segment, so the title and
/// chat index survive longest; if even head + index overflow, the title is
/// truncated with an ellipsis while the index suffix is kept. The `%=` marker
/// takes no display columns.
pub fn truncate_title_segments(segments: &TitleSegments, max_width: usize) -> String {
    use unicode_width::UnicodeWidthStr;

    let mut kept = vec![segments.head.as_str(), segments.index.as_str()];
    for segment in [&segments.queue] {
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
    let right: String = kept.concat();

    // Choreo sits left of the right-aligned title and drops before the queue
    let choreo =
        if segments.choreo.is_empty() || segments.choreo.width() + right.width() <= max_width {
            segments.choreo.as_str()
        } else {
            ""
        };

    if right.width() <= max_width {
        return format!("{choreo}%={right}");
    }

    // Head + index alone overflow: truncate the head, keep the index
    let index_width = segments.index.width();
    if index_width >= max_width {
        return format!("%={}", truncate_with_ellipsis(&segments.index, max_width));
    }
    let head = truncate_with_ellipsis(&segments.head, max_width - index_width);
    format!("%={head}{}", segments.index)
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
    use super::*;
    use unicode_width::UnicodeWidthStr;

    impl TitleSegments {
        /// Winbar content without the `%=` marker, for width assertions
        fn join(&self) -> String {
            format!("{}{}{}{}", self.choreo, self.head, self.index, self.queue)
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
    fn test_title_segments_with_queue() {
        let mut state = MetadataState::new();
        let values = MetadataValues {
            title: Some("Test Chat".to_string()),
            work_queue_count: 2,
            ..MetadataValues::test_default()
        };

        // Populate state
        state.should_render_title(&values);

        assert_eq!(
            state.title_segments(&values).join(),
            "Test Chat 1 of 1, queue: 2"
        );
    }

    #[test]
    fn test_title_segments_with_choreo() {
        let mut state = MetadataState::new();
        let values = MetadataValues {
            title: Some("Test Chat".to_string()),
            choreo: Some(("implement_code".to_string(), 2)),
            ..MetadataValues::test_default()
        };
        state.should_render_title(&values);

        let segments = state.title_segments(&values);
        assert_eq!(segments.choreo, "\u{ef0d}  implement_code \u{f292} 2");
        assert_eq!(
            segments.join(),
            "\u{ef0d}  implement_code \u{f292} 2Test Chat 1 of 1"
        );

        // No active choreo: empty segment
        let none = state.title_segments(&MetadataValues::test_default());
        assert_eq!(none.choreo, "");
    }

    #[test]
    fn test_truncate_title_segments_fits_unchanged() {
        let segments = TitleSegments {
            choreo: String::new(),
            head: "Test".to_string(),
            index: " 1 of 2".to_string(),
            queue: ", queue: 3".to_string(),
        };
        let full = segments.join();
        assert_eq!(truncate_title_segments(&segments, 200), format!("%={full}"));
    }

    #[test]
    fn test_truncate_title_segments_drops_queue_first() {
        let segments = TitleSegments {
            choreo: String::new(),
            head: "Test".to_string(),
            index: " 1 of 2".to_string(),
            queue: ", queue: 3".to_string(),
        };
        let full_width = segments.join().width();

        // One column short: queue dropped
        assert_eq!(
            truncate_title_segments(&segments, full_width - 1),
            "%=Test 1 of 2"
        );

        // Queue no longer fits
        assert_eq!(
            truncate_title_segments(&segments, "Test 1 of 2".width()),
            "%=Test 1 of 2"
        );
    }

    #[test]
    fn test_truncate_title_segments_drops_choreo_first() {
        let segments = TitleSegments {
            choreo: "\u{ef0d}  implement_code \u{f292} 2".to_string(),
            head: "Test".to_string(),
            index: " 1 of 2".to_string(),
            queue: ", queue: 3".to_string(),
        };
        let full_width = segments.join().width();

        // Exactly full: choreo left-aligned, title right-aligned
        assert_eq!(
            truncate_title_segments(&segments, full_width),
            "\u{ef0d}  implement_code \u{f292} 2%=Test 1 of 2, queue: 3"
        );

        // One column short: choreo dropped, queue kept
        assert_eq!(
            truncate_title_segments(&segments, full_width - 1),
            "%=Test 1 of 2, queue: 3"
        );

        // Narrower: queue dropped too
        assert_eq!(
            truncate_title_segments(&segments, "Test 1 of 2".width()),
            "%=Test 1 of 2"
        );
    }

    #[test]
    fn test_metadata_should_render_when_choreo_changes() {
        let mut state = MetadataState::new();
        let values1 = MetadataValues::test_default();
        assert!(state.should_render_title(&values1));

        let values2 = MetadataValues {
            choreo: Some(("implement_code".to_string(), 1)),
            ..values1.clone()
        };
        assert!(state.should_render_title(&values2));
        assert!(!state.should_render_title(&values2));

        let values3 = MetadataValues {
            choreo: Some(("implement_code".to_string(), 2)),
            ..values2.clone()
        };
        assert!(state.should_render_title(&values3));
    }

    #[test]
    fn test_truncate_title_segments_truncates_head_keeps_index() {
        let segments = TitleSegments {
            choreo: String::new(),
            head: "A very long chat title".to_string(),
            index: " 3 of 9".to_string(),
            queue: String::new(),
        };

        let truncated = truncate_title_segments(&segments, 10);
        assert_eq!(truncated, "%=A … 3 of 9");
    }
}
