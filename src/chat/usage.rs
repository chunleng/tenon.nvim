use rig::completion::Usage;

/// Tracks token usage for a chat session.
///
/// `accumulated` is the running total across all exchanges in this session.
/// `last_exchange` is the delta from the most recent exchange (ephemeral, not persisted).
#[derive(Debug, Clone)]
pub struct SessionUsage {
    pub accumulated: Usage,
    pub last_exchange: Usage,
}

impl Default for SessionUsage {
    fn default() -> Self {
        Self {
            accumulated: Usage::new(),
            last_exchange: Usage::new(),
        }
    }
}

impl SessionUsage {
    /// Add a new usage to the accumulated total and update last_exchange.
    pub fn add(&mut self, usage: Usage) {
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
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
            cached_input_tokens: 0,
            cache_creation_input_tokens: 0,
        };
        session_usage.add(usage1.clone());

        assert_eq!(session_usage.accumulated.input_tokens, 100);
        assert_eq!(session_usage.accumulated.output_tokens, 50);
        assert_eq!(session_usage.last_exchange.input_tokens, 100);

        // Second exchange
        let usage2 = Usage {
            input_tokens: 200,
            output_tokens: 100,
            total_tokens: 300,
            cached_input_tokens: 50,
            cache_creation_input_tokens: 0,
        };
        session_usage.add(usage2.clone());

        // Accumulated should sum both
        assert_eq!(session_usage.accumulated.input_tokens, 300);
        assert_eq!(session_usage.accumulated.output_tokens, 150);
        assert_eq!(session_usage.accumulated.total_tokens, 450);
        assert_eq!(session_usage.accumulated.cached_input_tokens, 50);

        // Last exchange should be the second one only
        assert_eq!(session_usage.last_exchange.input_tokens, 200);
        assert_eq!(session_usage.last_exchange.output_tokens, 100);
    }

    #[test]
    fn test_session_usage_from_history() {
        let accumulated = Usage {
            input_tokens: 500,
            output_tokens: 250,
            total_tokens: 750,
            cached_input_tokens: 100,
            cache_creation_input_tokens: 0,
        };

        let session_usage = SessionUsage {
            accumulated,
            last_exchange: Usage::new(),
        };

        assert_eq!(session_usage.accumulated.input_tokens, 500);
        assert_eq!(session_usage.last_exchange.input_tokens, 0);
    }

    #[test]
    fn test_session_usage_default() {
        let session_usage = SessionUsage::default();

        assert_eq!(session_usage.accumulated.input_tokens, 0);
        assert_eq!(session_usage.last_exchange.input_tokens, 0);
    }
}
