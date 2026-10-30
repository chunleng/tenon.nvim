use nvim_oxi::{Function, api::types::LogLevel};

use crate::{
    clients::SelectedModel,
    get_chat_window,
    ui::picker::{FzfOption, SelectMode, box_single_select, pick},
    utils::GLOBAL_EXECUTION_HANDLER,
};

/// A parameter with configurable options and its (display, value) choices.
#[derive(Clone)]
struct ParameterChoice {
    name: String,
    options: Vec<(String, serde_json::Value)>,
}

/// Collects the current model's parameters that declare options, sorted by name.
fn configurable_parameters(model: &SelectedModel) -> Vec<ParameterChoice> {
    let mut entries: Vec<ParameterChoice> = model
        .definition()
        .config
        .parameters
        .iter()
        .filter_map(|(name, parameter)| {
            let options = parameter
                .options
                .as_ref()
                .filter(|options| !options.is_empty())?
                .iter()
                .map(|value| {
                    let display = match value {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    (display, value.clone())
                })
                .collect();
            Some(ParameterChoice {
                name: name.clone(),
                options,
            })
        })
        .collect();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    entries
}

/// Reads the session's runtime value for a parameter and maps it to the matching option display, so
/// the picker can mark it as the current value.
fn current_option_display(entry: &ParameterChoice) -> Option<String> {
    let win_arc = get_chat_window();
    let win = win_arc.lock().ok()?;
    let loaded = win.loaded_chat_session.read().ok()?;
    let session = loaded.read().ok()?;
    let current = session.engine.model.parameters.get(&entry.name)?;
    entry
        .options
        .iter()
        .find(|(_, value)| value == current)
        .map(|(display, _)| display.clone())
}

/// Writes a parameter's runtime value on the session state. The next request picks it up; persisted
/// user config is untouched.
fn set_parameter(name: &str, value: serde_json::Value) {
    let win_arc = get_chat_window();
    if let Ok(win) = win_arc.lock()
        && let Ok(loaded) = win.loaded_chat_session.read()
        && let Ok(mut session) = loaded.write()
    {
        session
            .engine
            .model
            .parameters
            .insert(name.to_string(), value);
        win.force_render();
    }
}

/// Runs one current-selectable picker per parameter, in sequence: picking a
/// value (or canceling) moves on to the next parameter.
fn run_parameter_pickers(entries: Vec<ParameterChoice>) {
    let mut entries = entries;
    let Some(entry) = entries.first().cloned() else {
        return;
    };
    entries.remove(0);
    let remaining = entries;

    let current_display = current_option_display(&entry);

    let option_displays: Vec<String> = entry.options.iter().map(|(d, _)| d.clone()).collect();
    let options: Vec<&str> = option_displays.iter().map(|s| s.as_str()).collect();

    if let Err(e) = pick(
        &options,
        FzfOption {
            prompt: format!("Set {}", entry.name),
            select_mode: SelectMode::single(current_display, true),
            callback: box_single_select(move |selected| {
                if let Some(display) = selected
                    && let Some((_, value)) = entry.options.iter().find(|(d, _)| *d == display)
                {
                    set_parameter(&entry.name, value.clone());
                }
                if !remaining.is_empty() {
                    run_parameter_pickers(remaining);
                }
            }),
            ..Default::default()
        },
    ) {
        GLOBAL_EXECUTION_HANDLER
            .notify_on_main_thread(format!("picker error: {}", e), LogLevel::Error);
    }
}

/// Show a picker per configurable parameter of the current model's provider,
/// letting the user change each parameter's value.
pub fn select_parameters_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            let model: Option<SelectedModel> = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                let loaded = win.loaded_chat_session.read().ok()?;
                let session = loaded.read().ok()?;
                Some(session.engine.model.clone())
            })();

            let Some(model) = model else {
                GLOBAL_EXECUTION_HANDLER
                    .notify_on_main_thread("no active chat session".to_string(), LogLevel::Error);
                return;
            };

            let entries = configurable_parameters(&model);
            if entries.is_empty() {
                GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                    format!("No configurable parameters for model {}", model.id),
                    LogLevel::Error,
                );
                return;
            }

            run_parameter_pickers(entries);
        }
    })
}
