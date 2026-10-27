//! rscaps CLI library — native render pipeline (cpu/skia/fframes + ffmpeg).
//!
//! This crate can be used as a library by Tauri or other native hosts.

pub mod doc;
pub mod fframes_engine;
pub mod fonts;
pub mod images;
pub mod render;
pub mod server;

pub use render::{
    RenderVideoOpts,
    render_video,
    tool_bin,
    probe_video,
    VideoProbe,
    hw_encoder_available,
    hw_encode_args,
};