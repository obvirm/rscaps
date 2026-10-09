//! Shared segment pipeline: cue → words → sentence/clause split → char-limit
//! split → text effects → sorted windows (`gap_free` when declared).
//!
//! Ports of the tscaps engine stages (verified against the AGPL sources as
//! behavioural facts, code written from scratch):
//! - `CueTextTokenizer` → [`tokenize`]
//! - `WordTimingEstimator.spread` → [`spread`]
//! - `BoundarySegmentSplitter` → [`split_boundary`]
//! - `BoundaryScoreLimitByCharsSegmentSplitter` → [`split_char_limit`]
//! - effects array (in declared order) → [`crate::templates::effects`]
//! - `gap_free` effect → [`collect_segments`] (only when declared)
//!
//! Per-template knobs (separator set, char limits, effects, max gap) travel
//! in [`SplitConfig`].

/// One whitespace-delimited token with its estimated narration window.
/// `text` is the transcription source of truth (splitters and weight see
/// it); `display` is what renders after the effects array — line splitting
/// at render time measures `display`.
#[derive(Debug, Clone)]
pub struct TimedWord {
    pub text: String,
    pub display: String,
    pub start_ms: f32,
    pub end_ms: f32,
}

impl TimedWord {
    pub fn new(text: impl Into<String>, start_ms: f32, end_ms: f32) -> Self {
        let text = text.into();
        Self {
            display: text.clone(),
            text,
            start_ms,
            end_ms,
        }
    }
}

/// A segment ready for display: the words on screen together plus the
/// window (after `gap_free`) during which the segment owns the screen.
#[derive(Debug, Clone)]
pub struct WindowedSegment {
    pub words: Vec<TimedWord>,
    pub start_ms: f32,
    pub end_ms: f32,
    /// Base direction for this segment's line (document majority, flipped
    /// when the line is single-direction the other way) - feeds
    /// `layout::word_x_positions` so RTL scripts lay out right-to-left.
    pub direction: crate::templates::stack::Direction,
}

/// First-stage split: break after boundary characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    /// Every word ending in a sentence terminator closes a segment group.
    Sentence,
    /// Sentence terminators plus clause punctuation (`, ; :` family) —
    /// the `clause` preset of `BoundarySegmentSplitterDescriptor`.
    Clause,
    /// No terminator-based splitting; one group per cue.
    None,
}

/// Per-template pipeline configuration.
#[derive(Debug, Clone, Copy)]
pub struct SplitConfig {
    pub boundary: Boundary,
    /// `boundary_score_limit_by_chars.maxChars`.
    pub max_chars: usize,
    /// `boundary_score_limit_by_chars.minChars`.
    pub min_chars: usize,
    /// `gap_free.maxGapMs` (default 1000). Only consulted when the
    /// effects list contains [`effects::Effect::GapFree`].
    pub max_gap_ms: f32,
    /// The template's `effects` array, in declared order.
    pub effects: &'static [crate::templates::effects::Effect],
}

// ── text → words ──────────────────────────────────────────────────────────

/// Strips WebVTT angle-bracket tags and SubRip brace overrides (only when a
/// closing delimiter exists, matching the engine's `MARKUP` regex), then
/// splits on whitespace runs — the port of `CueTextTokenizer`.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut cleaned = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let close = match chars[i] {
            '<' => '>',
            '{' => '}',
            c => {
                cleaned.push(c);
                i += 1;
                continue;
            }
        };
        match (i + 1..chars.len()).find(|&j| chars[j] == close) {
            Some(close_idx) => i = close_idx + 1,
            None => {
                cleaned.push(chars[i]);
                i += 1;
            }
        }
    }
    cleaned.split_whitespace().map(str::to_string).collect()
}

/// Shares the cue window across tokens proportionally to each token's
/// UTF-16 length — the port of `WordTimingEstimator.spread` (JS `.length`).
pub fn spread(tokens: &[String], start_ms: f32, end_ms: f32) -> Vec<TimedWord> {
    let total: usize = tokens.iter().map(|t| t.encode_utf16().count()).sum();
    let duration = end_ms - start_ms;
    if total == 0 || duration <= 0.0 {
        return tokens
            .iter()
            .map(|t| TimedWord::new(t.as_str(), start_ms, end_ms))
            .collect();
    }
    let mut words = Vec::with_capacity(tokens.len());
    let mut elapsed: usize = 0;
    for token in tokens {
        let word_start = start_ms + (elapsed as f32 / total as f32) * duration;
        elapsed += token.encode_utf16().count();
        let word_end = start_ms + (elapsed as f32 / total as f32) * duration;
        words.push(TimedWord::new(token.as_str(), word_start, word_end));
    }
    words
}

// ── segment splitters ─────────────────────────────────────────────────────

const CLOSING_QUOTES: [&str; 4] = ["\"", "\u{201D}", "\u{2019}", "\u{BB}"];

fn with_quotes(bases: &[&str]) -> Vec<String> {
    let mut seps = Vec::with_capacity(bases.len() * (CLOSING_QUOTES.len() + 1));
    for base in bases {
        seps.push((*base).to_string());
        for quote in CLOSING_QUOTES {
            seps.push(format!("{base}{quote}"));
        }
    }
    seps
}

/// Sentence preset chars of `BoundarySegmentSplitterDescriptor`:
/// bases × (bare + base followed by any closing quote).
fn sentence_separators() -> &'static [String] {
    static SEPS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    SEPS.get_or_init(|| {
        with_quotes(&SENTENCE_BASES)
    })
}

/// Clause preset = sentence bases + clause-only bases, each × quotes.
fn clause_separators() -> &'static [String] {
    static SEPS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    SEPS.get_or_init(|| with_quotes(&[&SENTENCE_BASES[..], &CLAUSE_ONLY_BASES[..]].concat()))
}

const SENTENCE_BASES: [&str; 10] = [
    ".",
    "?",
    "!",
    "...",
    "\u{2026}",
    "\u{61F}",
    "\u{6D4}",
    "\u{3002}",
    "\u{FF1F}",
    "\u{FF01}",
];

const CLAUSE_ONLY_BASES: [&str; 9] = [
    ",",
    ";",
    ":",
    "\u{60C}",
    "\u{61B}",
    "\u{3001}",
    "\u{FF0C}",
    "\u{FF1B}",
    "\u{FF1A}",
];

/// Splits after every word that ends with a boundary separator — the port of
/// `BoundarySegmentSplitter`. `Boundary::None` yields one group.
pub fn split_boundary(words: &[TimedWord], boundary: Boundary) -> Vec<Vec<TimedWord>> {
    match boundary {
        Boundary::None => {
            if words.is_empty() {
                Vec::new()
            } else {
                vec![words.to_vec()]
            }
        }
        Boundary::Sentence => split_at(words, sentence_separators()),
        Boundary::Clause => split_at(words, clause_separators()),
    }
}

fn split_at(words: &[TimedWord], seps: &[String]) -> Vec<Vec<TimedWord>> {
    let mut out: Vec<Vec<TimedWord>> = Vec::new();
    let mut current: Vec<TimedWord> = Vec::new();
    for word in words {
        let ends = seps.iter().any(|sep| word.text.ends_with(sep.as_str()));
        current.push(word.clone());
        if ends {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// `(to - from - 1) + Σ utf16 length` — the weight used by
/// `BoundaryScoreLimitByCharsSegmentSplitter` (gaps counted as one char).
fn weight(words: &[TimedWord], from: usize, to: usize) -> usize {
    if to <= from {
        return 0;
    }
    let mut chars = to - from - 1;
    for k in from..to {
        chars += words[k].text.encode_utf16().count();
    }
    chars
}

fn largest_cut_within_max(words: &[TimedWord], start: usize, max_chars: usize) -> usize {
    let n = words.len();
    let mut end = start + 1;
    while end < n && weight(words, start, end + 1) <= max_chars {
        end += 1;
    }
    end
}

fn smallest_cut_meeting_min(words: &[TimedWord], start: usize, upper: usize, min_chars: usize) -> usize {
    let mut k = start + 1;
    while k <= upper && weight(words, start, k) < min_chars {
        k += 1;
    }
    k
}

/// No boundary scorer in v1, so `highestScoredCut` degrades to the last
/// (largest) candidate that keeps the tail ≥ minChars, else everything that
/// remains.
fn pick_cut(words: &[TimedWord], start: usize, max_chars: usize, min_chars: usize) -> usize {
    let n = words.len();
    let k_high = largest_cut_within_max(words, start, max_chars);
    if k_high == n {
        return n;
    }
    let k_low = smallest_cut_meeting_min(words, start, k_high, min_chars);
    if k_low > k_high {
        return k_high;
    }
    let candidates: Vec<usize> = (k_low..=k_high)
        .filter(|&k| weight(words, k, n) >= min_chars)
        .collect();
    match candidates.last() {
        Some(&k) => k,
        None => n,
    }
}

/// Splits one sentence into char-limited segments — the port of
/// `BoundaryScoreLimitByCharsSegmentSplitter.splitSegment`.
pub fn split_char_limit(
    words: Vec<TimedWord>,
    max_chars: usize,
    min_chars: usize,
) -> Vec<Vec<TimedWord>> {
    let n = words.len();
    if n == 0 {
        return Vec::new();
    }
    let mut out: Vec<Vec<TimedWord>> = Vec::new();
    let mut start = 0;
    while start < n {
        let end = pick_cut(&words, start, max_chars, min_chars);
        out.push(words[start..end].to_vec());
        start = end;
    }
    out
}

// ── document → windowed segments ──────────────────────────────────────────

/// Tokenizes every cue, splits it (boundary → char limit), applies the
/// declared text effects in order, sorts the segments in time order and —
/// when `gap_free` is among them — extends windows: a gap ≤ `max_gap_ms`
/// bridges to the next segment's start, a larger gap keeps the word span
/// plus one max-gap tail, and the last segment's tail clamps to the video
/// duration (`gap <= 0` adds nothing). Without `gap_free` the window is
/// exactly the word span (a frame between two segments renders nothing —
/// `BatchPlanner` only mounts a segment whose window contains `t`).
pub fn collect_segments(doc: &crate::CaptionDocument, cfg: &SplitConfig) -> Vec<WindowedSegment> {
    use crate::templates::stack::{detect_document_direction, resolve_line_direction};

    let doc_direction = detect_document_direction(doc);
    let mut groups: Vec<Vec<TimedWord>> = Vec::new();
    for cue in &doc.cues {
        let tokens = tokenize(&cue.text);
        let words = spread(&tokens, cue.start_ms as f32, cue.end_ms as f32);
        for sentence in split_boundary(&words, cfg.boundary) {
            for chunk in split_char_limit(sentence, cfg.max_chars, cfg.min_chars) {
                if !chunk.is_empty() {
                    groups.push(chunk);
                }
            }
        }
    }
    crate::templates::effects::apply_text_effects(&mut groups, cfg.effects);

    let mut segments: Vec<WindowedSegment> = groups
        .into_iter()
        .filter_map(|words| {
            let start_ms = words.first()?.start_ms;
            let end_ms = words.last()?.end_ms;
            let segment_text = words
                .iter()
                .map(|w| w.display.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            let direction = resolve_line_direction(&segment_text, doc_direction);
            Some(WindowedSegment {
                words,
                start_ms,
                end_ms,
                direction,
            })
        })
        .collect();
    segments.sort_by(|a, b| {
        a.start_ms
            .total_cmp(&b.start_ms)
            .then(a.end_ms.total_cmp(&b.end_ms))
    });

    if !cfg
        .effects
        .contains(&crate::templates::effects::Effect::GapFree)
    {
        return segments;
    }

    let video_end_ms =
        doc.duration_frames as f32 * 1000.0 / (<crate::CaptionVideo as fframes::Video>::FPS as f32);
    for i in 0..segments.len() {
        let word_end = segments[i].end_ms;
        let new_end = match segments.get(i + 1) {
            Some(next) => {
                let gap = next.start_ms - word_end;
                if gap <= 0.0 {
                    word_end
                } else if gap <= cfg.max_gap_ms {
                    next.start_ms
                } else {
                    word_end + cfg.max_gap_ms
                }
            }
            None => {
                let padded = (word_end + cfg.max_gap_ms).min(video_end_ms);
                if padded > word_end {
                    padded
                } else {
                    word_end
                }
            }
        };
        segments[i].end_ms = new_end;
    }
    segments
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::templates::effects::Effect;

    fn cfg() -> SplitConfig {
        SplitConfig {
            boundary: Boundary::Sentence,
            max_chars: 26,
            min_chars: 8,
            max_gap_ms: 1000.0,
            effects: &[Effect::GapFree],
        }
    }

    fn word(text: &str) -> TimedWord {
        TimedWord::new(text, 0.0, 100.0)
    }

    #[test]
    fn tokenize_strips_markup_and_splits_whitespace() {
        assert_eq!(
            tokenize("Hello <c>world</c> {\\an8}there"),
            vec!["Hello", "world", "there"]
        );
        assert_eq!(tokenize("  a \n b\tc "), vec!["a", "b", "c"]);
        // No closing delimiter → the angle bracket is plain text (JS regex).
        assert_eq!(tokenize("a < b"), vec!["a", "<", "b"]);
    }

    #[test]
    fn spread_uses_utf16_lengths() {
        // "a" = 1 unit, "👍" = 2 units → thirds at 1/3 and 1.
        let tokens = vec!["a".to_string(), "👍".to_string()];
        let words = spread(&tokens, 0.0, 3000.0);
        assert_eq!(words.len(), 2);
        assert!((words[0].start_ms - 0.0).abs() < 1e-3);
        assert!((words[0].end_ms - 1000.0).abs() < 1e-3);
        assert!((words[1].start_ms - 1000.0).abs() < 1e-3);
        assert!((words[1].end_ms - 3000.0).abs() < 1e-3);
    }

    #[test]
    fn spread_zero_duration_shares_the_window() {
        let tokens = vec!["a".to_string(), "b".to_string()];
        let words = spread(&tokens, 500.0, 500.0);
        assert!(words.iter().all(|w| w.start_ms == 500.0 && w.end_ms == 500.0));
    }

    #[test]
    fn boundary_splits_after_sentence_enders() {
        let tokens = tokenize("Hello world. How are you? Fine!");
        let words = spread(&tokens, 0.0, 3000.0);
        let segments = split_boundary(&words, Boundary::Sentence);
        let texts: Vec<String> = segments
            .iter()
            .map(|s| s.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(texts, vec!["Hello world.", "How are you?", "Fine!"]);
    }

    #[test]
    fn boundary_handles_ellipsis_but_not_bare_quotes() {
        // Bases × (bare | base+quote) only: a closing quote on its own does
        // not end a sentence; "…" does.
        let tokens = tokenize("Wait\u{2026} really\u{201D} yes\u{BB}");
        let words = spread(&tokens, 0.0, 3000.0);
        let segments = split_boundary(&words, Boundary::Sentence);
        let texts: Vec<String> = segments
            .iter()
            .map(|s| s.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(texts, vec!["Wait\u{2026}", "really\u{201D} yes\u{BB}"]);
    }

    #[test]
    fn boundary_splits_after_quote_terminated_sentence() {
        let tokens = tokenize("He said \"stop!\" then left.");
        let words = spread(&tokens, 0.0, 3000.0);
        let segments = split_boundary(&words, Boundary::Sentence);
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn boundary_none_keeps_one_group() {
        let tokens = tokenize("One. Two!");
        let words = spread(&tokens, 0.0, 3000.0);
        let segments = split_boundary(&words, Boundary::None);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].len(), 2);
    }

    #[test]
    fn char_limit_greedy_split() {
        // 8 × 4 chars: first chunk takes 5 words (weight 24 ≤ 26, adding the
        // 6th makes 29), the tail of 3 words follows.
        let words: Vec<TimedWord> = (0..8).map(|_| word("aaaa")).collect();
        let chunks = split_char_limit(words, 26, 8);
        assert_eq!(
            chunks.iter().map(|c| c.len()).collect::<Vec<_>>(),
            vec![5, 3]
        );
    }

    #[test]
    fn char_limit_absorbs_tiny_tail() {
        // A 30-char first word forces the cut to kHigh=1, and the 1-char
        // tail can never reach minChars → candidates empty → the tail is
        // absorbed into the oversized chunk.
        let words = vec![word(&"a".repeat(30)), word("b")];
        let chunks = split_char_limit(words, 26, 8);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 2);
    }

    #[test]
    fn gap_free_bridges_short_gaps() {
        let doc = crate::CaptionDocument {
            duration_frames: 300,
            cues: vec![
                crate::Cue {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "One sentence.".into(),
                },
                crate::Cue {
                    start_ms: 1500,
                    end_ms: 2500,
                    text: "Second one.".into(),
                },
            ],
            ..Default::default()
        };
        let segments = collect_segments(&doc, &cfg());
        assert_eq!(segments.len(), 2);
        // 500ms gap ≤ 1s → first segment stretches to the second's start.
        assert!((segments[0].end_ms - 1500.0).abs() < 1e-3);
        // Last segment gets its 1s tail (words end at 2500 → 3500),
        // clamped to video duration (300 frames = 10s).
        assert!((segments[1].end_ms - 3500.0).abs() < 1e-3);
    }

    #[test]
    fn gap_free_keeps_far_apart_segments_separate() {
        let doc = crate::CaptionDocument {
            duration_frames: 3000,
            cues: vec![
                crate::Cue {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "One sentence.".into(),
                },
                crate::Cue {
                    start_ms: 4000,
                    end_ms: 5000,
                    text: "Far away.".into(),
                },
            ],
            ..Default::default()
        };
        let segments = collect_segments(&doc, &cfg());
        // Gap 3s > 1s → word span + exactly 1s of tail, then blank.
        assert!((segments[0].end_ms - 2000.0).abs() < 1e-3);
    }

    #[test]
    fn without_gap_free_the_window_is_the_word_span() {
        let doc = crate::CaptionDocument {
            duration_frames: 3000,
            cues: vec![
                crate::Cue {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "One sentence.".into(),
                },
                crate::Cue {
                    start_ms: 1500,
                    end_ms: 2500,
                    text: "Second one.".into(),
                },
            ],
            ..Default::default()
        };
        let no_gap = SplitConfig {
            effects: &[],
            ..cfg()
        };
        let segments = collect_segments(&doc, &no_gap);
        assert_eq!(segments.len(), 2);
        assert!((segments[0].start_ms - 0.0).abs() < 1e-3);
        assert!((segments[0].end_ms - 1000.0).abs() < 1e-3);
        assert!((segments[1].end_ms - 2500.0).abs() < 1e-3);
    }

    #[test]
    fn clause_boundary_splits_on_commas_too() {
        let tokens = tokenize("Wait, what. Fine, then!");
        let words = spread(&tokens, 0.0, 4000.0);
        let segments = split_boundary(&words, Boundary::Clause);
        let texts: Vec<String> = segments
            .iter()
            .map(|s| s.iter().map(|w| w.text.as_str()).collect::<Vec<_>>().join(" "))
            .collect();
        assert_eq!(texts, vec!["Wait,", "what.", "Fine,", "then!"]);
        // Sentence preset would keep the comma groups whole.
        let segments = split_boundary(&words, Boundary::Sentence);
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn display_defaults_to_text_and_survives_untouched() {
        let segments = {
            let doc = crate::CaptionDocument {
                duration_frames: 300,
                cues: vec![crate::Cue {
                    start_ms: 0,
                    end_ms: 1000,
                    text: "Hello.".into(),
                }],
                ..Default::default()
            };
            collect_segments(&doc, &cfg())
        };
        assert!(segments
            .iter()
            .flat_map(|s| &s.words)
            .all(|w| w.display == w.text));
    }
}
