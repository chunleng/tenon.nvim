use std::sync::{Arc, RwLock};

use crate::chat::log::TenonLogData;
use crate::chat::log_indexer::ChatLogIndexer;
use crate::clients::get_agent;
use crate::directive::{Directive, DirectiveSource};
use crate::get_application_config;

/// Handles title generation for chat sessions.
///
/// Owns the title state and a reference to the log indexer to read
/// the first user message. No cancellation token — generation is
/// guarded by checking `is_generating()` and whether a title exists.
pub struct TitleHandler {
    pub title: Arc<RwLock<Option<String>>>,
    log_indexer: Arc<RwLock<ChatLogIndexer>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl TitleHandler {
    pub fn new(log_indexer: Arc<RwLock<ChatLogIndexer>>) -> Self {
        Self {
            title: Arc::new(RwLock::new(None)),
            log_indexer,
            thread: None,
        }
    }

    pub fn from_history(title: Option<String>, log_indexer: Arc<RwLock<ChatLogIndexer>>) -> Self {
        Self {
            title: Arc::new(RwLock::new(title)),
            log_indexer,
            thread: None,
        }
    }

    pub fn is_generating(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }

    /// Generates a title from the first user message in the log indexer.
    /// Early-returns if a title is already set or generation is in progress.
    pub fn generate_title(&mut self) {
        if self.title.read().map(|t| t.is_some()).unwrap_or(false) {
            return;
        }

        if self.is_generating() {
            return;
        }

        let first_message = match self.log_indexer.read() {
            Ok(indexer) => indexer.logs.iter().find_map(|indexed| {
                if let TenonLogData::User(crate::chat::TenonUserMessage::Text(msg)) =
                    indexed.log.data()
                {
                    Some(msg.0.clone())
                } else {
                    None
                }
            }),
            Err(_) => None,
        };

        let first_message = match first_message {
            Some(msg) => msg,
            None => return,
        };

        let config = get_application_config();

        let model = config.title.model.clone().or_else(|| {
            config
                .agents
                .get(&config.default_agent)
                .map(|a| a.model.clone())
        });

        let model = match model {
            Some(m) => m,
            None => return,
        };

        let title = Arc::clone(&self.title);

        self.thread = Some(std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async move {
                let directive = vec![Directive {
                    condition: None,
                    source: DirectiveSource::Text {
                        value: config.title.prompt.clone().unwrap_or_else(|| {
                            "Title generation\n
                            - Output title only\n
                            - 2-6 words\n
                            - Output \"Untitled\" when not enough context to form meaningful title\n\n
                            Example:\n
                            - prompt: Fix login bug in auth module\n
                              reply: Login bug fix\n
                            - prompt: Fix bug, add features for task manager\n
                              reply: Task manager code change\n
                            - prompt: Hey yo!\n
                              reply: Untitled\n
                            - prompt: X\n
                              reply: Untitled\n"
                                .to_string()
                        }),
                    },
                }];

                let agent = get_agent(model, directive, vec![], false);

                match agent
                    .chat(format!("Generate title:\n```\n{}\n```", first_message))
                    .await
                {
                    Ok(generated) => {
                        let trimmed = generated.trim();
                        if !trimmed.is_empty()
                            && let Ok(mut t) = title.write()
                        {
                            *t = Some(
                                trimmed
                                    .lines()
                                    .collect::<Vec<_>>()
                                    .first()
                                    .map(|x| x.to_string())
                                    .unwrap_or("Untitled".to_string()),
                            );
                        }
                    }
                    Err(e) => {
                        eprintln!("[tenon] Failed to generate title: {}", e);
                    }
                }
            });
        }));
    }
}
