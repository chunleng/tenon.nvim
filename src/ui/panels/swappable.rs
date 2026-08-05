use std::collections::HashMap;
use std::sync::Arc;

use crate::ui::{
    nvim_primitives::window::{NvimWindow, NvimWindowOption, NvimWindowType},
    widget::Widget,
};
use nvim_oxi::{Result as OxiResult, api::opts::BufDeleteOpts};

/// Unlike FixedBufferPanel, this does NOT set `winfixbuf` so buffers can be swapped.
#[derive(Debug, Clone)]
pub struct SwappablePanelOption {
    pub wrap: bool,
    pub line_break: bool,
    pub number: bool,
    pub relative_number: bool,
    pub sign_column: String,
    pub smoothscroll: bool,
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
            smoothscroll: false,
            window_option: NvimWindowType::CenteredFloat {
                height: 0.6,
                width: 0.6,
            },
        }
    }
}

/// A panel with a single window that can hold multiple widget stacks, identified by key.
///
/// Each key maps to a stack of widgets. Only one key's stack is visible at a
/// time (the "active" key). The top widget of the active stack is displayed.
/// Widgets can be pushed onto and popped from a key's stack, enabling overlay
/// patterns (e.g. a select widget on top of an input widget).
#[derive(Clone)]
pub struct SwappableBufferPanel {
    pub window: NvimWindow,
    pub widgets: HashMap<String, Vec<Arc<dyn Widget>>>,
    pub active_key: String,
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
            winfixbuf: true,
            smoothscroll: option.smoothscroll,
            window_option: option.window_option.clone(),
        };
        let window = NvimWindow::new(buffer, window_option)?;
        widget.set_window(window.clone());
        let _ = widget.render();

        let widget = Arc::from(widget);
        let mut widgets = HashMap::new();
        widgets.insert(key.to_string(), vec![Arc::clone(&widget)]);

        Ok(Self {
            window,
            widgets,
            active_key: key.to_string(),
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
            return Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(format!(
                "Widget key '{}' already exists in this panel",
                key
            ))));
        }
        widget.buffer().set_bufhidden("hide")?;
        widget.set_window(self.window.clone());
        let _ = widget.render();
        self.widgets
            .insert(key.to_string(), vec![Arc::from(widget)]);
        Ok(())
    }

    /// Swaps the window to display the top widget of the stack identified by `key`.
    ///
    /// If the key doesn't exist, returns an error. If the key is already
    /// active, this is a no-op.
    pub fn swap_to(&mut self, key: impl Into<String>) -> OxiResult<()> {
        let key = key.into();

        if self.active_key == key {
            return Ok(());
        }

        let widget = self.widget(&key).ok_or_else(|| {
            nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(format!(
                "No widget with key '{}' in this panel",
                key
            )))
        })?;

        self.swap_buffer(&widget)?;
        self.active_key = key;
        Ok(())
    }

    /// Pushes a widget onto the stack for `key`, making it the new top.
    /// The window buffer is swapped to display the pushed widget.
    pub fn push_widget(&mut self, key: &str, mut widget: Box<dyn Widget>) -> OxiResult<()> {
        widget.buffer().set_bufhidden("hide")?;
        widget.set_window(self.window.clone());
        let _ = widget.render();
        let widget = Arc::from(widget);

        let stack = self.widgets.get_mut(key).ok_or_else(|| {
            nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(format!(
                "No widget with key '{}' in this panel",
                key
            )))
        })?;
        stack.push(Arc::clone(&widget));
        if self.active_key == key {
            self.swap_buffer(&widget)?;
        }
        Ok(())
    }

    /// Pops the top widget from the stack for `key`.
    ///
    /// Restores the widget below (swaps buffer).
    /// Returns `None` if the key doesn't exist or the stack has only one
    /// widget (the base, which cannot be popped).
    pub fn pop_widget(&mut self, key: &str) -> OxiResult<Option<Arc<dyn Widget>>> {
        let stack = match self.widgets.get_mut(key) {
            Some(s) if s.len() > 1 => s,
            _ => return Ok(None),
        };
        let popped = stack.pop();

        let new_top = Arc::clone(stack.last().expect("stack has at least one widget"));
        if self.active_key == key {
            self.swap_buffer(&new_top)?;
        }

        if let Some(popped) = &popped
            && let Some(buffer) = popped.buffer().get_buffer()
        {
            let _ = buffer.delete(&BufDeleteOpts::default());
        }

        Ok(popped)
    }

    /// Removes an entire widget stack from the panel by key.
    pub fn remove_widget(&mut self, key: impl Into<String>) -> Option<Vec<Arc<dyn Widget>>> {
        self.widgets.remove(&key.into())
    }

    /// Returns a clone of the top widget Arc for `key`, or `None`.
    pub fn widget(&self, key: &str) -> Option<Arc<dyn Widget>> {
        self.widgets.get(key).and_then(|s| s.last().cloned())
    }

    /// Returns an iterator over all widget keys.
    pub fn widget_keys(&self) -> impl Iterator<Item = &str> {
        self.widgets.keys().map(|s| s.as_str())
    }

    /// Toggles winfixbuf off, sets the window buffer to `widget`'s buffer,
    /// then re-enables winfixbuf.
    fn swap_buffer(&mut self, widget: &Arc<dyn Widget>) -> OxiResult<()> {
        let win_opts = nvim_oxi::api::opts::OptionOpts::builder()
            .win(self.window.inner.clone())
            .build();
        nvim_oxi::api::set_option_value("winfixbuf", false, &win_opts)?;
        self.window.inner.set_buf(&widget.buffer().inner)?;
        nvim_oxi::api::set_option_value("winfixbuf", true, &win_opts)?;
        Ok(())
    }
}
