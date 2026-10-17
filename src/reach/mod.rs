//! Static card-reachability, split along the report's seams: `census.rs`
//! (site census), `world.rs` (world-static predicates + the
//! Analyzer/WorldFacts host + the risky-freeze model), `fixpoint.rs` (the
//! naive round-robin committability/provenance fixpoint + `site_proven`),
//! `paths.rs` (the mint universe, dead verbs, deck caps and the per-card
//! report), `render.rs` (predicate/proof/chain/reason text + the CLI's
//! stdout shapes), `world_report.rs` (the `--world` global dashboard — the
//! world-static facts re-rendered, zero new analysis), plus the two campaign
//! engines the bins and the test
//! suite share: `fuzz.rs` (seeded differential campaigns) and `scale.rs`
//! (the CS-scale synthetic world; spec §13.3).
//!
//! Discipline: the fixpoint is a NAIVE round-robin re-test in content order
//! (verbs in content.verbs order, then sites in live order) until nothing
//! changes; `count(X) <= k` gates make `path_open` anti-monotone, so proven-set
//! membership is schedule-dependent and any worklist/event-driven rewrite
//! can change output. The three identity-keyed caches collapse into one
//! `Analyzer<'c>` holding (content, initial); memoizing `match_query` is
//! deliberately omitted (`match_query` is cheap). Object identity keys are
//! indices: sites live in an arena `Vec<RawSite>` keyed by `usize`, verbs by
//! their content.verbs insertion index; every insertion-ordered Map/Set is
//! an IndexMap/IndexSet.

pub mod census;
pub mod fixpoint;
pub mod fuzz;
pub mod paths;
pub mod render;
pub mod scale;
pub mod world;
pub mod world_report;

pub use paths::{
    analyze_card, CardCandidate, CardOptions, CardReport, CardVerdict, Existence, PathDeck, PathOp,
    ReachPath, Require,
};
pub use render::{pred_text, render_path, render_report};
pub use world::Analyzer;
pub use world_report::{world_report, WorldReport};
