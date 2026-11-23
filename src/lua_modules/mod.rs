mod close;
mod open;
mod setup;
mod toggle;

pub mod action;

use nvim_oxi::{Dictionary, Object};

use crate::lua_modules::action::create_lua_action_module;

/// Build the top-level plugin module exposed to Lua. Only `setup` lives here;
/// the entry functions are handed out solely through setup's return value, so
/// no Lua path can reach them before config is validated.
pub fn create_lua_module() -> Dictionary {
    let mut module = Dictionary::new();
    module.insert("setup", Object::from(setup::setup_fn()));
    module
}

/// Build the entry table returned by a successful setup call.
pub fn entries() -> Dictionary {
    let mut entries = Dictionary::new();
    entries.insert("open", Object::from(open::open_fn()));
    entries.insert("toggle", Object::from(toggle::toggle_fn()));
    entries.insert("close", Object::from(close::close_fn()));
    entries.insert("action", Object::from(create_lua_action_module()));
    entries
}
