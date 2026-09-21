use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use nvim_oxi::{Dictionary, Result as OxiResult, mlua::lua};

use crate::{
    chat::choreo::Choreo, config::TenonConfig, directive::Directive,
    lua_modules::create_lua_module, ui::ChatWindow, utils::GLOBAL_EXECUTION_HANDLER,
};

pub static CHAT_WINDOW: OnceLock<Arc<Mutex<ChatWindow>>> = OnceLock::new();

pub fn get_chat_window() -> Arc<Mutex<ChatWindow>> {
    CHAT_WINDOW
        .get_or_init(|| Arc::new(Mutex::new(ChatWindow::new())))
        .clone()
}

pub static CONFIG: OnceLock<TenonConfig> = OnceLock::new();

pub fn get_application_config() -> TenonConfig {
    CONFIG.get_or_init(TenonConfig::default).clone()
}

pub static DIRECTIVE_REGISTRY: OnceLock<HashMap<String, Directive>> = OnceLock::new();

pub fn get_directive_registry() -> HashMap<String, Directive> {
    DIRECTIVE_REGISTRY
        .get_or_init(directive::load_system_directives)
        .clone()
}

pub static CHOREO_REGISTRY: OnceLock<HashMap<String, Arc<Choreo>>> = OnceLock::new();

pub fn get_choreo_registry() -> HashMap<String, Arc<Choreo>> {
    CHOREO_REGISTRY
        .get_or_init(|| {
            chat::choreo::load_system_choreos()
                .into_iter()
                .map(|c| (c.id.clone(), c))
                .collect()
        })
        .clone()
}

mod agent;
mod chat;
mod clients;
mod config;
mod directive;
mod hooks;
mod lua_modules;
mod mcp;
mod rag;
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
            vim.api.nvim_set_hl(0, 'TenonLineToolSuccess', { fg = '#5f5f00', ctermfg = 58 })
            vim.api.nvim_set_hl(0, 'TenonLineToolError', { fg = '#870000', ctermfg = 88 })
            vim.api.nvim_set_hl(0, 'TenonSignThought', { fg = '#939393', ctermfg = 8 })
            vim.api.nvim_set_hl(0, 'TenonLineThought', { italic = true })
            vim.api.nvim_set_hl(0, 'TenonSignProcessing', { fg = '#939393', ctermfg = 8 })
            vim.api.nvim_set_hl(0, 'TenonLineProcessing', { fg = '#939393', ctermfg = 8 })
            vim.api.nvim_set_hl(0, 'TenonSignSelectRecommended', { fg = '#6f95d8', ctermfg = 12 })
            vim.api.nvim_set_hl(0, 'TenonSignSelectBullet', { fg = '#939393', ctermfg = 8 })
            vim.api.nvim_set_hl(0, 'TenonLineSelectTitle', { fg = '#939393', ctermfg = 8, bold = true })
            vim.api.nvim_set_hl(0, 'TenonLineChatMeta', { fg = '#28869c', ctermfg = 6 })
            "#,
        )
        .exec();

    LazyLock::force(&GLOBAL_EXECUTION_HANDLER);

    utils::PLUGIN_ROOT.get_or_init(|| {
        let path: String = lua()
            .load(r#"return vim.fn.fnamemodify(vim.api.nvim_get_runtime_file("lua/tenon.so", false)[1], ":h:h")"#)
            .eval()
            .unwrap_or_default();
        PathBuf::from(path)
    });

    Ok(create_lua_module())
}
