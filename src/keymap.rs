use nvim_oxi::{Dictionary, Function, Object, api::types::LogLevel};

use crate::{
    chat::ActiveAgent,
    get_application_config, get_chat_window,
    tools::all_tool_names,
    ui::picker::{pick, pick_multi},
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
    keymap_dict.insert("select_model", Object::from(select_model_fn()));
    keymap_dict.insert("select_tools", Object::from(select_tools_fn()));
    keymap_dict.insert("toggle_focus", Object::from(toggle_focus_fn()));
    keymap_dict.insert("select_history", Object::from(select_history_fn()));

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

fn select_tools_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            // Read current tool names on the main thread (just Rust struct access).
            let current_tool_names: Vec<String> = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                Some(session.active_agent.tool_names.clone())
            })()
            .unwrap_or_default();

            // all_tool_names() may call MCP (off-thread only), so run it off the main thread.
            std::thread::spawn(move || {
                let all_names = all_tool_names();
                let options: Vec<&str> = all_names.iter().map(|s| s.as_str()).collect();
                let current_refs: Vec<&str> =
                    current_tool_names.iter().map(|s| s.as_str()).collect();

                if let Err(e) = pick_multi("Select Tools", &options, &current_refs, |selected| {
                    if let Some(tools) = selected {
                        let win_arc = get_chat_window();
                        if let Ok(win) = win_arc.lock() {
                            if let Ok(loaded) = win.loaded_chat_session.read() {
                                if let Ok(mut session) = loaded.write() {
                                    session.active_agent.inner.tool_names = tools;
                                    win.force_render();
                                }
                            }
                        }
                    }
                }) {
                    GLOBAL_EXECUTION_HANDLER
                        .notify_on_main_thread(format!("picker error: {}", e), LogLevel::Error);
                }
            });
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
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                Some(session.active_agent.name.clone())
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
                                if let Ok(loaded) = win.loaded_chat_session.read() {
                                    if let Ok(mut session) = loaded.write() {
                                        session.active_agent = ActiveAgent {
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

fn select_model_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            let config = get_application_config();
            let model_list: Vec<String> = config.models.iter().map(|m| m.display_name()).collect();

            let current_model_display: Option<String> = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                Some(session.active_agent.inner.model.display_name())
            })();

            let options: Vec<&str> = model_list.iter().map(|s| s.as_str()).collect();

            if let Err(e) = pick(
                "Select Model",
                &options,
                current_model_display.as_deref(),
                |selected| match selected {
                    Some(display_name) => {
                        let config = get_application_config();
                        if let Some(model) = config
                            .models
                            .iter()
                            .find(|m| m.display_name() == display_name)
                            .cloned()
                        {
                            let win_arc = get_chat_window();
                            if let Ok(win) = win_arc.lock() {
                                if let Ok(loaded) = win.loaded_chat_session.read() {
                                    if let Ok(mut session) = loaded.write() {
                                        session.active_agent.inner.model = model;
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

fn select_history_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            let history_dir = get_application_config().history.directory.clone();

            std::thread::spawn(move || {
                let histories = crate::chat::history::load_history_entries(&history_dir);
                if histories.is_empty() {
                    GLOBAL_EXECUTION_HANDLER
                        .notify_on_main_thread("no chat history found", LogLevel::Warn);
                    return;
                }

                let options: Vec<String> = histories
                    .iter()
                    .map(|h| {
                        let datetime =
                            h.id.rsplit_once('_')
                                .map(|(dt, _)| dt.replace('T', " "))
                                .unwrap_or_else(|| h.id.clone());
                        let title = h.title.as_deref().unwrap_or("Untitled");
                        format!(
                            "{} | {} | agent: {} | {}",
                            datetime, title, h.agent_name, h.model_display
                        )
                    })
                    .collect();

                let options_clone = options.clone();
                let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

                if let Err(e) = pick("Select History", &options_refs, None, move |selected| {
                    if let Some(selection) = selected {
                        let idx = options_clone.iter().position(|s| *s == selection);
                        if let Some(idx) = idx {
                            if let Some(history) = histories.into_iter().nth(idx) {
                                // Serialize history to JSON so we can pass it through execute_on_main_thread
                                if let Ok(history_json) = serde_json::to_string(&history) {
                                    if let Err(e) = GLOBAL_EXECUTION_HANDLER
                                        .execute_rust_on_main_thread(move || {
                                            match serde_json::from_str::<
                                                crate::chat::history::ChatHistory,
                                            >(
                                                &history_json
                                            ) {
                                                Ok(history) => {
                                                    let win_arc = get_chat_window();
                                                    if let Ok(mut win) = win_arc.lock() {
                                                        if let Err(e) = win
                                                            .load_or_create_chat_from_history(
                                                                history,
                                                            )
                                                        {
                                                            notify(
                                                                format!("{}", e),
                                                                LogLevel::Error,
                                                            );
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    notify(
                                                        format!("failed to parse history: {}", e),
                                                        LogLevel::Error,
                                                    );
                                                }
                                            }
                                            Ok(())
                                        })
                                    {
                                        GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                                            format!("failed to load history: {}", e),
                                            LogLevel::Error,
                                        );
                                    }
                                }
                            }
                        }
                    }
                }) {
                    GLOBAL_EXECUTION_HANDLER
                        .notify_on_main_thread(format!("picker error: {}", e), LogLevel::Error);
                }
            });
        }
    })
}
