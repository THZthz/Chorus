//! chorus — Rust port of the TS text-game engine (docs/rust-port/plan.md).
//!
//! Module tree mirrors `src/` one-to-one (plan §2.2). W0 ships the RNG spine
//! (`core::dice` + the JS-semantics scaffolding in `js`); W1 the core
//! (`core::*`); W2 the content layer (`content::script` — the line-segment
//! script parser + lint — and `content::content`, the compiler, which also
//! hosts the §9.42 deck machinery folded in from the W0 staging module
//! `core::deck_rng`); later waves fill `engine/`, `session/`, `reach/` in.
//!
//! Porting discipline (plan §7): every ported unit carries a
//! `// ts: <file>:<line>` anchor; frozen quirks carry `// QUIRK(<id>):`;
//! error messages are byte-exact (`ChorusError`); iteration order is
//! observable behavior (IndexMap, never std HashMap at order-sensitive
//! points).

pub mod content;
pub mod core;
pub mod engine;
pub mod js;
pub mod reach;
pub mod session;
