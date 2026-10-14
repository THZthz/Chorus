//! Shared test infrastructure — plan §2.2 `tests/common/`. Mounted from each
//! integration test with `mod common;`. W1 ships `harness.rs`; W3 adds the
//! 扩展 (per plan §5.2.1): `snap.rs` / `settle.rs` / `seed.rs` /
//! `explorer_lib.rs` — the TS test/ lib quartet (TEST-11: they live outside
//! src/ and are the easiest to miss).

pub mod explorer_lib;
pub mod harness;
pub mod seed;
pub mod settle;
pub mod snap;
