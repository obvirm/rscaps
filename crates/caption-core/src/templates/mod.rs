use crate::CaptionDocument;
use fframes::{FFramesContext, Svgr};

pub mod ada;
pub mod anya;
pub mod cleo;
pub mod effects;
pub mod elio;
pub mod enzo;
pub mod ezra;
pub mod freya;
pub mod hugo;
pub mod ivo;
pub mod iris;
pub mod juno;
pub mod kai;
pub mod kel;
pub mod layout;
pub mod lena;
pub mod levi;
pub mod lewis;
pub mod loki;
pub mod luca;
pub mod luna;
pub mod lyra;
pub mod milo;
pub mod mira;
pub mod naya;
pub mod noor;
pub mod nova;
pub mod nyx;
pub mod otto;
pub mod pastor;
pub mod pepper;
pub mod pico;
pub mod pipeline;
pub mod recipes;
pub mod remi;
pub mod rhea;
pub mod sara;
pub mod selene;
pub mod sol;
pub mod stack;
pub mod state;
pub mod tala;
pub mod theo;
pub mod tito;
pub mod vera;
pub mod yuki;
pub mod zara;

/// Renders `doc` through the native template identified by `template`.
///
/// Returns `None` for an unknown id so the caller can fall back to the v0
/// renderer, and `Some(Svgr::empty())` when the id is known but nothing is
/// on screen at `now_ms`.
pub fn render<'a>(
    template: &str,
    doc: &CaptionDocument,
    now_ms: f32,
    ctx: &FFramesContext<'a, '_>,
) -> Option<Svgr<'a>> {
    match template {
        "ada" => Some(ada::render(doc, now_ms, ctx)),
        "anya" => Some(anya::render(doc, now_ms, ctx)),
        "cleo" => Some(cleo::render(doc, now_ms, ctx)),
        "elio" => Some(elio::render(doc, now_ms, ctx)),
        "enzo" => Some(enzo::render(doc, now_ms, ctx)),
        "ezra" => Some(ezra::render(doc, now_ms, ctx)),
        "freya" => Some(freya::render(doc, now_ms, ctx)),
        "hugo" => Some(hugo::render(doc, now_ms, ctx)),
        "ivo" => Some(ivo::render(doc, now_ms, ctx)),
        "iris" => Some(iris::render(doc, now_ms, ctx)),
        "juno" => Some(juno::render(doc, now_ms, ctx)),
        "kai" => Some(kai::render(doc, now_ms, ctx)),
        "kel" => Some(kel::render(doc, now_ms, ctx)),
        "lena" => Some(lena::render(doc, now_ms, ctx)),
        "levi" => Some(levi::render(doc, now_ms, ctx)),
        "lewis" => Some(lewis::render(doc, now_ms, ctx)),
        "loki" => Some(loki::render(doc, now_ms, ctx)),
        "luca" => Some(luca::render(doc, now_ms, ctx)),
        "luna" => Some(luna::render(doc, now_ms, ctx)),
        "lyra" => Some(lyra::render(doc, now_ms, ctx)),
        "milo" => Some(milo::render(doc, now_ms, ctx)),
        "mira" => Some(mira::render(doc, now_ms, ctx)),
        "naya" => Some(naya::render(doc, now_ms, ctx)),
        "noor" => Some(noor::render(doc, now_ms, ctx)),
        "nova" => Some(nova::render(doc, now_ms, ctx)),
        "nyx" => Some(nyx::render(doc, now_ms, ctx)),
        "otto" => Some(otto::render(doc, now_ms, ctx)),
        "pastor" => Some(pastor::render(doc, now_ms, ctx)),
        "pepper" => Some(pepper::render(doc, now_ms, ctx)),
        "pico" => Some(pico::render(doc, now_ms, ctx)),
        "remi" => Some(remi::render(doc, now_ms, ctx)),
        "rhea" => Some(rhea::render(doc, now_ms, ctx)),
        "sara" => Some(sara::render(doc, now_ms, ctx)),
        "selene" => Some(selene::render(doc, now_ms, ctx)),
        "sol" => Some(sol::render(doc, now_ms, ctx)),
        "tala" => Some(tala::render(doc, now_ms, ctx)),
        "theo" => Some(theo::render(doc, now_ms, ctx)),
        "tito" => Some(tito::render(doc, now_ms, ctx)),
        "vera" => Some(vera::render(doc, now_ms, ctx)),
        "yuki" => Some(yuki::render(doc, now_ms, ctx)),
        "zara" => Some(zara::render(doc, now_ms, ctx)),
        _ => None,
    }
}
