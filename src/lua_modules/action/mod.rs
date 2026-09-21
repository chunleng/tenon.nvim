mod continue_chat;
mod dismiss_chat;
mod insert_selection;
mod new_chat;
mod next_chat;
mod prev_chat;
mod rename;
mod select_agent;
mod select_chat;
mod select_history;
mod select_hooks;
mod select_model;
mod select_tools;
mod send;
mod show_chat;
mod show_detail;
mod stop_streaming;
mod toggle_focus;

use nvim_oxi::{Dictionary, Object};

pub fn create_lua_action_module() -> Dictionary {
    let mut action_dict = Dictionary::new();
    action_dict.insert(
        "insert_selection",
        Object::from(insert_selection::insert_selection_fn()),
    );
    action_dict.insert("select_chat", Object::from(select_chat::select_chat_fn()));
    action_dict.insert(
        "continue_chat",
        Object::from(continue_chat::continue_chat_fn()),
    );
    action_dict.insert("show_detail", Object::from(show_detail::show_detail_fn()));
    action_dict.insert("show_chat", Object::from(show_chat::show_chat_fn()));
    action_dict.insert("send", Object::from(send::send_fn()));
    action_dict.insert("next_chat", Object::from(next_chat::next_chat_fn()));
    action_dict.insert("prev_chat", Object::from(prev_chat::prev_chat_fn()));
    action_dict.insert("new_chat", Object::from(new_chat::new_chat_fn()));
    action_dict.insert(
        "dismiss_chat",
        Object::from(dismiss_chat::dismiss_chat_fn()),
    );
    action_dict.insert(
        "stop_streaming",
        Object::from(stop_streaming::stop_streaming_fn()),
    );
    action_dict.insert(
        "select_agent",
        Object::from(select_agent::select_agent_fn()),
    );
    action_dict.insert(
        "select_model",
        Object::from(select_model::select_model_fn()),
    );
    action_dict.insert(
        "select_tools",
        Object::from(select_tools::select_tools_fn()),
    );
    action_dict.insert(
        "toggle_focus",
        Object::from(toggle_focus::toggle_focus_fn()),
    );
    action_dict.insert(
        "select_history",
        Object::from(select_history::select_history_fn()),
    );
    action_dict.insert(
        "select_hooks",
        Object::from(select_hooks::select_hooks_fn()),
    );
    action_dict.insert("rename", Object::from(rename::rename_fn()));
    action_dict
}
