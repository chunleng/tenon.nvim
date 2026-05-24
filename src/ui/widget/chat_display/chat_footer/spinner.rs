use std::sync::atomic::{AtomicUsize, Ordering};

const SPINNER_CHARS: [&str; 8] = ["⣀⣤", "⣤⣶", "⣶⣿", "⣿⣿", "⣿⣶", "⣶⣤", "⣤⣀", "⣀⣀"];

pub struct SpinnerState {
    frame: AtomicUsize,
    previous_processing: bool,
}

impl SpinnerState {
    pub fn new() -> Self {
        Self {
            frame: AtomicUsize::new(0),
            previous_processing: false,
        }
    }

    /// Returns true if spinner should render this frame.
    /// Renders when processing OR when state transitions from true to false (to clear spinner).
    pub fn should_render(&mut self, is_processing: bool) -> bool {
        let should_render = is_processing || self.previous_processing;
        self.previous_processing = is_processing;
        should_render
    }

    /// Get current spinner character
    pub fn get_char(&self) -> &'static str {
        let frame = self.frame.load(Ordering::SeqCst) % SPINNER_CHARS.len();
        SPINNER_CHARS[frame]
    }

    /// Advance to next frame
    pub fn advance(&self) {
        self.frame.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spinner_frame_advances() {
        let spinner = SpinnerState::new();
        assert_eq!(spinner.get_char(), "⣀⣤");
        spinner.advance();
        assert_eq!(spinner.get_char(), "⣤⣶");
    }

    #[test]
    fn test_spinner_should_render_on_state_change() {
        let mut spinner = SpinnerState::new();
        // First processing: should render
        assert!(spinner.should_render(true));
        // Still processing: should render
        assert!(spinner.should_render(true));
        // Processing stops: should render (to clear)
        assert!(spinner.should_render(false));
        // Not processing: should not render
        assert!(!spinner.should_render(false));
    }

    #[test]
    fn test_spinner_wraps_around() {
        let spinner = SpinnerState::new();
        for _ in 0..8 {
            spinner.advance();
        }
        // After 8 advances, should be back to first frame
        assert_eq!(spinner.get_char(), "⣀⣤");
    }
}
