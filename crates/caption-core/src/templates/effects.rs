//! Document-effect ports (tscaps `modules/effect/*`) applied to the
//! post-split word groups before windowing.
//!
//! Fact sources: `GapFreeEffect.ts` (window stage lives in
//! [`crate::templates::pipeline::collect_segments`]),
//! `RemovePunctuationEffect.ts`, `CarryQuotesEffect.ts`,
//! `SmartPunctuationEffect.ts`, `SmartLowercaseEffect.ts`.
//!
//! Declaration order matters: `DocumentDeriver` applies effects in
//! `template.json` `effects` array order (sheet-then-effect). Templates
//! hand their list to [`crate::templates::pipeline::SplitConfig`] and
//! [`apply_text_effects`] runs it verbatim. Splitters see raw `text`;
//! effects rewrite `display` only, and line splitting (render-time
//! measurement) reads the display.

use super::pipeline::TimedWord;

/// One entry of a template's `effects` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Extends segment windows so they do not snap shut at the last word.
    /// Handled by the window stage, not [`apply_text_effects`].
    GapFree,
    /// `RemovePunctuationEffect`: strips trailing punctuation from the
    /// display text (trailing quotes survive and are stripped past).
    RemovePunctuation,
    /// `CarryQuotesEffect`: repeats surrounding quotes onto every segment
    /// of a quotation split across segment boundaries.
    CarryQuotes,
    /// `SmartPunctuationEffect`: ASCII punctuation → typographic forms.
    SmartPunctuation,
    /// `SmartLowercaseEffect`: lowercases display text, preserving the
    /// English pronoun `I` / `I'm` (tag preservation needs a tagger — v1
    /// has none).
    SmartLowercase,
}

/// Runs the text-effect list in declaration order over the segment groups.
/// `GapFree` entries are ignored here (window stage).
pub fn apply_text_effects(groups: &mut [Vec<TimedWord>], effects: &[Effect]) {
    for effect in effects {
        match effect {
            Effect::GapFree => {}
            Effect::RemovePunctuation => {
                for group in groups.iter_mut() {
                    for w in group.iter_mut() {
                        let stripped = strip_trailing_punct(&w.text);
                        if stripped != w.display {
                            w.display = stripped;
                        }
                    }
                }
            }
            Effect::SmartPunctuation => {
                for group in groups.iter_mut() {
                    for w in group.iter_mut() {
                        w.display = smartify(&w.display);
                    }
                }
            }
            Effect::SmartLowercase => {
                for group in groups.iter_mut() {
                    for w in group.iter_mut() {
                        smart_lowercase(w);
                    }
                }
            }
            Effect::CarryQuotes => carry_quotes(groups),
        }
    }
}

// ── RemovePunctuationEffect ───────────────────────────────────────────────

// JS regex punct class (6): "..." "." "," ";" ":" "…" (U+2026).
const TRAILING_QUOTES: [char; 6] = ['"', '\u{201C}', '\u{201D}', '\'', '\u{2018}', '\u{2019}'];

/// `(?:punct)+(?=(?:quote)*$)` on `text`, without lookaround: find the
/// trailing-quote suffix, then strip the punctuation run directly before it.
fn strip_trailing_punct(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut k = chars.len();
    while k > 0 && TRAILING_QUOTES.contains(&chars[k - 1]) {
        k -= 1;
    }
    let mut p = k;
    while p > 0 && punct_char(chars[p - 1]) {
        p -= 1;
    }
    if p == k {
        return text.to_string();
    }
    chars[..p].iter().chain(chars[k..].iter()).collect()
}

fn punct_char(c: char) -> bool {
    c == '.' || c == ',' || c == ';' || c == ':' || c == '\u{2026}'
}

// ── SmartPunctuationEffect ────────────────────────────────────────────────

fn smartify(text: &str) -> String {
    let expanded = replace_multi_char_glyphs(text);
    replace_quotes(&expanded)
}

/// `...` → `…` (single left-to-right pass, like JS `String.replace`),
/// `--` → `—`.
fn replace_multi_char_glyphs(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '.' && i + 2 < chars.len() && chars[i + 1] == '.' && chars[i + 2] == '.' {
            out.push('\u{2026}');
            i += 3;
        } else if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            out.push('\u{2014}');
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// `"` → `“` when the next char is a letter or digit, `”` otherwise;
/// `'` → `’`.
fn replace_quotes(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '"' => {
                let next = chars.get(i + 1).copied().unwrap_or('\0');
                out.push(if next.is_alphanumeric() {
                    '\u{201C}'
                } else {
                    '\u{201D}'
                });
            }
            '\'' => out.push('\u{2019}'),
            _ => out.push(c),
        }
    }
    out
}

// ── SmartLowercaseEffect ──────────────────────────────────────────────────

fn smart_lowercase(w: &mut TimedWord) {
    if i_pronoun(&w.text) {
        return;
    }
    let lowered = w.display.to_lowercase();
    if lowered != w.display {
        w.display = lowered;
    }
}

/// JS `/^I(?:$|['’])/`.
fn i_pronoun(text: &str) -> bool {
    let mut it = text.chars();
    if it.next() != Some('I') {
        return false;
    }
    match it.next() {
        None => true,
        Some('\'') | Some('\u{2019}') => true,
        _ => false,
    }
}

// ── CarryQuotesEffect ─────────────────────────────────────────────────────

const QUOTE_PAIRS: [(char, char); 3] = [
    ('"', '"'),
    ('\u{201C}', '\u{201D}'),
    ('\u{00AB}', '\u{00BB}'),
];

/// Repeats a quotation's surrounding quotes on every segment it spans —
/// only when the run crosses two or more segments. Operates on `display`
/// (matching the engine's word edits) but detects pairs from `text`.
fn carry_quotes(groups: &mut [Vec<TimedWord>]) {
    // Flatten: (segment index, word index, text).
    let mut flat: Vec<(usize, usize, &str)> = Vec::new();
    for (seg, group) in groups.iter().enumerate() {
        for (wi, w) in group.iter().enumerate() {
            flat.push((seg, wi, w.text.as_str()));
        }
    }

    #[derive(Default, Clone, Copy)]
    struct Edit {
        prepend: Option<char>,
        append: Option<char>,
    }
    let mut edits: Vec<Vec<Edit>> = groups
        .iter()
        .map(|g| vec![Edit::default(); g.len()])
        .collect();

    let mut any = false;
    let mut i = 0;
    while i < flat.len() {
        let opener = flat[i].2.chars().next().and_then(|c| {
            QUOTE_PAIRS.iter().find(|(k, _)| *k == c).map(|&(k, v)| (k, v))
        });
        let Some((open, close)) = opener else {
            i += 1;
            continue;
        };
        // Scan forward for the word that ends the run (inclusive).
        let mut k = i;
        while k < flat.len() && !flat[k].2.ends_with(close) {
            k += 1;
        }
        if k >= flat.len() {
            // Unclosed run: no edits (engine drops it too).
            i += 1;
            continue;
        }
        let run = &flat[i..=k];
        let crosses = run.iter().any(|&(seg, _, _)| seg != run[0].0);
        if crosses {
            any = true;
            // Segment groups of the run in encounter order; first/last word
            // index within each group.
            let mut seg_order: Vec<usize> = Vec::new();
            let mut first_last: std::collections::HashMap<usize, (usize, usize)> =
                std::collections::HashMap::new();
            for &(seg, wi, _) in run {
                if seg_order.last() != Some(&seg) {
                    seg_order.push(seg);
                }
                let e = first_last.entry(seg).or_insert((wi, wi));
                e.1 = wi;
            }
            for (n, &seg) in seg_order.iter().enumerate() {
                let (first_wi, last_wi) = first_last[&seg];
                if n > 0 {
                    edits[seg][first_wi].prepend = Some(open);
                }
                if n + 1 < seg_order.len() {
                    edits[seg][last_wi].append = Some(close);
                }
            }
        }
        i = k + 1;
    }

    if !any {
        return;
    }
    for (seg, group) in groups.iter_mut().enumerate() {
        for (wi, w) in group.iter_mut().enumerate() {
            let e = edits[seg][wi];
            if let Some(c) = e.prepend {
                w.display = format!("{c}{}", w.display);
            }
            if let Some(c) = e.append {
                w.display.push(c);
            }
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str) -> TimedWord {
        TimedWord::new(text, 0.0, 100.0)
    }

    #[test]
    fn remove_punctuation_strips_trailing_run_but_keeps_quotes() {
        assert_eq!(strip_trailing_punct("end."), "end");
        assert_eq!(strip_trailing_punct("end.\""), "end\"");
        assert_eq!(strip_trailing_punct("said \"hello\"."), "said \"hello\"");
        assert_eq!(strip_trailing_punct("wait\u{2026}"), "wait");
        // Inner symbols survive.
        assert_eq!(strip_trailing_punct("google.com"), "google.com");
        assert_eq!(strip_trailing_punct("I'm"), "I'm");
        // Trailing quotes alone are kept (no punct run before them).
        assert_eq!(strip_trailing_punct("stop\u{201D}"), "stop\u{201D}");
    }

    #[test]
    fn smart_punctuation_typography() {
        assert_eq!(smartify("wait... \"hi\" it's -- ok"),
            "wait\u{2026} \u{201C}hi\u{201D} it\u{2019}s \u{2014} ok");
        // Quote before a letter opens; otherwise closes.
        assert_eq!(smartify("\"a"), "\u{201C}a");
        assert_eq!(smartify("a\""), "a\u{201D}");
        // Multi-char pass is one left-to-right sweep: "....." → "….".
        assert_eq!(smartify("....."), "\u{2026}..");
    }

    #[test]
    fn smart_lowercase_keeps_pronoun_i() {
        let mut w = word("I'm");
        smart_lowercase(&mut w);
        assert_eq!(w.display, "I'm");
        let mut w = word("Hello");
        smart_lowercase(&mut w);
        assert_eq!(w.display, "hello");
    }

    #[test]
    fn carry_quotes_repeats_on_split_segments() {
        let mut groups = vec![
            vec![word("\"hello"), word("world")],
            vec![word("again\""), word("yes")],
        ];
        carry_quotes(&mut groups);
        // Last word of every non-last segment of the run gets the closer,
        // first word of every non-first segment the opener.
        assert_eq!(groups[0][1].display, "world\"");
        assert_eq!(groups[1][0].display, "\"again\"");
        assert_eq!(groups[0][0].display, "\"hello");
        assert_eq!(groups[1][1].display, "yes");
    }

    #[test]
    fn carry_quotes_single_segment_run_is_left_alone() {
        let mut groups = vec![vec![word("\"all"), word("here\"")]];
        carry_quotes(&mut groups);
        assert_eq!(groups[0][0].display, "\"all");
        assert_eq!(groups[0][1].display, "here\"");
    }

    #[test]
    fn apply_runs_in_declared_order() {
        // remove before smart: "Hi..." → strip → "Hi" → smart no-op.
        let mut groups = vec![vec![word("Hi...")]];
        apply_text_effects(
            &mut groups,
            &[Effect::RemovePunctuation, Effect::SmartPunctuation],
        );
        assert_eq!(groups[0][0].display, "Hi");
    }
}
