use nvim_oxi::{Function, api::types::LogLevel};

use crate::constants::special_chars::{ASSISTANT, VERTICAL_SEPARATOR};
use crate::{
    get_application_config, get_chat_window,
    ui::picker::{FzfOption, box_single_select, pick},
    utils::{GLOBAL_EXECUTION_HANDLER, notify},
};

/// Show a picker to load a chat from history.
pub fn select_history_fn() -> Function<(), ()> {
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
                        let messages = h.logs.len();
                        format!(
                            "{} {VERTICAL_SEPARATOR} {:>3} msg {VERTICAL_SEPARATOR} {} ({ASSISTANT} {}, {})",
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
