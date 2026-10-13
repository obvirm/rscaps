use std::cell::{Cell, RefCell};
use std::rc::Rc;
use slint::{ComponentHandle, Model};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

slint::include_modules!();

// Fase 3 glue: wrappers over the standalone pkg modules (additive; call
// sites still use the rlib deps until the flip).
mod wasm_mods;
// Overlay caption asli (render_frame -> SVG -> slint::Image, paritas main).
mod overlay_render;

fn log(msg: &str) {
    web_sys::console::log_1(&msg.into());
}

// Layout responsif instan (mode android langsung, tanpa animasi tunda):
// Rust mendorong win-narrow/playbar-narrow dari DOM innerWidth — Slint
// tak boleh menurunkan struktur dari width-nya sendiri (binding loop).
// Ambang STATIS 920/960 (sengaja tak mengikuti inspector-w: ambang
// dinamis membalik layout di tengah drag divider — regresi yang
// dilaporkan user). Row-min wide kini hanya 588+insp (playbar
// dirampingkan: 458) sehingga 960 muat struktural dengan insp 320
// (margin 52px incl border). <=960 -> stacked android; 1000+ -> wide.
// 1293 -> wide normal (tak ada android aneh); laptop 720p fullscreen
// wide lagi. Geser divider tak pernah pindah mode; playbar-narrow
// tetap adaptif dari lebar kolom tengah. Hysteresis 40px menahan flap.
// Commit langsung saat berubah (tanpa debounce); pemanggil: event resize
// + poll backstop + sync awal boot.
fn apply_narrow_flags(ui: &App, w: f32, state: &RefCell<(bool, bool)>, src: &str) {
    if w < 200.0 {
        // Transient garbage during resize/load (observed w=10) — ignore.
        return;
    }
    let (cn, cp) = *state.borrow();
    let insp = (ui.get_inspector_w() as f32).clamp(240.0, 520.0);
    let narrow = if w < 960.0 {
        true
    } else if w > 1000.0 {
        false
    } else {
        cn
    };
    let center = w - 24.0 - if narrow { 0.0 } else { 72.0 + 12.0 + insp };
    let pb = if center < 540.0 {
        true
    } else if center > 580.0 {
        false
    } else {
        cp
    };
    if narrow != cn || pb != cp {
        log(&format!(
            "narrow[{src}]: w={w:.0} insp={insp:.0} narrow {cn}->{narrow} pb {cp}->{pb}"
        ));
        *state.borrow_mut() = (narrow, pb);
        ui.set_win_narrow(narrow);
        ui.set_playbar_narrow(pb);
    }
}

fn fmt_dur(sec: f64) -> String {
    let s = sec.max(0.0).floor() as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn fmt_dur_ms(sec: f64) -> String {
    let s = sec.max(0.0);
    let m = (s / 60.0).floor() as i64;
    format!("{}:{:06.3}", m, s - m as f64 * 60.0)
}

fn today_str() -> String {
    let iso = String::from(js_sys::Date::new_0().to_iso_string());
    iso.get(..10).unwrap_or("").to_string()
}

/// Hidden DOM helpers for picking a video file and grabbing one frame.
/// All browser interaction stays in Rust via web-sys (no hand-written JS).
struct PickerState {
    input: web_sys::HtmlInputElement,
    sub_input: web_sys::HtmlInputElement,
    video: web_sys::HtmlVideoElement,
    canvas: web_sys::HtmlCanvasElement,
    scan_canvas: web_sys::HtmlCanvasElement,
    object_url: Option<String>,
    video_file: Option<web_sys::Blob>,
    attach_to: Option<String>,
    // True only after an explicit mute press; play restores sound otherwise.
    user_muted: bool,
}

#[derive(Default)]
struct PendingVideo {
    name: String,
    meta: String,
    duration: String,
    duration_secs: f64,
    thumb: Option<slint::Image>,
    thumb_raw: Option<(u32, u32, Vec<u8>)>,
}

impl PendingVideo {
    fn clear(&mut self) {
        *self = PendingVideo::default();
    }
}

// Undo/redo over the whole caption document (full-JSON snapshots).
// Mutations are rare (transcribe / subtitle import), so snapshots are
// cheap and always exact. Cap 50 deep.
struct DocHistory {
    undo: Vec<String>,
    redo: Vec<String>,
}

impl DocHistory {
    fn push(&mut self, snap: String) {
        self.undo.push(snap);
        while self.undo.len() > 50 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    fn sync_buttons(&self, ui: &App) {
        ui.set_can_undo(!self.undo.is_empty());
        ui.set_can_redo(!self.redo.is_empty());
    }
}

fn doc_json_now() -> Option<String> {
    caption_core::document()
        .read()
        .ok()
        .and_then(|d| serde_json::to_string(&*d).ok())
}

// ── Project persistence (localStorage, Svelte parity: "stored in this browser") ──
// Videos cannot persist (megabytes, session-only); only metadata + a small
// RGBA thumb per project. Key "rscaps:projects", shape
// {"v":1,"projects":[{"id","name","date","dur"[,"tw","th","px"]}]}.
// Codec is hand-rolled (no new deps): the writer emits one fixed shape,
// the parser accepts only that shape and fails safe to empty on any error.
const PROJECTS_KEY: &str = "rscaps:projects";
const STORAGE_THUMB_W: u32 = 256;
const MAX_STORED_PROJECTS: usize = 200;

struct StoredProject {
    id: String,
    name: String,
    date: String,
    duration: String,
    thumb: Option<(u32, u32, Vec<u8>)>, // downsampled RGBA
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);
    for c in bytes.chunks(3) {
        let b0 = c[0] as u32;
        let b1 = *c.get(1).unwrap_or(&0) as u32;
        let b2 = *c.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64[((n >> 18) & 63) as usize] as char);
        out.push(B64[((n >> 12) & 63) as usize] as char);
        out.push(if c.len() > 1 {
            B64[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            B64[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn b64_val(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as u32),
        b'a'..=b'z' => Some((c - b'a' + 26) as u32),
        b'0'..=b'9' => Some((c - b'0' + 52) as u32),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    if b.is_empty() {
        return Some(Vec::new());
    }
    if b.len() % 4 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 4 * 3);
    for c in b.chunks(4) {
        let mut vals = [0u32; 4];
        let mut pad = 0usize;
        for (i, &ch) in c.iter().enumerate() {
            if ch == b'=' {
                pad += 1;
            } else {
                if pad > 0 {
                    return None;
                }
                vals[i] = b64_val(ch)?;
            }
        }
        if pad > 2 {
            return None;
        }
        let n = (vals[0] << 18) | (vals[1] << 12) | (vals[2] << 6) | vals[3];
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

fn json_escape_into(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn projects_to_json(list: &[StoredProject]) -> String {
    let mut s = String::from("{\"v\":1,\"projects\":[");
    for (i, p) in list.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str("{\"id\":");
        json_escape_into(&p.id, &mut s);
        s.push_str(",\"name\":");
        json_escape_into(&p.name, &mut s);
        s.push_str(",\"date\":");
        json_escape_into(&p.date, &mut s);
        s.push_str(",\"dur\":");
        json_escape_into(&p.duration, &mut s);
        if let Some((w, h, px)) = &p.thumb {
            s.push_str(&format!(",\"tw\":{w},\"th\":{h},\"px\":\"{}\"", b64_encode(px)));
        }
        s.push('}');
    }
    s.push_str("]}");
    s
}

struct Cur<'a> {
    b: &'a [u8],
    i: usize,
    depth: u32,
}

impl<'a> Cur<'a> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn byte(&mut self, expected: u8) -> bool {
        self.ws();
        if self.b.get(self.i) == Some(&expected) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    fn literal(&mut self, t: &str) -> bool {
        self.ws();
        let tb = t.as_bytes();
        if self.b.get(self.i..self.i + tb.len()) == Some(tb) {
            self.i += tb.len();
            true
        } else {
            false
        }
    }
    fn unsigned(&mut self) -> Option<u32> {
        self.ws();
        let s = self.i;
        while self.i < self.b.len() && self.b[self.i].is_ascii_digit() {
            self.i += 1;
        }
        if self.i == s || self.i - s > 10 {
            return None;
        }
        self.b[s..self.i]
            .iter()
            .fold(Some(0u32), |a, &d| a?.checked_mul(10)?.checked_add((d - b'0') as u32))
    }
    fn string(&mut self) -> Option<String> {
        self.ws();
        if self.b.get(self.i) != Some(&b'"') {
            return None;
        }
        self.i += 1;
        let mut out = String::new();
        loop {
            let &c = self.b.get(self.i)?;
            self.i += 1;
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let &e = self.b.get(self.i)?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            if self.i + 4 > self.b.len() {
                                return None;
                            }
                            let hex = std::str::from_utf8(&self.b[self.i..self.i + 4]).ok()?;
                            let cp = u32::from_str_radix(hex, 16).ok()?;
                            self.i += 4;
                            out.push(char::from_u32(cp)?);
                        }
                        _ => return None,
                    }
                }
                0x20..=0x7E => out.push(c as char),
                _ => {
                    // Raw UTF-8 passthrough (the writer emits non-ASCII raw).
                    let rest = std::str::from_utf8(&self.b[self.i - 1..]).ok()?;
                    let ch = rest.chars().next()?;
                    out.push(ch);
                    self.i += ch.len_utf8() - 1;
                }
            }
        }
    }
    fn skip_object(&mut self) -> Option<()> {
        self.ws();
        if self.byte(b'}') {
            return Some(());
        }
        loop {
            self.string()?;
            if !self.byte(b':') {
                return None;
            }
            self.skip_value()?;
            self.ws();
            if self.byte(b'}') {
                return Some(());
            }
            if !self.byte(b',') {
                return None;
            }
        }
    }
    fn skip_array(&mut self) -> Option<()> {
        self.ws();
        if self.byte(b']') {
            return Some(());
        }
        loop {
            self.skip_value()?;
            self.ws();
            if self.byte(b']') {
                return Some(());
            }
            if !self.byte(b',') {
                return None;
            }
        }
    }
    fn skip_value(&mut self) -> Option<()> {
        self.ws();
        match self.b.get(self.i)? {
            b'"' => self.string().map(|_| ()),
            b'{' => {
                if self.depth >= 24 {
                    return None;
                }
                self.i += 1;
                self.depth += 1;
                let r = self.skip_object();
                self.depth -= 1;
                r
            }
            b'[' => {
                if self.depth >= 24 {
                    return None;
                }
                self.i += 1;
                self.depth += 1;
                let r = self.skip_array();
                self.depth -= 1;
                r
            }
            b't' => self.literal("true").then_some(()),
            b'f' => self.literal("false").then_some(()),
            b'n' => self.literal("null").then_some(()),
            c if *c == b'-' || c.is_ascii_digit() => {
                self.i += 1;
                while self.i < self.b.len()
                    && matches!(self.b[self.i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    self.i += 1;
                }
                Some(())
            }
            _ => None,
        }
    }
}

fn parse_project(c: &mut Cur) -> Option<StoredProject> {
    if !c.byte(b'{') {
        return None;
    }
    let (mut id, mut name, mut date, mut dur) = (None, None, None, None);
    let (mut tw, mut th, mut px) = (None, None, None);
    loop {
        c.ws();
        if c.byte(b'}') {
            break;
        }
        let key = c.string()?;
        if !c.byte(b':') {
            return None;
        }
        match key.as_str() {
            "id" => id = Some(c.string()?),
            "name" => name = Some(c.string()?),
            "date" => date = Some(c.string()?),
            "dur" => dur = Some(c.string()?),
            "tw" => tw = Some(c.unsigned()?),
            "th" => th = Some(c.unsigned()?),
            "px" => px = Some(c.string()?),
            _ => {
                c.skip_value()?;
            }
        }
        c.ws();
        if c.byte(b'}') {
            break;
        }
        if !c.byte(b',') {
            return None;
        }
    }
    let (id, name, date, dur) = (id?, name?, date?, dur?);
    if id.is_empty() || id.len() > 128 {
        return None;
    }
    if name.chars().count() > 512 || date.chars().count() > 64 || dur.chars().count() > 64 {
        return None;
    }
    let thumb = match (tw, th, px) {
        (Some(w), Some(h), Some(s)) => {
            if w == 0 || h == 0 || w > 512 || h > 512 {
                return None;
            }
            let bytes = b64_decode(&s)?;
            if bytes.len() != w as usize * h as usize * 4 {
                return None;
            }
            Some((w, h, bytes))
        }
        (None, None, None) => None,
        _ => return None,
    };
    Some(StoredProject {
        id,
        name,
        date,
        duration: dur,
        thumb,
    })
}

fn projects_from_json(s: &str) -> Option<Vec<StoredProject>> {
    let mut c = Cur {
        b: s.as_bytes(),
        i: 0,
        depth: 0,
    };
    if !c.byte(b'{') {
        return None;
    }
    let mut list: Option<Vec<StoredProject>> = None;
    loop {
        c.ws();
        if c.byte(b'}') {
            break;
        }
        let key = c.string()?;
        if !c.byte(b':') {
            return None;
        }
        if key == "projects" {
            if !c.byte(b'[') {
                return None;
            }
            let mut v = Vec::new();
            c.ws();
            if !c.byte(b']') {
                loop {
                    v.push(parse_project(&mut c)?);
                    if v.len() > MAX_STORED_PROJECTS {
                        return None;
                    }
                    c.ws();
                    if c.byte(b']') {
                        break;
                    }
                    if !c.byte(b',') {
                        return None;
                    }
                }
            }
            list = Some(v);
        } else {
            c.skip_value()?;
        }
        c.ws();
        if c.byte(b'}') {
            break;
        }
        if !c.byte(b',') {
            return None;
        }
    }
    c.ws();
    if c.i != c.b.len() {
        return None;
    }
    list
}

fn save_projects(list: &[StoredProject]) {
    let Some(win) = web_sys::window() else { return };
    let Ok(Some(st)) = win.local_storage() else { return };
    let _ = st.set_item(PROJECTS_KEY, &projects_to_json(list));
}

fn load_projects() -> Vec<StoredProject> {
    let Some(win) = web_sys::window() else {
        return Vec::new();
    };
    let Ok(Some(st)) = win.local_storage() else {
        return Vec::new();
    };
    let Ok(Some(raw)) = st.get_item(PROJECTS_KEY) else {
        return Vec::new();
    };
    projects_from_json(&raw).unwrap_or_default()
}

// Box-average downsample of RGBA to at most max_w wide (bounded storage).
fn downsample_rgba(w: u32, h: u32, px: &[u8], max_w: u32) -> (u32, u32, Vec<u8>) {
    if w == 0 || h == 0 || px.len() != w as usize * h as usize * 4 {
        return (0, 0, Vec::new());
    }
    if w <= max_w {
        return (w, h, px.to_vec());
    }
    let nw = max_w;
    let nh = ((h as u64 * max_w as u64 / w as u64).max(1)) as u32;
    let mut out = vec![0u8; nw as usize * nh as usize * 4];
    for y in 0..nh {
        let y0 = (y as u64 * h as u64 / nh as u64) as u32;
        let y1 = (((y + 1) as u64 * h as u64 + nh as u64 - 1) / nh as u64)
            .max(y0 as u64 + 1)
            .min(h as u64) as u32;
        for x in 0..nw {
            let x0 = (x as u64 * w as u64 / nw as u64) as u32;
            let x1 = (((x + 1) as u64 * w as u64 + nw as u64 - 1) / nw as u64)
                .max(x0 as u64 + 1)
                .min(w as u64) as u32;
            let mut acc = [0u64; 4];
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let o = (yy as usize * w as usize + xx as usize) * 4;
                    for k in 0..4 {
                        acc[k] += px[o + k] as u64;
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as u64;
            let o = (y as usize * nw as usize + x as usize) * 4;
            for k in 0..4 {
                out[o + k] = (acc[k] / n) as u8;
            }
        }
    }
    (nw, nh, out)
}

/// Draw the current video frame to the canvas (max 480px wide) and wrap
/// the raw RGBA pixels as a Slint image. Mirrors tscaps ThumbnailGenerator
/// (frame near 0.1s, caller seeks before invoking).
fn capture_frame(
    video: &web_sys::HtmlVideoElement,
    canvas: &web_sys::HtmlCanvasElement,
) -> Result<(slint::Image, Vec<u8>, u32, u32), String> {
    let sw = video.video_width();
    let sh = video.video_height();
    if sw == 0 || sh == 0 {
        return Err("video has no decoded frames yet".into());
    }
    let scale = (480.0 / sw as f64).min(1.0);
    let w = ((sw as f64 * scale).round() as u32).max(1);
    let h = ((sh as f64 * scale).round() as u32).max(1);
    canvas.set_width(w);
    canvas.set_height(h);
    let ctx_val = canvas
        .get_context("2d")
        .map_err(|_| "2d canvas context unavailable".to_string())?
        .ok_or_else(|| "2d canvas context unavailable".to_string())?;
    let ctx: web_sys::CanvasRenderingContext2d =
        ctx_val.dyn_into().map_err(|_| "context cast failed".to_string())?;
    ctx.draw_image_with_html_video_element(video, 0.0, 0.0)
        .map_err(|_| "drawImage failed".to_string())?;
    let data = ctx
        .get_image_data(0.0, 0.0, w as f64, h as f64)
        .map_err(|_| "getImageData failed".to_string())?
        .data();
    let bytes = data.0;
    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
    buf.make_mut_bytes().copy_from_slice(&bytes);
    Ok((slint::Image::from_rgba8(buf), bytes, w, h))
}

// ── Real transcription (whisper-spike, same single wasm) ─────────────
// Mirrors studio/src/lib/transcribe.ts: engine init -> model files ->
// 16 kHz mono decode -> 30 s inference windows -> cues -> document.
// Progress/stage copy drive the overlay directly; the Slint tr-timer
// only fires completion as a backstop.
const WHISPER_MODELS: [&str; 5] = ["tiny", "base", "small", "medium", "large"];
const WHISPER_LANGS: [&str; 22] = [
    "en", "id", "ms", "ar", "zh", "ja", "ko", "hi", "ur", "fa", "he", "tr", "ru", "es", "fr", "de",
    "pt", "it", "nl", "pl", "vi", "th",
];
const WHISPER_LANG_ORDER: &str = "en zh de es ru ko fr ja pt tr pl ca nl ar sv it id hi fi vi he uk el ms cs ro da hu ta no th ur hr bg lt la mi ml cy sk te fa lv bn sr az sl kn et mk br eu hy ne mn bs kk sq sw gl mr pa si km sn yo so af oc ka be tg sd gu am yi lo uz fo ht ps tk nn mt sa lb my bo tl mg as tt haw ln ha ba jw su yue";
    const TRANSCRIBE_TASK_ID: i64 = 50359;
    const TRANSCRIBE_MAX_TOKENS: usize = 448;
    const TRANSCRIBE_CHUNK: usize = 30 * 16000;
    // (NO_SPEECH_DROP retired with the gate at the chunk filter.)
    const SINGLE_SEG_DROP: f64 = -0.6;

const TEMPLATE_IDS: [&str; 41] = [
    "ada", "anya", "cleo", "elio", "enzo", "ezra", "freya", "hugo", "iris", "ivo", "juno", "kai",
    "kel", "lena", "levi", "lewis", "loki", "luca", "luna", "lyra", "milo", "mira", "naya", "noor",
    "nova", "nyx", "otto", "pastor", "pepper", "pico", "remi", "rhea", "sara", "selene", "sol",
    "tala", "theo", "tito", "vera", "yuki", "zara",
];

fn lang_token_id(code: &str) -> i64 {
    let idx = WHISPER_LANG_ORDER
        .split(' ')
        .position(|c| c == code)
        .map(|i| i as i64)
        .unwrap_or(0);
    50259 + idx
}

fn template_id(idx: i32) -> String {
    if idx <= 0 {
        return String::new();
    }
    TEMPLATE_IDS
        .get(idx as usize - 1)
        .copied()
        .unwrap_or("")
        .to_string()
}

// Galeri template (paritas tscaps, data dari template.json kita, MIT):
// seksi + nama tampil + idx nyata ke TEMPLATE_IDS (1-based; 0 = none).
// Urutan VIRAL mengikuti builtin tscaps (Loki, Pepper, lewis, ...),
// seksi lain alfabetis.
const TPL_VIRAL: [(i32, &str); 12] = [
    (17, "Loki"),
    (29, "Pepper"),
    (16, "Lewis"),
    (23, "Naya"),
    (38, "Tito"),
    (11, "Juno"),
    (7, "Freya"),
    (31, "Remi"),
    (10, "Ivo"),
    (36, "Tala"),
    (12, "Kai"),
    (41, "Zara"),
];
const TPL_MODERN: [(i32, &str); 10] = [
    (3, "Cleo"),
    (5, "Enzo"),
    (8, "Hugo"),
    (22, "Mira"),
    (24, "Noor"),
    (25, "Nova"),
    (32, "Rhea"),
    (33, "Sara"),
    (34, "Selene"),
    (35, "Sol"),
];
const TPL_CLASSIC: [(i32, &str); 7] = [
    (1, "Ada"),
    (2, "Anya"),
    (13, "Kel"),
    (27, "Otto"),
    (37, "Theo"),
    (39, "Vera"),
    (40, "Yuki"),
];
const TPL_KEY: [(i32, &str); 7] = [
    (4, "Elio"),
    (6, "Ezra"),
    (15, "Levi"),
    (18, "Luca"),
    (19, "Luna"),
    (21, "Milo"),
    (28, "Pastor"),
];
const TPL_LAB: [(i32, &str); 5] = [
    (9, "Iris"),
    (14, "Lena"),
    (20, "Lyra"),
    (26, "Nyx"),
    (30, "Pico"),
];

fn tpl_items(sec: &[(i32, &str)], query: &str) -> Vec<TemplateItem> {
    let q = query.to_lowercase();
    sec.iter()
        .enumerate()
        .filter(|(_, (_, name))| q.is_empty() || name.to_lowercase().contains(&q))
        .enumerate()
        .map(|(pos, (_, (idx, name)))| TemplateItem {
            name: (*name).into(),
            idx: *idx,
            row: (pos / 3) as i32,
            col: (pos % 3) as i32,
        })
        .collect()
}

fn set_template_models(ui: &App, query: &str) {
    ui.set_template_viral(Rc::new(slint::VecModel::from(tpl_items(&TPL_VIRAL, query))).into());
    ui.set_template_modern(Rc::new(slint::VecModel::from(tpl_items(&TPL_MODERN, query))).into());
    ui.set_template_classic(Rc::new(slint::VecModel::from(tpl_items(&TPL_CLASSIC, query))).into());
    ui.set_template_key(Rc::new(slint::VecModel::from(tpl_items(&TPL_KEY, query))).into());
    ui.set_template_lab(Rc::new(slint::VecModel::from(tpl_items(&TPL_LAB, query))).into());
}

// Gaya overlay per template (slice 2a). GENERATED dari
// crates/caption-core/src/templates/*/template.json (data MIT,
// defaults styleControls + typography + alignment.verticalOffset).
// Urutan = TEMPLATE_IDS (idx 1-based); 0 = Default v0 (gaya lama).
struct TplStyle {
    fs: f32,
    fw: i32,
    fg: (u8, u8, u8),
    bg: Option<(u8, u8, u8)>,
    br: f32,
    px: f32,
    py: f32,
    y: f32,
    fam: &'static str,
}
const TPL_STYLE: [TplStyle; 41] = [
    // ada (Ada)
    TplStyle { fs: 2.8, fw: 700, fg: (28,30,29), bg: Some((231,229,231)), br: 0.3, px: 1.3, py: 0.2, y: 0.78, fam: "poppins" },
    // anya (Anya)
    TplStyle { fs: 2.97, fw: 700, fg: (255,248,231), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "playfair-display" },
    // cleo (Cleo)
    TplStyle { fs: 4.3, fw: 500, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.65, fam: "lora" },
    // elio (Elio)
    TplStyle { fs: 6.0, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.5, fam: "bebas-neue" },
    // enzo (Enzo)
    TplStyle { fs: 5.2, fw: 900, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.72, fam: "inter" },
    // ezra (Ezra)
    TplStyle { fs: 3.6, fw: 400, fg: (245,240,208), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.55, fam: "inter" },
    // freya (Freya)
    TplStyle { fs: 5.5, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.84, fam: "anton" },
    // hugo (Hugo)
    TplStyle { fs: 3.5, fw: 600, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.78, fam: "gabarito" },
    // iris (Iris)
    TplStyle { fs: 4.5, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.81, fam: "righteous" },
    // ivo (Ivo)
    TplStyle { fs: 5.08, fw: 400, fg: (255,255,255), bg: None, br: 0.09, px: 0.23, py: 0.09, y: 0.72, fam: "anton" },
    // juno (Juno)
    TplStyle { fs: 3.91, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "bungee" },
    // kai (Kai)
    TplStyle { fs: 5.0, fw: 800, fg: (232,232,239), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.68, fam: "bricolage-grotesque" },
    // kel (Kel)
    TplStyle { fs: 2.6, fw: 400, fg: (212,212,212), bg: Some((30,30,30)), br: 0.18, px: 0.0, py: 0.0, y: 0.78, fam: "jetbrains-mono" },
    // lena (Lena)
    TplStyle { fs: 2.5, fw: 400, fg: (255,255,255), bg: Some((41,151,255)), br: 0.6, px: 0.47, py: 0.3, y: 0.85, fam: "inter" },
    // levi (Levi)
    TplStyle { fs: 6.0, fw: 800, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.4, fam: "anton" },
    // lewis (Lewis)
    TplStyle { fs: 4.92, fw: 900, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.64, fam: "gabarito" },
    // loki (Loki)
    TplStyle { fs: 4.5, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "komika-axis" },
    // luca (Luca)
    TplStyle { fs: 4.0, fw: 600, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.55, fam: "caveat" },
    // luna (Luna)
    TplStyle { fs: 5.0, fw: 900, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.2, fam: "poppins" },
    // lyra (Lyra, tanpa primary -> putih)
    TplStyle { fs: 6.0, fw: 800, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.82, fam: "bricolage-grotesque" },
    // milo (Milo)
    TplStyle { fs: 3.5, fw: 600, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.6, fam: "lobster" },
    // mira (Mira)
    TplStyle { fs: 3.9, fw: 600, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.65, fam: "montserrat" },
    // naya (Naya)
    TplStyle { fs: 4.69, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "anton" },
    // noor (Noor)
    TplStyle { fs: 6.2, fw: 900, fg: (255,216,77), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.82, fam: "anton" },
    // nova (Nova)
    TplStyle { fs: 3.2, fw: 700, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.74, fam: "poppins" },
    // nyx (Nyx)
    TplStyle { fs: 3.44, fw: 400, fg: (0,255,65), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.7, fam: "vt323" },
    // otto (Otto)
    TplStyle { fs: 2.7, fw: 600, fg: (255,255,255), bg: Some((0,0,0)), br: 0.15, px: 0.46, py: 0.23, y: 0.85, fam: "inter" },
    // pastor (Pastor)
    TplStyle { fs: 3.6, fw: 500, fg: (230,214,71), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "manrope" },
    // pepper (Pepper)
    TplStyle { fs: 4.0, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "anton" },
    // pico (Pico)
    TplStyle { fs: 2.81, fw: 400, fg: (212,212,212), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.7, fam: "jetbrains-mono" },
    // remi (Remi)
    TplStyle { fs: 6.0, fw: 700, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.68, fam: "caveat" },
    // rhea (Rhea)
    TplStyle { fs: 3.6, fw: 700, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.72, fam: "montserrat" },
    // sara (Sara)
    TplStyle { fs: 4.92, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.62, fam: "im-fell-english" },
    // selene (Selene)
    TplStyle { fs: 3.6, fw: 600, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.78, fam: "montserrat" },
    // sol (Sol)
    TplStyle { fs: 4.0, fw: 300, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.74, fam: "montserrat" },
    // tala (Tala)
    TplStyle { fs: 4.0, fw: 400, fg: (255,255,255), bg: Some((26,26,26)), br: 0.17, px: 0.28, py: 0.11, y: 0.73, fam: "anton" },
    // theo (Theo)
    TplStyle { fs: 3.0, fw: 900, fg: (255,255,255), bg: Some((0,0,0)), br: 0.2, px: 0.55, py: 0.25, y: 0.78, fam: "inter" },
    // tito (Tito)
    TplStyle { fs: 3.5, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "bungee" },
    // vera (Vera)
    TplStyle { fs: 3.0, fw: 900, fg: (255,255,255), bg: Some((0,0,0)), br: 0.2, px: 0.2, py: 0.25, y: 0.78, fam: "inter" },
    // yuki (Yuki)
    TplStyle { fs: 2.7, fw: 700, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "inter" },
    // zara (Zara)
    TplStyle { fs: 4.69, fw: 400, fg: (255,255,255), bg: None, br: 0.0, px: 0.0, py: 0.0, y: 0.75, fam: "anton" },
];

// Font template asli (padanan ingest_font + @font-face main, diverifikasi
// dari sumber: fontique lowercase-normalized sehingga query "poppins"
// cocok face "Poppins"; registrasi via slint::fontique_011 yang
// didokumentasikan untuk font ter-download). Diambil latar di boot agar
// first paint tak menunggu (~2.4 MB); overlay pakai fallback sampai tiba.
const TPLFONTS: [&str; 33] = [
    "amiri-arabic-700-normal.ttf",
    "anton-latin-400-normal.ttf",
    "assistant-hebrew-700-normal.ttf",
    "bebas-neue-latin-400-normal.ttf",
    "bricolage-grotesque-latin-800-normal.ttf",
    "bungee-latin-400-normal.ttf",
    "cairo-arabic-700-normal.ttf",
    "caveat-latin-600-normal.ttf",
    "caveat-latin-700-normal.ttf",
    "gabarito-latin-600-normal.ttf",
    "gabarito-latin-900-normal.ttf",
    "heebo-hebrew-700-normal.ttf",
    "im-fell-english-latin-400-normal.ttf",
    "inter-latin-400-normal.ttf",
    "inter-latin-600-normal.ttf",
    "inter-latin-700-normal.ttf",
    "inter-latin-900-normal.ttf",
    "jetbrains-mono-latin-400-normal.ttf",
    "komika-axis-latin-400-normal.ttf",
    "lalezar-arabic-400-normal.ttf",
    "lobster-latin-400-normal.ttf",
    "lora-latin-500-normal.ttf",
    "manrope-latin-500-normal.ttf",
    "montserrat-latin-300-normal.ttf",
    "montserrat-latin-600-normal.ttf",
    "montserrat-latin-700-normal.ttf",
    "playfair-display-latin-700-normal.ttf",
    "playfair-display-latin-800-normal.ttf",
    "poppins-latin-700-normal.ttf",
    "poppins-latin-900-normal.ttf",
    "righteous-latin-400-normal.ttf",
    "vazirmatn-arabic-700-normal.ttf",
    "vt323-latin-400-normal.ttf",
];

fn preload_tplfonts() {
    let fut = async {
        let origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_else(|| "http://127.0.0.1:5178".to_string());
        let mut n = 0u32;
        let mut coll = slint::fontique_011::shared_collection();
        // Font sistem utk family default dokumen - paritas main (vite.config.ts
        // fontRoot = C:\Windows\Fonts; FONT_SOURCES[0..2] = ariblk/arial).
        // Tanpa ini fframes text_break_lines -> None (tak bisa pakai font
        // sistem di editor) -> svg body kosong.
        for (f, fam, w) in [
            ("ariblk.ttf", "Arial", 900u16),
            ("arial.ttf", "Arial", 400u16),
        ] {
            let url = format!("{origin}/{f}");
            match fetch_bytes(&url).await {
                Ok(b) => overlay_render::ingest_system_font(fam, w, &b),
                Err(e) => log(&format!("sysfont fetch failed {f}: {e}")),
            }
        }
        for f in TPLFONTS {
            let url = format!("{origin}/assets/{f}");
            match fetch_bytes(&url).await {
                Ok(b) => {
                    // fframes (text_break_lines) + @font-face inline overlay.
                    overlay_render::ingest_font(f, &b);
                    let blob =
                        slint::fontique_011::fontique::Blob::new(std::sync::Arc::new(b));
                    if !coll.register_fonts(blob, None).is_empty() {
                        n += 1;
                    }
                }
                Err(e) => log(&format!("tplfont fetch failed {f}: {e}")),
            }
        }
        log(&format!("tplfonts registered {n}/{}", TPLFONTS.len()));
    };
    wasm_bindgen_futures::spawn_local(fut);
}

fn tpl_rgb(c: (u8, u8, u8)) -> slint::Color {
    slint::Color::from_rgb_u8(c.0, c.1, c.2)
}

// Mendorong gaya overlay dari template terpilih (dipanggil saat kartu
// diklik + saat boot). idx 0 = Default v0 (gaya lama: putih tebal).
fn apply_template_style(ui: &App, idx: i32) {
    if idx <= 0 {
        ui.set_overlay_fs(7.0);
        ui.set_overlay_fw(800);
        ui.set_overlay_fg(slint::Color::from_rgb_u8(255, 255, 255));
        ui.set_overlay_bg_on(false);
        ui.set_overlay_br(0.0);
        ui.set_overlay_pdx(0.0);
        ui.set_overlay_pdy(0.0);
        ui.set_overlay_y(0.82);
        ui.set_overlay_fam("".into());
        return;
    }
    let Some(s) = TPL_STYLE.get(idx as usize - 1) else {
        return;
    };
    ui.set_overlay_fs(s.fs);
    ui.set_overlay_fw(s.fw);
    ui.set_overlay_fg(tpl_rgb(s.fg));
    match s.bg {
        Some(c) => {
            ui.set_overlay_bg(tpl_rgb(c));
            ui.set_overlay_bg_on(true);
        }
        None => ui.set_overlay_bg_on(false),
    }
    ui.set_overlay_br(s.br);
    ui.set_overlay_pdx(s.px);
    ui.set_overlay_pdy(s.py);
    ui.set_overlay_y(s.y);
    ui.set_overlay_fam(s.fam.into());
}

fn js_str(e: &JsValue) -> String {
    e.as_string().unwrap_or_else(|| "unknown error".to_string())
}

fn set_progress(ui_w: &slint::Weak<App>, frac: f32, title: &str, sub: &str) {
    if let Some(ui) = ui_w.upgrade() {
        ui.set_tr_pct((frac * 100.0).round() as i32);
        ui.set_tr_title(title.into());
        ui.set_tr_sub(sub.into());
        ui.set_status(title.into());
    }
}

fn fail_transcribe(ui_w: &slint::Weak<App>, msg: String) {
    if let Some(ui) = ui_w.upgrade() {
        ui.set_status(msg.into());
        ui.set_status_is_error(true);
        ui.set_show_transcribe(false);
        ui.set_tr_pct(0);
    }
}

// Caption overlay (paritas SubtitleOverlay tscaps, bentuk Slint-native):
// teks cue yang mengandung t (half-open start<=t<end, sama seperti
// TimeFragment.contains tscaps), selain itu string kosong.
fn active_cue_at(t_ms: u32) -> String {
    let Ok(doc) = caption_core::document().read() else {
        return String::new();
    };
    for cue in &doc.cues {
        if cue.start_ms <= t_ms && t_ms < cue.end_ms {
            return cue.text.clone();
        }
    }
    String::new()
}

// Empty-result contract (tscaps no-audio spec): finding no speech is a normal
// outcome, not an error. Neutral status inviting manual captioning; the
// editor's existing import/add paths are the "write by hand" affordance.
fn empty_transcribe(ui_w: &slint::Weak<App>) {
    if let Some(ui) = ui_w.upgrade() {
        ui.set_status(
            "No speech found — nothing to transcribe. Add captions by hand or try another video."
                .into(),
        );
        ui.set_status_is_error(false);
        ui.set_show_transcribe(false);
        ui.set_tr_pct(0);
    }
}

// Force the threaded ORT build to stay single-threaded (Svelte parity:
// the threaded build hangs during session init otherwise).
fn force_single_thread() {
    let g = js_sys::global();
    let get = |o: &JsValue, k: &str| js_sys::Reflect::get(o, &k.into());
    if let Ok(ort) = get(&g, "ort") {
        if !ort.is_undefined() {
            if let Ok(env) = get(&ort, "env") {
                if let Ok(wasm) = get(&env, "wasm") {
                    let _ = js_sys::Reflect::set(&wasm, &"numThreads".into(), &1.into());
                }
            }
        }
    }
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let win = web_sys::window().ok_or_else(|| format!("fetch failed: {url}"))?;
    let p = win.fetch_with_str(url);
    let v = JsFuture::from(p)
        .await
        .map_err(|_| format!("fetch failed: {url}"))?;
    let resp: web_sys::Response = v
        .dyn_into()
        .map_err(|_| format!("bad response: {url}"))?;
    if !resp.ok() {
        return Err(format!("fetch {}: {url}", resp.status()));
    }
    let b = resp
        .array_buffer()
        .map_err(|_| format!("body failed: {url}"))?;
    let bv = JsFuture::from(b)
        .await
        .map_err(|_| format!("body failed: {url}"))?;
    Ok(js_sys::Uint8Array::new(&bv).to_vec())
}

// Decode any audio/video blob to mono 16 kHz (mirrors audio.ts: Web Audio
// decodeAudioData, then downmix/resample through OfflineAudioContext).
async fn decode_16k(blob: &web_sys::Blob) -> Result<(Vec<f32>, f32), String> {
    let ab_promise = blob.array_buffer();
    let ab_val = JsFuture::from(ab_promise)
        .await
        .map_err(|_| "audio read failed".to_string())?;
    let actx = web_sys::AudioContext::new().map_err(|_| "no AudioContext".to_string())?;
    // No promise overload in web-sys for decodeAudioData: call it via
    // Reflect (no callbacks -> returns a Promise in all modern browsers).
    let decode_fn: js_sys::Function = js_sys::Reflect::get(&actx, &JsValue::from_str("decodeAudioData"))
        .map_err(|_| "audio decode failed".to_string())?
        .dyn_into()
        .map_err(|_| "audio decode failed".to_string())?;
    let dargs = js_sys::Array::new();
    dargs.push(&ab_val);
    let dec_promise: js_sys::Promise = js_sys::Reflect::apply(&decode_fn, &JsValue::from(&actx), &dargs)
        .map_err(|_| "audio decode failed".to_string())?
        .dyn_into()
        .map_err(|_| "audio decode failed".to_string())?;
    let dec_val = JsFuture::from(dec_promise)
        .await
        .map_err(|_| "audio decode failed".to_string())?;
    let audio: web_sys::AudioBuffer = dec_val
        .dyn_into()
        .map_err(|_| "audio decode failed".to_string())?;
    let duration = audio.duration() as f32;
    let len = ((duration * 16000.0).ceil() as u32).max(1);
    let off = web_sys::OfflineAudioContext::new_with_number_of_channels_and_length_and_sample_rate(
        1, len, 16000.0,
    )
    .map_err(|_| "audio resample failed".to_string())?;
    let src = off
        .create_buffer_source()
        .map_err(|_| "audio resample failed".to_string())?;
    src.set_buffer(Some(&audio));
    src.connect_with_audio_node(&off.destination())
        .map_err(|_| "audio resample failed".to_string())?;
    src.start().map_err(|_| "audio resample failed".to_string())?;
    let rend_promise = off
        .start_rendering()
        .map_err(|_| "audio resample failed".to_string())?;
    let rend_val = JsFuture::from(rend_promise)
        .await
        .map_err(|_| "audio resample failed".to_string())?;
    let rendered: web_sys::AudioBuffer = rend_val
        .dyn_into()
        .map_err(|_| "audio resample failed".to_string())?;
    let ch = rendered
        .get_channel_data(0)
        .map_err(|_| "audio resample failed".to_string())?;
    Ok((ch.to_vec(), duration))
}

#[derive(serde::Deserialize)]
struct TrSegment {
    start: f64,
    end: f64,
    text: String,
}

#[derive(serde::Deserialize)]
struct TrOutput {
    text: String,
    segments: Vec<TrSegment>,
    #[serde(default)]
    #[allow(dead_code)] // retired gate; kept for research + back-compat
    no_speech_prob: f32,
    #[serde(default)]
    avg_logprob: f64,
}

async fn run_transcribe(
    ui_w: slint::Weak<App>,
    gen_cell: Rc<Cell<u64>>,
    ready: Rc<RefCell<bool>>,
    generation: u64,
    blob: web_sys::Blob,
    lang_idx: i32,
    template_idx: i32,
    hist: Rc<RefCell<DocHistory>>,
) {
    let cancelled = || gen_cell.get() != generation;
    set_progress(
        &ui_w,
        0.02,
        "Preparing the engine.",
        "Loading the transcription engine.",
    );
    if !*ready.borrow() {
        let origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_else(|| "http://127.0.0.1:5178".to_string());
        let dist = format!("{origin}/");
        if let Err(e) = wasm_mods::w_init(Some(&dist)).await {
            fail_transcribe(&ui_w, format!("engine init failed: {e}"));
            return;
        }
        force_single_thread();
        set_progress(
            &ui_w,
            0.05,
            "Downloading the model.",
            "First run only — the model is cached after this.",
        );
        {
            // Model bytes (~80 MB) must die before inference: freed pages
            // serve the decode loop so wasm linear memory never grows
            // mid-run (growth detaches ort-web ArrayBuffer views ->
            // "Cannot perform Construct on a detached ArrayBuffer").
            let enc = match fetch_bytes(&format!("{origin}/encoder_model.onnx")).await {
            Ok(b) => b,
            Err(e) => {
                fail_transcribe(&ui_w, e);
                return;
            }
        };
        set_progress(
            &ui_w,
            0.18,
            "Downloading the model.",
            "First run only — the model is cached after this.",
        );
        let dec = match fetch_bytes(&format!("{origin}/decoder_model_merged.onnx")).await {
            Ok(b) => b,
            Err(e) => {
                fail_transcribe(&ui_w, e);
                return;
            }
        };
        set_progress(
            &ui_w,
            0.28,
            "Downloading the model.",
            "First run only — the model is cached after this.",
        );
        let tok_b = match fetch_bytes(&format!("{origin}/tokenizer.json")).await {
            Ok(b) => b,
            Err(e) => {
                fail_transcribe(&ui_w, e);
                return;
            }
        };
        let tok = match String::from_utf8(tok_b) {
            Ok(s) => s,
            Err(_) => {
                fail_transcribe(&ui_w, "tokenizer decode failed".to_string());
                return;
            }
        };
        set_progress(
            &ui_w,
            0.29,
            "Downloading the model.",
            "First run only — the model is cached after this.",
        );
        if cancelled() {
            return;
        }
        if let Err(e) = wasm_mods::w_load_encoder(&enc).await {
            fail_transcribe(&ui_w, format!("encoder load failed: {e}"));
            return;
        }
        if let Err(e) = wasm_mods::w_load_decoder(&dec).await {
            fail_transcribe(&ui_w, format!("decoder load failed: {e}"));
            return;
        }
        if let Err(e) = wasm_mods::w_load_tokenizer(&tok).await {
            fail_transcribe(&ui_w, format!("tokenizer load failed: {e}"));
            return;
        }
        }
        *ready.borrow_mut() = true;
    } else {
        set_progress(
            &ui_w,
            0.29,
            "Downloading the model.",
            "First run only — the model is cached after this.",
        );
    }
    if cancelled() {
        return;
    }
    set_progress(&ui_w, 0.32, "Preparing your audio.", "Decoding to 16 kHz mono.");
    let (samples, _dur) = match decode_16k(&blob).await {
        Ok(v) => v,
        Err(e) => {
            fail_transcribe(&ui_w, e);
            return;
        }
    };
    // Real heap headroom for the decode loop: allocate, touch, and free
    // 128 MB so the loop's transient allocations reuse freed pages
    // instead of growing linear memory mid-run (see above).
    drop(vec![0u8; 128 * 1024 * 1024]);
    let total = ((samples.len() + TRANSCRIBE_CHUNK - 1) / TRANSCRIBE_CHUNK).max(1);
    let lang = WHISPER_LANGS
        .get(lang_idx as usize)
        .copied()
        .unwrap_or("en");
    let lang_id = lang_token_id(lang);
    let mut texts: Vec<String> = Vec::new();
    let mut cues: Vec<caption_core::Cue> = Vec::new();
    for i in 0..total {
        if cancelled() {
            return;
        }
        let s = i * TRANSCRIBE_CHUNK;
        let e = (s + TRANSCRIBE_CHUNK).min(samples.len());
        if s >= e {
            break;
        }
        set_progress(
            &ui_w,
            0.40 + (i as f32 / total as f32) * 0.58,
            "Transcribing your video in this browser.",
            "Keep this tab open until it finishes.",
        );
        let chunk = &samples[s..e];
        // Energy gate (tscaps slack-cutting, cheap form): skip near-silent
        // chunks before spending inference on them — silence is Whisper's
        // surest hallucination trigger.
        let rms = (chunk.iter().fold(0.0f32, |a, x| a + x * x) / chunk.len().max(1) as f32)
            .sqrt();
        if rms < 0.01 {
            continue;
        }
        let hidden = match wasm_mods::w_encode_audio(chunk).await {
            Ok(h) => h,
            Err(e) => {
                fail_transcribe(&ui_w, format!("encode failed: {e}"));
                return;
            }
        };
        let window_dur = chunk.len() as f32 / 16000.0;
        let raw = match wasm_mods::w_transcribe(
            &hidden,
            lang_id,
            TRANSCRIBE_TASK_ID,
            true,
            TRANSCRIBE_MAX_TOKENS,
            window_dur,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                fail_transcribe(&ui_w, format!("transcribe failed: {e}"));
                return;
            }
        };
        let out: TrOutput = match serde_json::from_str(&raw) {
            Ok(o) => o,
            Err(_) => {
                fail_transcribe(&ui_w, "transcript parse failed".to_string());
                return;
            }
        };
        // NOTE: no_speech_prob gate retired — first-step softmax reads ~0 on
        // all inputs, so it gated nothing. Live defenses: energy gate above,
        // loop-tail excision in the engine, punctuation filter below.
        // Low-confidence loner gate (measured: speech -0.31, thud -0.97,
        // noise -1.17): a chunk yielding a single segment with weak mean
        // logprob is a hallucinated blip, not speech. Multi-segment chunks
        // are never touched; confident loners ("Ya.") pass.
        if out.segments.len() <= 1 && out.avg_logprob < SINGLE_SEG_DROP {
            continue;
        }
        let off = s as f64 / 16000.0;
        for seg in out.segments {
            let text = seg.text.trim().to_string();
            if text.is_empty() {
                continue;
            }
            let start_ms = ((seg.start + off) * 1000.0).round().max(0.0) as u32;
            let end_ms = ((seg.end + off) * 1000.0).round().max(0.0) as u32;
            if end_ms <= start_ms {
                continue;
            }
            // Drop degenerate cues that are only punctuation/symbols.
            if !text.chars().any(|c| c.is_alphanumeric()) {
                continue;
            }
            cues.push(caption_core::Cue {
                start_ms,
                end_ms,
                text,
            });
        }
        if !out.text.trim().is_empty() {
            texts.push(out.text.trim().to_string());
        }
    }
    if cancelled() {
        return;
    }
    set_progress(&ui_w, 1.0, "Finishing up.", "Almost done…");
    let _ = texts;
    if cues.is_empty() {
        empty_transcribe(&ui_w);
        return;
    }
    let cues_json = match serde_json::to_string(&cues) {
        Ok(j) => j,
        Err(_) => {
            fail_transcribe(&ui_w, "doc build failed".to_string());
            return;
        }
    };
    let pre_undo = doc_json_now();
    if let Err(e) = caption_core::replace_cues_json(&cues_json) {
        fail_transcribe(&ui_w, format!("doc build failed: {e}"));
        return;
    }
    if let Ok(mut doc) = caption_core::document().write() {
        doc.template = template_id(template_idx);
    }
    let doc_str = match caption_core::document().read() {
        Ok(d) => match serde_json::to_string(&*d) {
            Ok(s) => s,
            Err(_) => {
                fail_transcribe(&ui_w, "doc build failed".to_string());
                return;
            }
        },
        Err(_) => {
            fail_transcribe(&ui_w, "doc build failed".to_string());
            return;
        }
    };
    if let Some(ui) = ui_w.upgrade() {
        if let Some(snap) = pre_undo {
            hist.borrow_mut().push(snap);
            hist.borrow().sync_buttons(&ui);
        }
        ui.set_doc_text(doc_str.into());
        ui.set_status(
            format!(
                "transcribed {} cue{}",
                cues.len(),
                if cues.len() == 1 { "" } else { "s" }
            )
            .into(),
        );
        ui.set_status_is_error(false);
        ui.set_frame(0);
        ui.set_overlay_text(active_cue_at(0).into());
        overlay_render::invalidate();
        overlay_render::request(&ui, 0);
        ui.set_show_transcribe(false);
        ui.set_tr_pct(100);
    }
}

// ── Video blob persistence (IndexedDB, Svelte VideoBlobCache parity) ──
// Rows persist metadata; the actual bytes persist here so reopened
// projects get their video back after reload. Out-of-line keys, no
// keyPath/indexes. All operations fail safe to None/() with a log.
const VIDEO_DB: &str = "rscaps";
const VIDEO_STORE: &str = "videos";

fn idb_await_request(req: &web_sys::IdbRequest) -> js_sys::Promise {
    js_sys::Promise::new(&mut |resolve, reject| {
        let rs = resolve.clone();
        let ok = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let v = e
                .target()
                .and_then(|t| t.dyn_into::<web_sys::IdbRequest>().ok())
                .and_then(|r| r.result().ok())
                .unwrap_or(JsValue::UNDEFINED);
            let _ = rs.call1(&JsValue::NULL, &v);
        });
        req.set_onsuccess(Some(ok.as_ref().unchecked_ref()));
        ok.forget();
        let rj = reject.clone();
        let er = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let _ = rj.call0(&JsValue::NULL);
        });
        req.set_onerror(Some(er.as_ref().unchecked_ref()));
        er.forget();
    })
}

fn idb_open_db() -> Result<js_sys::Promise, String> {
    let window = web_sys::window().ok_or_else(|| "idb unavailable".to_string())?;
    let factory = window
        .indexed_db()
        .map_err(|_| "idb unavailable".to_string())?
        .ok_or_else(|| "idb unavailable".to_string())?;
    let req = factory
        .open_with_u32(VIDEO_DB, 1)
        .map_err(|_| "idb open failed".to_string())?;
    let up = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
        let db: Option<web_sys::IdbDatabase> = e
            .target()
            .and_then(|t| t.dyn_into::<web_sys::IdbRequest>().ok())
            .and_then(|r| r.result().ok())
            .and_then(|v| v.dyn_into().ok());
        if let Some(db) = db {
            let _ = db.create_object_store(VIDEO_STORE);
        }
    });
    req.set_onupgradeneeded(Some(up.as_ref().unchecked_ref()));
    up.forget();
    Ok(idb_await_request(&req))
}

async fn idb_get_db() -> Result<web_sys::IdbDatabase, String> {
    let v = JsFuture::from(idb_open_db()?)
        .await
        .map_err(|_| "idb open failed".to_string())?;
    v.dyn_into::<web_sys::IdbDatabase>()
        .map_err(|_| "idb open failed".to_string())
}

async fn idb_put(id: &str, val: &JsValue) -> Result<(), String> {
    let db = idb_get_db().await?;
    let tx = db
        .transaction_with_str_and_mode(VIDEO_STORE, web_sys::IdbTransactionMode::Readwrite)
        .map_err(|_| "idb tx failed".to_string())?;
    let store = tx
        .object_store(VIDEO_STORE)
        .map_err(|_| "idb store failed".to_string())?;
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &JsValue::from_str("id"), &JsValue::from_str(id))
        .map_err(|_| "idb put failed".to_string())?;
    js_sys::Reflect::set(&obj, &JsValue::from_str("blob"), val)
        .map_err(|_| "idb put failed".to_string())?;
    let key = JsValue::from_str(id);
    let req = store
        .put_with_key(&obj.into(), &key)
        .map_err(|_| "idb put failed".to_string())?;
    JsFuture::from(idb_await_request(&req))
        .await
        .map_err(|_| "idb put failed".to_string())?;
    Ok(())
}

async fn idb_get(id: &str) -> Result<Option<web_sys::Blob>, String> {
    let db = idb_get_db().await?;
    let tx = db
        .transaction_with_str_and_mode(VIDEO_STORE, web_sys::IdbTransactionMode::Readonly)
        .map_err(|_| "idb tx failed".to_string())?;
    let store = tx
        .object_store(VIDEO_STORE)
        .map_err(|_| "idb store failed".to_string())?;
    let req = store
        .get(&JsValue::from_str(id))
        .map_err(|_| "idb get failed".to_string())?;
    let v = JsFuture::from(idb_await_request(&req))
        .await
        .map_err(|_| "idb get failed".to_string())?;
    if v.is_undefined() {
        return Ok(None);
    }
    let blob_val = js_sys::Reflect::get(&v, &JsValue::from_str("blob"))
        .map_err(|_| "idb get failed".to_string())?;
    if blob_val.is_undefined() {
        return Ok(None);
    }
    Ok(Some(
        blob_val
            .dyn_into::<web_sys::Blob>()
            .map_err(|_| "idb get failed".to_string())?,
    ))
}

async fn idb_del(id: &str) -> Result<(), String> {
    let db = idb_get_db().await?;
    let tx = db
        .transaction_with_str_and_mode(VIDEO_STORE, web_sys::IdbTransactionMode::Readwrite)
        .map_err(|_| "idb tx failed".to_string())?;
    let store = tx
        .object_store(VIDEO_STORE)
        .map_err(|_| "idb store failed".to_string())?;
    let req = store
        .delete(&JsValue::from_str(id))
        .map_err(|_| "idb del failed".to_string())?;
    JsFuture::from(idb_await_request(&req))
        .await
        .map_err(|_| "idb del failed".to_string())?;
    Ok(())
}

// Resolve on the next occurrence of a DOM event (one-shot).
fn once_event(target: &web_sys::EventTarget, name: &str) -> js_sys::Promise {
    let name = name.to_string();
    js_sys::Promise::new(&mut |resolve, _reject| {
        let rs = resolve.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let _ = rs.call0(&JsValue::NULL);
        });
        let _ = target.add_event_listener_with_callback(&name, cb.as_ref().unchecked_ref());
        cb.forget();
    })
}

// Same as once_event, but rejects after `ms` so a media event that never
// fires (browser decode stall) can't hang the restore task forever.
fn once_event_timeout(target: &web_sys::EventTarget, name: &str, ms: i32) -> js_sys::Promise {
    let name = name.to_string();
    js_sys::Promise::new(&mut |resolve, reject| {
        let rs = resolve.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let _ = rs.call0(&JsValue::NULL);
        });
        let _ = target.add_event_listener_with_callback(&name, cb.as_ref().unchecked_ref());
        cb.forget();
        let rj = reject.clone();
        let timeout_cb = Closure::<dyn FnMut()>::new(move || {
            let _ = rj.call1(&JsValue::NULL, &JsValue::from_str("timeout"));
        });
        if let Some(win) = web_sys::window() {
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_cb.as_ref().unchecked_ref(),
                ms,
            );
        }
        timeout_cb.forget();
    })
}

// Resolve after `ms` milliseconds (setTimeout promise).
fn sleep_ms(ms: i32) -> js_sys::Promise {
    js_sys::Promise::new(&mut |resolve, _reject| {
        let rs = resolve.clone();
        let timeout_cb = Closure::<dyn FnMut()>::new(move || {
            let _ = rs.call0(&JsValue::NULL);
        });
        if let Some(win) = web_sys::window() {
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(
                timeout_cb.as_ref().unchecked_ref(),
                ms,
            );
        }
        timeout_cb.forget();
    })
}

// Attach a video blob to the current project: object URL, wait for a
// decodable frame, show it, persist the bytes. Shared by project-open
// restore and the editor Import button.
async fn attach_video(
    ui_w: slint::Weak<App>,
    picker: Rc<RefCell<PickerState>>,
    pid: String,
    blob: Option<web_sys::Blob>,
) {
    let blob = match blob {
        Some(b) => b,
        None => match idb_get(&pid).await {
            Ok(Some(b)) => b,
            Ok(None) => return,
            Err(e) => {
                log(&format!("video restore failed: {e}"));
                return;
            }
        },
    };
    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
        Ok(u) => u,
        Err(_) => return,
    };
    let video = picker.borrow().video.clone();
    video.set_src(&url);
    {
        let mut st = picker.borrow_mut();
        if let Some(old) = st.object_url.take() {
            let _ = web_sys::Url::revoke_object_url(&old);
        }
        st.object_url = Some(url);
        st.video_file = Some(blob);
    }
    if JsFuture::from(once_event(&video, "loadedmetadata")).await.is_err() {
        return;
    }
    video.set_current_time(0.1);
    if JsFuture::from(once_event(&video, "seeked")).await.is_err() {
        return;
    }
    let vw = video.video_width();
    let vh = video.video_height();
    if vw == 0 || vh == 0 {
        return;
    }
    let (img, dur) = {
        let st = picker.borrow();
        let scale = (640.0 / vw as f64).min(1.0);
        let w = ((vw as f64 * scale).round() as u32).max(1);
        let h = ((vh as f64 * scale).round() as u32).max(1);
        (grab_frame(&video, &st.canvas, w, h), video.duration())
    };
    if let Some(img) = img {
        if let Some(ui) = ui_w.upgrade() {
            ui.set_preview_image(img);
            ui.set_has_video(true);
            ui.set_video_aspect(vw as f32 / vh as f32);
            ui.set_time_text(fmt_dur(0.0).into());
            ui.set_overlay_text(active_cue_at(0).into());
            overlay_render::request(&ui, 0);
            ui.set_frame(0);
            if dur.is_finite() && dur > 0.0 {
                ui.set_duration_text(fmt_dur(dur).into());
                ui.set_scrub_max((dur * 30.0) as i32);
            }
        }
    }
    if let Some(f) = picker.borrow().video_file.clone() {
        let val: JsValue = f.into();
        if let Err(e) = idb_put(&pid, &val).await {
            log(&format!("video persist failed: {e}"));
        }
    }
}

// ── HiDPI canvas sync (blur fix) ──────────────────────────────────
// Winit writes the canvas width/height attributes from its own often stale
// idea of the size, so the browser can rescale a wrong-resolution framebuffer:
// blur. Recompute the attrs from the rendered canvas box * devicePixelRatio.
// A MutationObserver on the attributes fires as a microtask right after every
// winit write, so this correction always lands last, before paint.
fn sync_canvas_dpr() {
    let Some(win) = web_sys::window() else { return };
    let Some(doc) = win.document() else { return };
    let Some(el) = doc.query_selector("#canvas").ok().flatten() else { return };
    let Ok(canvas) = el.dyn_into::<web_sys::HtmlCanvasElement>() else { return };
    let cw = canvas.client_width() as f64;
    let ch = canvas.client_height() as f64;
    let dpr = win.device_pixel_ratio();
    if cw < 1.0 || ch < 1.0 {
        return;
    }
    let pw = (cw * dpr).round().max(1.0) as u32;
    let ph = (ch * dpr).round().max(1.0) as u32;
    if canvas.width() != pw {
        canvas.set_width(pw);
    }
    if canvas.height() != ph {
        canvas.set_height(ph);
    }
}

// Re-apply correct attrs after ANY width/height attribute mutation
// (winit's resize path). Microtask timing = after winit, before paint.
fn arm_canvas_attr_observer() {
    let Some(win) = web_sys::window() else { return };
    let Some(doc) = win.document() else { return };
    let Some(el) = doc.query_selector("#canvas").ok().flatten() else { return };
    let Ok(canvas) = el.dyn_into::<web_sys::HtmlCanvasElement>() else { return };
    let cb = Closure::<dyn FnMut(js_sys::Array, web_sys::MutationObserver)>::new(
        move |_mutations, _observer| {
            sync_canvas_dpr();
        },
    );
    let Ok(obs) = web_sys::MutationObserver::new(cb.as_ref().unchecked_ref()) else { return };
    let opts = web_sys::MutationObserverInit::new();
    opts.set_attributes(true);
    let filter = js_sys::Array::new();
    filter.push(&"width".into());
    filter.push(&"height".into());
    opts.set_attribute_filter(&filter);
    let _ = obs.observe_with_options(&canvas, &opts);
    cb.forget();
}

// Per-frame path for the rAF loop: element cached across frames and the
// CSS box read without forcing layout. #canvas is pinned to 100vw/100vh
// with overflow:hidden (index.html), so innerWidth/innerHeight always
// equal its client box; attrs come out identical to sync_canvas_dpr.
// (The element is never replaced by trunk/winit, only its attrs.)
fn sync_canvas_dpr_fast(cached: &mut Option<web_sys::HtmlCanvasElement>) {
    let Some(win) = web_sys::window() else { return };
    if cached.is_none() {
        *cached = win
            .document()
            .and_then(|d| d.query_selector("#canvas").ok().flatten())
            .and_then(|e| e.dyn_into::<web_sys::HtmlCanvasElement>().ok());
    }
    let Some(canvas) = cached.as_ref() else { return };
    let cw = win
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let ch = win
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    if cw < 1.0 || ch < 1.0 {
        return;
    }
    let dpr = win.device_pixel_ratio();
    let pw = (cw * dpr).round().max(1.0) as u32;
    let ph = (ch * dpr).round().max(1.0) as u32;
    if canvas.width() != pw {
        canvas.set_width(pw);
    }
    if canvas.height() != ph {
        canvas.set_height(ph);
    }
}

// Copy the current headless-video frame into a Slint image (downscaled:
// low-res preview is reference-sanctioned). Called on the Slint thread by
// the frame-pump timer, so each grab is one cheap blit, never blocking.
fn grab_frame(
    video: &web_sys::HtmlVideoElement,
    canvas: &web_sys::HtmlCanvasElement,
    w: u32,
    h: u32,
) -> Option<slint::Image> {
    if w == 0 || h == 0 {
        return None;
    }
    if canvas.width() != w || canvas.height() != h {
        canvas.set_width(w);
        canvas.set_height(h);
    }
    let ctx_val = canvas.get_context("2d").ok().flatten()?;
    let ctx: web_sys::CanvasRenderingContext2d = ctx_val.dyn_into().ok()?;
    ctx.draw_image_with_html_video_element_and_dw_and_dh(video, 0.0, 0.0, w as f64, h as f64)
        .ok()?;
    let data = ctx.get_image_data(0.0, 0.0, w as f64, h as f64).ok()?;
    let bytes = data.data().0;
    if bytes.len() != w as usize * h as usize * 4 {
        return None;
    }
    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
    buf.make_mut_bytes().copy_from_slice(&bytes);
    Some(slint::Image::from_rgba8(buf))
}

// Poll grab_frame until the captured pixels are non-black (or tries run
// out). drawImage can capture a blank frame when 'seeked' fires before the
// compositor presents a paintable frame — a single fixed sleep is a race,
// so keep grabbing until real pixels show up.
async fn grab_frame_settled(
    video: &web_sys::HtmlVideoElement,
    canvas: &web_sys::HtmlCanvasElement,
    w: u32,
    h: u32,
) -> Option<slint::Image> {
    for _ in 0..20 {
        if let Some(img) = grab_frame(video, canvas, w, h) {
            let mean: Option<u64> = (|| {
                let ctx_val = canvas.get_context("2d").ok().flatten()?;
                let ctx: web_sys::CanvasRenderingContext2d = ctx_val.dyn_into().ok()?;
                let data = ctx.get_image_data(0.0, 0.0, w as f64, h as f64).ok()?;
                let bytes = data.data().0;
                if bytes.len() < 4 {
                    return None;
                }
                let mut sum: u64 = 0;
                let mut n: u64 = 0;
                for px in bytes.chunks_exact(4).step_by(503) {
                    sum += px[0] as u64 + px[1] as u64 + px[2] as u64;
                    n += 1;
                }
                if n == 0 {
                    return None;
                }
                Some(sum / (n * 3))
            })();
            if mean.unwrap_or(0) > 8 {
                return Some(img);
            }
        }
        let _ = JsFuture::from(sleep_ms(100)).await;
    }
    None
}

// Seek to t seconds, wait for a paintable frame, show it. Shared by
// paused stepping (frame / scene navigate) so the stage follows.
async fn seek_grab_frame(
    ui_w: slint::Weak<App>,
    picker: Rc<RefCell<PickerState>>,
    t: f64,
    frame_i: i32,
) {
    let (video, canvas) = {
        let st = picker.borrow();
        (st.video.clone(), st.canvas.clone())
    };
    video.set_current_time(t);
    if JsFuture::from(once_event_timeout(&video, "seeked", 10_000))
        .await
        .is_err()
    {
        return;
    }
    let vw = video.video_width();
    let vh = video.video_height();
    if vw == 0 || vh == 0 {
        return;
    }
    let scale = (640.0 / vw as f64).min(1.0);
    let w = ((vw as f64 * scale).round() as u32).max(1);
    let h = ((vh as f64 * scale).round() as u32).max(1);
    if let Some(img) = grab_frame_settled(&video, &canvas, w, h).await {
        if let Some(ui) = ui_w.upgrade() {
            ui.set_preview_image(img);
            ui.set_time_text(fmt_dur_ms(t).into());
            ui.set_overlay_text(active_cue_at((t * 1000.0) as u32).into());
            ui.set_frame(frame_i);
            overlay_render::request(&ui, frame_i);
        }
    }
}

fn fmt_rate(r: f64) -> String {
    // {} prints 1.0 as "1", 0.5 as "0.5" — exactly the popup labels.
    format!("{r}×")
}

// 16-bin luma histogram of the scan canvas (64x36, fixed at creation).
fn frame_hist(
    video: &web_sys::HtmlVideoElement,
    canvas: &web_sys::HtmlCanvasElement,
) -> Option<Vec<u32>> {
    let ctx_val = canvas.get_context("2d").ok().flatten()?;
    let ctx: web_sys::CanvasRenderingContext2d = ctx_val.dyn_into().ok()?;
    ctx.draw_image_with_html_video_element_and_dw_and_dh(video, 0.0, 0.0, 64.0, 36.0)
        .ok()?;
    let data = ctx.get_image_data(0.0, 0.0, 64.0, 36.0).ok()?;
    let bytes = data.data().0;
    let mut hist = vec![0u32; 16];
    for px in bytes.chunks_exact(4) {
        let luma = (px[0] as u32 * 299 + px[1] as u32 * 587 + px[2] as u32 * 114) / 1000;
        hist[(luma.min(255) / 16) as usize] += 1;
    }
    Some(hist)
}

// Normalized L1 distance of two histograms, 0.0 (same) .. 1.0 (nothing
// shared). Cuts score high; dissolves/fades mid; static noise low.
fn hist_diff(a: &[u32], b: &[u32]) -> f64 {
    let ta: u32 = a.iter().sum();
    let tb: u32 = b.iter().sum();
    if ta == 0 || tb == 0 {
        return 0.0;
    }
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64 / ta as f64 - *y as f64 / tb as f64).abs())
        .sum::<f64>()
        / 2.0
}

// Coarse frame-difference scene scan over the whole video. Returns
// boundary times (always starting at 0.0), cached per project by the
// caller. Aborts (None) when superseded or the project changes.
// NOTE: browser behavior unverified (no browser tests in this setup);
// threshold/min-gap are starting points, tune against real footage.
async fn scan_scenes(
    ui_w: slint::Weak<App>,
    picker: Rc<RefCell<PickerState>>,
    current: Rc<RefCell<Option<String>>>,
    pid: String,
    gen_cell: Rc<Cell<u64>>,
    my_gen: u64,
) -> Option<Vec<f64>> {
    let (video, canvas) = {
        let st = picker.borrow();
        (st.video.clone(), st.scan_canvas.clone())
    };
    let dur = video.duration();
    if !dur.is_finite() || dur <= 0.0 {
        return None;
    }
    let step = (dur / 300.0).max(0.5);
    let was_playing = !video.paused();
    let t0 = video.current_time();
    let _ = video.pause();
    let mut bounds = vec![0.0f64];
    let mut prev: Option<Vec<u32>> = None;
    let mut t = 0.0;
    let mut n: u64 = 0;
    let total = (dur / step).ceil().max(1.0) as u64;
    while t < dur {
        if gen_cell.get() != my_gen {
            return None;
        }
        if current.borrow().as_deref() != Some(pid.as_str()) {
            return None;
        }
        video.set_current_time(t);
        if JsFuture::from(once_event_timeout(&video, "seeked", 8000))
            .await
            .is_err()
        {
            break;
        }
        if let Some(h) = frame_hist(&video, &canvas) {
            if let Some(p) = &prev {
                if hist_diff(p, &h) > 0.45 && t - bounds.last().copied().unwrap_or(-10.0) > 1.0 {
                    bounds.push(t);
                }
            }
            prev = Some(h);
        }
        n += 1;
        if n % 10 == 0 {
            if let Some(ui) = ui_w.upgrade() {
                ui.set_status(format!("Scanning scenes… {}%", (n * 100 / total).min(100)).into());
            }
        }
        t += step;
    }
    video.set_current_time(t0);
    if was_playing {
        let _ = video.play();
    }
    Some(bounds)
}

// Self-scheduling requestAnimationFrame loop: re-check every frame so
// the sync always lands after winit's async canvas writes.
fn arm_canvas_sync_loop() {
    let cb_slot: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let cache: Rc<RefCell<Option<web_sys::HtmlCanvasElement>>> = Rc::new(RefCell::new(None));
    let slot = cb_slot.clone();
    let cache_in = cache.clone();
    // (sum-start, frames, windows, last-t, long-logs)
    let meter: Rc<RefCell<(f64, u32, u32, f64, u32)>> =
        Rc::new(RefCell::new((0.0, 0, 0, 0.0, 0)));
    let meter_in = meter.clone();
    let cb = Closure::<dyn FnMut()>::new(move || {
        sync_canvas_dpr_fast(&mut cache_in.borrow_mut());
        if let Some(win) = web_sys::window() {
            // fps meter: average rAF interval over 120 frames, first 5
            // windows only (console, no visual change).
            if let Some(perf) = win.performance() {
                let now = perf.now();
                let mut m = meter_in.borrow_mut();
                if m.1 == 0 {
                    m.0 = now;
                }
                // Long-frame logger: every rAF gap > 10.5ms (the 100Hz
                // budget is ~10ms — anything over drops a vblank and
                // reads as judder), first 60 hits.
                if m.3 > 0.0 {
                    let dt = now - m.3;
                    if dt > 10.5 && m.4 < 60 {
                        m.4 += 1;
                        log(&format!(
                            "long-frame #{m4}: {dt:.1}ms gap (t={now:.0}ms)",
                            m4 = m.4
                        ));
                    }
                }
                m.3 = now;
                m.1 += 1;
                if m.1 >= 120 {
                    let avg = (now - m.0) / m.1 as f64;
                    m.2 += 1;
                    if m.2 <= 5 {
                        log(&format!(
                            "fps-meter: avg {avg:.2} ms/frame (~{:.0} fps)",
                            1000.0 / avg.max(0.01)
                        ));
                    }
                    m.0 = now;
                    m.1 = 0;
                }
            }
            if let Some(c) = slot.borrow().as_ref() {
                let _ = win.request_animation_frame(c.as_ref().unchecked_ref());
            }
        }
    });
    *cb_slot.borrow_mut() = Some(cb);
    if let Some(win) = web_sys::window() {
        if let Some(c) = cb_slot.borrow().as_ref() {
            let _ = win.request_animation_frame(c.as_ref().unchecked_ref());
        }
    }
}

// One-shot media query matched to the current DPR; when it stops
// matching (zoom, monitor move), sync the canvas, re-layout Slint, and
// re-arm for the next change. Same pattern winit uses natively.
fn arm_dpr_listener(weak: slint::Weak<App>) {
    let Some(win) = web_sys::window() else { return };
    let dpr = win.device_pixel_ratio();
    let query = format!("(resolution: {dpr}dppx)");
    let Ok(Some(mql)) = win.match_media(&query) else { return };
    let closure = Closure::<dyn FnMut(web_sys::MediaQueryListEvent)>::new(move |_| {
        sync_canvas_dpr();
        if let Some(ui) = weak.upgrade() {
            if let Some(win) = web_sys::window() {
                let w = win.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                let h = win.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                if w >= 1.0 && h >= 1.0 {
                    let _ = ui.window().dispatch_event_with_result(
                        slint::platform::WindowEvent::Resized {
                            size: slint::LogicalSize::new(w, h),
                        },
                    );
                }
            }
            arm_dpr_listener(ui.as_weak());
        }
    });
    let _ = mql.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
    closure.forget();
}

#[wasm_bindgen(start)]
pub fn main() {
    let ui = App::new().unwrap();

    // Projects live in memory, but metadata + small thumbs persist in
    // localStorage ("stored in this browser", Svelte parity). Videos are
    // session-only: the blob cannot fit in storage and is re-imported.
    let rows: Rc<slint::VecModel<ProjectMeta>> = Rc::new(slint::VecModel::from(Vec::new()));
    let store: Rc<RefCell<Vec<StoredProject>>> = Rc::new(RefCell::new(Vec::new()));
    let engine_ready: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let transcribe_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let current_id: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let doc_hist: Rc<RefCell<DocHistory>> = Rc::new(RefCell::new(DocHistory {
        undo: Vec::new(),
        redo: Vec::new(),
    }));
    {
        let mut n = 0usize;
        for sp in load_projects().into_iter().take(MAX_STORED_PROJECTS) {
            if sp.id.is_empty() {
                continue;
            }
            let (has_thumb, thumb, keep) = match sp.thumb {
                Some((w, h, px)) if w > 0 && h > 0 && px.len() == w as usize * h as usize * 4 => {
                    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(w, h);
                    buf.make_mut_bytes().copy_from_slice(&px);
                    (true, slint::Image::from_rgba8(buf), Some((w, h, px)))
                }
                _ => (false, slint::Image::default(), None),
            };
            rows.insert(
                n,
                ProjectMeta {
                    id: sp.id.clone().into(),
                    name: sp.name.clone().into(),
                    date: sp.date.clone().into(),
                    duration: sp.duration.clone().into(),
                    has_thumb,
                    thumb,
                },
            );
            store.borrow_mut().push(StoredProject {
                id: sp.id,
                name: sp.name,
                date: sp.date,
                duration: sp.duration,
                thumb: keep,
            });
            n += 1;
        }
    }
    ui.set_projects(slint::ModelRc::new(rows.clone()));
    ui.set_project_count(rows.row_count() as i32);

    // ── hidden file picker + frame grabber ──────────────────────────────
    let picker: Rc<RefCell<PickerState>> = {
        let window = web_sys::window().expect("no window");
        let doc = window.document().expect("no document");

        let input: web_sys::HtmlInputElement = doc
            .create_element("input")
            .expect("create input")
            .dyn_into()
            .expect("input cast");
        input.set_type("file");
        input.set_accept("video/*");
        // Testability: attached (hidden) inputs accept programmatic files
        // via CDP; detached nodes cannot. No visual change.
        input
            .style()
            .set_property("display", "none")
            .expect("hide input");
        doc.body()
            .expect("body")
            .append_child(&input)
            .expect("attach input");

        // Subtitle file picker (Svelte parity: "Use subtitle file instead")
        let sub_input: web_sys::HtmlInputElement = doc
            .create_element("input")
            .expect("create sub input")
            .dyn_into()
            .expect("sub input cast");
        sub_input.set_type("file");
        sub_input.set_accept(".srt,.vtt,text/vtt");
        sub_input
            .style()
            .set_property("display", "none")
            .expect("hide sub input");
        doc.body()
            .expect("body")
            .append_child(&sub_input)
            .expect("attach sub input");

        let video: web_sys::HtmlVideoElement = doc
            .create_element("video")
            .expect("create video")
            .dyn_into()
            .expect("video cast");
        video.set_muted(true);
        video.set_preload("auto");

        let canvas: web_sys::HtmlCanvasElement = doc
            .create_element("canvas")
            .expect("create canvas")
            .dyn_into()
            .expect("canvas cast");

        // Tiny dedicated canvas for the scene scanner (never resized by
        // grab_frame, so no borrow/size races with the preview path).
        let scan_canvas: web_sys::HtmlCanvasElement = doc
            .create_element("canvas")
            .expect("create scan canvas")
            .dyn_into()
            .expect("scan canvas cast");
        scan_canvas.set_width(64);
        scan_canvas.set_height(36);

        Rc::new(RefCell::new(PickerState {
            input,
            sub_input,
            video,
            canvas,
            scan_canvas,
            object_url: None,
            video_file: None,
            attach_to: None,
            user_muted: false,
        }))
    };
    let pending: Rc<RefCell<PendingVideo>> = Rc::new(RefCell::new(PendingVideo::default()));

    // change -> load file into hidden video
    {
        let picker_c = picker.clone();
        let pending_c = pending.clone();
        let weak_c = ui.as_weak();
        let picker_attach = picker.clone();
        let on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let file = {
                let st = picker_c.borrow();
                st.input.files().and_then(|l| l.get(0))
            };
            let Some(file) = file else { return };
            let name: String = file.name();
            match file.clone().dyn_into::<web_sys::Blob>() {
                Ok(b) => {
                    picker_c.borrow_mut().video_file = Some(b);
                }
                Err(_) => {
                    log("video blob failed");
                    return;
                }
            }
            // Editor Import path: attach this pick to the current project
            // instead of opening the new-project dialog flow.
            if let Some(pid) = picker_c.borrow_mut().attach_to.take() {
                match file.clone().dyn_into::<web_sys::Blob>() {
                    Ok(b) => {
                        let ui_w = weak_c.clone();
                        let picker_a = picker_attach.clone();
                        wasm_bindgen_futures::spawn_local(attach_video(ui_w, picker_a, pid, Some(b)));
                    }
                    Err(_) => {
                        log("attach blob failed");
                    }
                }
            }
            {
                let mut st = picker_c.borrow_mut();
                if let Some(old) = st.object_url.take() {
                    let _ = web_sys::Url::revoke_object_url(&old);
                }
                match web_sys::Url::create_object_url_with_blob(&file) {
                    Ok(url) => {
                        let _ = st.video.pause();
                        st.video.set_src(&url);
                        st.object_url = Some(url);
                    }
                    Err(_) => {
                        log("createObjectURL failed");
                        return;
                    }
                }
            }
            pending_c.borrow_mut().name = name;
        });
        picker
            .borrow()
            .input
            .add_event_listener_with_callback("change", on_change.as_ref().unchecked_ref())
            .expect("change listener");
        on_change.forget();
    }
    // subtitle picker change -> parse .srt/.vtt into the live document
    // (tscaps Import = subtitle import, not video).
    {
        let weak_sub = ui.as_weak();
        let hist_sub = doc_hist.clone();
        let sub_on_change = Closure::<dyn FnMut(web_sys::Event)>::new(move |e: web_sys::Event| {
            let input: web_sys::HtmlInputElement = e
                .target()
                .unwrap()
                .dyn_into()
                .expect("sub input cast");
            let file = input.files().and_then(|l| l.get(0));
            let _ = input.set_value("");
            let Some(file) = file else { return };
            let name: String = file.name();
            let fmt = if name.rsplit('.').next().unwrap_or("").to_lowercase() == "vtt" {
                "vtt"
            } else {
                "srt"
            }
            .to_string();
            let weak = weak_sub.clone();
            // Per-invocation clone: the async block moves its captures and
            // this handler runs on every file pick.
            let hist_c = hist_sub.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let ab = match JsFuture::from(file.text()).await {
                    Ok(v) => v,
                    Err(_) => {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_status("subtitle read failed".into());
                            ui.set_status_is_error(true);
                        }
                        return;
                    }
                };
                let text = match ab.as_string() {
                    Some(t) => t,
                    None => {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_status("subtitle read failed".into());
                            ui.set_status_is_error(true);
                        }
                        return;
                    }
                };
                let parsed = match caption_core::parse_subtitle_json(&text, &fmt) {
                    Ok(p) => p,
                    Err(e) => {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_status(format!("subtitle parse failed: {e}").into());
                            ui.set_status_is_error(true);
                        }
                        return;
                    }
                };
                let (cues_json, n, dur) = match serde_json::from_str::<serde_json::Value>(&parsed) {
                    Ok(v) => {
                        let cues = v.get("cues").cloned().unwrap_or(serde_json::Value::Null);
                        let n = cues.as_array().map(|a| a.len()).unwrap_or(0);
                        let d = v
                            .get("duration_frames")
                            .and_then(|x| x.as_u64())
                            .unwrap_or(0) as u32;
                        (cues.to_string(), n, d)
                    }
                    Err(_) => {
                        if let Some(ui) = weak.upgrade() {
                            ui.set_status("subtitle parse failed".into());
                            ui.set_status_is_error(true);
                        }
                        return;
                    }
                };
                let pre_undo = doc_json_now();
                if let Err(e) = caption_core::replace_cues_json(&cues_json) {
                    if let Some(ui) = weak.upgrade() {
                        ui.set_status(format!("subtitle apply failed: {e}").into());
                        ui.set_status_is_error(true);
                    }
                    return;
                }
                if dur > 0 {
                    if let Ok(mut doc) = caption_core::document().write() {
                        doc.duration_frames = dur;
                    }
                }
                overlay_render::invalidate();
                let doc_str = caption_core::document()
                    .read()
                    .ok()
                    .and_then(|d| serde_json::to_string(&*d).ok())
                    .unwrap_or_else(|| "{}".to_string());
                if let Some(ui) = weak.upgrade() {
                    if let Some(snap) = pre_undo {
                        hist_c.borrow_mut().push(snap);
                        hist_c.borrow().sync_buttons(&ui);
                    }
                    ui.set_doc_text(doc_str.into());
                    overlay_render::request(&ui, ui.get_frame());
                    ui.set_status(
                        format!("imported {n} cue{} from {name}", if n == 1 { "" } else { "s" })
                            .into(),
                    );
                    ui.set_status_is_error(false);
                }
            });
        });
        picker
            .borrow()
            .sub_input
            .add_event_listener_with_callback("change", sub_on_change.as_ref().unchecked_ref())
            .expect("sub change listener");
        sub_on_change.forget();
    }
    // loadedmetadata -> probe real dimensions/duration into plain Rust state.
    // The Slint dialog is filled from the probe pump (np-check), never here:
    // DOM events run outside Slint's event loop.
    {
        let video = picker.borrow().video.clone();
        let video_m = video.clone();
        let pending_m = pending.clone();
        let on_meta = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let d = video_m.duration();
            let t = if d.is_finite() {
                0.1f64.min((d - 0.05).max(0.0))
            } else {
                0.0
            };
            video_m.set_current_time(t);
            let w = video_m.video_width();
            let h = video_m.video_height();
            let mut p = pending_m.borrow_mut();
            p.duration = fmt_dur(d);
            p.duration_secs = if d.is_finite() { d } else { 0.0 };
            p.meta = format!("{w}×{h} · {}", p.duration);
        });
        video
            .add_event_listener_with_callback("loadedmetadata", on_meta.as_ref().unchecked_ref())
            .expect("metadata listener");
        on_meta.forget();
    }
    // seeked -> capture frame pixels into pending state only (no Slint
    // calls here either; thumbnail rides along when the dialog opens).
    {
        let picker_s = picker.clone();
        let pending_s = pending.clone();
        let on_seeked = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let res = {
                let st = picker_s.borrow();
                capture_frame(&st.video, &st.canvas)
            };
            match res {
                Ok((img, bytes, w, h)) => {
                    let raw = downsample_rgba(w, h, &bytes, STORAGE_THUMB_W);
                    let mut ps = pending_s.borrow_mut();
                    ps.thumb = Some(img);
                    ps.thumb_raw =
                        if raw.0 > 0 && raw.1 > 0 && raw.2.len() == raw.0 as usize * raw.1 as usize * 4
                        {
                            Some(raw)
                        } else {
                            None
                        };
                    // NOTE: object URL stays alive — revoked centrally on
                    // pick/back/cancel. Revoking here killed playback.
                }
                Err(e) => log(&format!("thumbnail failed: {e}")),
            }
        });
        picker
            .borrow()
            .video
            .add_event_listener_with_callback("seeked", on_seeked.as_ref().unchecked_ref())
            .expect("seeked listener");
        on_seeked.forget();
    }
    // ended -> stop the pump cleanly (timer is bound to `playing`).
    {
        let weak_end = ui.as_weak();
        let on_ended = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            if let Some(ui) = weak_end.upgrade() {
                ui.set_playing(false);
            }
        });
        picker
            .borrow()
            .video
            .add_event_listener_with_callback("ended", on_ended.as_ref().unchecked_ref())
            .expect("ended listener");
        on_ended.forget();
    }
    // error -> log + cleanup (Svelte shows a probeError notice; no slot on Home yet)
    {
        let picker_e = picker.clone();
        let on_error = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            log("Could not read video");
            if let Some(url) = picker_e.borrow_mut().object_url.take() {
                let _ = web_sys::Url::revoke_object_url(&url);
            }
        });
        picker
            .borrow()
            .video
            .add_event_listener_with_callback("error", on_error.as_ref().unchecked_ref())
            .expect("error listener");
        on_error.forget();
    }

    // ── wiring ──────────────────────────────────────────────────────────
    // Svelte parity: "+ New project" opens the native picker directly.
    // The dialog opens only after the probe fills name + meta.
    let picker_new = picker.clone();
    let pending_new = pending.clone();
    let weak_new = ui.as_weak();
    ui.on_new_project(move || {
        log("new-project fired");
        pending_new.borrow_mut().clear();
        if let Some(ui) = weak_new.upgrade() {
            ui.set_probe_pending(true);
        }
        let st = picker_new.borrow();
        st.input.set_value("");
        st.input.click();
    });

    // Probe pump target: runs inside Slint's event loop via probe-timer.
    // Carries a finished probe (plain Rust state) into dialog props.
    let weak = ui.as_weak();
    let pending_check = pending.clone();
    ui.on_np_check(move || {
        let ready = !pending_check.borrow().meta.is_empty();
        if !ready {
            return;
        }
        if let Some(ui) = weak.upgrade() {
            if ui.get_show_new_project() {
                ui.set_probe_pending(false);
                return;
            }
            let p = pending_check.borrow();
            // Default project name = filename without extension (Svelte parity).
            let stem = p
                .name
                .rsplit_once('.')
                .map(|(b, _)| b.to_string())
                .unwrap_or_else(|| p.name.clone());
            ui.set_np_video_name(p.name.clone().into());
            ui.set_np_video_meta(p.meta.clone().into());
            ui.set_np_name(stem.into());
            ui.set_show_new_project(true);
            ui.set_probe_pending(false);
        }
    });

    let weak = ui.as_weak();
    let pending_start = pending.clone();
    let rows_start = rows.clone();
    let store_start = store.clone();
    let picker_start = picker.clone();
    let gen_start = transcribe_gen.clone();
    let engine_start = engine_ready.clone();
    let current_start = current_id.clone();
    let hist_start = doc_hist.clone();
    ui.on_np_start(move || {
        log("np-start fired");
        if let Some(ui) = weak.upgrade() {
            // Unique id (Svelte parity: randomUUID with timestamp fallback).
            let ms = js_sys::Date::now() as u64;
            let r = (js_sys::Math::random() * 4294967295.0) as u64;
            let id = format!("p{ms:x}_{r:x}");
            let mut ps = pending_start.borrow_mut();
            let thumb_img = ps.thumb.clone();
            let thumb_raw = ps.thumb_raw.take();
            let has_thumb = thumb_img.is_some();
            let duration = if ps.duration.is_empty() {
                "0:00".to_string()
            } else {
                ps.duration.clone()
            };
            drop(ps);
            rows_start.insert(
                0,
                ProjectMeta {
                    id: id.clone().into(),
                    name: ui.get_np_name(),
                    date: today_str().into(),
                    duration: duration.clone().into(),
                    has_thumb,
                    thumb: thumb_img.unwrap_or_default(),
                },
            );
            store_start.borrow_mut().insert(
                0,
                StoredProject {
                    id: id.clone(),
                    name: ui.get_np_name().to_string(),
                    date: today_str(),
                    duration,
                    thumb: thumb_raw,
                },
            );
            save_projects(&store_start.borrow());
            current_start.borrow_mut().replace(id.clone());
            {
                let save_id = id.clone();
                if let Some(f) = picker_start.borrow().video_file.clone() {
                    let val: JsValue = f.into();
                    wasm_bindgen_futures::spawn_local(async move {
                        if let Err(e) = idb_put(&save_id, &val).await {
                            log(&format!("video persist failed: {e}"));
                        }
                    });
                }
            }
            ui.set_project_count(rows_start.row_count() as i32);
            ui.set_probe_pending(false);
            ui.set_project_name(ui.get_np_name());
            ui.set_show_new_project(false);
            ui.set_route("editor".into());
            ui.set_status("ready".into());
            let model_idx = ui.get_np_model_selected();
            let lang_idx = ui.get_np_language_selected();
            let template_idx = ui.get_np_template_selected();
            // Set template SEGERA — jangan tunggu transcribe sukses. Kalau
            // transcribe gagal (audio decode, model error, dst), template
            // tetap harus aktif saat user import SRT / tambah cue manual.
            if let Ok(mut doc) = caption_core::document().write() {
                doc.template = template_id(template_idx);
            }
            overlay_render::reset(&ui);
            let blob = picker_start.borrow().video_file.clone();
            let Some(blob) = blob else {
                ui.set_status("no video found to transcribe".into());
                ui.set_status_is_error(true);
                return;
            };
            let model = WHISPER_MODELS
                .get(model_idx as usize)
                .copied()
                .unwrap_or("base");
            if model != "base" {
                ui.set_status(
                    format!(
                        "model \"{model}\" is not supported by this engine build yet (only: base). Pick Base in Advanced."
                    )
                    .into(),
                );
                ui.set_status_is_error(true);
                return;
            }
            ui.set_has_video(true);
            {
                // First frame immediately (probe already decoded 0.1s),
                // so the stage is never black while video exists.
                let st = picker_start.borrow();
                let vw = st.video.video_width();
                let vh = st.video.video_height();
                if vw > 0 && vh > 0 {
                    ui.set_video_aspect(vw as f32 / vh as f32);
                    let scale = (640.0 / vw as f64).min(1.0);
                    let w = ((vw as f64 * scale).round() as u32).max(1);
                    let h = ((vh as f64 * scale).round() as u32).max(1);
                    if let Some(img) = grab_frame(&st.video, &st.canvas, w, h) {
                        ui.set_preview_image(img);
                    }
                }
                let _ = st.video.pause();
                st.video.set_current_time(0.0);
            }
            {
                let secs = pending_start.borrow().duration_secs;
                if secs.is_finite() && secs > 0.0 {
                    ui.set_duration_text(fmt_dur_ms(secs).into());
                    ui.set_scrub_max((secs * 30.0) as i32);
                    ui.set_time_text(fmt_dur_ms(0.0).into());
                    ui.set_frame(0);
                }
            }
            ui.set_status_is_error(false);
            ui.set_show_transcribe(true);
            ui.set_tr_pct(0);
            ui.set_tr_title("Preparing the engine.".into());
            ui.set_tr_sub("Loading the transcription engine.".into());
            let g = gen_start.get().wrapping_add(1);
            gen_start.set(g);
            wasm_bindgen_futures::spawn_local(run_transcribe(
                weak.clone(),
                gen_start.clone(),
                engine_start.clone(),
                g,
                blob,
                lang_idx,
                template_idx,
                hist_start.clone(),
            ));
        }
    });

    let weak = ui.as_weak();
    let pending_cancel = pending.clone();
    let picker_cancel = picker.clone();
    ui.on_np_cancel(move || {
        log("np-cancel fired");
        {
            let mut st = picker_cancel.borrow_mut();
            let _ = st.video.pause();
            if let Some(url) = st.object_url.take() {
                let _ = web_sys::Url::revoke_object_url(&url);
            }
            st.video_file = None;
        }
        if let Some(ui) = weak.upgrade() {
            pending_cancel.borrow_mut().clear();
            ui.set_probe_pending(false);
            ui.set_show_new_project(false);
        }
    });

    let picker_sub = picker.clone();
    ui.on_np_use_subtitle(move || {
        log("np-use-subtitle fired");
        let st = picker_sub.borrow();
        st.sub_input.set_value("");
        st.sub_input.click();
    });

    let weak = ui.as_weak();
    ui.on_np_toggle_advanced(move || {
        if let Some(ui) = weak.upgrade() {
            let cur = ui.get_np_advanced_open();
            ui.set_np_advanced_open(!cur);
        }
    });

    let weak = ui.as_weak();
    ui.on_export(move || {
        log("export fired");
        if let Some(ui) = weak.upgrade() {
            ui.set_show_export(true);
        }
    });

    let weak = ui.as_weak();
    ui.on_ex_export(move || {
        log("ex-export fired");
        if let Some(ui) = weak.upgrade() {
            ui.set_ex_exporting(true);
            ui.set_ex_status("Exporting…".into());
        }
    });

    let weak = ui.as_weak();
    ui.on_ex_cancel(move || {
        log("ex-cancel fired");
        if let Some(ui) = weak.upgrade() {
            ui.set_playing(false);
            ui.set_has_video(false);
            ui.set_video_aspect(16.0 / 9.0);
            ui.set_route("home".into());
        }
    });

    let current_att = current_id.clone();
    let picker_att = picker.clone();
    let weak_scrub = ui.as_weak();
    let picker_scrub = picker.clone();
    ui.on_scrub_to(move |f: f32| {
        let st = picker_scrub.borrow();
        let d = st.video.duration();
        if !(d.is_finite() && d > 0.0) {
            return;
        }
        let t = (f as f64 * d).clamp(0.0, d);
        st.video.set_current_time(t);
        let vw = st.video.video_width();
        let vh = st.video.video_height();
        let img = if vw > 0 && vh > 0 {
            let scale = (640.0 / vw as f64).min(1.0);
            let w = ((vw as f64 * scale).round() as u32).max(1);
            let h = ((vh as f64 * scale).round() as u32).max(1);
            grab_frame(&st.video, &st.canvas, w, h)
        } else {
            None
        };
        if let Some(ui) = weak_scrub.upgrade() {
            if let Some(img) = img {
                ui.set_preview_image(img);
            }
            ui.set_time_text(fmt_dur_ms(t).into());
            let frame = (t * 30.0) as i32;
            ui.set_frame(frame);
            ui.set_overlay_text(active_cue_at((t * 1000.0) as u32).into());
            overlay_render::request(&ui, frame);
        }
    });

    let weak_vol = ui.as_weak();
    let picker_vol = picker.clone();
    ui.on_volume_to(move |f: f32| {
        let v = f.clamp(0.0, 1.0) as f64;
        let mut st = picker_vol.borrow_mut();
        st.video.set_volume(v);
        st.video.set_muted(false);
        st.user_muted = false;
        if let Some(ui) = weak_vol.upgrade() {
            ui.set_volume_frac(v as f32);
        }
    });

    let weak_mute = ui.as_weak();
    let picker_mute = picker.clone();
    ui.on_mute_toggle(move || {
        let mut st = picker_mute.borrow_mut();
        let m = !st.video.muted();
        st.video.set_muted(m);
        st.user_muted = m;
        if let Some(ui) = weak_mute.upgrade() {
            ui.set_volume_frac(if m { 0.0 } else { st.video.volume() as f32 });
        }
    });

    const PLAY_RATES: [f64; 4] = [0.5, 1.0, 1.5, 2.0];
    let weak_rate = ui.as_weak();
    let picker_rate = picker.clone();
    ui.on_rate_step(move |dir: i32| {
        let st = picker_rate.borrow();
        let cur = st.video.playback_rate();
        let mut idx = 0usize;
        let mut best = f64::INFINITY;
        for (i, &r) in PLAY_RATES.iter().enumerate() {
            let d = (r - cur).abs();
            if d < best {
                best = d;
                idx = i;
            }
        }
        idx = (idx as i32 + dir).clamp(0, PLAY_RATES.len() as i32 - 1) as usize;
        let next = PLAY_RATES[idx];
        st.video.set_playback_rate(next);
        if let Some(ui) = weak_rate.upgrade() {
            ui.set_rate_text(fmt_rate(next).into());
            ui.set_status(format!("playback {next}×").into());
            ui.set_status_is_error(false);
        }
    });

    ui.on_attach_video(move || {
        let pid = current_att.borrow().clone();
        let Some(pid) = pid else { return };
        picker_att.borrow_mut().attach_to = Some(pid);
        let st = picker_att.borrow();
        st.input.set_value("");
        st.input.click();
    });

    let weak = ui.as_weak();
    ui.on_ex_toggle_advanced(move || {
        if let Some(ui) = weak.upgrade() {
            let cur = ui.get_ex_advanced_open();
            ui.set_ex_advanced_open(!cur);
        }
    });

    let weak = ui.as_weak();
    ui.on_ex_toggle_subtitles(move || {
        if let Some(ui) = weak.upgrade() {
            let cur = ui.get_ex_subtitles_open();
            ui.set_ex_subtitles_open(!cur);
        }
    });

    let weak = ui.as_weak();
    ui.on_ex_export_subs(move |i| {
        log(&format!("ex-export-subs fired: {}", i));
        if let Some(ui) = weak.upgrade() {
            ui.set_ex_status("Subtitle exported.".into());
        }
    });

    let weak = ui.as_weak();
    let rows_open = rows.clone();
    let picker_open = picker.clone();
    let current_open = current_id.clone();
    ui.on_project_open(move |i| {
        log(&format!("project-open fired: {}", i));
        if let Some(ui) = weak.upgrade() {
            let idx = i as usize;
            let mut pid: Option<String> = None;
            if idx < rows_open.row_count() {
                if let Some(pm) = rows_open.row_data(idx) {
                    ui.set_project_name(pm.name);
                    pid = Some(pm.id.to_string());
                }
            }
            if let Some(ref pid) = pid {
                current_open.borrow_mut().replace(pid.clone());
            }
            ui.set_route("editor".into());
            ui.set_status("ready".into());
            // Dokumen project lain: reset overlay + cache SVG supaya render
            // berikutnya pasti fresh (tanpa ini caption bisa hilang).
            overlay_render::reset(&ui);
            // Restore the persisted video blob (if any): reattach, wait
            // for a decodable frame, then show it. No blob -> stage stays
            // black with SAMPLE (same as before this change).
            if let Some(pid) = pid {
                let ui_w = weak.clone();
                let picker_o = picker_open.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let blob = match idb_get(&pid).await {
                        Ok(Some(b)) => b,
                        Ok(None) => return,
                        Err(e) => {
                            log(&format!("video restore failed: {e}"));
                            return;
                        }
                    };
                    let url = match web_sys::Url::create_object_url_with_blob(&blob) {
                        Ok(u) => u,
                        Err(_) => return,
                    };
                    let video = picker_o.borrow().video.clone();
                    video.set_src(&url);
                    {
                        let mut st = picker_o.borrow_mut();
                        if let Some(old) = st.object_url.take() {
                            let _ = web_sys::Url::revoke_object_url(&old);
                        }
                        st.object_url = Some(url);
                        st.video_file = Some(blob);
                    }
                    if JsFuture::from(once_event_timeout(&video, "loadedmetadata", 10_000))
                        .await
                        .is_err()
                    {
                        log("restore: metadata timeout");
                        return;
                    }
                    video.set_current_time(0.1);
                    if JsFuture::from(once_event_timeout(&video, "seeked", 10_000))
                        .await
                        .is_err()
                    {
                        log("restore: seeked timeout");
                        return;
                    }
                    let vw = video.video_width();
                    let vh = video.video_height();
                    if vw == 0 || vh == 0 {
                        log("restore: vw/vh zero after seek");
                        return;
                    }
                    let scale = (640.0 / vw as f64).min(1.0);
                    let w = ((vw as f64 * scale).round() as u32).max(1);
                    let h = ((vh as f64 * scale).round() as u32).max(1);
                    // 'seeked' can fire before the compositor has a
                    // paintable frame — poll until pixels are non-black.
                    // (clone canvas: must not hold the RefCell borrow
                    // across the awaits inside grab_frame_settled)
                    let canvas = picker_o.borrow().canvas.clone();
                    let img = grab_frame_settled(&video, &canvas, w, h).await;
                    if let Some(img) = img {
                        if let Some(ui) = ui_w.upgrade() {
                            ui.set_preview_image(img);
                            ui.set_has_video(true);
                            ui.set_video_aspect(vw as f32 / vh as f32);
                            ui.set_rate_text(fmt_rate(video.playback_rate()).into());
                            ui.set_time_text(fmt_dur(0.0).into());
                            ui.set_frame(0);
                            let d = video.duration();
                            if d.is_finite() && d > 0.0 {
                                ui.set_duration_text(fmt_dur_ms(d).into());
                                ui.set_scrub_max((d * 30.0) as i32);
                            }
                        }
                    } else {
                        log("restore: grab frame failed");
                    }
                });
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_project_export(move |i| {
        log(&format!("project-export fired: {}", i));
        if let Some(ui) = weak.upgrade() {
            ui.set_show_export(true);
        }
    });

    let weak = ui.as_weak();
    let rows_del = rows.clone();
    let store_del = store.clone();
    ui.on_project_delete(move |i| {
        log(&format!("project-delete fired: {}", i));
        let idx = i as usize;
        if idx < rows_del.row_count() {
            let del_id = rows_del.row_data(idx).map(|pm| pm.id.to_string());
            rows_del.remove(idx);
            {
                let mut st = store_del.borrow_mut();
                if idx < st.len() {
                    st.remove(idx);
                }
            }
            save_projects(&store_del.borrow());
            if let Some(did) = del_id {
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(e) = idb_del(&did).await {
                        log(&format!("video delete failed: {e}"));
                    }
                });
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_project_count(rows_del.row_count() as i32);
            }
        }
    });

    let weak = ui.as_weak();
    let picker_back = picker.clone();
    let current_back = current_id.clone();
    ui.on_back(move || {
        log("back fired");
        {
            let mut st = picker_back.borrow_mut();
            let _ = st.video.pause();
            let _ = st.video.remove_attribute("src");
            if let Some(url) = st.object_url.take() {
                let _ = web_sys::Url::revoke_object_url(&url);
            }
            st.video_file = None;
        }
        if let Some(ui) = weak.upgrade() {
            current_back.borrow_mut().take();
            ui.set_playing(false);
            ui.set_has_video(false);
            ui.set_video_aspect(16.0 / 9.0);
            ui.set_route("home".into());
        }
    });

    let picker_imp = picker.clone();
    ui.on_import_video(move || {
        picker_imp.borrow().sub_input.set_value("");
        picker_imp.borrow().sub_input.click();
    });

    let weak_exp = ui.as_weak();
    ui.on_export_subs(move |fmt: slint::SharedString| {
        let fmt = if fmt.as_str() == "vtt" { "vtt" } else { "srt" };
        let cues_json = match caption_core::document().read() {
            Ok(d) => serde_json::to_string(&d.cues).unwrap_or_else(|_| "[]".to_string()),
            Err(_) => "[]".to_string(),
        };
        let text = match caption_core::format_subtitle_json(&cues_json, fmt) {
            Ok(t) => t,
            Err(e) => {
                if let Some(ui) = weak_exp.upgrade() {
                    ui.set_status(format!("subtitle export failed: {e}").into());
                    ui.set_status_is_error(true);
                }
                return;
            }
        };
        let array = js_sys::Array::new();
        array.push(&JsValue::from_str(&text));
        let parts: JsValue = array.into();
        let blob = match web_sys::Blob::new_with_str_sequence(&parts) {
            Ok(b) => b,
            Err(_) => return,
        };
        let url = match web_sys::Url::create_object_url_with_blob(&blob) {
            Ok(u) => u,
            Err(_) => return,
        };
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Ok(a) = doc.create_element("a") {
                if let Ok(a) = a.dyn_into::<web_sys::HtmlAnchorElement>() {
                    a.set_href(&url);
                    a.set_download(&format!("captions.{fmt}"));
                    a.click();
                }
            }
        }
        let _ = web_sys::Url::revoke_object_url(&url);
        if let Some(ui) = weak_exp.upgrade() {
            ui.set_status(format!("subtitle exported ({fmt}).").into());
            ui.set_status_is_error(false);
        }
    });

    let weak = ui.as_weak();
    let picker_play = picker.clone();
    ui.on_toggle_play(move || {
        if let Some(ui) = weak.upgrade() {
            let p = !ui.get_playing();
            ui.set_playing(p);
            let mut st = picker_play.borrow_mut();
            if p {
                // Probe default is muted; a play press is a user gesture, so
                // restore sound unless the user explicitly muted.
                if !st.user_muted {
                    st.video.set_muted(false);
                }
                let _ = st.video.play();
            } else {
                let _ = st.video.pause();
            }
        }
    });

    let weak = ui.as_weak();
    ui.on_name_edited(move |t| {
        if let Some(ui) = weak.upgrade() {
            ui.set_project_name(t);
        }
    });

    let weak = ui.as_weak();
    ui.on_template_chosen(move |i| {
        if let Some(ui) = weak.upgrade() {
            ui.set_template_selected(i);
            apply_template_style(&ui, i);
            // Pipeline template asli (41 template caption-core) membaca
            // doc.template. Index picker 1-based (0 = Default v0);
            // template_id() konversi ke 0-based TEMPLATE_IDS.
            if let Ok(mut doc) = caption_core::document().write() {
                doc.template = template_id(i);
            }
            overlay_render::invalidate();
            overlay_render::request(&ui, ui.get_frame());
        }
    });

    // Galeri template: model awal + filter search (case-insensitive).
    set_template_models(&ui, "");
    apply_template_style(&ui, 0);
    // Paritas main (state.svelte.ts DEFAULT_PARAMS): family "Arial" + weight
    // 400 - WasmFontSource.resolve_font = HashMap EXACT match (name+weight),
    // face arial.ttf name-table "Arial"/400 harus persis; default caption-core
    // ("Arial Black"/700) tak pernah resolve di editor.
    {
        let mut boot = caption_core::CaptionDocument::default();
        boot.params.font_family = "Arial".into();
        boot.params.font_size = 110.0;
        boot.params.font_weight = 400;
        boot.params.color = "#ffdd57".into();
        boot.params.y_frac = 0.78;
        boot.params.width_frac = 0.78;
        boot.params.pop_in_ms = 250;
        boot.params.travel_px = 48.0;
        if let Ok(s) = serde_json::to_string(&boot) {
            let _ = caption_core::set_document_json(&s);
        }
    }
    // Font template asli di latar (tak blokir first paint).
    preload_tplfonts();
    let weak = ui.as_weak();
    ui.on_template_search(move |q| {
        if let Some(ui) = weak.upgrade() {
            set_template_models(&ui, &q);
        }
    });

    let weak = ui.as_weak();
    ui.on_direction_chosen(move |i| {
        if let Some(ui) = weak.upgrade() {
            ui.set_direction_selected(i);
        }
    });

    let weak = ui.as_weak();
    ui.on_apply(move || {
        log("apply fired");
        if let Some(ui) = weak.upgrade() {
            ui.set_status("applied".into());
        }
    });

    let weak_undo = ui.as_weak();
    let hist_undo = doc_hist.clone();
    ui.on_undo(move || {
        let snap = hist_undo.borrow_mut().undo.pop();
        let Some(snap) = snap else { return };
        let cur = doc_json_now();
        if caption_core::set_document_json(&snap).is_err() {
            hist_undo.borrow_mut().undo.push(snap);
            return;
        }
        overlay_render::invalidate();
        if let Some(c) = cur {
            hist_undo.borrow_mut().redo.push(c);
        }
        if let Some(ui) = weak_undo.upgrade() {
            if let Some(s) = doc_json_now() {
                ui.set_doc_text(s.into());
            }
            hist_undo.borrow().sync_buttons(&ui);
            overlay_render::request(&ui, ui.get_frame());
            ui.set_status("undone".into());
        }
    });
    let weak_redo = ui.as_weak();
    let hist_redo = doc_hist.clone();
    ui.on_redo(move || {
        let snap = hist_redo.borrow_mut().redo.pop();
        let Some(snap) = snap else { return };
        let cur = doc_json_now();
        if caption_core::set_document_json(&snap).is_err() {
            hist_redo.borrow_mut().redo.push(snap);
            return;
        }
        overlay_render::invalidate();
        if let Some(c) = cur {
            hist_redo.borrow_mut().undo.push(c);
        }
        if let Some(ui) = weak_redo.upgrade() {
            if let Some(s) = doc_json_now() {
                ui.set_doc_text(s.into());
            }
            hist_redo.borrow().sync_buttons(&ui);
            overlay_render::request(&ui, ui.get_frame());
            ui.set_status("redone".into());
        }
    });

    let weak = ui.as_weak();
    let picker_seek = picker.clone();
    ui.on_seek(move |f| {
        picker_seek.borrow().video.set_current_time(f as f64 / 30.0);
        if let Some(ui) = weak.upgrade() {
            ui.set_frame(f);
            ui.set_overlay_text(active_cue_at((f as f64 / 30.0 * 1000.0) as u32).into());
            overlay_render::request(&ui, f);
        }
    });

    // Paused stepping (settings popup steppers + F/W/S shortcuts).
    let weak_nf = ui.as_weak();
    let picker_nf = picker.clone();
    ui.on_nav_frame_step(move |dir: i32| {
        let (f, max) = match weak_nf.upgrade() {
            Some(ui) => (ui.get_frame(), ui.get_scrub_max()),
            None => return,
        };
        let nf = (f + dir).clamp(0, max.max(0));
        let ui_w = weak_nf.clone();
        let picker_c = picker_nf.clone();
        wasm_bindgen_futures::spawn_local(async move {
            seek_grab_frame(ui_w, picker_c, nf as f64 / 30.0, nf).await;
        });
    });

    let weak_nw = ui.as_weak();
    ui.on_nav_word_step(move |_dir: i32| {
        // Honest signpost: the engine emits segment timestamps only.
        // Wire real word times here when whisper-spike gains them.
        if let Some(ui) = weak_nw.upgrade() {
            ui.set_status(
                "Word stepping needs word timestamps — this engine build emits segment times only."
                    .into(),
            );
            ui.set_status_is_error(false);
        }
    });

    let scene_cache: Rc<RefCell<std::collections::HashMap<String, Vec<f64>>>> =
        Rc::new(RefCell::new(std::collections::HashMap::new()));
    let scene_gen: Rc<Cell<u64>> = Rc::new(Cell::new(0));
    let weak_ns = ui.as_weak();
    let picker_ns = picker.clone();
    let cache_ns = scene_cache.clone();
    let gen_ns = scene_gen.clone();
    let current_ns = current_id.clone();
    ui.on_nav_scene_step(move |dir: i32| {
        let pid = match current_ns.borrow().clone() {
            Some(p) => p,
            None => return,
        };
        let g = gen_ns.get().wrapping_add(1);
        gen_ns.set(g);
        let ui_w = weak_ns.clone();
        let picker_c = picker_ns.clone();
        let cache_c = cache_ns.clone();
        let gen_c = gen_ns.clone();
        let current_c = current_ns.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let bounds = match cache_c.borrow().get(&pid).cloned() {
                Some(b) => b,
                None => match scan_scenes(
                    ui_w.clone(),
                    picker_c.clone(),
                    current_c.clone(),
                    pid.clone(),
                    gen_c,
                    g,
                )
                .await
                {
                    Some(b) => {
                        cache_c.borrow_mut().insert(pid.clone(), b.clone());
                        b
                    }
                    None => return,
                },
            };
            if current_c.borrow().as_deref() != Some(pid.as_str()) {
                return;
            }
            let t = picker_c.borrow().video.current_time();
            let target = if dir > 0 {
                bounds.iter().find(|b| **b > t + 0.05).copied()
            } else {
                bounds.iter().rev().find(|b| **b < t - 0.05).copied()
            };
            match target {
                Some(bt) => {
                    if let Some(ui) = ui_w.upgrade() {
                        ui.set_status(format!("scene @ {}", fmt_dur_ms(bt)).into());
                    }
                    seek_grab_frame(ui_w, picker_c, bt, (bt * 30.0) as i32).await;
                }
                None => {
                    if let Some(ui) = ui_w.upgrade() {
                        ui.set_status("No further scene in that direction.".into());
                    }
                }
            }
        });
    });

    let weak_pump = ui.as_weak();
    let picker_pump = picker.clone();
    let last_grab: Rc<Cell<f64>> = Rc::new(Cell::new(-1.0));
    ui.on_pump_frame(move || {
        let (video, canvas) = {
            let st = picker_pump.borrow();
            (st.video.clone(), st.canvas.clone())
        };
        let vw = video.video_width();
        let vh = video.video_height();
        if vw == 0 || vh == 0 {
            return;
        }
        // New frame due? Gate drops duplicate grabs (sources up to 50fps).
        let t0 = video.current_time();
        if t0 - last_grab.get() < 0.02 {
            return;
        }
        last_grab.set(t0);
        let scale = (640.0 / vw as f64).min(1.0);
        let w = ((vw as f64 * scale).round() as u32).max(1);
        let h = ((vh as f64 * scale).round() as u32).max(1);
        if let Some(img) = grab_frame(&video, &canvas, w, h) {
            if let Some(ui) = weak_pump.upgrade() {
                ui.set_preview_image(img);
                let t = video.current_time();
                let d = video.duration();
                if d.is_finite() && d > 0.0 {
                    ui.set_time_text(fmt_dur_ms(t).into());
                    ui.set_overlay_text(active_cue_at((t * 1000.0) as u32).into());
                    ui.set_duration_text(fmt_dur_ms(d).into());
                    ui.set_frame((t * 30.0) as i32);
                    ui.set_scrub_max((d * 30.0) as i32);
                    overlay_render::request(&ui, (t * 30.0) as i32);
                }
            }
        }
    });

    let weak = ui.as_weak();
    let tr_gen = transcribe_gen.clone();
    ui.on_tr_cancel(move || {
        log("tr-cancel fired");
        tr_gen.set(tr_gen.get().wrapping_add(1));
        if let Some(ui) = weak.upgrade() {
            ui.set_show_transcribe(false);
            ui.set_tr_pct(0);
        }
    });

    let weak = ui.as_weak();
    ui.on_tr_done(move || {
        if let Some(ui) = weak.upgrade() {
            ui.set_show_transcribe(false);
        }
    });

    // ── live viewport sync (browser zoom / Ctrl + and -) ───────────────
    // winit's ResizeObserver only reports the *physical* canvas size.
    // Browser zoom shrinks the logical viewport (innerWidth) while the
    // physical size stays the same, so no winit Resized event fires and
    // the layout would only catch up after a reload. The browser does
    // fire `resize` on zoom, so forward it into a Slint Resized event
    // carrying the logical viewport size.
    // Responsif instan (tanpa poll-delay): event resize langsung mendorong
    // win-narrow/playbar-narrow — poll di bawah tinggal backstop.
    let narrow_state: Rc<RefCell<(bool, bool)>> = Rc::new(RefCell::new((false, false)));
    {
        let weak = ui.as_weak();
        let narrow_state = narrow_state.clone();
        let on_resize = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(win) = web_sys::window() else { return };
            let w = win.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let h = win.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            if w < 1.0 || h < 1.0 {
                return;
            }
            sync_canvas_dpr();
            apply_narrow_flags(&ui, w, &narrow_state, "resize");
            let _ = ui.window().dispatch_event_with_result(slint::platform::WindowEvent::Resized {
                size: slint::LogicalSize::new(w, h),
            });
        });
        web_sys::window()
            .expect("no window")
            .add_event_listener_with_callback("resize", on_resize.as_ref().unchecked_ref())
            .expect("resize listener");
        on_resize.forget();
    }

    // One-time environment facts for fps diagnosis (console only).
    {
        // Reflect (not Navigator::gpu()) — absent WebGPU must read as
        // undefined, never throw.
        let gpu = web_sys::window()
            .map(|w| {
                let n: JsValue = w.navigator().into();
                js_sys::Reflect::get(&n, &JsValue::from_str("gpu"))
                    .map(|v| !v.is_undefined())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let dpr = web_sys::window()
            .map(|w| w.device_pixel_ratio())
            .unwrap_or(0.0);
        let cores = web_sys::window()
            .map(|w| w.navigator().hardware_concurrency())
            .unwrap_or(0.0);
        log(&format!("env: webgpu={gpu} dpr={dpr} cores={cores:.0}"));
    }
    // Theme persistence (same key as the Svelte app: "rscaps:theme").
    {
        let stored = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item("rscaps:theme").ok().flatten());
        if stored.as_deref() == Some("light") {
            ui.set_app_is_dark(false);
        }
    }
    {
        ui.on_theme_toggled(move |dark: bool| {
            if let Some(st) = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
            {
                let _ = st.set_item("rscaps:theme", if dark { "dark" } else { "light" });
            }
        });
    }

    // HiDPI: keep attrs = rendered box * dpr (blur fix; MutationObserver wins
    // the race against winit, rAF loop + resize cover the rest).
    sync_canvas_dpr();
    arm_canvas_sync_loop();
    arm_canvas_attr_observer();
    arm_dpr_listener(ui.as_weak());

    // Reveal: by the time this fires (wasm already initialized, Slint event
    // loop running), the first frames are on the canvas.
    {
        let on_done = Closure::<dyn FnMut()>::new(move || {
            if let Some(el) = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector(".boot-splash").ok().flatten())
            {
                el.remove();
            }
        });
        let _ = web_sys::window()
            .expect("no window")
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                on_done.as_ref().unchecked_ref(),
                350,
            );
        on_done.forget();
    }

    // Width poll untuk layout responsif: backstop 120ms (jalan instan
    // lewat event resize di atas). Slint hanya baca flag.
    let weak_w = ui.as_weak();
    let narrow_poll = narrow_state.clone();
    ui.on_poll_width(move || {
        let Some(ui) = weak_w.upgrade() else { return };
        // Lebar dari DOM (window.innerWidth), BUKAN Slint window.size():
        // size() mengembalikan sampah yang melompat-lompat (10/791/913/
        // 1379/1496 di jendela diam) sehingga flag flap dan layout churn.
        let w = web_sys::window()
            .and_then(|w| w.inner_width().ok())
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        apply_narrow_flags(&ui, w, &narrow_poll, "poll");
    });
    // Sinkron awal: jangan tunggu poll pertama agar boot sempit langsung benar.
    {
        let w = web_sys::window()
            .and_then(|w| w.inner_width().ok())
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as f32;
        apply_narrow_flags(&ui, w, &narrow_state, "boot");
    }

    // Test hook: open New project dialog without a video probe when the
    // page loads with #np (headless harness cannot decode video).
    if web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .map(|h| h == "#np")
        .unwrap_or(false)
    {
        ui.set_np_name("test-video".into());
        ui.set_np_video_name("rscaps-test.webm".into());
        ui.set_np_video_meta("640×360 · 0:03".into());
        ui.set_show_new_project(true);
    }

    // Test hook: direct transcription without the probe dialog (headless
    // harness cannot load video metadata, so the dialog never opens there).
    // Load with #transcribe=<project-id>; the blob is fetched from IDB and
    // run_transcribe spawns immediately in the editor route.
    if let Some(id) = web_sys::window()
        .and_then(|w| w.location().hash().ok())
        .and_then(|h| h.strip_prefix("#transcribe=").map(|s| s.to_string()))
    {
        if let Some(ui) = ui.as_weak().upgrade() {
            ui.set_project_name("e2e-test".into());
            ui.set_route("editor".into());
            ui.set_has_video(true);
        }
        current_id.borrow_mut().replace(id.clone());
        let weak_h = ui.as_weak();
        let gen_h = transcribe_gen.clone();
        let eng_h = engine_ready.clone();
        let hist_h = doc_hist.clone();
        let pick_h = picker.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match idb_get(&id).await {
                Ok(Some(blob)) => {
                    pick_h.borrow_mut().video_file = Some(blob.clone());
                    let g = gen_h.get().wrapping_add(1);
                    gen_h.set(g);
                    run_transcribe(weak_h, gen_h, eng_h, g, blob, 0, 0, hist_h).await;
                }
                _ => log("e2e hook: video missing in IDB"),
            }
        });
    }

    let _ = ui.run();
}
