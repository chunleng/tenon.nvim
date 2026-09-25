use nvim_oxi::{Function, api::types::LogLevel};

use crate::{
    get_application_config, get_chat_window,
    ui::picker::{FzfOption, SelectMode, box_single_select, pick},
    utils::GLOBAL_EXECUTION_HANDLER,
};

/// Show a picker to select the active agent.
pub fn select_agent_fn() -> Function<(), ()> {
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
                Some(session.active_agent_name.clone())
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
                                    session.active_agent_name = name.clone();
                                    session.engine.model = agent.model.clone();
                                    session.engine.directive = agent.directive.clone();
                                    session.engine.set_tools(agent.tool_names.clone());
                                    session.engine.choreos = agent.choreos.clone();
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
