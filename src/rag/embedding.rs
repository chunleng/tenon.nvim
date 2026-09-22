use crate::chat::log::{TenonLog, TenonLogData};
use crate::tools::{ToolClassification, get_tool_classification};
use crate::utils::path_from_str;
use anyhow::Result;
use fastembed::similarity::top_k;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::sync::Arc;

#[cfg(test)]
const MAX_TEXT_CHARS: usize = 50;
#[cfg(not(test))]
const MAX_TEXT_CHARS: usize = 50_000;

/// Max texts per embed call. Embedding sorted-by-length chunks keeps
/// tokenizer padding waste low when text lengths vary widely.
const EMBED_CHUNK_SIZE: usize = 16;

/// Generates embeddings for multiple logs.
/// Returns one embedding per input log, in the same order.
/// Idempotent tool logs are skipped (embedded as empty strings) since their
/// results are reproducible and add no retrieval signal.
/// Texts exceeding MAX_TEXT_CHARS are embedded as empty strings.
pub fn generate_embeddings(logs: &[Arc<TenonLog>]) -> Result<Vec<Vec<f32>>> {
    if logs.is_empty() {
        return Ok(vec![]);
    }

    let texts: Vec<String> = logs.iter().map(|log| log.to_embeddable_text()).collect();

    // Substitute an empty string for skipped logs so output positions stay
    // aligned with input positions.
    let embeddable: Vec<&str> = logs
        .iter()
        .zip(&texts)
        .map(|(log, text)| match log.data() {
            TenonLogData::Tool(tool_log)
                if get_tool_classification(&tool_log.tool_call.name)
                    == ToolClassification::Idempotent =>
            {
                ""
            }
            _ => text.as_str(),
        })
        .collect();

    embed_texts(&embeddable)
}

/// Generates an embedding for a single text using FastEmbed.
/// Returns the embedding vector.
pub fn generate_embedding(text: &str) -> Result<Vec<f32>> {
    let mut embeddings = embed_texts(&[text])?;

    Ok(embeddings
        .pop()
        .expect("Single text should produce exactly one embedding"))
}

/// Embeds raw texts. Returns one embedding per input text, in the same order.
/// Texts exceeding MAX_TEXT_CHARS are embedded as empty strings.
/// TODO: Replace the character-count guard with a proper token count once
/// a tokenizer is available.
fn embed_texts(texts: &[&str]) -> Result<Vec<Vec<f32>>> {
    // Substitute an empty string for oversized texts so output positions stay
    // aligned with input positions.
    let embeddable: Vec<&str> = texts
        .iter()
        .map(|t| if t.len() > MAX_TEXT_CHARS { "" } else { t })
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
    let options = InitOptions::new(EmbeddingModel::SnowflakeArcticEmbedXSQ)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(false);

    let mut model = TextEmbedding::try_new(options)?;

    let mut result = vec![Vec::new(); embeddable.len()];
    for chunk in order.chunks(EMBED_CHUNK_SIZE) {
        let chunk_texts: Vec<&str> = chunk.iter().map(|&i| embeddable[i]).collect();
        // batch_size = None for default
        let embeddings = model.embed(chunk_texts, None)?;
        for (slot, emb) in chunk.iter().zip(embeddings) {
            result[*slot] = emb;
        }
    }

    Ok(result)
}

/// Finds the top-k most similar embeddings to the query using cosine similarity.
/// Returns indices into the embeddings array sorted by similarity (most similar first).
pub fn find_top_k_similar(
    query_embedding: &[f32],
    embeddings: &[Vec<f32>],
    k: usize,
) -> Vec<usize> {
    if embeddings.is_empty() || query_embedding.is_empty() {
        return Vec::new();
    }

    // Empty embeddings carry no signal; fastembed scores them 0.0, which
    // would rank into top-k whenever real similarities go negative. Skip
    // them and map scores back to original indices.
    let candidates: Vec<(usize, &Vec<f32>)> = embeddings
        .iter()
        .enumerate()
        .filter(|(_, emb)| !emb.is_empty())
        .collect();

    let corpus: Vec<&Vec<f32>> = candidates.iter().map(|(_, emb)| *emb).collect();

    // top_k returns (corpus position, score), best first. Map positions back
    // to original embedding indices.
    top_k(query_embedding, &corpus, k)
        .into_iter()
        .map(|(pos, _)| candidates[pos].0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::log::{TenonLog, TenonLogData, TenonToolCall, TenonToolLog, TenonUserMessage};
    use std::sync::Arc;

    fn user_log(text: &str) -> Arc<TenonLog> {
        Arc::new(TenonLog::new(TenonLogData::User(TenonUserMessage::Text(
            text.to_string(),
        ))))
    }

    fn tool_log(name: &str) -> Arc<TenonLog> {
        Arc::new(TenonLog::new(TenonLogData::Tool(TenonToolLog {
            tool_call: TenonToolCall {
                id: "1".into(),
                internal_call_id: "1".into(),
                name: name.into(),
                args: serde_json::json!({}),
            },
            tool_result: None,
        })))
    }

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
        let logs = vec![user_log("Hello world"), user_log("Goodbye world")];

        let result = generate_embeddings(&logs);

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
        let logs = vec![
            user_log("Hello world"),
            user_log(&"a".repeat(MAX_TEXT_CHARS + 1)),
            user_log(""),
            user_log("Goodbye world"),
        ];

        let result = generate_embeddings(&logs);

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
    fn test_generate_embeddings_skips_idempotent_tool() {
        let logs = vec![
            user_log("Hello world"),
            tool_log("read_file"),   // Idempotent: skipped
            tool_log("run_command"), // Mutating: embedded
        ];

        let result = generate_embeddings(&logs);

        assert!(
            result.is_ok(),
            "generate_embeddings failed: {:?}",
            result.err()
        );
        let embeddings = result.unwrap();
        assert_eq!(
            embeddings.len(),
            3,
            "Skipped tool should still yield one embedding per input log"
        );
        assert!(
            embeddings[1].is_empty(),
            "Idempotent tool log should produce an empty embedding"
        );
        assert_eq!(embeddings[0].len(), 384);
        assert_eq!(embeddings[2].len(), 384);
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
