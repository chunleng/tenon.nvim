use nvim_oxi::{
    Result as OxiResult,
    api::{
        self, Buffer as OxiBuffer,
        opts::{OptionOpts, SetKeymapOpts},
        types::Mode,
    },
};

#[derive(Debug, Clone)]
pub struct NvimKeymap {
    pub modes: Vec<Mode>,
    pub lhs: String,
    pub rhs: String,
    pub opts: SetKeymapOpts,
}

#[derive(Debug, Clone)]
pub struct NvimBufferOption {
    pub buf_type: String,
    pub buf_listed: bool,
    pub buf_hidden: String,
    pub swap_file: bool,
    pub file_type: String,
    pub modifiable: bool,
    pub undo_levels: isize,
    pub text_width: isize,
    pub buf_keymaps: Vec<NvimKeymap>,
}

impl Default for NvimBufferOption {
    fn default() -> Self {
        Self {
            buf_type: "nofile".to_string(),
            buf_listed: false,
            buf_hidden: "wipe".to_string(),
            swap_file: false,
            file_type: "".to_string(),
            modifiable: true,
            undo_levels: 1000,
            text_width: 0,
            buf_keymaps: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct NvimBuffer {
    pub inner: OxiBuffer,
}
impl NvimBuffer {
    pub fn new(option: NvimBufferOption) -> OxiResult<Self> {
        let mut buffer = api::create_buf(option.buf_listed, false)?;

        let buf_opts = OptionOpts::builder().buffer(buffer.clone()).build();
        api::set_option_value("bufhidden", option.buf_hidden, &buf_opts)?;
        api::set_option_value("buftype", option.buf_type, &buf_opts)?;
        api::set_option_value("buflisted", option.buf_listed, &buf_opts)?;
        api::set_option_value("swapfile", option.swap_file, &buf_opts)?;
        api::set_option_value("filetype", option.file_type, &buf_opts)?;
        api::set_option_value("modifiable", option.modifiable, &buf_opts)?;
        api::set_option_value("undolevels", option.undo_levels, &buf_opts)?;
        api::set_option_value("textwidth", option.text_width, &buf_opts)?;

        for keymap in option.buf_keymaps {
            for mode in keymap.modes {
                buffer.set_keymap(mode, &keymap.lhs, &keymap.rhs, &keymap.opts)?;
            }
        }

        Ok(Self { inner: buffer })
    }

    pub fn set_bufhidden(&self, value: &str) -> OxiResult<()> {
        let opts = OptionOpts::builder().buffer(self.inner.clone()).build();
        api::set_option_value("bufhidden", value, &opts)?;
        Ok(())
    }

    pub fn get_buffer(&self) -> Option<api::Buffer> {
        if self.inner.is_valid() {
            Some(self.inner.clone())
        } else {
            None
        }
    }
}
