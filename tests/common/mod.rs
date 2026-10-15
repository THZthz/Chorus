//! Shared test infrastructure. Mounted from each integration test with
//! `mod common;`:
//!
//! - `harness.rs` — assertion helpers (`throws!` / `eq!` / round-trip / snapshot)
//! - `snap.rs` — full-state snapshot for fold-equality assertions
//! - `settle.rs` — deterministic clock-settle helper for parallel-station tests
//! - `seed.rs` — the seed world + seed content fixtures
//! - `explorer_lib.rs` — shared machinery for the nightjar explorer tests
//! - `reach_fuzz_lib.rs` — seeded differential-reachability campaign generator
//! - `reach_scale_lib.rs` — CS-scale world generator (also #[path]-mounted by
//!   the reach-scale bin for its `--worker` interface)

pub mod explorer_lib;
pub mod harness;
pub mod reach_fuzz_lib;
pub mod reach_scale_lib;
pub mod seed;
pub mod settle;
pub mod snap;
