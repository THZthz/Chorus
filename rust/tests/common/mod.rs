//! Shared test infrastructure — plan §2.2 `tests/common/`. Mounted from each
//! integration test with `mod common;`. W1 ships `harness.rs` only; the W3
//! 扩展 (per plan §5.2.1) lands `snap.rs` / `settle.rs` / `seed.rs` /
//! `explorer_lib.rs` beside it as siblings of this module file.

pub mod harness;
