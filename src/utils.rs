use nvim_oxi::{
    Dictionary,
    api::{self, types::LogLevel},
};

/// A wrapper for [nvim_oxi::api::notify]
///
/// This wrapper is created as [nvim_oxi::api::notify] has problem when being passed multi line str
pub fn notify(message: impl ToString, log_level: LogLevel) {
    let lines = message
        .to_string()
        .lines()
        .map(|x| x.to_string())
        .collect::<Vec<String>>();
    for line in lines {
        let _ = api::notify(&line, log_level, &Dictionary::new());
    }
}
