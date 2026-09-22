mod embedding;

pub use embedding::{find_top_k_similar, generate_embedding, generate_embeddings};

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
            let new_embeddings: Vec<Vec<f32>> =
                generate_embeddings(&logs[cached_len..]).unwrap_or_default();

            if new_embeddings.is_empty() {
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
        let embeddings: Vec<Vec<f32>> = generate_embeddings(logs).unwrap_or_default();

        if embeddings.is_empty() {
            return None;
        }

        if let Ok(mut lock) = self.embeddings.write() {
            *lock = Some(embeddings.clone());
        }

        Some(embeddings)
    }

    /// Find the top-k relevant logs for a query message.
    /// Returns empty Vec if no relevant context is found.
    pub fn build_context(&self, logs: &[Arc<TenonLog>], message: &str) -> Vec<Arc<TenonLog>> {
        if logs.is_empty() {
            return Vec::new();
        }

        let embeddings = match self.get_or_generate_embeddings(logs) {
            Some(emb) => emb,
            None => return Vec::new(),
        };
        let msg_embedding = match generate_embedding(message) {
            Ok(emb) => emb,
            Err(_) => return Vec::new(),
        };

        let top_indices = find_top_k_similar(&msg_embedding, &embeddings, 3);
        top_indices
            .into_iter()
            .filter_map(|i| logs.get(i).cloned())
            .collect()
    }
}

impl Default for RagContext {
    fn default() -> Self {
        Self::new()
    }
}
