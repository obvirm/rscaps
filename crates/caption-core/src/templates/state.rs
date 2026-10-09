//! Per-frame narration state of segments, lines and words.
//!
//! tscaps' CSS contract: every element carries exactly one state class per
//! frame — {not-narrated-yet, being-narrated, already-narrated} — and the
//! class drives colour/animation while never changing layout. The same
//! states, computed against absolute time, drive the native render.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Narration {
    /// `now < start` — the element has not started narrating.
    NotYet,
    /// `start <= now < end` — currently narrating.
    Being,
    /// `now >= end` — narration finished.
    Already,
}

impl Narration {
    /// One state rule for any element with a `[start, end)` window.
    pub fn at(now_ms: f32, start_ms: f32, end_ms: f32) -> Self {
        if now_ms < start_ms {
            Narration::NotYet
        } else if now_ms < end_ms {
            Narration::Being
        } else {
            Narration::Already
        }
    }

    pub fn is_being(self) -> bool {
        self == Narration::Being
    }
}

/// Narration state of one word at `now_ms`.
pub fn word_narration(now_ms: f32, word_start: f32, word_end: f32) -> Narration {
    Narration::at(now_ms, word_start, word_end)
}

/// A line's narration window spans its first word's start to its last
/// word's end (tscaps line states derive from the words on the line).
#[derive(Debug, Clone, Copy)]
pub struct LineSpan {
    pub start_ms: f32,
    pub end_ms: f32,
}

impl LineSpan {
    /// Window of `words` (must be non-empty).
    pub fn of_words(words: &[super::pipeline::TimedWord]) -> Self {
        Self {
            start_ms: words.first().map(|w| w.start_ms).unwrap_or(0.0),
            end_ms: words.last().map(|w| w.end_ms).unwrap_or(0.0),
        }
    }

    pub fn narration_at(&self, now_ms: f32) -> Narration {
        Narration::at(now_ms, self.start_ms, self.end_ms)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::pipeline::TimedWord;

    #[test]
    fn state_is_a_half_open_window() {
        assert_eq!(Narration::at(99.0, 100.0, 200.0), Narration::NotYet);
        assert_eq!(Narration::at(100.0, 100.0, 200.0), Narration::Being);
        assert_eq!(Narration::at(199.9, 100.0, 200.0), Narration::Being);
        assert_eq!(Narration::at(200.0, 100.0, 200.0), Narration::Already);
    }

    #[test]
    fn zero_length_window_is_never_being() {
        // A zero-duration element (degenerate cue) flips NotYet → Already
        // exactly at its start.
        assert_eq!(Narration::at(49.0, 50.0, 50.0), Narration::NotYet);
        assert_eq!(Narration::at(50.0, 50.0, 50.0), Narration::Already);
    }

    #[test]
    fn line_span_runs_first_start_to_last_end() {
        let words = vec![
            TimedWord::new("a", 100.0, 300.0),
            TimedWord::new("b", 300.0, 800.0),
        ];
        let span = LineSpan::of_words(&words);
        assert_eq!(span.start_ms, 100.0);
        assert_eq!(span.end_ms, 800.0);
        assert_eq!(span.narration_at(500.0), Narration::Being);
        assert_eq!(span.narration_at(900.0), Narration::Already);
    }
}
