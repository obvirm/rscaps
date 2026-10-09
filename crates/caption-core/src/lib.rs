use fframes::{
    AudioMap, BreakLinesOpts, Color, Duration, FFramesContext, FontQuery, Frame, Svgr, TextAlign,
    Transform, Video,
};
use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};

pub mod srt;
pub mod templates;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cue {
    pub start_ms: u32,
    pub end_ms: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionParams {
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_font_weight")]
    pub font_weight: u16,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_y_frac")]
    pub y_frac: f32,
    #[serde(default = "default_width_frac")]
    pub width_frac: f32,
    #[serde(default = "default_pop_in_ms")]
    pub pop_in_ms: u32,
    #[serde(default = "default_travel_px")]
    pub travel_px: f32,
    /// Base text direction: `"auto"` (strong-char majority over the whole
    /// document), `"ltr"`, or `"rtl"`. `"rtl"` is the right knob for Arabic/
    /// Hebrew content — mixed lines then keep the declared RTL base and the
    /// word highlight follows spoken order (right→left, jumping across an
    /// embedded Latin run) instead of flipping the whole line to LTR.
    #[serde(default = "default_direction")]
    pub direction: String,
}

fn default_font_family() -> String {
    "Arial Black".to_string()
}
fn default_font_size() -> f32 {
    96.0
}
fn default_font_weight() -> u16 {
    700
}
fn default_color() -> String {
    "#ffffff".to_string()
}
fn default_y_frac() -> f32 {
    0.82
}
fn default_width_frac() -> f32 {
    0.8
}
fn default_pop_in_ms() -> u32 {
    220
}
fn default_travel_px() -> f32 {
    44.0
}
fn default_direction() -> String {
    "auto".to_string()
}

impl Default for CaptionParams {
    fn default() -> Self {
        Self {
            font_family: default_font_family(),
            font_size: default_font_size(),
            font_weight: default_font_weight(),
            color: default_color(),
            y_frac: default_y_frac(),
            width_frac: default_width_frac(),
            pop_in_ms: default_pop_in_ms(),
            travel_px: default_travel_px(),
            direction: default_direction(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptionDocument {
    #[serde(default = "default_duration_frames")]
    pub duration_frames: u32,
    #[serde(default)]
    pub cues: Vec<Cue>,
    #[serde(default)]
    pub params: CaptionParams,
    /// Native template id; empty falls back to the v0 renderer.
    #[serde(default)]
    pub template: String,
}

fn default_duration_frames() -> u32 {
    900
}

impl Default for CaptionDocument {
    fn default() -> Self {
        Self {
            duration_frames: default_duration_frames(),
            cues: Vec::new(),
            params: CaptionParams::default(),
            template: String::new(),
        }
    }
}

static DOC: OnceLock<RwLock<CaptionDocument>> = OnceLock::new();

// BreakLinesOpts needs &'a str (tied to the frame context lifetime), so the
// dynamic family/color strings are leaked once per document update instead of
// being borrowed from the document lock guard.
static LIVE_FONT_FAMILY: RwLock<&'static str> = RwLock::new("Arial Black");
static LIVE_TEXT_COLOR: RwLock<&'static str> = RwLock::new("#ffffff");

fn leak_str(value: &str) -> &'static str {
    Box::leak(value.to_string().into_boxed_str())
}

fn live_family() -> &'static str {
    LIVE_FONT_FAMILY.read().map(|guard| *guard).unwrap_or("Arial Black")
}

fn live_color() -> &'static str {
    LIVE_TEXT_COLOR.read().map(|guard| *guard).unwrap_or("#ffffff")
}


pub fn document() -> &'static RwLock<CaptionDocument> {
    DOC.get_or_init(|| RwLock::new(CaptionDocument::default()))
}

pub fn set_document_json(json: &str) -> Result<(), String> {
    let parsed: CaptionDocument =
        serde_json::from_str(json).map_err(|err| format!("invalid document: {err}"))?;
    if let Ok(mut guard) = LIVE_FONT_FAMILY.write() {
        *guard = leak_str(&parsed.params.font_family);
    }
    if let Ok(mut guard) = LIVE_TEXT_COLOR.write() {
        *guard = leak_str(&parsed.params.color);
    }
    let mut guard = document()
        .write()
        .map_err(|_| "document lock poisoned".to_string())?;
    *guard = parsed;
    Ok(())
}

pub fn replace_cues_json(json: &str) -> Result<(), String> {
    let cues: Vec<Cue> =
        serde_json::from_str(json).map_err(|err| format!("invalid cues: {err}"))?;
    let mut guard = document()
        .write()
        .map_err(|_| "document lock poisoned".to_string())?;
    guard.cues = cues;
    Ok(())
}

/// Parse a subtitle file (`.srt` / `.vtt`) → JSON
/// `{"duration_frames": u32, "cues": [Cue, …]}` for the editor to merge.
/// Pure: the live document is untouched, the caller decides.
pub fn parse_subtitle_json(text: &str, format: &str) -> Result<String, String> {
    let format = srt::Format::parse_str(format)?;
    let parsed = srt::parse(text, format)?;
    let value = serde_json::json!({
        "duration_frames": parsed.duration_frames,
        "cues": parsed.cues,
    });
    serde_json::to_string(&value).map_err(|err| format!("serialize cues: {err}"))
}

/// Cues JSON (`[Cue, …]`) → subtitle file text. `format` is `"srt"` or `"vtt"`.
pub fn format_subtitle_json(cues_json: &str, format: &str) -> Result<String, String> {
    let format = srt::Format::parse_str(format)?;
    let cues: Vec<Cue> =
        serde_json::from_str(cues_json).map_err(|err| format!("invalid cues: {err}"))?;
    Ok(srt::format(&cues, format))
}

#[derive(Debug, Default)]
pub struct CaptionVideo;

impl Video for CaptionVideo {
    const FPS: usize = 30;
    const WIDTH: usize = 1920;
    const HEIGHT: usize = 1080;
    const BACKGROUND_COLOR: Color = Color::TRANSPARENT;

    fn duration(&self) -> Duration<'_> {
        let frames = document()
            .read()
            .map(|doc| doc.duration_frames as usize)
            .unwrap_or_else(|_| default_duration_frames() as usize);
        Duration::Frames(frames)
    }

    fn audio(&self) -> AudioMap<'_> {
        AudioMap::none()
    }

    fn render_frame<'a>(&'a self, mut frame: Frame, ctx: &FFramesContext<'a, '_>) -> Svgr<'a> {
        let Ok(doc) = document().read() else {
            return Svgr::empty();
        };

        let now_ms = frame.seconds() * 1000.0;

        // Native templates own their whole document window (gap-free tails
        // cross cue boundaries), so they run before the per-cue lookup.
        if !doc.template.is_empty() {
            if let Some(svg) = templates::render(&doc.template, &doc, now_ms, ctx) {
                return svg;
            }
        }

        let Some(cue) = doc
            .cues
            .iter()
            .find(|cue| now_ms >= cue.start_ms as f32 && now_ms < cue.end_ms as f32)
        else {
            return Svgr::empty();
        };

        let params = &doc.params;
        let pop_in_ms = params.pop_in_ms.max(1) as f32;
        let local_ms = now_ms - cue.start_ms as f32;
        let progress = (local_ms / pop_in_ms).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - progress).powi(3);
        let opacity = eased;
        let dy = (1.0 - eased) * params.travel_px;

        let width = ctx.current_video_size.width;
        let height = ctx.current_video_size.height;
        let body = frame
            .text_break_lines(
                ctx,
                cue.text.as_str(),
                BreakLinesOpts {
                    font: FontQuery {
                        family: live_family(),
                        size: params.font_size.max(1.0) as usize,
                        weight: params.font_weight,
                        ..Default::default()
                    },
                    width: ((width as f32) * params.width_frac.clamp(0.1, 1.0)) as usize,
                    line_height: 1.1,
                    x: ((width as f32) / 2.0) as i32,
                    y: ((height as f32) * params.y_frac.clamp(0.0, 1.0)) as i32,
                    align: TextAlign::Center,
                    fill: live_color(),
                    dominant_baseline: "middle",
                    text_anchor: "middle",
                    opacity: 1.0,
                },
            )
            .unwrap_or_default();

        fframes::svgr!(
            <svg
                xmlns="http://www.w3.org/2000/svg"
                viewBox="0 0 1920 1080"
                width={width}
                height={height}
            >
                <g opacity={opacity} transform={Transform::translate(0.0, dy)}>
                    {body}
                </g>
            </svg>
        )
    }
}
