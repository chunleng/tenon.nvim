use crate::utils::path_from_str;
use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use lance_linalg::distance::cosine::cosine_distance_batch;

#[cfg(test)]
const MAX_TEXT_CHARS: usize = 50;
#[cfg(not(test))]
const MAX_TEXT_CHARS: usize = 50_000;

/// Max texts per embed call. Embedding sorted-by-length chunks keeps
/// tokenizer padding waste low when text lengths vary widely.
const EMBED_CHUNK_SIZE: usize = 16;

/// Generates embeddings for multiple texts.
/// Returns one embedding per input text, in the same order.
/// Texts exceeding MAX_TEXT_CHARS are embedded as empty strings.
pub fn generate_embeddings(texts: &[String]) -> Result<Vec<Vec<f32>>> {
    // TODO: Replace this character-count guard with a proper token count once
    // a tokenizer is available.
    if texts.is_empty() {
        return Ok(vec![]);
    }

    // Substitute an empty string for oversized texts so output positions stay
    // aligned with input positions.
    let empty = String::new();
    let embeddable: Vec<&String> = texts
        .iter()
        .map(|t| if t.len() > MAX_TEXT_CHARS { &empty } else { t })
        .collect();

    // Batched inference pads every text to the longest in its batch, so one
    // long text would inflate the cost of all short ones. Sorting by length
    // and chunking keeps each batch length-homogeneous.
    // Empty texts (original or substituted) skip the model and keep the
    // initial empty embedding in their slot.
    let mut order: Vec<usize> = (0..embeddable.len())
        .filter(|&i| !embeddable[i].is_empty())
        .collect();
    order.sort_by_key(|&i| embeddable[i].len());

    let cache_dir = path_from_str("~/.fastembed_cache");

    // We initiate all the time because this is a small model and fast to start.
    // The tradeoff is that we get to save on memory usage
    let options = InitOptions::new(EmbeddingModel::AllMiniLML6V2Q)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(false);

    let mut model = TextEmbedding::try_new(options)?;

    let mut result = vec![Vec::new(); embeddable.len()];
    for chunk in order.chunks(EMBED_CHUNK_SIZE) {
        let chunk_texts: Vec<&String> = chunk.iter().map(|&i| embeddable[i]).collect();
        // batch_size = None for default
        let embeddings = model.embed(chunk_texts, None)?;
        for (slot, emb) in chunk.iter().zip(embeddings) {
            result[*slot] = emb;
        }
    }

    Ok(result)
}

/// Generates an embedding for a single text using FastEmbed.
/// Returns the embedding vector.
pub fn generate_embedding(text: &str) -> Result<Vec<f32>> {
    // TODO: Replace this character-count guard with a proper token count once
    // a tokenizer is available.
    if text.len() > MAX_TEXT_CHARS {
        return Ok(vec![]);
    }

    let mut embeddings = generate_embeddings(&[text.to_string()])?;

    Ok(embeddings
        .pop()
        .expect("Single text should produce exactly one embedding"))
}

/// Finds the top-k most similar embeddings to the query using SIMD-optimized cosine distance.
/// Returns indices into the embeddings array sorted by similarity (most similar first).
pub fn find_top_k_similar(
    query_embedding: &[f32],
    embeddings: &[Vec<f32>],
    k: usize,
) -> Vec<usize> {
    if embeddings.is_empty() || query_embedding.is_empty() {
        return Vec::new();
    }

    let dimension = query_embedding.len();

    // Empty embeddings carry no signal and would shift the flat-array rows,
    // misaligning every subsequent distance. Skip them and map distances
    // back to original indices.
    let candidates: Vec<(usize, &Vec<f32>)> = embeddings
        .iter()
        .enumerate()
        .filter(|(_, emb)| !emb.is_empty())
        .collect();

    // Flatten embeddings into contiguous array for batch processing
    let flat_embeddings: Vec<f32> = candidates
        .iter()
        .flat_map(|(_, emb)| emb.iter().copied())
        .collect();

    // Compute cosine distances (distance = 1 - similarity, range [0, 2])
    let distances: Vec<f32> =
        cosine_distance_batch(query_embedding, &flat_embeddings, dimension).collect();

    // Sort by distance (ascending), return original indices
    let mut indexed: Vec<(usize, f32)> = candidates
        .iter()
        .zip(distances)
        .map(|((i, _), dist)| (*i, dist))
        .collect();
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
    fn test_find_top_k_similar_skips_empty_embeddings() {
        let query: Vec<f32> = vec![1.0, 0.0, 0.0];
        let embeddings: Vec<Vec<f32>> = vec![
            vec![1.0, 0.0, 0.0], // Same direction as query
            vec![],              // Empty embedding should be skipped
            vec![0.9, 0.1, 0.0], // Similar to query
        ];

        let top_indices = find_top_k_similar(&query, &embeddings, 2);

        assert_eq!(top_indices, vec![0, 2]);
    }

    #[test]
    fn test_find_top_k_similar_empty_query() {
        let query: Vec<f32> = vec![];
        let embeddings: Vec<Vec<f32>> = vec![vec![1.0, 0.0, 0.0]];

        assert!(find_top_k_similar(&query, &embeddings, 3).is_empty());
    }

    #[test]
    fn test_generate_embeddings_batch() {
        let texts = vec!["Hello world".to_string(), "Goodbye world".to_string()];

        let result = generate_embeddings(&texts);

        assert!(
            result.is_ok(),
            "generate_embeddings failed: {:?}",
            result.err()
        );
        let embeddings = result.unwrap();
        assert_eq!(embeddings.len(), 2, "Should return one embedding per text");
        assert!(
            embeddings.iter().all(|e| e.len() == 384),
            "Each embedding should have 384 dimensions"
        );
    }

    #[test]
    fn test_generate_embeddings_oversized_text_keeps_position() {
        let texts = vec![
            "Hello world".to_string(),
            "a".repeat(MAX_TEXT_CHARS + 1),
            String::new(),
            "Goodbye world".to_string(),
        ];

        let result = generate_embeddings(&texts);

        assert!(
            result.is_ok(),
            "generate_embeddings failed: {:?}",
            result.err()
        );
        let embeddings = result.unwrap();
        assert_eq!(
            embeddings.len(),
            4,
            "Oversized and empty texts should still yield one embedding per input text"
        );
        assert!(
            embeddings[1].is_empty(),
            "Oversized text should produce an empty embedding"
        );
        assert!(
            embeddings[2].is_empty(),
            "Originally empty text should produce an empty embedding"
        );
        assert_eq!(embeddings[0].len(), 384);
        assert_eq!(embeddings[3].len(), 384);
    }

    #[test]
    fn test_generate_embeddings_empty() {
        let result = generate_embeddings(&[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
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
