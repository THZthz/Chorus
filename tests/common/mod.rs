//! Shared test infrastructure. Mounted ONCE per integration-test binary from
//! its `main.rs` with `#[path = "../common/mod.rs"] mod common;` — test files
//! reach it as `crate::crate::common::…`:
//!
//! - `harness.rs` — assertion helpers (`assert_err_msg` / `assert_json_eq` /
//!   `canonical` / `assert_rejected` / `assert_refusal`)
//! - `rig.rs` — the shared run adapter (`settle_line`), the script-compile
//!   wrapper, the temp-log helper, the record-projection helpers, the one-verb
//!   INSPECT rig and the session auto-settle adapter
//! - `snap.rs` — full-state snapshot for fold-equality assertions
//! - `seed.rs` — the seed world + seed content fixtures
//! - `explorer_lib.rs` — shared machinery for the nightjar explorer tests
//!
//! The engine/reach-side helpers live in the library itself (they are
//! product surface for the reach-fuzz / reach-scale bins): the settle adapter
//! is `chorus::engine::settle`, the fuzz campaigns `chorus::reach::fuzz`, and
//! the CS-scale generator `chorus::reach::scale`.

pub mod explorer_lib;
pub mod harness;
pub mod rig;
pub mod seed;
pub mod snap;
