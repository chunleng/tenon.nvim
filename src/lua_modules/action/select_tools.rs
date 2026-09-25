use nvim_oxi::{Function, api::types::LogLevel};

use crate::{
    get_application_config, get_chat_window,
    tools::{all_tool_names, tool_matches_selectors},
    ui::picker::{FzfAction, FzfOption, SelectMode, action, pick},
    utils::GLOBAL_EXECUTION_HANDLER,
};

/// Show a picker to select tools for the current session.
pub fn select_tools_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            // Read current tool names and agent name on the main thread (just Rust struct access).
            let (current_tool_names, agent_name): (Vec<String>, String) = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                Some((
                    session
                        .engine
                        .tools
                        .iter()
                        .map(|t| t.name().to_string())
                        .collect(),
                    session.active_agent_name.clone(),
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
                                    session.engine.set_tools(tools);
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
