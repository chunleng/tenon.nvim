use crate::ui::{
    nvim_primitives::{
        buffer::{NvimBuffer, NvimBufferOption, NvimKeymap},
        window::{NvimWindow, NvimWindowOption, NvimWindowType},
    },
    widget::Widget,
};
use nvim_oxi::Result as OxiResult;

#[derive(Debug, Clone)]
pub struct FixedBufferPanelOption {
    pub buf_type: String,
    pub buf_listed: bool,
    pub swap_file: bool,
    pub file_type: String,
    pub modifiable: bool,
    pub wrap: bool,
    pub line_break: bool,
    pub undo_levels: isize,
    pub text_width: isize,
    pub number: bool,
    pub relative_number: bool,
    pub sign_column: String,
    pub buf_keymaps: Vec<NvimKeymap>,
    pub window_option: NvimWindowType,
}

impl Default for FixedBufferPanelOption {
    fn default() -> Self {
        Self {
            buf_type: String::from("nofile"),
            buf_listed: false,
            swap_file: false,
            file_type: String::from(""),
            modifiable: true,
            wrap: true,
            line_break: true,
            undo_levels: 1000,
            text_width: 0,
            sign_column: "auto".to_string(),
            number: true,
            relative_number: true,
            buf_keymaps: vec![],
            window_option: NvimWindowType::CenteredFloat {
                height: 0.6,
                width: 0.6,
            },
        }
    }
}

#[derive(Clone)]
pub struct FixedBufferPanel<T: Widget> {
    pub window: NvimWindow,
    pub widget: T,
}

impl<T: Widget> FixedBufferPanel<T> {
    pub fn new(option: &FixedBufferPanelOption, mut widget: T) -> OxiResult<Self> {
        let window = NvimWindow::try_from((widget.buffer(), option))?;
        widget.set_window(window.clone());
        Ok(Self { window, widget })
    }
}

impl TryFrom<&FixedBufferPanelOption> for NvimBuffer {
    type Error = nvim_oxi::Error;

    fn try_from(value: &FixedBufferPanelOption) -> Result<Self, Self::Error> {
        Self::new(NvimBufferOption {
            buf_type: value.buf_type.to_string(),
            buf_listed: value.buf_listed,
            // TODO FixedBufferPanel actually does not have to be wiped always, but we need to
            // think of ways to ensure that we don't get leftover hidden buffers.
            buf_hidden: "wipe".to_string(),
            swap_file: value.swap_file,
            file_type: value.file_type.to_string(),
            modifiable: value.modifiable,
            undo_levels: value.undo_levels,
            text_width: value.text_width,
            buf_keymaps: value.buf_keymaps.clone(),
        })
    }
}

impl TryFrom<(&NvimBuffer, &FixedBufferPanelOption)> for NvimWindow {
    type Error = nvim_oxi::Error;

    fn try_from(
        (buffer, option): (&NvimBuffer, &FixedBufferPanelOption),
    ) -> Result<Self, Self::Error> {
        Self::new(
            buffer.clone(),
            NvimWindowOption {
                wrap: option.wrap,
                line_break: option.line_break,
                number: option.number,
                relative_number: option.relative_number,
                sign_column: option.sign_column.to_string(),
                winfixbuf: true,
                window_option: option.window_option.clone(),
            },
        )
    }
}

