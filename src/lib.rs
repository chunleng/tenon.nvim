use nvim_oxi::{Dictionary, Function, Object};

use crate::chat::Chat;

mod chat;

#[nvim_oxi::plugin]
fn omnidash() -> Dictionary {
    let prompt = Function::from_fn(Chat::send_message);

    Dictionary::from_iter([("prompt", Object::from(prompt))])
}
