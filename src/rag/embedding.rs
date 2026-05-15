use anyhow::Result;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};

/// Generates embeddings for a list of texts using FastEmbed.
/// Returns a vector of embedding vectors.
pub fn generate_embeddings(texts: &[String]) -> Result<Vec<Vec<f64>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    // Use ~/.fastembed_cache for model storage
    let cache_dir = std::env::var("HOME")
        .map(|home| std::path::PathBuf::from(home).join(".fastembed_cache"))
        .unwrap_or_else(|_| std::path::PathBuf::from(".fastembed_cache"));

    let options = InitOptions::new(EmbeddingModel::AllMiniLML6V2Q)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(false);

    let model = TextEmbedding::try_new(options)?;

    // Generate embeddings (batch_size = None for default)
    let embeddings = model.embed(texts.iter().map(|s| s.as_str()).collect::<Vec<_>>(), None)?;

    // Convert Vec<Vec<f32>> to Vec<Vec<f64>>
    Ok(embeddings
        .into_iter()
        .map(|v| v.into_iter().map(|f| f as f64).collect())
        .collect())
}

/// Computes cosine similarity between two vectors.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}

/// Finds the top-k most similar embeddings to the query.
/// Returns indices into the embeddings array.
pub fn find_top_k_similar(
    query_embedding: &[f64],
    embeddings: &[Vec<f64>],
    k: usize,
) -> Vec<usize> {
    let mut similarities: Vec<(usize, f64)> = embeddings
        .iter()
        .enumerate()
        .map(|(i, emb)| (i, cosine_similarity(query_embedding, emb)))
        .collect();

    similarities.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    similarities.into_iter().take(k).map(|(i, _)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_embeddings_basic() {
        let texts = vec!["Hello world".to_string(), "Testing embeddings".to_string()];

        let result = generate_embeddings(&texts);

        assert!(
            result.is_ok(),
            "generate_embeddings failed: {:?}",
            result.err()
        );
        let embeddings = result.unwrap();

        assert_eq!(embeddings.len(), 2, "Should return 2 embeddings");

        // AllMiniLML6V2Q has 384 dimensions
        assert_eq!(
            embeddings[0].len(),
            384,
            "Each embedding should have 384 dimensions"
        );
        assert_eq!(
            embeddings[1].len(),
            384,
            "Each embedding should have 384 dimensions"
        );
    }
}
