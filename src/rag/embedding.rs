use crate::utils::path_from_str;
use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use lance_linalg::distance::cosine::cosine_distance_batch;

#[cfg(test)]
const MAX_TEXT_CHARS: usize = 50;
#[cfg(not(test))]
const MAX_TEXT_CHARS: usize = 50_000;

/// Generates an embedding for a single text using FastEmbed.
/// Returns the embedding vector.
pub fn generate_embedding(text: &str) -> Result<Vec<f32>> {
    // TODO: Replace this character-count guard with a proper token count once
    // a tokenizer is available.
    if text.len() > MAX_TEXT_CHARS {
        return Ok(vec![]);
    }

    let cache_dir = path_from_str("~/.fastembed_cache");

    let options = InitOptions::new(EmbeddingModel::AllMiniLML6V2Q)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(false);

    let model = TextEmbedding::try_new(options)?;

    // Generate embedding (batch_size = None for default)
    let embeddings = model.embed(vec![text], None)?;

    Ok(embeddings
        .into_iter()
        .next()
        .expect("Single text should produce exactly one embedding"))
}

/// Finds the top-k most similar embeddings to the query using SIMD-optimized cosine distance.
/// Returns indices into the embeddings array sorted by similarity (most similar first).
pub fn find_top_k_similar(
    query_embedding: &[f32],
    embeddings: &[Vec<f32>],
    k: usize,
) -> Vec<usize> {
    if embeddings.is_empty() {
        return Vec::new();
    }

    let dimension = query_embedding.len();

    // Flatten embeddings into contiguous array for batch processing
    let flat_embeddings: Vec<f32> = embeddings.iter().flatten().copied().collect();

    // Compute cosine distances (distance = 1 - similarity, range [0, 2])
    let distances: Vec<f32> =
        cosine_distance_batch(query_embedding, &flat_embeddings, dimension).collect();

    // Sort by distance (ascending), return indices
    let mut indexed: Vec<(usize, f32)> = distances.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    indexed.into_iter().take(k).map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_embedding_basic() {
        let text = "Hello world".to_string();

        let result = generate_embedding(&text);

        assert!(
            result.is_ok(),
            "generate_embedding failed: {:?}",
            result.err()
        );
        let embedding = result.unwrap();

        // AllMiniLML6V2Q has 384 dimensions
        assert_eq!(embedding.len(), 384, "Embedding should have 384 dimensions");
    }

    #[test]
    fn test_find_top_k_similar() {
        // Test embeddings with known similarity relationships
        let query: Vec<f32> = vec![1.0, 0.0, 0.0];
        let embeddings: Vec<Vec<f32>> = vec![
            vec![0.9, 0.1, 0.1], // Most similar to query
            vec![0.0, 1.0, 0.0], // Orthogonal
            vec![0.8, 0.2, 0.0], // Second most similar
        ];

        let top_indices = find_top_k_similar(&query, &embeddings, 2);

        assert_eq!(top_indices.len(), 2);
        assert_eq!(top_indices[0], 0); // Most similar
        assert_eq!(top_indices[1], 2); // Second most similar
    }

    #[test]
    fn test_generate_embedding_too_long() {
        let text = "a".repeat(MAX_TEXT_CHARS + 1);
        let result = generate_embedding(&text);
        assert!(
            result.is_ok(),
            "generate_embedding failed: {:?}",
            result.err()
        );
        let embedding = result.unwrap();
        assert!(
            embedding.is_empty(),
            "Input > 50 chars should return empty vec"
        );
    }
}
