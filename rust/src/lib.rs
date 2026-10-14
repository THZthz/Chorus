//! chorus — Rust port of the TS text-game engine (docs/rust-port/plan.md).
//!
//! Module tree mirrors `src/` one-to-one (plan §2.2). W0 ships the RNG spine
//! (`core::dice`, `core::deck_rng`) and the JS-semantics scaffolding (`js`);
//! later waves fill `content/`, `engine/`, `session/`, `reach/` in.
//!
//! Porting discipline (plan §7): every ported unit carries a
//! `// ts: <file>:<line>` anchor; frozen quirks carry `// QUIRK(<id>):`;
//! error messages are byte-exact (`ChorusError`); iteration order is
//! observable behavior (IndexMap, never std HashMap at order-sensitive
//! points).

pub mod core;
pub mod js;
