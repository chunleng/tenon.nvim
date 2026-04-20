use std::collections::HashMap;
use std::sync::Arc;

use crate::ui::{
    nvim_primitives::window::{NvimWindow, NvimWindowOption, NvimWindowType},
    widget::Widget,
};
use nvim_oxi::Result as OxiResult;

/// Window-level options for a swappable panel.
/// Unlike FixedBufferPanel, this does NOT set `winfixbuf` so buffers can be swapped.
#[derive(Debug, Clone)]
pub struct SwappablePanelOption {
    pub wrap: bool,
    pub line_break: bool,
    pub number: bool,
    pub relative_number: bool,
    pub sign_column: String,
    pub window_option: NvimWindowType,
}

impl Default for SwappablePanelOption {
    fn default() -> Self {
        Self {
            wrap: true,
            line_break: true,
            number: true,
            relative_number: true,
            sign_column: "auto".to_string(),
            window_option: NvimWindowType::CenteredFloat {
                height: 0.6,
                width: 0.6,
            },
        }
    }
}

/// The currently active widget in a swappable panel.
#[derive(Clone)]
pub struct ActiveWidget {
    pub key: String,
    pub widget: Arc<dyn Widget>,
}

/// A panel with a single window that can hold multiple widgets, swapping them in and out.
///
/// Each widget is identified by a unique string key. Only one widget is visible
/// in the window at a time (the "active" widget). Different widget types can be
/// stored via trait objects.
#[derive(Clone)]
pub struct SwappableBufferPanel {
    pub window: NvimWindow,
    widgets: HashMap<String, Arc<dyn Widget>>,
    active: ActiveWidget,
}

impl SwappableBufferPanel {
    /// Creates a new swappable panel with an initial widget.
    ///
    /// The window is opened using the initial widget's buffer. Additional widgets
    /// can be added later with `add_widget` and swapped in with `swap_to`.
    pub fn new(
        option: &SwappablePanelOption,
        key: &str,
        mut widget: Box<dyn Widget>,
    ) -> OxiResult<Self> {
        let buffer = widget.buffer().clone();
        buffer.set_bufhidden("hide")?;
        let window_option = NvimWindowOption {
            wrap: option.wrap,
            line_break: option.line_break,
            number: option.number,
            relative_number: option.relative_number,
            sign_column: option.sign_column.to_string(),
            winfixbuf: false,
            window_option: option.window_option.clone(),
        };
        let window = NvimWindow::new(buffer, window_option)?;
        widget.set_window(window.clone());
        let _ = widget.render();

        let widget = Arc::from(widget);
        let mut widgets = HashMap::new();
        widgets.insert(key.to_string(), Arc::clone(&widget));

        Ok(Self {
            window,
            widgets,
            active: ActiveWidget {
                key: key.to_string(),
                widget,
            },
        })
    }

    /// Adds a new widget without making it active.
    ///
    /// The widget's `render` method is called and the window reference is set.
    /// Returns an error if the key already exists.
    pub fn add_widget(
        &mut self,
        key: impl Into<String>,
        mut widget: Box<dyn Widget>,
    ) -> OxiResult<()> {
        let key = key.into();
        if self.widgets.contains_key(&key) {
            return Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
                format!("Widget key '{}' already exists in this panel", key).into(),
            )));
        }
        widget.buffer().set_bufhidden("hide")?;
        widget.set_window(self.window.clone());
        let _ = widget.render();
        self.widgets.insert(key.to_string(), Arc::from(widget));
        Ok(())
    }

    /// Swaps the window to display the widget identified by `key`.
    ///
    /// If the key doesn't exist, returns an error. If the key is already
    /// active, this is a no-op.
    pub fn swap_to(&mut self, key: impl Into<String>) -> OxiResult<()> {
        let key = key.into();
        if self.active.key == key {
            return Ok(());
        }

        let widget = self.widgets.get(&key).ok_or_else(|| {
            nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
                format!("No widget with key '{}' in this panel", key).into(),
            ))
        })?;

        self.window.inner.set_buf(&widget.buffer().inner)?;

        self.active = ActiveWidget {
            key: key.to_string(),
            widget: Arc::clone(widget),
        };
        Ok(())
    }

    /// Removes a widget from the panel.
    ///
    /// If the removed widget is currently active, the `Arc` is removed from
    /// the registry but the active reference remains valid until `swap_to`
    /// is called with another key.
    /// Returns the removed widget if it existed in the registry.
    pub fn remove_widget(&mut self, key: impl Into<String>) -> Option<Arc<dyn Widget>> {
        self.widgets.remove(&key.into())
    }

    /// Returns a reference to the active widget.
    pub fn active_widget(&self) -> &dyn Widget {
        &*self.active.widget
    }

    /// Returns an iterator over all widget keys.
    pub fn widget_keys(&self) -> impl Iterator<Item = &str> {
        self.widgets.keys().map(|s| s.as_str())
    }
}
