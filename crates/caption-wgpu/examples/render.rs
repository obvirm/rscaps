// Render satu SVG caption ke RGBA mentah (debug backend wgpu-lyon).
// Pakai: cargo run -p caption-wgpu --example render -- in.svg 1920 1080 out.rgba [fontdir]
use anyhow::{Context, Result};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let svg_path = args.next().context("usage: render <svg> <w> <h> <out.rgba> [fontdir]")?;
    let width: u32 = args.next().context("width")?.parse()?;
    let height: u32 = args.next().context("height")?.parse()?;
    let out = args.next().context("out")?;
    let svg = std::fs::read_to_string(&svg_path)?;

    let mut db = svgr::usvgr::fontdb::Database::new();
    db.load_system_fonts();
    if let Some(dir) = args.next() {
        db.load_fonts_dir(dir);
    }
    let mut renderer = caption_wgpu::WgpuLyonRenderer::with_fontdb(width, height, &db)?;
    let rgba = renderer.render_svg(&svg)?;
    std::fs::write(PathBuf::from(out), rgba)?;
    Ok(())
}
