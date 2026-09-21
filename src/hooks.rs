use std::process::Command;

use nvim_oxi::api::types::LogLevel;

/// When a hook fires.
///
/// `NeedsAttention`: the streaming loop ended (clean finish, cancel, or
/// error) or the agent is waiting for a user response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookType {
    NeedsAttention,
}

#[derive(Debug, Clone)]
pub struct Hook {
    pub name: String,
    pub command: String,
    pub enabled: bool,
    pub hook_type: HookType,
}

impl Hook {
    /// Runs the hook command via `bash -c` on a background thread.
    ///
    /// Non-blocking. On spawn failure or non-zero exit, notifies the user
    /// with the hook name and error. The chat title is exposed to the
    /// command as the `TENON_CHAT_TITLE` environment variable.
    pub fn run(&self, chat_title: &str) {
        let name = self.name.clone();
        let command = self.command.clone();
        let chat_title = if chat_title.is_empty() {
            "Untitled".to_string()
        } else {
            chat_title.to_string()
        };
        std::thread::spawn(move || {
            match Command::new("bash")
                .arg("-c")
                .arg(&command)
                .env("TENON_CHAT_TITLE", &chat_title)
                .output()
            {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    crate::utils::notify(
                        format!("Hook '{}' failed: {}", name, stderr),
                        LogLevel::Error,
                    );
                }
                Err(e) => {
                    crate::utils::notify(
                        format!("Hook '{}' failed to spawn: {}", name, e),
                        LogLevel::Error,
                    );
                }
            }
        });
    }
}

/// Runs every `NeedsAttention` hook whose name is in `active`.
pub fn run_needs_attention_hooks(
    hooks: &[Hook],
    active: &std::collections::HashSet<String>,
    chat_title: &str,
) {
    for hook in hooks {
        if active.contains(&hook.name) && hook.hook_type == HookType::NeedsAttention {
            hook.run(chat_title);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait_for_marker(path: &std::path::Path) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(content) = std::fs::read_to_string(path) {
                return content;
            }
            assert!(Instant::now() < deadline, "hook did not run in time");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn run_executes_command_with_shell_semantics() {
        let dir = std::env::temp_dir().join(format!("tenon_hook_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("done");

        let hook = Hook {
            name: "test".to_string(),
            command: format!("echo hello > {}", marker.to_str().unwrap()),
            enabled: true,
            hook_type: HookType::NeedsAttention,
        };
        hook.run("");

        // `>` redirection only works if the command went through bash -c
        let content = wait_for_marker(&marker);
        assert_eq!(content.trim(), "hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_exposes_chat_title_as_env_var() {
        let dir = std::env::temp_dir().join(format!("tenon_hook_env_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("title");

        let hook = Hook {
            name: "test".to_string(),
            command: format!("echo $TENON_CHAT_TITLE > {}", marker.to_str().unwrap()),
            enabled: true,
            hook_type: HookType::NeedsAttention,
        };
        hook.run("My Chat Title");

        let content = wait_for_marker(&marker);
        assert_eq!(content.trim(), "My Chat Title");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_uses_untitled_when_chat_title_is_empty() {
        let dir =
            std::env::temp_dir().join(format!("tenon_hook_env_untitled_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("title");

        let hook = Hook {
            name: "test".to_string(),
            command: format!("echo $TENON_CHAT_TITLE > {}", marker.to_str().unwrap()),
            enabled: true,
            hook_type: HookType::NeedsAttention,
        };
        hook.run("");

        let content = wait_for_marker(&marker);
        assert_eq!(content.trim(), "Untitled");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_needs_attention_hooks_runs_only_active_hooks() {
        let dir = std::env::temp_dir().join(format!("tenon_hook_filter_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let on_marker = dir.join("on");
        let off_marker = dir.join("off");

        let hooks = vec![
            Hook {
                name: "on".to_string(),
                command: format!("touch {}", on_marker.to_str().unwrap()),
                enabled: true,
                hook_type: HookType::NeedsAttention,
            },
            Hook {
                name: "off".to_string(),
                command: format!("touch {}", off_marker.to_str().unwrap()),
                enabled: true,
                hook_type: HookType::NeedsAttention,
            },
        ];
        let active = ["on".to_string()].into_iter().collect();

        run_needs_attention_hooks(&hooks, &active, "");

        wait_for_marker(&on_marker);
        std::thread::sleep(Duration::from_millis(100));
        assert!(!off_marker.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
