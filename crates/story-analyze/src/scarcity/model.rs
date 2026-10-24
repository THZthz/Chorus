//! The data models of the scarcity bound: bounds, goal clauses, and gain and drop route models.

use story_content::ir::{AspectKey, CmpOp, LegacyKey};
use story_core::{CardId, RecipeId, VerbId};

/// One necessary bound on a unique card's aspect: either `card.key >= n`
/// (`Low`) or `card.key <= n` (`High`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Bound {
    Low {
        card: CardId,
        key: AspectKey,
        n: i64,
    },
    High {
        card: CardId,
        key: AspectKey,
        n: i64,
    },
}

/// One conjunctive clause of a goal. A goal is a disjunction of clauses:
/// the state is dead when every clause in `clauses` is dead.
#[derive(Clone, Debug, Default)]
pub(super) struct Clause {
    pub(super) recipe: Option<RecipeId>,
    pub(super) min_open_clock: i64,
    pub(super) open_bounds: Vec<Bound>,
    pub(super) complete_bounds: Vec<Bound>,
    pub(super) legacy_gates: Vec<(LegacyKey, CmpOp, i64)>,
}

/// Execution-count constraints shared by gain and drop recipe routes.
#[derive(Clone, Debug)]
pub(super) struct RouteCap {
    pub(super) recipe: RecipeId,
    pub(super) verb: VerbId,
    pub(super) verb_instances: Option<u32>,
    pub(super) duration: i64,
    pub(super) max_runs: Option<u32>,
    pub(super) static_legacy_low: Vec<(LegacyKey, i64)>,
    pub(super) consumed: Vec<(AspectKey, i64)>,
    pub(super) carrier_moved: Vec<(CardId, AspectKey, i64)>,
    pub(super) reservoirs: Vec<(CardId, AspectKey, i64)>,
    pub(super) required_live: Vec<CardId>,
}

/// One additive term of a recipe's gain per execution.
#[derive(Clone, Debug)]
pub(super) enum GainTerm {
    Literal(i64),
    Stock {
        key: AspectKey,
        per_run_cap: Option<i64>,
        exclude_card: Option<CardId>,
        zone_only: bool,
    },
}

#[derive(Clone, Debug)]
pub(super) struct GainRoute {
    pub(super) cap: RouteCap,
    pub(super) terms: Vec<GainTerm>,
}

/// An autonomous verb that periodically drains `(card, key)` whenever
/// `card.key >= 1`.
#[derive(Clone, Debug)]
pub(super) struct AutonomousDrain {
    pub(super) verb: VerbId,
    pub(super) drain_recipe: RecipeId,
    pub(super) cycle_duration: i64,
    pub(super) sub: i64,
}

#[derive(Clone, Debug)]
pub(super) struct GainModel {
    pub(super) unbounded: bool,
    pub(super) routes: Vec<GainRoute>,
    pub(super) drains: Vec<AutonomousDrain>,
}

#[derive(Clone, Debug)]
pub(super) struct DropRoute {
    pub(super) cap: RouteCap,
    pub(super) drop_per_run: i64,
}

#[derive(Clone, Debug)]
pub(super) struct DropModel {
    pub(super) unbounded: bool,
    pub(super) routes: Vec<DropRoute>,
}
