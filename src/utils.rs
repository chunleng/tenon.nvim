use nvim_oxi::api::{self, types::LogLevel};

/// A wrapper for vim.notify that properly handles long lines and multiline messages
///
/// This uses Lua's vim.notify which:
/// - Respects user's notification manager (nvim-notify, noice.nvim, etc.)
/// - Properly handles long lines and multiline messages
/// - Supports log levels with appropriate highlighting
pub fn notify(message: impl ToString, log_level: LogLevel) {
    let msg = message.to_string();

    // Map nvim-oxi LogLevel to Lua vim.log.levels
    let lua_level = match log_level {
        LogLevel::Error => "vim.log.levels.ERROR",
        LogLevel::Warn => "vim.log.levels.WARN",
        LogLevel::Info => "vim.log.levels.INFO",
        LogLevel::Debug => "vim.log.levels.DEBUG",
        _ => "vim.log.levels.INFO", // Default to INFO for any other log levels
    };

    // Escape the message for Lua string literal
    let escaped = msg
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('"', "\\\"");

    let lua_code = format!("lua vim.notify(\"{}\", {})", escaped, lua_level);

    // Execute using command
    let _ = api::command(&lua_code);
}
