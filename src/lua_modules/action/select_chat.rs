use nvim_oxi::{Function, api::types::LogLevel};

use crate::{
    chat::CHAT_SESSIONS,
    get_chat_window,
    ui::picker::{FzfOption, SelectMode, box_single_select, pick},
    utils::{GLOBAL_EXECUTION_HANDLER, notify},
};

/// Show a picker to select and load a chat session.
pub fn select_chat_fn() -> Function<(), ()> {
    Function::from_fn({
        move |()| {
            // Get current chat index on main thread
            let current_index = (|| {
                let win_arc = get_chat_window();
                let win = win_arc.lock().ok()?;
                Some(
                    win.loaded_chat_index
                        .load(std::sync::atomic::Ordering::SeqCst),
                )
            })()
            .unwrap_or(0);

            // Spawn thread to read sessions and show picker
            std::thread::spawn(move || {
                let sessions = CHAT_SESSIONS.lock().unwrap();
                if sessions.is_empty() {
                    GLOBAL_EXECUTION_HANDLER
                        .notify_on_main_thread("no chat sessions", LogLevel::Warn);
                    return;
                }

                // Build display options: "Chat N | Title", with a spinner icon for chats still processing
                let options: Vec<String> = sessions
                    .iter()
                    .enumerate()
                    .map(|(i, session)| {
                        let guard = session.read().unwrap();
                        let title = guard
                            .title_handler
                            .title()
                            .unwrap_or("Untitled".to_string());
                        let processing = if guard.is_processing() {
                            " \u{F110}"
                        } else {
                            ""
                        };
                        format!("Chat {}{} | {}", i + 1, processing, title)
                    })
                    .collect();

                let options_clone = options.clone();
                let options_refs: Vec<&str> = options.iter().map(|s| s.as_str()).collect();
                let current_selection = options.get(current_index).map(|s| s.as_str());

                if let Err(e) = pick(
                    &options_refs,
                    FzfOption {
                        prompt: "Select Chat".to_string(),
                        select_mode: SelectMode::single(current_selection, false),
                        callback: box_single_select(move |selected| {
                            if let Some(selection) = selected {
                                let idx = options_clone.iter().position(|s| *s == selection);
                                if let Some(idx) = idx
                                    && let Err(e) = GLOBAL_EXECUTION_HANDLER
                                        .execute_rust_on_main_thread(move || {
                                            let win_arc = get_chat_window();
                                            if let Ok(mut win) = win_arc.lock()
                                                && let Err(e) = win.load_chat(idx)
                                            {
                                                notify(
                                                    format!("failed to load chat: {}", e),
                                                    LogLevel::Error,
                                                );
                                            }
                                            Ok(())
                                        })
                                {
                                    GLOBAL_EXECUTION_HANDLER.notify_on_main_thread(
                                        format!("failed to load chat: {}", e),
                                        LogLevel::Error,
                                    );
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
