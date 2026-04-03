use std::sync::{Arc, Mutex};

use nvim_oxi::{
    Dictionary, Function, Object, Result as OxiResult,
    api::{notify, types::LogLevel},
};

use crate::ui::ChatWindow;

mod chat;
mod ui;

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

    let mut keymap_dict = Dictionary::new();
    keymap_dict.insert(
        "send",
        Object::from(Function::from_fn({
            let win_clone = chat_window.clone();
            move |()| {
                if let Ok(mut win) = win_clone.lock()
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
                            let _ = notify(
                                "please enter your message before sending",
                                LogLevel::Error,
                                &Dictionary::new(),
                            );
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
        })),
    );

    let mut module = Dictionary::new();
    module.insert("open", Object::from(open_fn));
    module.insert("keymap", Object::from(keymap_dict));

    Ok(module)
}
