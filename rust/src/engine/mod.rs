// ts: src/engine/* — one Rust module per TS file (plan §2.2):
// `engine.rs` (the interpreter), `fold.rs` (state = fold(log)), `store.rs`
// (the append-only JSONL log). The log is the source of truth;
// entity / relation / window tables are projections the fold rebuilds (§7).
//
// Ownership model (plan §2.6): entities live in `State.entities` and are
// never removed; role bindings hold entity IDS, never cloned entities; no
// Rc/Arc/RefCell/Cell anywhere in the engine data face.

// The TS layout is src/engine/engine.ts — the module tree mirrors it
// one-to-one (plan §2.2), so engine/engine.rs is intentional inception.
#[allow(clippy::module_inception)]
pub mod engine;
pub mod fold;
