//! ts: src/content/* — one Rust module per TS file (plan §2.2). `script.rs`
//! is the line-segment script parser + load-time lint; `content.rs` is the
//! compiler (and hosts the §9.42 deck machinery folded in from W0's
//! `core/deck_rng.rs` staging module).

// `content/content.rs` mirrors the TS tree 1:1 (plan §2.2) — the clippy
// "inception" shape is the mapping's cost, not an accident.
#![allow(clippy::module_inception)]

pub mod content;
pub mod script;
