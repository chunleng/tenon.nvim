use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use nvim_oxi::{
    Dictionary, Function, Object, Result as OxiResult, api::types::LogLevel, mlua::lua,
    serde::Deserializer,
};
use serde::Deserialize;

use crate::{
    config::{TenonConfig, user::TenonUserConfig},
    keymap::create_lua_keymap_module,
    ui::ChatWindow,
    utils::{GLOBAL_EXECUTION_HANDLER, notify},
};

pub static CHAT_WINDOW: OnceLock<Arc<Mutex<ChatWindow>>> = OnceLock::new();

pub fn get_chat_window() -> Arc<Mutex<ChatWindow>> {
    CHAT_WINDOW
        .get_or_init(|| Arc::new(Mutex::new(ChatWindow::new())))
        .clone()
}

pub static CONFIG: OnceLock<TenonConfig> = OnceLock::new();

pub fn get_application_config() -> TenonConfig {
    CONFIG.get_or_init(|| TenonConfig::default()).clone()
}

mod chat;
mod clients;
mod config;
mod keymap;
mod mcp;
mod tools;
mod ui;
mod utils;

#[nvim_oxi::plugin]
fn tenon() -> OxiResult<Dictionary> {
    // Define highlight groups for sign icons using Lua to support integer ctermfg
    let _ = lua()
        .load(
            r#"
            vim.api.nvim_set_hl(0, 'TenonSignUser', { fg = '#6f95d8', ctermfg = 12 })
            vim.api.nvim_set_hl(0, 'TenonSignAssistantReasoning', { fg = '#939393', ctermfg = 8 })
            vim.api.nvim_set_hl(0, 'TenonSignAssistantTalk', { fg = '#6d9c10', ctermfg = 2 })
            vim.api.nvim_set_hl(0, 'TenonSignTool', { fg = '#d0d0d0', ctermfg = 15 })
            vim.api.nvim_set_hl(0, 'TenonLineAssistantReasoning', { link = 'Comment' })
            vim.api.nvim_set_hl(0, 'TenonLineTool', { link = 'Comment' })
            vim.api.nvim_set_hl(0, 'TenonSignProcessing', { fg = '#939393', ctermfg = 8 })
            vim.api.nvim_set_hl(0, 'TenonLineProcessing', { fg = '#939393', ctermfg = 8 })
            vim.api.nvim_set_hl(0, 'TenonLineChatMeta', { fg = '#28869c', ctermfg = 6 })
            "#,
        )
        .exec();

    LazyLock::force(&GLOBAL_EXECUTION_HANDLER);

    let open_fn = Function::from_fn_mut({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                let _ = win.open();
            }
        }
    });

    let setup_fn = Function::from_fn_mut({
        |conf: Object| {
            if CONFIG.get().is_some() {
                notify(
                    "[tenon.nvim] setup() called after config already initialized; ignoring",
                    LogLevel::Warn,
                );
                return;
            }
            CONFIG.get_or_init(|| {
                match TenonUserConfig::deserialize(Deserializer::new(conf))
                    .map_err(|e| e.into())
                    .and_then(|x| TenonConfig::try_from(x))
                {
                    Ok(res) => res,
                    Err(e) => {
                        notify(
                            format!("[tenon.nvim] error reading config: {}", e),
                            LogLevel::Error,
                        );
                        notify("[tenon.nvim] using default config", LogLevel::Warn);
                        TenonConfig::default()
                    }
                }
            });
        }
    });

    let mut module = Dictionary::new();
    module.insert("setup", Object::from(setup_fn));
    module.insert("open", Object::from(open_fn));
    module.insert("keymap", Object::from(create_lua_keymap_module()));

    Ok(module)
}
