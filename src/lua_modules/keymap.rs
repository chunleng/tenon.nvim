use nvim_oxi::{
    Dictionary, Function, Object, Result as OxiResult, api::types::LogLevel, mlua::lua,
};

use crate::{
    chat::ActiveAgent,
    chat::history::{SessionMetadata, save_to_history},
    clients::SupportedModels,
    get_application_config, get_chat_window,
    tools::{all_tool_names, tool_matches_selectors},
    ui::picker::{FzfAction, FzfOption, SelectMode, action, box_single_select, pick},
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
    keymap_dict.insert("rename", Object::from(rename_fn()));

    keymap_dict
}

fn send_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.send()
            {
                notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

fn next_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.load_next_chat()
            {
                notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

fn prev_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.load_prev_chat()
            {
                notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

fn new_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.new_chat()
            {
                notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

fn dismiss_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.dismiss_chat()
            {
                notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

fn stop_streaming_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.stop_streaming()
            {
                notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

fn toggle_focus_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            if let Ok(mut win) = get_chat_window().lock()
                && let Err(e) = win.toggle_focus()
            {
                notify(format!("{}", e), LogLevel::Error);
            }
        }
    })
}

fn select_tools_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            // Read current tool names and agent name on the main thread (just Rust struct access).
            let (current_tool_names, agent_name): (Vec<String>, String) = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                Some((
                    session.active_agent.tool_names.clone(),
                    session.active_agent.name.clone(),
                ))
            })()
            .unwrap_or_default();

            // all_tool_names() may call MCP (off-thread only), so run it off the main thread.
            std::thread::spawn(move || {
                let all_names = all_tool_names();
                let options: Vec<&str> = all_names.iter().map(|s| s.as_str()).collect();
                let current_refs: Vec<&str> =
                    current_tool_names.iter().map(|s| s.as_str()).collect();
                let resolved_current: Vec<&str> = options
                    .iter()
                    .filter(|o| tool_matches_selectors(o, &current_refs))
                    .copied()
                    .collect();

                let default_tool_names = get_application_config()
                    .agents
                    .get(&agent_name)
                    .map(|a| a.tool_names.clone())
                    .unwrap_or_default();

                let mut actions = std::collections::HashMap::new();
                if !default_tool_names.is_empty() {
                    actions.insert(
                        "ctrl-d".to_string(),
                        FzfAction::Fn {
                            builder: action(move |lua, resolve_fn| {
                                let defaults = default_tool_names.clone();
                                Ok(lua.create_function(move |_, ()| {
                                    resolve_fn.call::<()>(defaults.clone())?;
                                    Ok(())
                                })?)
                            }),
                            reload: false,
                            description: "reset to default".to_string(),
                        },
                    );
                }

                if let Err(e) = pick(
                    &options,
                    FzfOption {
                        prompt: "Select Tools".to_string(),
                        select_mode: SelectMode::multi(&resolved_current),
                        actions,
                        callback: Box::new(|selected| {
                            if let Some(tools) = selected {
                                let win_arc = get_chat_window();
                                if let Ok(win) = win_arc.lock()
                                    && let Ok(loaded) = win.loaded_chat_session.read()
                                    && let Ok(mut session) = loaded.write()
                                {
                                    session.active_agent.inner.tool_names = tools;
                                    win.force_render();
                                }
                            }
                        }),
                        ..Default::default()
                    },
                ) {
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
                &options,
                FzfOption {
                    prompt: "Select Agent".to_string(),
                    select_mode: SelectMode::single(current_agent_name),
                    callback: box_single_select(|selected| {
                        if let Some(name) = selected {
                            let config = get_application_config();
                            if let Some(agent) = config.agents.get(&name) {
                                let win_arc = get_chat_window();
                                if let Ok(win) = win_arc.lock()
                                    && let Ok(loaded) = win.loaded_chat_session.read()
                                    && let Ok(mut session) = loaded.write()
                                {
                                    session.active_agent = ActiveAgent {
                                        name: name.clone(),
                                        inner: agent.clone(),
                                    };
                                    win.force_render();
                                }
                            }
                        }
                    }),
                    ..Default::default()
                },
            ) {
                GLOBAL_EXECUTION_HANDLER
                    .notify_on_main_thread(format!("picker error: {}", e), LogLevel::Error);
            }
        }
    })
}

fn format_model_display(name: &str, model: &SupportedModels) -> String {
    let fixed_name: String = if name.chars().count() > 20 {
        name.chars().take(20).collect()
    } else {
        format!("{:<20}", name)
    };
    format!(
        "{} | {}/{}",
        fixed_name, model.connector_name, model.model_name
    )
}

fn select_model_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            let config = get_application_config();
            let entries: Vec<(String, SupportedModels)> = config
                .models
                .iter()
                .map(|(name, m)| (name.clone(), m.clone()))
                .collect();
            let model_list: Vec<String> = entries
                .iter()
                .map(|(name, m)| format_model_display(name, m))
                .collect();

            let current_model_display: Option<String> = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                let current = &session.active_agent.inner.model;
                entries.iter().find_map(|(name, m)| {
                    (m.connector_name == current.connector_name
                        && m.model_name == current.model_name)
                        .then(|| format_model_display(name, m))
                })
            })();

            let options: Vec<&str> = model_list.iter().map(|s| s.as_str()).collect();

            if let Err(e) = pick(
                &options,
                FzfOption {
                    prompt: "Select Model".to_string(),
                    select_mode: SelectMode::single(current_model_display),
                    callback: box_single_select(move |selected| {
                        if let Some(display) = selected
                            && let Some((_, model)) = entries
                                .iter()
                                .find(|(name, m)| format_model_display(name, m) == display)
                        {
                            let win_arc = get_chat_window();
                            if let Ok(win) = win_arc.lock()
                                && let Ok(loaded) = win.loaded_chat_session.read()
                                && let Ok(mut session) = loaded.write()
                            {
                                session.active_agent.inner.model = model.clone();
                                win.force_render();
                            }
                        }
                    }),
                    ..Default::default()
                },
            ) {
                GLOBAL_EXECUTION_HANDLER
                    .notify_on_main_thread(format!("picker error: {}", e), LogLevel::Error);
            }
        }
    })
}

fn rename_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            let current_title: Option<String> = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                session.title_handler.title()
            })();

            std::thread::spawn(move || {
                let result =
                    GLOBAL_EXECUTION_HANDLER.execute_rust_on_main_thread_async(move |resolver| {
                        let lua = lua();

                        let result: OxiResult<()> = (|| {
                            let input_opts = lua.create_table()?;
                            input_opts.set("prompt", "Rename chat: ")?;
                            if let Some(title) = &current_title {
                                input_opts.set("default", title.clone())?;
                            }

                            let resolver_clone = resolver.clone();
                            let callback =
                                lua.create_function(move |_, input: Option<String>| {
                                    resolver_clone.resolve(Ok(input));
                                    Ok(())
                                })?;

                            let vim_ui_input =
                                lua.load("return vim.ui.input").eval::<mlua::Function>()?;
                            vim_ui_input.call::<()>((input_opts, callback))?;
                            Ok(())
                        })();

                        if let Err(e) = result {
                            resolver.resolve(Err(e));
                        }
                    });

                if let Ok(Some(input)) = result {
                    let new_title: Option<String> = if input.trim().is_empty() {
                        None
                    } else {
                        Some(input.trim().to_string())
                    };

                    let win_arc = get_chat_window();
                    if let Ok(win) = win_arc.lock()
                        && let Ok(loaded) = win.loaded_chat_session.read()
                        && let Ok(session) = loaded.read()
                    {
                        if let Ok(mut title) = session.title_handler.title.write() {
                            *title = new_title.clone();
                        }

                        let history_dir = get_application_config().history.directory;
                        if let Ok(log_window) = session.log_handler.log_window.read() {
                            save_to_history(
                                SessionMetadata {
                                    id: &session.id,
                                    title: new_title.as_deref(),
                                    agent_name: &session.active_agent.name,
                                    model_display: &session.active_agent.inner.model.display_name(),
                                    session_datetime: session.session_datetime,
                                },
                                &log_window,
                                &session.usage,
                                &history_dir,
                            );
                        }

                        win.force_render();
                    }
                }
            });
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
                        let messages = h.logs.iter().count();
                        format!(
                            "{} │ {:>3} msg │ {} (󰚩  {}, {})",
                            datetime, messages, title, h.agent_name, h.model_display
                        )
                    })
                    .collect();

                let options_clone = options.clone();
                let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

                if let Err(e) = pick(
                    &options_refs,
                    FzfOption {
                        prompt: "Select History".to_string(),
                        sorting: false,
                        callback: box_single_select(move |selected| {
                            if let Some(selection) = selected {
                                let idx = options_clone.iter().position(|s| *s == selection);
                                if let Some(idx) = idx
                                    && let Some(history) = histories.into_iter().nth(idx)
                                {
                                    // Serialize history to JSON so we can pass it through execute_rust_on_main_thread
                                    if let Ok(history_json) = serde_json::to_string(&history)
                                        && let Err(e) = GLOBAL_EXECUTION_HANDLER
                                            .execute_rust_on_main_thread(move || {
                                                match serde_json::from_str::<
                                                    crate::chat::history::ChatHistory,
                                                >(
                                                    &history_json
                                                ) {
                                                    Ok(history) => {
                                                        let win_arc = get_chat_window();
                                                        if let Ok(mut win) = win_arc.lock()
                                                            && let Err(e) = win
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
                                                    Err(e) => {
                                                        notify(
                                                            format!(
                                                                "failed to parse history: {}",
                                                                e
                                                            ),
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
                        }),
                        ..Default::default()
                    },
                ) {
                    GLOBAL_EXECUTION_HANDLER
                        .notify_on_main_thread(format!("picker error: {}", e), LogLevel::Error);
                }
            });
        }
    })
}
