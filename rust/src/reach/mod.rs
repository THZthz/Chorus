//! Static card-reachability — ts: src/reach/reach.ts (1968 lines), split
//! along the report's seams (plan §2.2 / REACH-01): `census.rs` (site census,
//! L137-454), `world.rs` (world-static predicates + the Analyzer/WorldFacts
//! host + the risky-freeze model, L456-978), `fixpoint.rs` (the naive
//! round-robin committability/provenance fixpoint + siteProven, L980-1078 +
//! L1594-1911), `paths.rs` (the mint universe, dead verbs, deck caps and the
//! per-card report, L1080-1505 + helpers), `render.rs` (predicate/proof
//! text + the CLI's stdout shapes, L531-575 + L1917-1968).
//!
//! Porting discipline: REACH-10 (标红) — the fixpoint is a NAIVE round-robin
//! re-test in TS iteration order (verbs in content.verbs order, then sites in
//! live order) until nothing changes; `count(X) <= k` gates make pathOpen
//! anti-monotone, so proven-set membership is schedule-dependent and any
//! worklist/event-driven rewrite can change output. REACH-07 — the three
//! identity-keyed WeakMap caches collapse into one `Analyzer<'c>` holding
//! (content, initial); matchMemo is deliberately NOT ported (matchQuery is
//! cheap). §2.6.5 — object identity keys are indices: sites live in an arena
//! `Vec<RawSite>` keyed by `usize`, verbs by their content.verbs insertion
//! index; every insertion-ordered TS Map/Set is an IndexMap/IndexSet
//! (REACH-03).

pub mod census;
pub mod fixpoint;
pub mod paths;
pub mod render;
pub mod world;

pub use paths::{
    analyze_card, CardCandidate, CardOptions, CardReport, CardVerdict, Existence, PathDeck,
    PathOp, ReachPath, Require,
};
pub use render::{pred_text, render_path, render_report};
pub use world::Analyzer;
