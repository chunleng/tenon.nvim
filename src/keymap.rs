use std::sync::{Arc, Mutex};

use nvim_oxi::{Dictionary, Function, Object, api::types::LogLevel};

use crate::{ui::ChatWindow, utils::notify};

pub fn create_lua_keymap_module(chat_window: Arc<Mutex<ChatWindow>>) -> Dictionary {
    let mut keymap_dict = Dictionary::new();
    keymap_dict.insert("send", Object::from(send_fn(chat_window)));

    keymap_dict
}

fn send_fn(chat_window: Arc<Mutex<ChatWindow>>) -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = chat_window.lock()
                && let Ok(input_win) = win.get_or_create_input_window()
                && let Some(mut input_win_buffer) = input_win.get_buffer()
            {
                let mut message_sent = false;
                if let Ok(lines) = input_win_buffer.get_lines(0.., false) {
                    let message = lines
                        .map(|x| x.to_string())
                        .reduce(|acc, s| format!("{}\n{}", acc, s))
                        .unwrap()
                        .trim()
                        .to_string();
                    if message.is_empty() {
                        notify("please enter your message before sending", LogLevel::Error);
                    } else {
                        win.chat_process.send_message(message);
                        message_sent = true;
                    }
                }
                if message_sent {
                    let _ = input_win_buffer.set_lines(0.., false, [""]);
                }
            }
        }
    })
}
