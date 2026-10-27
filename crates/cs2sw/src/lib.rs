//! `cs2sw`: Cultist Simulator core content -> `.sw`. The converter maps, it
//! never interprets: tolerant JSON loading, an inventory pass, mechanical
//! mapping passes, and a gap report for every construct the language cannot
//! carry.

pub mod convert;
pub mod json;
pub mod model;
pub mod naming;
pub mod report;

pub use convert::{convert, Class, Conversion, GapRow, Gaps, Stats, SEED};
pub use model::{load_tree, CsData};
pub use story_present::catalog::PresentationCatalog;

/// The CS core root from the environment. The env-gated tests run only when
/// `CS2SW_CS_CORE` is set; this machine pins it in `.cargo/config.toml`.
pub fn cs_core_dir() -> Option<std::path::PathBuf> {
    std::env::var("CS2SW_CS_CORE")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
}

/// Load, convert, and compile-check in one call. The compile diagnostics come
/// back untouched: zero errors is the converter's own acceptance bar.
pub fn run(root: &std::path::Path) -> (Conversion, Vec<story_content::Diagnostic>) {
    let data = load_tree(root);
    let conversion = convert(&data);
    let out = story_lang::compile_source("cultist-simulator.sw", &conversion.sw);
    (conversion, out.diagnostics)
}
