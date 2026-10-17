// The engine layout: `exec.rs` (the interpreter), `validate.rs` (slots,
// recipes, refs, locks), `outcomes/` (§4.4 application: dispatch, mint
// rules, the RUN chain), `advance.rs` (the time pass and window
// completions), `replay.rs` (the fold half), `record.rs` (the log row and
// its builders), `store.rs` (the append-only JSONL log), `fold.rs`
// (state = fold(log)), `settle.rs` (the run-then-advance convenience the
// tests and the fuzz campaigns share). The log is the source of truth;
// entity / relation / window tables are projections the fold rebuilds
// (spec §7).
//
// Ownership model: entities live in `State.entities` and are never
// removed; role bindings hold entity IDS, never cloned entities; no
// Rc/Arc/RefCell/Cell anywhere in the engine data face.
//
// The five laws, as the crate's comments cite them:
// - law 1 — byte identity: save → load → save reproduces the log's bytes;
//   the serde field order on `ActionRecord` and its payload types is
//   pinned (see `store.rs`).
// - law 2 — canonical sentences round-trip: every engine-minted row's
//   sentence re-parses losslessly, format ∘ parse (see `core::parse`).
// - law 3 — the fold never re-derives: replay consumes journal data (the
//   ROLLED total, the DRAWN name), never a die or a permutation step
//   (see `core::dice`).
// - law 4 — draft discipline: speculative effects land on a draft that a
//   REFUSE or failure discards whole; nothing partial survives.
// - law 5 — one randomness source: the seeded counter RNG is the engine's
//   only randomness, and deck permutations are pure in seed+id, consuming
//   no counter (see `core::dice`).

mod advance;
mod exec;
mod outcomes;
mod record;
mod replay;
#[cfg(test)]
mod tests;
mod validate;

pub mod fold;
pub mod settle;
pub mod store;

pub use exec::execute_step;
pub use record::{ActionRecord, ExecOptions, ExecResult, Step};
pub use replay::apply_record;
