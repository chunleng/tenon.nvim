mod embedding;

pub use embedding::{find_top_k_similar, generate_embedding};

use std::sync::{Arc, RwLock};

use crate::chat::log::TenonLog;

/// RAG context holder - encapsulates embeddings for retrieval-augmented generation.
#[derive(Clone)]
pub struct RagContext {
    embeddings: Arc<RwLock<Option<Vec<Vec<f32>>>>>,
}

impl RagContext {
    pub fn new() -> Self {
        Self {
            embeddings: Arc::new(RwLock::new(None)),
        }
    }

    /// Gets cached embeddings or generates new ones for the given logs.
    /// Incrementally generates embeddings only for logs that don't have them cached.
    fn get_or_generate_embeddings(&self, logs: &[Arc<TenonLog>]) -> Option<Vec<Vec<f32>>> {
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

            let new_embeddings: Vec<Vec<f32>> = new_texts
                .iter()
                .filter_map(|text| generate_embedding(text).ok())
                .collect();

            if new_embeddings.is_empty() && !new_texts.is_empty() {
                return None;
            }

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

        let embeddings: Vec<Vec<f32>> = texts
            .iter()
            .filter_map(|text| generate_embedding(text).ok())
            .collect();

        if embeddings.is_empty() && !texts.is_empty() {
            return None;
        }

        if let Ok(mut lock) = self.embeddings.write() {
            *lock = Some(embeddings.clone());
        }

        Some(embeddings)
    }

    /// Build RAG context string for a query message.
    /// Returns None if no relevant context is found.
    pub fn build_context(&self, logs: &[Arc<TenonLog>], message: &str) -> Option<String> {
        if logs.is_empty() {
            return None;
        }

        let embeddings = self.get_or_generate_embeddings(logs)?;
        let msg_embedding = match generate_embedding(message) {
            Ok(emb) => emb,
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

        (!context_parts.is_empty()).then(|| format!("{}\n\n", context_parts.join("\n---\n")))
    }
}

impl Default for RagContext {
    fn default() -> Self {
        Self::new()
    }
}
