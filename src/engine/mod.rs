// The engine layout: `engine.rs` (the interpreter), `fold.rs`
// (state = fold(log)), `store.rs` (the append-only JSONL log). The log is
// the source of truth; entity / relation / window tables are projections
// the fold rebuilds (spec §7).
//
// Ownership model: entities live in `State.entities` and are never
// removed; role bindings hold entity IDS, never cloned entities; no
// Rc/Arc/RefCell/Cell anywhere in the engine data face.

// `mod engine` holding `engine.rs` is deliberate module inception.
#[allow(clippy::module_inception)]
pub mod engine;
pub mod fold;
pub mod store;
