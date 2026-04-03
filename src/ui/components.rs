use nvim_oxi::{
    Result as OxiResult,
    api::{
        self,
        opts::OptionOpts,
        types::{WindowConfig, WindowRelativeTo},
    },
};

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum SplitWindowOption {
    Top { height: f64 },
    Bottom { height: f64 },
    Left { width: f64 },
    Right { width: f64 },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum WindowOption {
    CenteredFloat { height: f64, width: f64 },
    Split(SplitWindowOption),
}

#[derive(Debug, Clone)]
pub struct FixedBufferVimWindowOption {
    pub buf_type: String,
    pub buf_listed: bool,
    pub swap_file: bool,
    pub file_type: String,
    pub modifiable: bool,
    pub wrap: bool,
    pub line_break: bool,
    pub window_option: WindowOption,
}

impl Default for FixedBufferVimWindowOption {
    fn default() -> Self {
        Self {
            buf_type: String::from("nofile"),
            buf_listed: false,
            swap_file: false,
            file_type: String::from(""),
            modifiable: true,
            wrap: true,
            line_break: true,
            window_option: WindowOption::CenteredFloat {
                height: 0.6,
                width: 0.6,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct FixedBufferVimWindow {
    buffer: api::Buffer,
    window: api::Window,
}

impl FixedBufferVimWindow {
    pub fn new(option: FixedBufferVimWindowOption) -> OxiResult<Self> {
        let buffer = api::create_buf(option.buf_listed, false)?;

        let buf_opts = OptionOpts::builder().buffer(buffer.clone()).build();
        // Needed for this struct as we want to make sure buffer are closed when window close
        api::set_option_value("bufhidden", "wipe", &buf_opts)?;

        api::set_option_value("buftype", option.buf_type, &buf_opts)?;
        api::set_option_value("buflisted", option.buf_listed, &buf_opts)?;
        api::set_option_value("swapfile", option.swap_file, &buf_opts)?;
        api::set_option_value("filetype", option.file_type, &buf_opts)?;
        api::set_option_value("modifiable", option.modifiable, &buf_opts)?;

        let window = match option.window_option {
            WindowOption::CenteredFloat { height, width } => {
                let ui_width = api::get_option_value::<i64>("columns", &OptionOpts::default())?;
                let win_width = (ui_width as f64 * width) as u32;
                let ui_height = api::get_option_value::<i64>("lines", &OptionOpts::default())?;
                let win_height = (ui_height as f64 * height) as u32;

                let row = ((ui_height as f64 * (1.0 - height)) / 2.0) as u32;
                let col = ((ui_width as f64 * (1.0 - width)) / 2.0) as u32;

                let win_config = WindowConfig::builder()
                    .relative(WindowRelativeTo::Editor)
                    .width(win_width)
                    .height(win_height)
                    .row(row)
                    .col(col)
                    .build();
                api::open_win(&buffer, true, &win_config)?
            }
            WindowOption::Split(split_type) => {
                match split_type {
                    SplitWindowOption::Top { .. } => api::command("topleft split")?,
                    SplitWindowOption::Bottom { .. } => api::command("botright split")?,
                    SplitWindowOption::Left { .. } => api::command("topleft vsplit")?,
                    SplitWindowOption::Right { .. } => api::command("botright vsplit")?,
                }
                match split_type {
                    SplitWindowOption::Top { height } | SplitWindowOption::Bottom { height } => {
                        let ui_height =
                            api::get_option_value::<i64>("lines", &OptionOpts::default())?;
                        let win_height = (ui_height as f64 * height) as u32;
                        api::command(&format!("horizontal resize {}", win_height))?;
                    }
                    SplitWindowOption::Left { width } | SplitWindowOption::Right { width } => {
                        let ui_width =
                            api::get_option_value::<i64>("columns", &OptionOpts::default())?;
                        let win_width = (ui_width as f64 * width) as u32;
                        api::command(&format!("vertical resize {}", win_width))?;
                    }
                }
                let mut win = api::get_current_win();
                win.set_buf(&buffer)?;
                win
            }
        };

        let win_opts = OptionOpts::builder().win(window.clone()).build();
        // Needed for this struct as we want to make sure window's buffer doesn't change
        api::set_option_value("winfixbuf", true, &win_opts)?;

        api::set_option_value("wrap", option.wrap, &win_opts)?;
        api::set_option_value("linebreak", option.line_break, &win_opts)?;

        Ok(Self { buffer, window })
    }

    pub fn get_buffer(&self) -> Option<api::Buffer> {
        if self.buffer.is_valid() {
            Some(self.buffer.clone())
        } else {
            None
        }
    }

    pub fn get_window(&self) -> Option<api::Window> {
        if self.window.is_valid() {
            Some(self.window.clone())
        } else {
            None
        }
    }
}
