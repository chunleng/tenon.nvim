use crate::chat::{ActiveChoreo, WorkQueue};
use std::sync::{Arc, RwLock};

/// Builds a choreo-wrapped prompt if there's an active choreo.
/// Queued work queue tasks are injected into their own context tag in both cases.
pub async fn build_choreo_prompt(
    active_choreo: &Arc<RwLock<Option<ActiveChoreo>>>,
    work_queue: &Arc<RwLock<WorkQueue>>,
    base_prompt: String,
) -> String {
    let queue_section = work_queue
        .read()
        .ok()
        .and_then(|queue| queue.render_context());

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

            // Only add default ending if at last move and no goto already ends choreo
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
                "<context type=\"choreo\">\n\
                    Currently in {} move of {} choreo.\n\
                    Execute \"Process\" in `instruction` tag step by step if numbered, not all at once; don't stop partway unless explicitly asked. \
                    Call a tool from `navigation` tag when the condition matches, or choose the best one when all steps in \"Process\" are finished, using artifact from the \"Choreo Move Artifact\" section, if available\n\
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

    if let Some(section) = queue_section {
        contexts.push(format!(
            "<context type=\"work_queue\">\n{}\n</context>\n",
            section
        ));
    }

    format!("{}{}", contexts.join(""), base_prompt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn empty_queue() -> Arc<RwLock<WorkQueue>> {
        Arc::new(RwLock::new(WorkQueue::default()))
    }

    #[tokio::test]
    async fn test_build_choreo_prompt_displays_memory() {
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

        let prompt = build_choreo_prompt(&active, &empty_queue(), "user input".to_string()).await;

        assert!(prompt.contains("<memory name=\"previous_output\">"));
        assert!(prompt.contains("test result"));
        assert!(prompt.contains("</memory>"));
    }

    #[tokio::test]
    async fn test_build_choreo_prompt_no_choreos() {
        crate::utils::PLUGIN_ROOT
            .set(std::env::current_dir().unwrap())
            .ok();

        let active = Arc::new(RwLock::new(None));
        let prompt = build_choreo_prompt(&active, &empty_queue(), "user input".to_string()).await;
        assert_eq!(prompt, "user input");
        assert!(!prompt.contains("<context>"));
    }

    #[tokio::test]
    async fn test_build_choreo_prompt_injects_work_queue_without_choreo() {
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

        let prompt = build_choreo_prompt(&active, &queue, "user input".to_string()).await;

        assert!(prompt.contains("<work_queue>"));
        assert!(prompt.contains("refactor: fix X"));
        assert!(prompt.contains("</work_queue>"));
        assert!(prompt.starts_with("<context type=\"work_queue\">"));
        assert!(prompt.ends_with("user input"));
    }

    #[tokio::test]
    async fn test_build_choreo_prompt_injects_work_queue_with_choreo() {
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

        let prompt = build_choreo_prompt(&active, &queue, "user input".to_string()).await;

        assert!(prompt.contains("<work_queue>"));
        assert!(prompt.contains("refactor: fix X"));
        // Queue gets its own context tag, placed after the choreo context tag
        assert!(prompt.contains("<context type=\"choreo\">"));
        let choreo_context_start = prompt.find("<context type=\"choreo\">").unwrap();
        let choreo_context_end = prompt.find("</context>").unwrap();
        let queue_context_start = prompt.find("<context type=\"work_queue\">").unwrap();
        assert!(choreo_context_start < choreo_context_end);
        assert!(choreo_context_end < queue_context_start);
    }

    #[tokio::test]
    async fn test_build_choreo_prompt_navigates_to_next_move() {
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

        let prompt = build_choreo_prompt(&active, &empty_queue(), "user input".to_string()).await;

        assert!(
            prompt.contains("navigate_choreo move:2"),
            "goto to next move should generate navigate_choreo line, got: {prompt}"
        );
    }

    #[tokio::test]
    async fn test_build_choreo_prompt_ends_at_final_move() {
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

        let prompt = build_choreo_prompt(&active, &empty_queue(), "user input".to_string()).await;

        assert!(
            prompt.contains("end_choreo"),
            "EndChoreo goto should generate end_choreo line, got: {prompt}"
        );
    }
}
