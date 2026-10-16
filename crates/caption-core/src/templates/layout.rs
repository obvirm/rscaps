//! Shared layout helpers: XML escaping, fframes measurement replica,
//! LTR/RTL word placement, and the `balanced-pixel-width` line splitter.
//!
//! The splitter is a port of tscaps' `BalancedPixelWidthLineSplitter`
//! (behavioural facts; code written from scratch): a greedy pass finds the
//! minimum number of lines that fit `max_width`, a recursive balanced cut
//! targets that line count, and `min_lines` forces the longest line to split
//! until the count is met (a single word can never split).

use std::collections::HashMap;

// ── text helpers ──────────────────────────────────────────────────────────

/// `&`, `<` are the only chars SVG/XML needs escaped in text nodes; the
/// apostrophes and quotes stay readable.
pub fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

/// Replica of fframes' private `calc_text_width`, driven through the public
/// `FontSource`/`FontFace` traits so the measured string stays a local borrow
/// (`Frame::text_width` would tie it to the context lifetime).
pub fn measure_units(
    face: &dyn fframes::FontFace,
    variant: fframes::FontVariant,
    size: usize,
    text: &str,
) -> usize {
    match text {
        "" | "\n" | "\r" => 0,
        text => text
            .chars()
            .map(|c| match variant {
                fframes::FontVariant::Monospaced(mono_width) => mono_width,
                fframes::FontVariant::Variable => {
                    face.resolve_char_width(size, c).unwrap_or_default()
                }
            })
            .sum(),
    }
}

/// Normalisasi atribut `<text …>`: buang kembar, pertahankan nilai
/// PALING AKHIR (override element inspector menang). Tag `<text>` yang
/// tanpa duplikat disentuh apa adanya; tag lain (`<g>`, `<rect>`, …)
/// dilewat utuh.
///
/// Kenapa wajib: preview (parser HTML `{@html}`) memegang atribut PERTAMA,
/// sedangkan export memuat SVG lewat data-URI (parser XML) — duplikat
/// `fill=` membuat preview ≠ export dan bisa membuat gambar gagal muat.
pub fn dedupe_text_attrs(svg: &str) -> String {
    let mut out = String::with_capacity(svg.len());
    let mut rest = svg;
    while let Some(start) = rest.find("<text ") {
        out.push_str(&rest[..start]);
        let Some(rel_end) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let end = start + rel_end;
        out.push_str(&dedupe_text_tag(&rest[start..=end]));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn dedupe_text_tag(tag: &str) -> String {
    let Some(inner) = tag
        .strip_prefix("<text ")
        .and_then(|t| t.strip_suffix('>'))
    else {
        return tag.to_string();
    };
    let chars: Vec<char> = inner.chars().collect();
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i] == ' ' {
            i += 1;
        }
        let name_start = i;
        while i < chars.len() && chars[i] != '=' && chars[i] != ' ' {
            i += 1;
        }
        let name: String = chars[name_start..i].iter().collect();
        if i >= chars.len() || chars[i] != '=' {
            break;
        }
        i += 1;
        if i >= chars.len() || chars[i] != '"' {
            break;
        }
        i += 1;
        let val_start = i;
        while i < chars.len() && chars[i] != '"' {
            i += 1;
        }
        let val: String = chars[val_start..i].iter().collect();
        if i < chars.len() {
            i += 1;
        }
        if !name.is_empty() {
            attrs.push((name, val));
        }
    }
    let mut last: HashMap<&str, usize> = HashMap::new();
    for (idx, (name, _)) in attrs.iter().enumerate() {
        last.insert(name.as_str(), idx);
    }
    let mut rebuilt = String::new();
    let mut first = true;
    for (idx, (name, val)) in attrs.iter().enumerate() {
        if last[name.as_str()] != idx {
            continue;
        }
        if !first {
            rebuilt.push(' ');
        }
        first = false;
        rebuilt.push_str(name);
        rebuilt.push_str("=\"");
        rebuilt.push_str(val);
        rebuilt.push('"');
    }
    format!("<text {rebuilt}>")
}

// ── word placement ────────────────────────────────────────────────────────

/// Left edge x of every word: LTR walks left→right from the line start,
/// RTL mirrors the walk so the first logical word sits at the right end.
/// `margin` is one side of the `.word` margin; adjacent words are separated
/// by `2 * margin` (inline-block margins never collapse).
pub fn word_x_positions(
    widths: &[f32],
    margin: f32,
    line_left: f32,
    line_w: f32,
    rtl: bool,
) -> Vec<f32> {
    let mut xs = Vec::with_capacity(widths.len());
    if rtl {
        let mut right = line_left + line_w;
        for &w in widths {
            xs.push(right - margin - w);
            right -= w + 2.0 * margin;
        }
    } else {
        let mut cursor = line_left;
        for &w in widths {
            xs.push(cursor + margin);
            cursor += w + 2.0 * margin;
        }
    }
    xs
}

// ── balanced-pixel-width line splitter ────────────────────────────────────

/// `lineSplitter: { type: "balanced-pixel-width", minLines, maxLines,
/// maxWidthRatio }` — the consumer resolves the pixel `max_width` from the
/// ratio before constructing this.
#[derive(Debug, Clone, Copy)]
pub struct BalancedPixelWidth {
    pub min_lines: usize,
    pub max_lines: usize,
    pub max_width: f32,
}

impl BalancedPixelWidth {
    /// Splits `widths` (one entry per word, each carrying its own margins)
    /// into line ranges `(start, end_exclusive)` over the same indices.
    pub fn split(&self, widths: &[f32]) -> Vec<(usize, usize)> {
        let n = widths.len();
        if n == 0 {
            return Vec::new();
        }

        // Prefix sums: rangeWidth(a, b inclusive) = prefix[b+1] - prefix[a].
        let mut prefix = Vec::with_capacity(n + 1);
        prefix.push(0.0f32);
        for &w in widths {
            prefix.push(prefix.last().unwrap() + w);
        }
        let range = |a: usize, b: usize| prefix[b + 1] - prefix[a];

        // Greedy pass → minimum number of lines that fit maxWidth.
        let mut lines_needed = 1;
        let mut line_start = 0;
        for i in 1..n {
            if range(line_start, i) > self.max_width && i > line_start {
                lines_needed += 1;
                line_start = i;
            }
        }

        let target_lines = lines_needed.min(self.max_lines.max(1));
        let mut lines = Vec::with_capacity(target_lines.max(1));
        if target_lines <= 1 {
            lines.push((0, n));
        } else {
            Self::split_recursive(&range, 0, n, target_lines, &mut lines);
        }
        Self::adjust_to_min_lines(&mut lines, self.min_lines);
        lines
    }

    /// Balanced cut: pick the split whose prefix closest to
    /// `totalWidth / linesLeft`, then recurse on the tail.
    fn split_recursive(
        range: &impl Fn(usize, usize) -> f32,
        offset: usize,
        count: usize,
        lines_left: usize,
        out: &mut Vec<(usize, usize)>,
    ) {
        if lines_left <= 1 || count == 0 {
            if count > 0 {
                out.push((offset, offset + count));
            }
            return;
        }
        let total = range(offset, offset + count - 1);
        let target = total / lines_left as f32;
        let mut best_split = 1usize;
        let mut best_diff = f32::INFINITY;
        for i in 0..count - 1 {
            let cum = range(offset, offset + i);
            let diff = (cum - target).abs();
            if diff < best_diff {
                best_diff = diff;
                best_split = i + 1;
            }
        }
        out.push((offset, offset + best_split));
        Self::split_recursive(range, offset + best_split, count - best_split, lines_left - 1, out);
    }

    /// Grow `lines` to `min_lines` by halving the line with the most words
    /// (first on ties); a line of one word can never split.
    fn adjust_to_min_lines(lines: &mut Vec<(usize, usize)>, min_lines: usize) {
        while lines.len() < min_lines {
            let mut longest_idx = 0;
            for i in 1..lines.len() {
                if (lines[i].1 - lines[i].0) > (lines[longest_idx].1 - lines[longest_idx].0) {
                    longest_idx = i;
                }
            }
            let (start, end) = lines[longest_idx];
            let count = end - start;
            if count <= 1 {
                break;
            }
            let mid = count / 2;
            lines.splice(longest_idx..longest_idx + 1, [(start, start + mid), (start + mid, end)]);
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupe_text_attrs_keeps_last_value() {
        let svg = "<svg><g><text x=\"1\" fill=\"#fff\" fill=\"#f00\" y=\"2\">hi</text></g></svg>";
        let out = dedupe_text_attrs(svg);
        assert_eq!(
            out,
            "<svg><g><text x=\"1\" fill=\"#f00\" y=\"2\">hi</text></g></svg>"
        );
        // Tag tanpa duplikat + tag non-text tak tersentuh.
        let plain = "<text x=\"1\" y=\"2\">ok</text><rect fill=\"a\" fill=\"b\"/>";
        assert_eq!(dedupe_text_attrs(plain), plain);
    }

    #[test]
    fn escape_xml_only_needs_amp_and_lt() {
        assert_eq!(escape_xml("a & b < c"), "a &amp; b &lt; c");
        assert_eq!(escape_xml("don't \"quote\" > x"), "don't \"quote\" > x");
    }

    #[test]
    fn rtl_positions_walk_right_to_left() {
        let widths = [10.0, 20.0, 30.0];
        let ltr = word_x_positions(&widths, 1.0, 0.0, 66.0, false);
        assert_eq!(ltr, vec![1.0, 13.0, 35.0]);
        let rtl = word_x_positions(&widths, 1.0, 0.0, 66.0, true);
        // First logical word lands rightmost; each next word goes left.
        assert_eq!(rtl, vec![55.0, 33.0, 1.0]);
    }

    fn splitter(min: usize, max: usize, max_w: f32) -> BalancedPixelWidth {
        BalancedPixelWidth {
            min_lines: min,
            max_lines: max,
            max_width: max_w,
        }
    }

    #[test]
    fn empty_segment_yields_no_lines() {
        assert_eq!(splitter(2, 6, 900.0).split(&[]), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn min_lines_splits_a_single_fitting_line() {
        // 2 words fit maxWidth → greedy says 1 line → adjustToMinLines
        // halves it anyway (minLines is a hard floor).
        assert_eq!(splitter(2, 6, 900.0).split(&[400.0, 400.0]), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn min_lines_splits_the_longest_line_in_half() {
        // One line of 3 fits maxWidth (300 ≤ 900) → target 1 → adjust:
        // 3 words → mid 1 → (0,1) + (1,3).
        assert_eq!(splitter(2, 6, 900.0).split(&[100.0, 100.0, 100.0]), vec![(0, 1), (1, 3)]);
        // The 2-line result is stable (longest now has 2 words, len ≥ 2).
    }

    #[test]
    fn min_lines_stops_at_single_word_lines() {
        // minLines 4, only 2 words → each would need to split, can't.
        assert_eq!(splitter(4, 6, 900.0).split(&[100.0, 100.0]), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn greedy_finds_minimum_lines_then_balances() {
        // 4 × 400 with maxWidth 900: pair per line (800 ≤ 900 < 1200).
        assert_eq!(splitter(1, 6, 900.0).split(&[400.0, 400.0, 400.0, 400.0]), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn max_lines_caps_the_greedy_target() {
        // 8 words × 100 with maxWidth 100 → greedy wants 8 lines, capped
        // at 6 → balanced recursion yields exactly 6.
        let lines = splitter(1, 6, 100.0).split(&[100.0; 8]);
        assert_eq!(lines.len(), 6);
        // Contiguous, ordered, covering everything.
        let mut cursor = 0;
        for (s, e) in &lines {
            assert_eq!(*s, cursor);
            assert!(e > s);
            cursor = *e;
        }
        assert_eq!(cursor, 8);
    }

    #[test]
    fn balanced_cut_prefers_target_evenness() {
        // total 1000, target 500 → split exactly at 500.
        assert_eq!(splitter(1, 2, 400.0).split(&[200.0, 300.0, 500.0]), vec![(0, 2), (2, 3)]);
    }
}
