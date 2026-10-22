use rig::completion::Usage;

/// Tracks token usage for a chat session.
///
/// `accumulated` is the running total across all exchanges in this session.
/// `last_exchange` is the delta from the most recent exchange (ephemeral, not persisted).
#[derive(Debug, Clone, Default)]
pub struct SessionUsage {
    pub accumulated: Usage,
    pub last_exchange: Usage,
}

impl SessionUsage {
    /// Add a new usage to the accumulated total and update last_exchange.
    pub fn add(&mut self, mut usage: Usage) {
        // Providers report input_tokens inconsistently (exclusive or inclusive
        // of cache reads), while total_tokens and output_tokens are stable, so
        // the inclusive input is derived as total - output.
        usage.input_tokens = match (usage.total_tokens, usage.output_tokens) {
            (Some(total), Some(output)) => Some(total.saturating_sub(output)),
            (Some(total), None) => Some(total),
            (None, _) => None,
        };
        self.accumulated += usage;
        self.last_exchange = usage;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_usage_accumulation() {
        let mut session_usage = SessionUsage::default();

        // First exchange
        let usage1 = Usage {
            input_tokens: Some(100),
            output_tokens: Some(50),
            total_tokens: Some(150),
            cached_input_tokens: Some(0),
            cache_creation_input_tokens: Some(0),
            ..Default::default()
        };
        session_usage.add(usage1);

        assert_eq!(session_usage.accumulated.input_tokens, Some(100));
        assert_eq!(session_usage.accumulated.output_tokens, Some(50));
        assert_eq!(session_usage.last_exchange.input_tokens, Some(100));

        // Second exchange
        let usage2 = Usage {
            input_tokens: Some(200),
            output_tokens: Some(100),
            total_tokens: Some(300),
            cached_input_tokens: Some(50),
            cache_creation_input_tokens: Some(0),
            ..Default::default()
        };
        session_usage.add(usage2);

        // Accumulated should sum both
        assert_eq!(session_usage.accumulated.input_tokens, Some(300));
        assert_eq!(session_usage.accumulated.output_tokens, Some(150));
        assert_eq!(session_usage.accumulated.total_tokens, Some(450));
        assert_eq!(session_usage.accumulated.cached_input_tokens, Some(50));

        // Last exchange should be the second one only
        assert_eq!(session_usage.last_exchange.input_tokens, Some(200));
        assert_eq!(session_usage.last_exchange.output_tokens, Some(100));
    }

    #[test]
    fn test_session_usage_from_history() {
        let accumulated = Usage {
            input_tokens: Some(500),
            output_tokens: Some(250),
            total_tokens: Some(750),
            cached_input_tokens: Some(100),
            cache_creation_input_tokens: Some(0),
            ..Default::default()
        };

        let session_usage = SessionUsage {
            accumulated,
            last_exchange: Usage::default(),
        };

        assert_eq!(session_usage.accumulated.input_tokens, Some(500));
        assert_eq!(session_usage.last_exchange.input_tokens, None);
    }

    #[test]
    fn test_session_usage_default() {
        let session_usage = SessionUsage::default();

        assert_eq!(session_usage.accumulated.input_tokens, None);
        assert_eq!(session_usage.last_exchange.input_tokens, None);
    }
}
