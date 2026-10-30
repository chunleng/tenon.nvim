use nvim_oxi::{Function, api::types::LogLevel};

use crate::{
    clients::{SelectedModel, SupportedModels},
    get_application_config, get_chat_window,
    ui::picker::{FzfOption, SelectMode, box_single_select, pick},
    utils::GLOBAL_EXECUTION_HANDLER,
};

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

/// Show a picker to select the model for the current session.
pub fn select_model_fn() -> Function<(), ()> {
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
                let current = &session.engine.model;
                entries.iter().find_map(|(name, m)| {
                    (name == &current.id).then(|| format_model_display(name, m))
                })
            })();

            let options: Vec<&str> = model_list.iter().map(|s| s.as_str()).collect();

            if let Err(e) = pick(
                &options,
                FzfOption {
                    prompt: "Select Model".to_string(),
                    select_mode: SelectMode::single(current_model_display, false),
                    callback: box_single_select(move |selected| {
                        if let Some(display) = selected
                            && let Some((name, _)) = entries
                                .iter()
                                .find(|(name, m)| format_model_display(name, m) == display)
                        {
                            let win_arc = get_chat_window();
                            if let Ok(win) = win_arc.lock()
                                && let Ok(loaded) = win.loaded_chat_session.read()
                                && let Ok(mut session) = loaded.write()
                            {
                                session.engine.model = SelectedModel::new(name.clone());
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
