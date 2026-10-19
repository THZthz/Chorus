//! The `--cost` face: the cost/count reading of the settled facts — "how
//! many verb applications, how many clock minutes, how many simultaneous
//! copies". ZERO new analysis over the post-fixpoint facts
//! ([`Analyzer::facts`]): the module only walks the PROVEN provenance DAG
//! (`mint_sources` / `fillers` — the very edges `path_for` renders) under
//! two weight functions, and re-renders `avail`'s counts. The model is a
//! LOWER-BOUND planning estimate, and every report says so:
//!
//! - **applications** — the shortest path over the provenance DAG where a
//!   recipe-owned proven site costs 1 (one verb application, its fillers
//!   assumed in hand) and a trigger-owned proven site costs 0 (the clock
//!   fires slot triggers; expiry/complete triggers are ARRANGED, not
//!   applied — their required fillers still recur, an expiry's named card
//!   being the one the path model carries). Initial entities cost 0.
//!   Memoized over names, depth-backstopped like `path_for` (12), tie-break
//!   stable (the first proven source in proof order wins — the chain's own
//!   walk order). The number is a lower bound: it assumes each needed
//!   recipe applies once with its fillers in hand; gate timing and the
//!   anti-monotone `count(X) <= k` gates are honored only through
//!   provenness (a site the gates would kill is not in `mint_sources`).
//! - **minutes** — the same DAG under the clock weights: a recipe hop costs
//!   its window's length (the recipe's own `dur`, else the verb's duration —
//!   the window completes), a slot-trigger hop its
//!   first crossing (`WorldFacts::first_cross`, re-anchored under the
//!   bounded-time faces), an expiry/complete hop 0 (arranged). The
//!   clock-cheapest plan may differ from the application-cheapest one —
//!   the number is reported on its own. A frozen world's note says the
//!   freeze boundary caps the clock.
//! - **copies** — the max simultaneous copies of the target the settled
//!   world supports, from `facts.avail`'s counts (initial multiplicities,
//!   deck-order multiplicities within the serving predicate's window):
//!   `MANY` renders `unbounded`; a fact whose count exceeds the
//!   [`FLATTEN_WEIGHT_CAP`] renders `{capped sum}+ (world cap)` — the
//!   world's own bookkeeping (`cards_of`'s `min(count, 32)`) cannot count
//!   past the cap, so the figure is a floor; otherwise the capped sum.
//!
//! Determinism is the contract: both walks iterate `proven_sources` in
//! proof order (the fixpoint's insertion order) and keep the strictly
//! better plan, so ties resolve to the FIRST proven source — the same
//! first-fit the chain reports. Two runs over the same (compiled content,
//! initial state) render byte-identical text. The face composes with
//! `--usable`/`--why`/`--all` and the bounded-time faces (the horizon
//! bounds the DAG — only sites proven inside the window participate; a
//! `--by` note says so; `--from-state` anchors to the replayed world for
//! free — same facts). It stays out of `--json` and is ignored under
//! `--demand` (the demand match's provenance is not costed) — documented,
//! not rejected.

use std::fmt::Write as _;

use indexmap::{IndexMap, IndexSet};

use crate::content::CompiledTrigger;

use super::census::{RawSite, MANY};
use super::world::{Analyzer, WorldFacts, FLATTEN_WEIGHT_CAP};

/// The witness/DAG depth backstop — `path_for`'s.
const MAX_COST_DEPTH: usize = 12;

// ---------------------------------------------------------------------------
// the report shape
// ---------------------------------------------------------------------------

/// The cheapest plan over the provenance DAG (both metrics resolved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostPlan {
    /// minimum verb applications (recipe hops weight 1, trigger hops 0,
    /// initial 0)
    pub apps: i64,
    /// the cheapest witness chain, one line: hops from the initial anchors
    /// to the target, joined by `→` (post-order over the witness tree — a
    /// shared anchor repeats where two branches need it). Each hop reuses
    /// the chain/via vocabulary: `initial entity X (kind)` for anchors,
    /// `{owner} mints {name}` for proven sites.
    pub witness: String,
    /// minimum clock minutes under the clock weights (recipe hops = the
    /// window length — the recipe's own `dur`, else the verb's duration;
    /// slot-trigger hops = the first crossing, arranged hops 0)
    pub minutes: i64,
}

/// The copies bound: the headline plus the per-fact detail (the fact `how`
/// strings are the settled world's own provenance lines).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopiesInfo {
    /// `unbounded`, `{n}+ (world cap)`, or the capped sum
    pub text: String,
    /// `×{count} {how}` rows, fact push order; `None` when unbounded
    pub detail: Option<String>,
}

/// The `--cost` section: the plan (None = no proven provenance DAG — the
/// target is unproven/candidate and the candidate sources sit outside
/// costing), the copies bound, and the honesty notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CostReport {
    pub plan: Option<CostPlan>,
    pub copies: Option<CopiesInfo>,
    pub notes: Vec<String>,
}

// ---------------------------------------------------------------------------
// the memoized DAG walk
// ---------------------------------------------------------------------------

/// Which weight function the walk optimizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    /// recipe hops 1, trigger hops 0, initial 0
    Apps,
    /// recipe hops = the window length (the recipe's own `dur`, else the
    /// verb's duration), slot hops = the first crossing, arranged hops 0,
    /// initial 0
    Minutes,
}

/// The chosen source of one name on a cheapest plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Src {
    Initial,
    Site(usize),
}

/// One settled cheapest-plan entry: the cost under the walk's metric and
/// the source that achieved it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Plan {
    cost: i64,
    src: Src,
}

/// The hop's own cost (before the required fillers recur).
fn hop_cost(facts: &WorldFacts<'_>, metric: Metric, site: &RawSite<'_>) -> i64 {
    match metric {
        Metric::Apps => match site.verb {
            Some(_) => 1,
            None => 0,
        },
        // §6.4: the recipe's own `dur` overrides its verb's `duration` —
        // the site knows its recipe exactly (the census keys it)
        Metric::Minutes => match site.verb {
            Some(vi) => facts
                .content
                .verbs
                .get_index(vi)
                .map(|(_, v)| super::world::effective_duration(v, site.recipe))
                .unwrap_or(0),
            None => match facts.trig_idx.get(&site.owner) {
                Some((CompiledTrigger::Slot { slot, .. }, _)) => facts.first_cross(*slot),
                // expiry/complete: arranged, not timed
                _ => 0,
            },
        },
    }
}

/// The required fillers whose costs recur — the SAME split `path_for`
/// renders: a recipe site needs its committability proof's filler names
/// (the chosen proofs' union, `facts.fillers` order), an expiry trigger its
/// named card; slot/complete triggers carry none.
fn site_reqs(facts: &WorldFacts<'_>, site: &RawSite<'_>) -> Vec<String> {
    match site.verb {
        Some(vi) => facts.fillers.get(&vi).map(|s| s.iter().cloned().collect()).unwrap_or_default(),
        None => match facts.trig_idx.get(&site.owner) {
            Some((CompiledTrigger::Expire { on_expire_of, .. }, _)) => {
                vec![on_expire_of.clone()]
            }
            _ => Vec::new(),
        },
    }
}

/// One memoized shortest-path walk over the proven provenance DAG.
/// The memo is per walk (per metric); `stack` carries the in-progress names
/// so a cycle (which the strict proof order makes impossible — a proven
/// recipe's fillers are proven strictly earlier) degrades to an unusable
/// branch instead of diverging.
struct Walker<'a, 'c> {
    facts: &'a WorldFacts<'c>,
    metric: Metric,
    memo: IndexMap<String, Option<Plan>>,
}

impl<'a, 'c> Walker<'a, 'c> {
    fn new(facts: &'a WorldFacts<'c>, metric: Metric) -> Self {
        Walker { facts, metric, memo: IndexMap::new() }
    }

    /// The cheapest plan for `name`: 0 for an initial anchor (it wins every
    /// tie — both metrics are non-negative), else the strictly-better minimum
    /// over the proven sources in proof order. `None` = no proven source
    /// resolves (memoized: the settled facts cannot change under the walk).
    fn walk(&mut self, name: &str, depth: usize, stack: &mut IndexSet<String>) -> Option<Plan> {
        if let Some(p) = self.memo.get(name) {
            return p.clone();
        }
        if stack.contains(name) || depth > MAX_COST_DEPTH {
            // cycle / backstop: this branch is unusable, left UNmemoized
            // (the answer is path-dependent until the outer frames settle)
            return None;
        }
        if self.facts.init_names.contains_key(name) {
            // the anchor's cost is 0 under both metrics
            let plan = Some(Plan { cost: 0, src: Src::Initial });
            self.memo.insert(name.to_string(), plan.clone());
            return plan;
        }
        stack.insert(name.to_string());
        let mut best: Option<Plan> = None;
        for (si, _sup) in self.facts.proven_sources(name) {
            let site = &self.facts.census.sites[si];
            let mut total = hop_cost(self.facts, self.metric, site);
            let mut resolves = true;
            for r in site_reqs(self.facts, site) {
                match self.walk(&r, depth + 1, stack) {
                    Some(p) => total += p.cost,
                    None => {
                        resolves = false;
                        break;
                    }
                }
            }
            if resolves {
                let plan = Plan { cost: total, src: Src::Site(si) };
                // strictly better only: the FIRST proven source wins ties
                // (proof order — the chain's own first-fit)
                if best.as_ref().is_none_or(|b| plan.cost < b.cost) {
                    best = Some(plan);
                }
            }
        }
        stack.swap_remove(name);
        self.memo.insert(name.to_string(), best.clone());
        best
    }
}

/// Follow the settled memo from `name`, collecting each name's chosen
/// source (the cheapest witness tree — shared names appear once with their
/// one chosen source).
fn collect_choices(
    facts: &WorldFacts<'_>,
    name: &str,
    memo: &IndexMap<String, Option<Plan>>,
    choices: &mut IndexMap<String, Src>,
) {
    if let Some(Some(p)) = memo.get(name) {
        choices.insert(name.to_string(), p.src);
        if let Src::Site(si) = p.src {
            let site = &facts.census.sites[si];
            for r in site_reqs(facts, site) {
                collect_choices(facts, &r, memo, choices);
            }
        }
    }
}

/// Flatten the witness tree into the one-line chain: post-order (an anchor
/// or a filler renders before the site that needs it), hops joined by
/// ` → `. A name two branches share renders once per need — the honest
/// shape of "each needed recipe applies once with fillers in hand".
fn witness_line(
    facts: &WorldFacts<'_>,
    name: &str,
    choices: &IndexMap<String, Src>,
    hops: &mut Vec<String>,
    arranged: &mut IndexSet<String>,
) {
    let Some(&src) = choices.get(name) else {
        return;
    };
    match src {
        Src::Initial => {
            let kind = facts.init_names.get(name).map(|e| e.kind.clone()).unwrap_or_default();
            hops.push(format!("initial entity {name} ({kind})"));
        }
        Src::Site(si) => {
            let site = &facts.census.sites[si];
            for r in site_reqs(facts, site) {
                witness_line(facts, &r, choices, hops, arranged);
            }
            hops.push(format!("{} mints {name}", site.owner));
            // the arranged hops ride the witness's honesty notes
            if site.verb.is_none() {
                match facts.trig_idx.get(&site.owner) {
                    Some((CompiledTrigger::Expire { .. }, _)) => {
                        arranged.insert(format!(
                            "{}'s firing is arranged (its required card's expiry) — costed 0; the required card still recurs",
                            site.owner
                        ));
                    }
                    Some((CompiledTrigger::Complete { .. }, _)) => {
                        arranged.insert(format!(
                            "{}'s firing is arranged (a window completion) — costed 0",
                            site.owner
                        ));
                    }
                    _ => {}
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// the copies bound
// ---------------------------------------------------------------------------

/// The copies face over `facts.avail[target]` — the settled world's own
/// multiplicities (initial counts, deck windows the serving predicate
/// registered). `MANY` is unbounded; a fact past the flatten weight cap
/// makes the figure a floor (`{sum}+ (world cap)` — `cards_of`'s
/// `min(count, 32)` bookkeeping cannot count past the cap); otherwise the
/// capped sum. No facts (an unproven/candidate target) — no copies line.
fn copies_of(facts: &WorldFacts<'_>, target: &str) -> Option<CopiesInfo> {
    let list = facts.avail.get(target)?;
    if list.is_empty() {
        return None;
    }
    if list.iter().any(|f| f.count >= MANY) {
        return Some(CopiesInfo { text: "unbounded".to_string(), detail: None });
    }
    let total: i64 = list.iter().map(|f| std::cmp::min(f.count, FLATTEN_WEIGHT_CAP)).sum();
    let saturated = list.iter().any(|f| f.count > FLATTEN_WEIGHT_CAP);
    let text = if saturated { format!("{total}+ (world cap)") } else { total.to_string() };
    let rows: Vec<String> = list.iter().map(|f| format!("×{} {}", f.count, f.how)).collect();
    Some(CopiesInfo { text, detail: Some(rows.join("; ")) })
}

// ---------------------------------------------------------------------------
// the face
// ---------------------------------------------------------------------------

/// The `--cost` report for one target over the analyzer's settled facts —
/// zero new analysis, the same discipline as `--world`/`--why`. The two
/// metric walks share the facts and nothing else; the honesty notes are
/// assembled in one place so the renderer stays a printer.
#[must_use]
pub fn cost_report(analyzer: &Analyzer<'_>, target: &str) -> CostReport {
    let facts = analyzer.facts();

    let mut apps_w = Walker::new(facts, Metric::Apps);
    let mut stack: IndexSet<String> = IndexSet::new();
    let plan = apps_w.walk(target, 0, &mut stack);
    // the clock walk is independent: the clock-cheapest plan may route
    // differently (a slower recipe behind a faster trigger)
    let mut min_w = Walker::new(facts, Metric::Minutes);
    let mut stack_min: IndexSet<String> = IndexSet::new();
    let minutes = min_w.walk(target, 0, &mut stack_min).map(|p| p.cost);

    let mut notes: Vec<String> = Vec::new();
    let plan = match (plan, minutes) {
        (Some(p), Some(min)) => {
            // the model's honesty, always stated on a costed face
            notes.push(
                "lower-bound planning estimate — each needed recipe applies once with its fillers in hand; gate timing is honored only through provenness"
                    .to_string(),
            );
            let mut choices: IndexMap<String, Src> = IndexMap::new();
            collect_choices(facts, target, &apps_w.memo, &mut choices);
            let mut hops: Vec<String> = Vec::new();
            let mut arranged: IndexSet<String> = IndexSet::new();
            witness_line(facts, target, &choices, &mut hops, &mut arranged);
            for a in &arranged {
                notes.push(a.clone());
            }
            if facts.frozen {
                notes.push(format!(
                    "the world freezes {} min out — the clock estimate cannot see past the boundary",
                    facts.freeze_dist
                ));
            }
            if let Some(m) = facts.by {
                notes.push(format!(
                    "the by {m} window bounds the DAG — a source proven only beyond it is excluded"
                ));
            }
            Some(CostPlan { apps: p.cost, witness: hops.join(" → "), minutes: min })
        }
        // no proven DAG — the candidate sources sit outside costing; no
        // partial figures (the copies line would read 0 the same way)
        _ => None,
    };
    let copies = if plan.is_some() { copies_of(facts, target) } else { None };

    CostReport { plan, copies, notes }
}

/// The `--cost` section appended after a card report (the `--why`
/// precedent): one header line, the three answers when a proven DAG
/// exists, the one-line exclusion face when it does not, then the honesty
/// notes. Without `--cost` nothing here renders (the frozen output is
/// byte-identical).
pub fn render_cost(out: &mut String, file: &str, card: &str, r: &CostReport) {
    let _ = writeln!(out, "{file} · card {card} — cost");
    match &r.plan {
        Some(p) => {
            let _ = writeln!(out, "  applications: {} (lower-bound planning estimate)", p.apps);
            let _ = writeln!(out, "  witness: {}", p.witness);
            let _ = writeln!(out, "  minutes: {} (planning estimate)", p.minutes);
        }
        None => {
            let _ = writeln!(
                out,
                "  (no proven provenance DAG for '{card}' — the candidate sources above are excluded from costing)"
            );
        }
    }
    if let Some(c) = &r.copies {
        match &c.detail {
            Some(d) => {
                let _ = writeln!(out, "  copies: {} — {d}", c.text);
            }
            None => {
                let _ = writeln!(out, "  copies: {}", c.text);
            }
        }
    }
    for n in &r.notes {
        let _ = writeln!(out, "  note: {n}");
    }
}
