use nvim_oxi::{
    Result as OxiResult,
    api::{
        self, Window as OxiWindow,
        opts::OptionOpts,
        types::{WindowConfig, WindowRelativeTo},
    },
};

use crate::ui::nvim_primitives::buffer::NvimBuffer;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum NvimWindowType {
    CenteredFloat {
        height: f64,
        width: f64,
    },
    Split {
        direction: NvimSplitWindowType,
        ratio_wh: f64,
        edge: bool,
    },
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum NvimSplitWindowType {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub struct NvimWindowOption {
    pub wrap: bool,
    pub line_break: bool,
    pub number: bool,
    pub relative_number: bool,
    pub sign_column: String,
    pub winfixbuf: bool,
    pub smoothscroll: bool,
    pub window_option: NvimWindowType,
}

impl Default for NvimWindowOption {
    fn default() -> Self {
        Self {
            wrap: true,
            line_break: true,
            sign_column: "auto".to_string(),
            number: true,
            relative_number: true,
            winfixbuf: true,
            smoothscroll: false,
            window_option: NvimWindowType::CenteredFloat {
                height: 0.6,
                width: 0.6,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct NvimWindow {
    pub inner: OxiWindow,
}

impl NvimWindow {
    pub fn new(buffer: NvimBuffer, option: NvimWindowOption) -> OxiResult<Self> {
        let window =
            match option.window_option {
                NvimWindowType::CenteredFloat { height, width } => {
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
                    api::open_win(&buffer.inner, true, &win_config)?
                }
                NvimWindowType::Split {
                    direction,
                    edge,
                    ratio_wh,
                } => {
                    let split_type =
                        match (&direction, &edge) {
                            (NvimSplitWindowType::Top, true)
                            | (NvimSplitWindowType::Left, true) => "topleft",
                            (NvimSplitWindowType::Bottom, true)
                            | (NvimSplitWindowType::Right, true) => "botright",
                            (NvimSplitWindowType::Top, false)
                            | (NvimSplitWindowType::Left, false) => "aboveleft",
                            (NvimSplitWindowType::Bottom, false)
                            | (NvimSplitWindowType::Right, false) => "belowright",
                        };
                    let vh = match &direction {
                        NvimSplitWindowType::Top | NvimSplitWindowType::Bottom => "split",
                        NvimSplitWindowType::Left | NvimSplitWindowType::Right => "vsplit",
                    };
                    api::command(&format!("{} {}", split_type, vh))?;

                    match &direction {
                        NvimSplitWindowType::Top | NvimSplitWindowType::Bottom => {
                            let ui_height =
                                api::get_option_value::<i64>("lines", &OptionOpts::default())?;
                            let win_height = (ui_height as f64 * ratio_wh) as u32;
                            api::command(&format!("horizontal resize {}", win_height))?;
                        }
                        NvimSplitWindowType::Left | NvimSplitWindowType::Right => {
                            let ui_width =
                                api::get_option_value::<i64>("columns", &OptionOpts::default())?;
                            let win_width = (ui_width as f64 * ratio_wh) as u32;
                            api::command(&format!("vertical resize {}", win_width))?;
                        }
                    }
                    let mut win = api::get_current_win();
                    win.set_buf(&buffer.inner)?;
                    win
                }
            };

        let win_opts = OptionOpts::builder().win(window.clone()).build();
        if option.winfixbuf {
            api::set_option_value("winfixbuf", true, &win_opts)?;
        }

        api::set_option_value("wrap", option.wrap, &win_opts)?;
        api::set_option_value("linebreak", option.line_break, &win_opts)?;
        api::set_option_value("signcolumn", option.sign_column, &win_opts)?;
        api::set_option_value("number", option.number, &win_opts)?;
        api::set_option_value("relativenumber", option.relative_number, &win_opts)?;
        api::set_option_value("smoothscroll", option.smoothscroll, &win_opts)?;

        Ok(Self { inner: window })
    }

    pub fn get_window(&self) -> Option<api::Window> {
        if self.inner.is_valid() {
            Some(self.inner.clone())
        } else {
            None
        }
    }

    /// Moves the cursor to the last character of the last line and snaps the
    /// view so the last line is visible at the bottom of the window.
    ///
    /// Scrolling uses `winrestview` because it is not affected by `scrolloff`
    /// or `smoothscroll`, unlike `zb`. The `winrestview` dict omits `col`, so
    /// the cursor column set by `cursor()` survives.
    pub fn snap_to_bottom(&self) {
        let Some(window) = self.get_window() else {
            return;
        };
        let Ok(line_count) = window.get_buf().and_then(|b| b.line_count()) else {
            return;
        };
        let Ok(win_height) = window.get_height() else {
            return;
        };

        let topline = line_count
            .saturating_sub(win_height as usize)
            .saturating_add(1);
        let lnum = line_count;
        let _ = window.call(move |()| {
            let _ = api::command(&format!("call cursor({lnum}, 1)"));
            // col('$') must be evaluated with the cursor already on {lnum},
            // so it cannot be merged into the first cursor() call.
            let _ = api::command(&format!("call cursor({lnum}, col('$')+1)"));
            let winline: i64 = api::eval("winline()").unwrap_or(0);
            let winheight: i64 = api::eval("winheight(0)").unwrap_or(0);
            if winline < winheight {
                let _ = api::command(&format!(
                    "lua vim.fn.winrestview({{topline = {topline}, lnum = {lnum}}})"
                ));
            }
        });
    }
}
