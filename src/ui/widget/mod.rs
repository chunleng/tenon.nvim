use dyn_clone::{DynClone, clone_trait_object};
use nvim_oxi::Result as OxiResult;

use crate::ui::nvim_primitives::{buffer::NvimBuffer, window::NvimWindow};

pub mod chat_display;

pub trait Widget: DynClone + Send + Sync {
    fn render(&mut self) -> OxiResult<()>;
    fn buffer(&self) -> &NvimBuffer;
    fn set_window(&mut self, _window: NvimWindow) {}
}

/// A plain widget that holds a buffer but performs no rendering.
/// Used for panels that only need a buffer (e.g. input windows).
#[derive(Clone)]
pub struct BasicWidget {
    buffer: NvimBuffer,
}

impl BasicWidget {
    pub fn new(buffer: NvimBuffer) -> Self {
        Self { buffer }
    }
}

impl Widget for BasicWidget {
    fn render(&mut self) -> OxiResult<()> {
        Ok(())
    }

    fn buffer(&self) -> &NvimBuffer {
        &self.buffer
    }
}

clone_trait_object!(Widget);
