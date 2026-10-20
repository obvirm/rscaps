//! Debug: render satu frame loki & v0, cetak SVG-nya — inspeksi koordinat.
use caption_core::{CaptionDocument, CaptionVideo, Cue};
use fframes::{FFramesContext, FFramesMode, Frame, TimeBase, Video, VideoSize};

fn main() {
    let doc = CaptionDocument {
        duration_frames: 180,
        cues: vec![Cue {
            start_ms: 0,
            end_ms: 2000,
            text: "Hello world".into(),
            sheet: None,
        }],
        params: caption_core::CaptionParams::default(),
        template: "loki".into(),
        sheets: Vec::new(),
        ..Default::default()
    };
    // Ukuran video UJI: 4:3 (speech-test.mp4 = 320×240 → kotak 4:3), canvas
    // preview memakai rasio ini di ui.slint; render engine pakai1920×1080.
    let ctx = FFramesContext {
        time_base: TimeBase {
            fps: CaptionVideo::FPS,
            sample_rate: 48000,
        },
        current_video_size: VideoSize {
            width: 1920,
            height: 1080,
        },
        duration_in_frames: 180,
        mode: FFramesMode::Renderer,
        scenes: None,
        media_source: None,
        font_source: None,
        abort_signal: None,
    };
    let frame = Frame::__internal_make_for_renderer(
        0,
        0,
        CaptionVideo::FPS,
        None,
        fframes::VideoDecodersWorker::new(4),
    );
    let svg = caption_core::templates::render("loki", &doc, 0.0, &ctx).expect("loki renders");
    let s = svg.to_string();
    // Cari elemen yang menentukan posisi.
    for line in s.split(['<', '>']) {
        let l = line.trim();
        if l.starts_with("text") || l.starts_with("g transform") || l.starts_with("svg") {
            println!("<{l}>");
        }
    }
}
