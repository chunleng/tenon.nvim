use std::sync::{Arc, LazyLock, Mutex};

use nvim_oxi::{Dictionary, Function, Object, Result as OxiResult, mlua::lua};

use crate::{keymap::create_lua_keymap_module, ui::ChatWindow, utils::GLOBAL_EXECUTION_HANDLER};

pub static GLOBAL_CHAT_WINDOW: LazyLock<Arc<Mutex<ChatWindow>>> =
    LazyLock::new(|| Arc::new(Mutex::new(ChatWindow::new())));

mod chat;
mod clients;
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
        let win_clone = GLOBAL_CHAT_WINDOW.clone();
        move |()| {
            if let Ok(mut win) = win_clone.lock() {
                let _ = win.open();
            }
        }
    });

    let mut module = Dictionary::new();
    module.insert("open", Object::from(open_fn));
    module.insert("keymap", Object::from(create_lua_keymap_module()));

    Ok(module)
}
