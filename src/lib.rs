use std::sync::{Arc, LazyLock, Mutex};

use nvim_oxi::{Dictionary, Function, Object, Result as OxiResult};

use crate::{keymap::create_lua_keymap_module, ui::ChatWindow, utils::GLOBAL_EXECUTION_HANDLER};

mod chat;
mod clients;
mod keymap;
mod mcp;
mod tools;
mod ui;
mod utils;

#[nvim_oxi::plugin]
fn tenon() -> OxiResult<Dictionary> {
    let chat_window = Arc::new(Mutex::new(ChatWindow::new()));
    LazyLock::force(&GLOBAL_EXECUTION_HANDLER);

    let open_fn = Function::from_fn_mut({
        let win_clone = chat_window.clone();
        move |()| {
            if let Ok(mut win) = win_clone.lock() {
                let _ = win.open();
            }
        }
    });

    let mut module = Dictionary::new();
    module.insert("open", Object::from(open_fn));
    module.insert(
        "keymap",
        Object::from(create_lua_keymap_module(chat_window)),
    );

    Ok(module)
}
