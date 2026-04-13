use std::sync::{
    LazyLock,
    mpsc::{self, Sender},
};

use nvim_oxi::{
    Result as OxiResult,
    api::{self, types::LogLevel},
    libuv::AsyncHandle,
    mlua::lua,
    schedule,
};
use serde_json::Value;

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

pub static GLOBAL_EXECUTION_HANDLER: LazyLock<NeovimExecutionHandler> =
    LazyLock::new(|| NeovimExecutionHandler::new());

#[derive(Clone)]
pub struct NeovimExecutionHandler {
    handle: AsyncHandle,
    sender: Sender<(String, Sender<String>)>,
}

impl NeovimExecutionHandler {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<(String, Sender<String>)>();
        Self {
            handle: AsyncHandle::new(move || {
                if let Ok((data, tx)) = rx.recv() {
                    schedule(move |_| {
                        let res = lua().load(data.trim()).eval::<mlua::Value>();
                        match res {
                            Ok(x) => {
                                if let Ok(serialized) = serde_json::to_string(&x) {
                                    let _ = tx.send(serialized);
                                }
                            }
                            Err(e) => {
                                notify(format!("{:?}", e), LogLevel::Error);
                            }
                        }
                    });
                }
            })
            .unwrap(),
            sender: tx,
        }
    }

    pub fn execute_on_main_thread(&self, lua_code: &str) -> OxiResult<Value> {
        let (tx, rx) = mpsc::channel::<String>();

        self.sender.send((lua_code.to_string(), tx)).unwrap();
        self.handle.send()?;

        rx.recv()
            .map_err(|e| nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(e.to_string())))
            .and_then(|json_str| {
                serde_json::from_str::<Value>(&json_str).map_err(|e| {
                    nvim_oxi::Error::Mlua(mlua::Error::RuntimeError(format!(
                        "Failed to parse JSON: {}",
                        e
                    )))
                })
            })
    }
}
