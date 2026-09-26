use nvim_oxi::{Function, Result as OxiResult, mlua::lua};

use crate::{
    chat::history::{SessionMetadata, save_to_history},
    get_application_config, get_chat_window,
    utils::GLOBAL_EXECUTION_HANDLER,
};

/// Prompt to rename the current chat session.
pub fn rename_fn() -> Function<(), ()> {
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
                        if let Ok(log_window) = session.engine.log_handler.log_window.read() {
                            save_to_history(
                                SessionMetadata {
                                    id: &session.id,
                                    title: new_title.as_deref(),
                                    agent_name: &session.active_agent_name,
                                    model_display: &session.engine.model.display_name(),
                                    session_datetime: session.session_datetime,
                                },
                                &log_window,
                                &session.usage,
                                &session.work_queue,
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
