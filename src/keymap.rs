use nvim_oxi::{Dictionary, Function, Object, api::types::LogLevel};

use crate::{
    chat::ActiveAgent,
    get_application_config, get_chat_window,
    ui::picker::pick,
    utils::{GLOBAL_EXECUTION_HANDLER, notify},
};

pub fn create_lua_keymap_module() -> Dictionary {
    let mut keymap_dict = Dictionary::new();
    keymap_dict.insert("send", Object::from(send_fn()));
    keymap_dict.insert("next_chat", Object::from(next_chat_fn()));
    keymap_dict.insert("prev_chat", Object::from(prev_chat_fn()));
    keymap_dict.insert("new_chat", Object::from(new_chat_fn()));
    keymap_dict.insert("dismiss_chat", Object::from(dismiss_chat_fn()));
    keymap_dict.insert("stop_streaming", Object::from(stop_streaming_fn()));
    keymap_dict.insert("select_agent", Object::from(select_agent_fn()));
    keymap_dict.insert("toggle_focus", Object::from(toggle_focus_fn()));

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

fn toggle_focus_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock() {
                if let Err(e) = win.toggle_focus() {
                    notify(format!("{}", e), LogLevel::Error);
                }
            }
        }
    })
}

fn select_agent_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            let config = get_application_config();
            let mut agent_names: Vec<String> = config.agents.keys().cloned().collect();
            agent_names.sort();

            let current_agent_name: Option<String> = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_process.read().ok()?;
                let process = loaded.read().ok()?;
                Some(process.active_agent.name.clone())
            })();

            let options: Vec<&str> = agent_names.iter().map(|s| s.as_str()).collect();

            if let Err(e) = pick(
                "Select Agent",
                &options,
                current_agent_name.as_deref(),
                |selected| match selected {
                    Some(name) => {
                        let config = get_application_config();
                        if let Some(agent) = config.agents.get(&name) {
                            let win_arc = get_chat_window();
                            if let Ok(win) = win_arc.lock() {
                                if let Ok(loaded) = win.loaded_chat_process.read() {
                                    if let Ok(mut process) = loaded.write() {
                                        process.active_agent = ActiveAgent {
                                            name: name.clone(),
                                            inner: agent.clone(),
                                        };
                                        win.force_render();
                                    }
                                }
                            }
                        }
                    }
                    None => {}
                },
            ) {
                GLOBAL_EXECUTION_HANDLER
                    .notify_on_main_thread(format!("picker error: {}", e), LogLevel::Error);
            }
        }
    })
}
