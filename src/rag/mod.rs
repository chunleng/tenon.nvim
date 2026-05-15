mod embedding;

pub use embedding::{find_top_k_similar, generate_embeddings};

use std::sync::{Arc, RwLock};

use crate::chat::log::TenonLog;

/// RAG context holder - encapsulates logs and embeddings for retrieval-augmented generation.
#[derive(Clone)]
pub struct RagContext {
    logs: Arc<RwLock<Vec<TenonLog>>>,
    embeddings: Arc<RwLock<Option<Vec<Vec<f64>>>>>,
}

impl RagContext {
    pub fn new() -> Self {
        Self {
            logs: Arc::new(RwLock::new(Vec::new())),
            embeddings: Arc::new(RwLock::new(None)),
        }
    }

    /// Add logs to the RAG context (called during truncation)
    pub fn add_logs(&self, new_logs: Vec<TenonLog>) {
        if let Ok(mut logs) = self.logs.write() {
            for log in new_logs {
                logs.push(log);
            }
        }
    }

    /// Gets cached embeddings or generates new ones for the given logs.
    /// Incrementally generates embeddings only for logs that don't have them cached.
    fn get_or_generate_embeddings(&self) -> Option<Vec<Vec<f64>>> {
        let logs = self.logs.read().ok()?;
        let cached_len = self
            .embeddings
            .read()
            .ok()?
            .as_ref()
            .map(|c| c.len())
            .unwrap_or(0);

        // Check if cache has all embeddings
        if cached_len == logs.len() {
            return self.embeddings.read().ok()?.as_ref().cloned();
        }

        // Cache is missing some embeddings - generate only for new logs
        if cached_len < logs.len() {
            // Generate embeddings for logs that don't have them yet
            let new_texts: Vec<_> = logs[cached_len..]
                .iter()
                .filter_map(|log| log.to_embeddable_text())
                .collect();

            let new_embeddings = match generate_embeddings(&new_texts) {
                Ok(embeddings) => embeddings,
                Err(_) => {
                    return None;
                }
            };

            // Append to existing cache
            if let Ok(mut lock) = self.embeddings.write() {
                match lock.as_mut() {
                    Some(existing) => existing.extend(new_embeddings),
                    None => *lock = Some(new_embeddings),
                }
            }

            return self.embeddings.read().ok()?.as_ref().cloned();
        }

        // Cache has more embeddings than logs (shouldn't happen, but regenerate to be safe)
        let texts: Vec<_> = logs
            .iter()
            .filter_map(|log| log.to_embeddable_text())
            .collect();

        let embeddings = match generate_embeddings(&texts) {
            Ok(embeddings) => embeddings,
            Err(_) => {
                return None;
            }
        };

        if let Ok(mut lock) = self.embeddings.write() {
            *lock = Some(embeddings.clone());
        }

        Some(embeddings)
    }

    /// Build RAG context string for a query message.
    /// Returns None if no relevant context is found.
    pub fn build_context(&self, message: &str) -> Option<String> {
        let logs = self.logs.read().ok()?;
        if logs.is_empty() {
            return None;
        }

        let embeddings = self.get_or_generate_embeddings()?;
        let msg_embedding = match generate_embeddings(&[message.to_string()]) {
            Ok(mut embs) => embs.pop()?,
            Err(_) => {
                return None;
            }
        };

        let top_indices = find_top_k_similar(&msg_embedding, &embeddings, 3);
        let context_parts: Vec<_> = top_indices
            .into_iter()
            .filter_map(|i| logs.get(i))
            .filter_map(|log| log.to_embeddable_text())
            .collect();

        (!context_parts.is_empty()).then(|| {
            format!(
                "Relevant context from earlier conversation:\n{}\n\n",
                context_parts.join("\n---\n")
            )
        })
    }

    /// Get a clone of the logs Arc for external use
    pub fn logs(&self) -> Arc<RwLock<Vec<TenonLog>>> {
        Arc::clone(&self.logs)
    }

    /// Get a clone of the embeddings Arc for external use
    pub fn embeddings(&self) -> Arc<RwLock<Option<Vec<Vec<f64>>>>> {
        Arc::clone(&self.embeddings)
    }
}

impl Default for RagContext {
    fn default() -> Self {
        Self::new()
    }
}
