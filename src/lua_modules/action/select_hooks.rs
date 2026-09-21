use nvim_oxi::{Function, api::types::LogLevel};

use crate::{
    get_application_config, get_chat_window,
    hooks::HookType,
    ui::picker::{FzfOption, SelectMode, pick},
    utils::GLOBAL_EXECUTION_HANDLER,
};

/// Show a multi-select picker to choose which hooks are active for the
/// current chat session.
pub fn select_hooks_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            let hooks = get_application_config().hooks;
            if hooks.is_empty() {
                return;
            }

            // Picker label: `type | name`.
            let options: Vec<String> = hooks
                .iter()
                .map(|h| {
                    let type_label = match h.hook_type {
                        HookType::NeedsAttention => "needs_attention",
                    };
                    format!("{} | {}", type_label, h.name)
                })
                .collect();
            let options: Vec<&str> = options.iter().map(|s| s.as_str()).collect();

            let current: Vec<String> = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                Some(session.active_hooks.read().ok()?.iter().cloned().collect())
            })()
            .unwrap_or_default();

            if let Err(e) = pick(
                &options,
                FzfOption {
                    prompt: "Select Hooks".to_string(),
                    select_mode: SelectMode::multi(current),
                    callback: Box::new(|selected| {
                        if let Some(names) = selected {
                            let win_arc = get_chat_window();
                            if let Ok(win) = win_arc.lock()
                                && let Ok(loaded) = win.loaded_chat_session.read()
                                && let Ok(session) = loaded.read()
                                && let Ok(mut active) = session.active_hooks.write()
                            {
                                active.clear();
                                for label in names {
                                    // Label is `type | name`; store the plain name.
                                    let name = label.split_once(" | ").map(|(_, n)| n);
                                    if let Some(name) = name {
                                        active.insert(name.to_string());
                                    }
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
