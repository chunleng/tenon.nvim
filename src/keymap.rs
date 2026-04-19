use nvim_oxi::{Dictionary, Function, Object, api::types::LogLevel};

use crate::{get_chat_window, utils::notify};

pub fn create_lua_keymap_module() -> Dictionary {
    let mut keymap_dict = Dictionary::new();
    keymap_dict.insert("send", Object::from(send_fn()));
    keymap_dict.insert("close", Object::from(close_fn()));
    keymap_dict.insert("next_chat", Object::from(next_chat_fn()));
    keymap_dict.insert("prev_chat", Object::from(prev_chat_fn()));
    keymap_dict.insert("new_chat", Object::from(new_chat_fn()));
    keymap_dict.insert("dismiss_chat", Object::from(dismiss_chat_fn()));
    keymap_dict.insert("stop_streaming", Object::from(stop_streaming_fn()));

    keymap_dict
}

fn send_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                if let Err(e) = win.send() {
                    notify(format!("{}", e), LogLevel::Error);
                }
            }
        }
    })
}

fn close_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                if let Err(e) = win.close() {
                    notify(format!("{}", e), LogLevel::Error);
                }
            }
        }
    })
}

fn next_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                if let Err(e) = win.load_next_chat() {
                    notify(format!("{}", e), LogLevel::Error);
                }
            }
        }
    })
}

fn prev_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                if let Err(e) = win.load_prev_chat() {
                    notify(format!("{}", e), LogLevel::Error);
                }
            }
        }
    })
}

fn new_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                if let Err(e) = win.new_chat() {
                    notify(format!("{}", e), LogLevel::Error);
                }
            }
        }
    })
}

fn dismiss_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                if let Err(e) = win.dismiss_chat() {
                    notify(format!("{}", e), LogLevel::Error);
                }
            }
        }
    })
}

fn stop_streaming_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                if let Err(e) = win.stop_streaming() {
                    notify(format!("{}", e), LogLevel::Error);
                }
            }
        }
    })
}
