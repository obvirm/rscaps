//! SubRip (.srt) and WebVTT (.vtt) parsing/formatting.
//!
//! One source of truth for both preview import and export: the studio reads a
//! file the user brought, the same code turns a `CaptionDocument` back into a
//! subtitle file. Mirrors what tscaps' `EngineSubtitleFileReader` does on the
//! TypeScript side, but lives next to the cues it feeds.
//!
//! Import decisions (match how browsers/tscaps treat these files):
//! - `HH:MM:SS,mmm` and `HH:MM:SS.mmm` both accepted; VTT short `MM:SS.mmm` too.
//! - VTT cue settings after the timestamp (`line:`, `align:` …) are dropped.
//! - `<c>`-style VTT tags and SubRip `{...}` overrides are stripped from text —
//!   neither survives a round-trip through plain cues, and the render pipeline
//!   would strip them anyway (`pipeline::strip_markup`).
//! - Blocks with no readable timestamp are a hard error (same guidance
//!   tscaps gives: "export it again as SubRip (.srt) or WebVTT (.vtt)").
//! - Cue blocks with empty text after stripping are skipped, not errors.
//!
//! Export: 1-based index for SRT, `WEBVTT` header + `.` ms for VTT, `\n`
//! line endings (parsers accept both).

use crate::Cue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Srt,
    Vtt,
}

impl Format {
    /// `"srt"` / `"vtt"` (case-insensitive) → format. Anything else is an error
    /// so a typo in the UI can't silently pick the wrong serializer.
    pub fn parse_str(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "srt" => Ok(Self::Srt),
            "vtt" => Ok(Self::Vtt),
            other => Err(format!("unsupported subtitle format: {other}")),
        }
    }
}

/// One subtitle file, ready to merge into a `CaptionDocument`.
#[derive(Debug, Clone)]
pub struct Parsed {
    pub cues: Vec<Cue>,
    /// Frame count covering the last cue end (30 fps, rounded up, floor 900 —
    /// the document default — so a 2 s file doesn't shrink the timeline).
    pub duration_frames: u32,
}

const FPS: f64 = 30.0;
const MIN_DURATION_FRAMES: u32 = 900;

pub fn parse(text: &str, _format: Format) -> Result<Parsed, String> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cues = Vec::new();
    let mut saw_timestamp = false;
    let mut block_index = 0usize;

    for block in normalized.split("\n\n") {
        block_index += 1;
        let lines: Vec<&str> = block.lines().map(str::trim_end).collect();
        if lines.iter().all(|line| line.trim().is_empty()) {
            continue;
        }
        // VTT header / NOTE / STYLE / REGION blocks carry no cue timing.
        let first = lines[0].trim();
        if first.starts_with("WEBVTT")
            || first.starts_with("NOTE")
            || first.starts_with("STYLE")
            || first.starts_with("REGION")
        {
            continue;
        }

        // Optional numeric index line, then the timing line.
        let mut cursor = 0usize;
        if !looks_like_timing(lines[cursor]) {
            if lines.len() >= 2 && looks_like_timing(lines[1]) {
                cursor = 1;
            } else {
                return Err(format!(
                    "block {block_index} has no timestamp like 00:00:01,000 --> 00:00:04,000. \
                     If this is a transcript or script, export it again as SubRip (.srt) or WebVTT (.vtt)."
                ));
            }
        }
        let (start_ms, end_ms) = parse_timing(lines[cursor]).ok_or_else(|| {
            format!("block {block_index}: unreadable timestamp `{}`", lines[cursor].trim())
        })?;
        saw_timestamp = true;

        let body = lines[cursor + 1..].join("\n");
        let clean = strip_markup(&body);
        if clean.is_empty() {
            continue;
        }
        cues.push(Cue {
            start_ms,
            end_ms,
            text: clean,
        });
    }

    if !saw_timestamp {
        return Err(
            "no timestamped cue found — expected lines like 00:00:01,000 --> 00:00:04,000".to_string(),
        );
    }

    let duration_frames = cues
        .iter()
        .map(|cue| (f64::from(cue.end_ms) / 1000.0 * FPS).ceil() as u32)
        .max()
        .unwrap_or(0)
        .max(MIN_DURATION_FRAMES);

    Ok(Parsed {
        cues,
        duration_frames,
    })
}

pub fn format(cues: &[Cue], format: Format) -> String {
    let mut out = String::new();
    if format == Format::Vtt {
        out.push_str("WEBVTT\n\n");
    }
    for (i, cue) in cues.iter().enumerate() {
        let (sep, index_line) = match format {
            Format::Srt => (',', Some((i + 1).to_string())),
            Format::Vtt => ('.', None),
        };
        if let Some(index) = index_line {
            out.push_str(&index);
            out.push('\n');
        }
        out.push_str(&format_timing(cue.start_ms, sep));
        out.push_str(" --> ");
        out.push_str(&format_timing(cue.end_ms, sep));
        out.push('\n');
        out.push_str(&cue.text);
        out.push_str("\n\n");
    }
    out
}

fn looks_like_timing(line: &str) -> bool {
    let line = line.trim();
    let arrow = match line.find("-->") {
        Some(idx) => idx,
        None => return false,
    };
    parse_ts(line[..arrow].trim()).is_some() && !line[arrow + 3..].trim().is_empty()
}

fn parse_timing(line: &str) -> Option<(u32, u32)> {
    let arrow = line.find("-->")?;
    let start = parse_ts(line[..arrow].trim())?;
    // VTT allows cue settings after the end timestamp — cut them off.
    let rest = line[arrow + 3..].trim();
    let end_token = rest.split_whitespace().next()?;
    let end = parse_ts(end_token)?;
    Some((start, end))
}

/// `HH:MM:SS,mmm` / `HH:MM:SS.mmm`, or VTT's short `MM:SS.mmm`.
fn parse_ts(value: &str) -> Option<u32> {
    let value = value.trim();
    let (hms, ms) = value.rsplit_once([',', '.'])?;
    if ms.is_empty() || ms.len() > 3 || !ms.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits: u32 = ms.parse().ok()?;
    let ms = digits * match ms.len() {
        1 => 100,
        2 => 10,
        _ => 1,
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let (h, m, s) = match parts.as_slice() {
        [h, m, s] => (*h, *m, *s),
        [m, s] => ("0", *m, *s),
        _ => return None,
    };
    if m.parse::<u32>().ok()? >= 60 || s.parse::<u32>().ok()? >= 60 {
        return None;
    }
    let total = h.parse::<u32>().ok()? * 3_600_000
        + m.parse::<u32>().ok()? * 60_000
        + s.parse::<u32>().ok()? * 1_000
        + ms;
    Some(total)
}

fn format_timing(ms: u32, sep: char) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1_000;
    let frac = ms % 1_000;
    format!("{h:02}:{m:02}:{s:02}{sep}{frac:03}")
}

/// Drop VTT inline tags and SubRip brace overrides, then trim.
fn strip_markup(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                for next in chars.by_ref() {
                    if next == '>' {
                        break;
                    }
                }
            }
            '{' => {
                for next in chars.by_ref() {
                    if next == '}' {
                        break;
                    }
                }
            }
            _ => out.push(ch),
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(start: u32, end: u32, text: &str) -> Cue {
        Cue {
            start_ms: start,
            end_ms: end,
            text: text.to_string(),
        }
    }

    #[test]
    fn parses_srt_block_with_index() {
        let parsed = parse(
            "1\n00:00:01,000 --> 00:00:04,000\nHello world\n\n2\n00:00:05,500 --> 00:00:07,250\nSecond cue\n",
            Format::Srt,
        )
        .unwrap();
        assert_eq!(
            parsed.cues,
            vec![cue(1_000, 4_000, "Hello world"), cue(5_500, 7_250, "Second cue")]
        );
        assert_eq!(parsed.duration_frames, 900); // 7.25 s < 30 s floor
    }

    #[test]
    fn parses_vtt_header_and_dot_milliseconds() {
        let parsed = parse(
            "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\nHello\n\nNOTE a comment\nignored\n\n00:00:05.000 --> 00:00:06.500\nBye\n",
            Format::Vtt,
        )
        .unwrap();
        assert_eq!(parsed.cues.len(), 2);
        assert_eq!(parsed.cues[0].text, "Hello");
        assert_eq!(parsed.cues[1].end_ms, 6_500);
    }

    #[test]
    fn strips_vtt_tags_and_brace_overrides() {
        let parsed = parse(
            "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<c.loud>Hi</c> {\\an8}there\n",
            Format::Vtt,
        )
        .unwrap();
        assert_eq!(parsed.cues[0].text, "Hi there");
    }

    #[test]
    fn multiline_cue_text_joined() {
        let parsed = parse(
            "1\n00:00:01,000 --> 00:00:03,000\nline one\nline two\n",
            Format::Srt,
        )
        .unwrap();
        assert_eq!(parsed.cues[0].text, "line one\nline two");
    }

    #[test]
    fn errors_when_no_timestamp_anywhere() {
        let err = parse("just some notes\n\nand more notes", Format::Srt).unwrap_err();
        assert!(err.contains("timestamp"), "{err}");
        assert!(err.contains(".srt"), "{err}");
    }

    #[test]
    fn errors_on_block_without_timing() {
        let err = parse("1\nnot a timestamp\ntext\n", Format::Srt).unwrap_err();
        assert!(err.contains("block 1"), "{err}");
    }

    #[test]
    fn skips_empty_cue_text() {
        let parsed = parse("1\n00:00:01,000 --> 00:00:02,000\n\n2\n00:00:03,000 --> 00:00:04,000\nok\n", Format::Srt).unwrap();
        assert_eq!(parsed.cues.len(), 1);
        assert_eq!(parsed.cues[0].text, "ok");
    }

    #[test]
    fn vtt_cue_settings_after_timestamp_ignored() {
        let parsed = parse(
            "WEBVTT\n\n00:00:01.000 --> 00:00:04.000 line:90% align:center\nHi\n",
            Format::Vtt,
        )
        .unwrap();
        assert_eq!(parsed.cues[0].end_ms, 4_000);
    }

    #[test]
    fn srt_roundtrip_preserves_cues() {
        let cues = vec![
            cue(0, 2_500, "Welcome to rscaps"),
            cue(3_000, 6_400, "خط عربي للاختبار"),
            cue(7_000, 9_999, "second\nline"),
        ];
        let text = format(&cues, Format::Srt);
        assert!(text.contains("00:00:00,000 --> 00:00:02,500"));
        assert!(text.starts_with("1\r\n") || text.starts_with("1\n"));
        let back = parse(&text, Format::Srt).unwrap();
        assert_eq!(back.cues, cues);
    }

    #[test]
    fn vtt_roundtrip_has_header_and_dot_ms() {
        let cues = vec![cue(1_500, 4_000, "Héllo")];
        let text = format(&cues, Format::Vtt);
        assert!(text.starts_with("WEBVTT"));
        assert!(text.contains("00:00:01.500 --> 00:00:04.000"));
        let back = parse(&text, Format::Vtt).unwrap();
        assert_eq!(back.cues, cues);
    }

    #[test]
    fn format_str_rejects_unknown() {
        assert!(Format::parse_str("ass").is_err());
        assert_eq!(Format::parse_str("SRT").unwrap(), Format::Srt);
    }

    #[test]
    fn hours_above_99_parse() {
        assert_eq!(parse_ts("100:00:00,000"), Some(360_000_000));
    }

    #[test]
    fn json_wrappers_roundtrip() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\nWelcome to rscaps\n";
        let json = crate::parse_subtitle_json(srt, "srt").unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        let cues_json = parsed["cues"].to_string();
        let out = crate::format_subtitle_json(&cues_json, "vtt").unwrap();
        assert!(out.starts_with("WEBVTT"));
        assert!(out.contains("Welcome to rscaps"));
        assert!(crate::format_subtitle_json(&cues_json, "ass").is_err());
    }
}
