use std::sync::{Arc, Mutex};

use nvim_oxi::{Dictionary, Function, Object, Result as OxiResult};

use crate::{keymap::create_lua_keymap_module, ui::ChatWindow};

mod chat;
mod keymap;
mod tools;
mod ui;
mod utils;

#[nvim_oxi::plugin]
fn omnidash() -> OxiResult<Dictionary> {
    let chat_window = Arc::new(Mutex::new(ChatWindow::new()));

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
