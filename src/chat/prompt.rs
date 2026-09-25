use crate::chat::{ActiveChoreo, WorkQueue};
use rig::prelude::Message;
use std::sync::{Arc, RwLock};

/// Builds messages for a turn: work queue and choreo contexts as their own
/// system messages, followed by the base prompt as a user message (if non-empty).
pub async fn build_choreo_messages(
    active_choreo: &Arc<RwLock<Option<ActiveChoreo>>>,
    work_queue: &Arc<RwLock<WorkQueue>>,
    base_prompt: String,
) -> Vec<Message> {
    let queue_section = work_queue
        .read()
        .ok()
        .and_then(|queue| queue.render_context());

    let mut messages: Vec<Message> = Vec::new();

    if let Some(section) = queue_section {
        messages.push(Message::system(format!(
            "<context type=\"work_queue\">\n{}\n</context>\n",
            section
        )));
    }

    if let Some(choreo_context) = build_choreo_context(active_choreo).await {
        messages.push(Message::system(choreo_context));
    }

    if !base_prompt.is_empty() {
        messages.push(Message::user(base_prompt));
    }

    messages
}

/// Renders the active choreo's context tag, if any.
async fn build_choreo_context(active_choreo: &Arc<RwLock<Option<ActiveChoreo>>>) -> Option<String> {
    let mut contexts: Vec<String> = Vec::new();

    if let Ok(active_lock) = active_choreo.read()
        && let Some(active) = active_lock.as_ref()
    {
        let choreo = &active.choreo;
        let total_moves = choreo.moves.len();
        if let Some(current_move) = choreo.moves.get(active.r#move - 1) {
            let mut goto_lines: Vec<String> = current_move
                .goto_instructions
                .iter()
                .map(|instr| {
                    let condition = instr
                        .condition
                        .as_ref()
                        .map(|x| format!("{} → ", x))
                        .unwrap_or_default();
                    let target_move = instr.to.resolve_move_index(active.r#move);
                    match target_move {
                        None => format!("{}end_choreo", condition),
                        Some(move_number) if move_number > total_moves => {
                            format!("{}end_choreo", condition)
                        }
                        Some(move_number) => {
                            format!("{}navigate_choreo move:{}", condition, move_number)
                        }
                    }
                })
                .collect();

            // Add default navigation only when no goto already covers it:
            // end the choreo at the last move, otherwise advance to the next move
            if active.r#move == total_moves {
                let has_ending_goto = current_move.goto_instructions.iter().any(|instr| {
                    let target_move = instr.to.resolve_move_index(active.r#move);
                    match target_move {
                        None => true,
                        Some(m) => m > total_moves,
                    }
                });
                if !has_ending_goto {
                    goto_lines.push("end_choreo".to_string());
                }
            } else {
                let next_move = active.r#move + 1;
                let has_next_move_goto = current_move
                    .goto_instructions
                    .iter()
                    .any(|instr| instr.to.resolve_move_index(active.r#move) == Some(next_move));
                if !has_next_move_goto {
                    goto_lines.push(format!("navigate_choreo move:{next_move}"));
                }
            }

            let goto_instruction = goto_lines.join("\n");

            // Build memory section if there's stored memory
            let memory_section = if active.memory.is_empty() {
                String::new()
            } else {
                let memory_entries: Vec<String> = active
                    .memory
                    .iter()
                    .map(|(k, v)| format!("<memory name=\"{}\">{}</memory>", k, v))
                    .collect();
                memory_entries.join("\n")
            };

            contexts.push(format!(
                "<context type=\"choreo-state\">\n\
                    Currently in \"{}\" move of {} choreo.\n\
                    1. Execute the \"Process\" section of the `instruction` tag. If its steps are numbered, run them in order, completing all of them within this turn; do not stop partway unless the user explicitly asks.\n\
                    2. Navigate only after all \"Process\" steps are done, unless the instruction explicitly says to navigate earlier. Reference and select from the `navigation` tag: call the tool whose condition matches; use an unconditioned entry only when no conditioned entry matches.\n\
                    3. If the instruction contains a \"Choreo Move Artifact\" section, produce the artifact it specifies for the navigation target you are calling, and pass it in the `move_artifact` field of `navigate_choreo` or `end_choreo`. If the section is absent, omit the field.\n\
                    \n\n\
                    {}\
                    <instruction>\n\
                    {}\n\
                    </instruction>\n\
                    <navigation>\n\
                    {}\n\
                    </navigation>\n\
                    </context>\n",
                current_move.title,
                choreo.title,
                memory_section,
                current_move.instruction.resolve().unwrap_or_default(),
                goto_instruction
            ));
        }
    }

    if contexts.is_empty() {
        return None;
    }
    Some(contexts.join(""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::message::UserContent;
    use std::collections::HashMap;

    fn empty_queue() -> Arc<RwLock<WorkQueue>> {
        Arc::new(RwLock::new(WorkQueue::default()))
    }

    fn message_text(message: &Message) -> String {
        match message {
            Message::System { content } => content.clone(),
            Message::User { content } => content
                .iter()
                .filter_map(|c| match c {
                    UserContent::Text(text) => Some(text.text.clone()),
                    _ => None,
                })
                .collect(),
            _ => String::new(),
        }
    }

    fn assert_system(message: &Message, expected: &str) {
        assert!(
            matches!(message, Message::System { .. }),
            "expected system message, got: {message:?}"
        );
        let text = message_text(message);
        assert!(text.contains(expected), "missing `{expected}` in: {text}");
    }

    fn assert_user(message: &Message, expected: &str) {
        assert!(
            matches!(message, Message::User { .. }),
            "expected user message, got: {message:?}"
        );
        assert_eq!(message_text(message), expected);
    }

    #[tokio::test]
    async fn test_build_choreo_messages_displays_memory() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();

        let active = Arc::new(RwLock::new(Some(ActiveChoreo {
            choreo,
            r#move: 1,
            memory: {
                let mut m = HashMap::new();
                m.insert("previous_output".to_string(), "test result".to_string());
                m
            },
        })));

        let messages =
            build_choreo_messages(&active, &empty_queue(), "user input".to_string()).await;

        assert_eq!(messages.len(), 2);
        assert_system(&messages[0], "<context type=\"choreo-state\">");
        let choreo_text = message_text(&messages[0]);
        assert!(choreo_text.contains("<memory name=\"previous_output\">"));
        assert!(choreo_text.contains("test result"));
        assert!(choreo_text.contains("</memory>"));
        assert_user(&messages[1], "user input");
    }

    #[tokio::test]
    async fn test_build_choreo_messages_no_choreos() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let active = Arc::new(RwLock::new(None));
        let messages =
            build_choreo_messages(&active, &empty_queue(), "user input".to_string()).await;
        assert_eq!(messages.len(), 1);
        assert_user(&messages[0], "user input");
    }

    #[tokio::test]
    async fn test_build_choreo_messages_empty_base_prompt_no_choreos() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let active = Arc::new(RwLock::new(None));
        let messages = build_choreo_messages(&active, &empty_queue(), String::new()).await;
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_build_choreo_messages_injects_work_queue_without_choreo() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let active = Arc::new(RwLock::new(None));
        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        queue.write().unwrap().push(
            "refactor".to_string(),
            "fix X".to_string(),
            "long X".to_string(),
        );

        let messages = build_choreo_messages(&active, &queue, "user input".to_string()).await;

        assert_eq!(messages.len(), 2);
        assert_system(&messages[0], "<context type=\"work_queue\">");
        let queue_text = message_text(&messages[0]);
        assert!(queue_text.contains("<work_queue>"));
        assert!(queue_text.contains("refactor: fix X"));
        assert!(queue_text.contains("</work_queue>"));
        assert_user(&messages[1], "user input");
    }

    #[tokio::test]
    async fn test_build_choreo_messages_injects_work_queue_with_choreo() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();

        let active = Arc::new(RwLock::new(Some(ActiveChoreo {
            choreo,
            r#move: 1,
            memory: HashMap::new(),
        })));

        let queue = Arc::new(RwLock::new(WorkQueue::default()));
        queue.write().unwrap().push(
            "refactor".to_string(),
            "fix X".to_string(),
            "long X".to_string(),
        );

        let messages = build_choreo_messages(&active, &queue, "user input".to_string()).await;

        assert_eq!(messages.len(), 3);
        // Work queue context first, then choreo context, then the user message
        assert_system(&messages[0], "<context type=\"work_queue\">");
        assert!(message_text(&messages[0]).contains("refactor: fix X"));
        assert_system(&messages[1], "<context type=\"choreo-state\">");
        assert_user(&messages[2], "user input");
    }

    #[tokio::test]
    async fn test_build_choreo_messages_navigates_to_next_move() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();

        let active = Arc::new(RwLock::new(Some(ActiveChoreo {
            choreo,
            r#move: 1,
            memory: HashMap::new(),
        })));

        let messages =
            build_choreo_messages(&active, &empty_queue(), "user input".to_string()).await;

        let choreo_text = message_text(&messages[0]);
        assert_eq!(
            choreo_text.matches("navigate_choreo move:2").count(),
            1,
            "explicit next-move goto must not be duplicated by the fallback: {choreo_text}"
        );
    }

    #[tokio::test]
    async fn test_build_choreo_messages_adds_default_next_move_fallback() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();

        // Move 5 of 6 only has a conditional goto back to move 2, so the
        // default next-move navigation must be appended.
        let active = Arc::new(RwLock::new(Some(ActiveChoreo {
            choreo,
            r#move: 5,
            memory: HashMap::new(),
        })));

        let messages =
            build_choreo_messages(&active, &empty_queue(), "user input".to_string()).await;

        assert_system(&messages[0], "navigate_choreo move:6");
    }

    #[tokio::test]
    async fn test_build_choreo_messages_ends_at_final_move() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let registry = crate::get_choreo_registry();
        let choreo = registry.get("implement_code").unwrap().clone();

        let active = Arc::new(RwLock::new(Some(ActiveChoreo {
            choreo,
            r#move: 6,
            memory: HashMap::new(),
        })));

        let messages =
            build_choreo_messages(&active, &empty_queue(), "user input".to_string()).await;

        assert_system(&messages[0], "end_choreo");
    }
}
