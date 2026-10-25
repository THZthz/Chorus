//! The buffered report sections: one printer per requested mode (slice,
//! reach, classify, mdp, pareto, audit), each writing into its own string
//! so the fixed section order holds.

use story_analyze::audit::{audit, Evidence, FaultEvent, FaultWitness};
use story_analyze::classify::{classify, ending_set, ClassifyConfig};
use story_analyze::mdp::{MdpConfig, MdpOutcome};
use story_analyze::pareto::{analyze as pareto_analyze, Objective, ParetoConfig};
use story_analyze::query::{GoalPredicate, ReachQuery, SearchObjective};
use story_analyze::search::{Analyzer, ExploreConfig, SolveOutcome};
use story_analyze::slice::{InterfaceBoundary, SliceTarget};
use story_analyze::transitions::{PlayerStep, WitnessPath};
use story_core::Time;
use story_engine::Event;

use super::fmt::{fraction_or_decimal, probability_text};
use super::run::DEFAULT_MAX_STATES;
use super::{AssumptionArg, Options};

/// Buffered-report print: section writers append to a string so the
/// parallel sections can print in their fixed order.
macro_rules! outln {
    ($out:expr $(,)?) => { $out.push('\n') };
    ($out:expr, $($arg:tt)*) => {{
        $out.push_str(&format!($($arg)*));
        $out.push('\n');
    }};
}

/// How many list entries a section prints before summarizing the rest.
const LIST_CAP: usize = 8;

fn explore_config(
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
    extra_invariant: Option<&GoalPredicate>,
) -> ExploreConfig {
    let mut cfg = ExploreConfig {
        rng_mode: opts.rng,
        max_states: opts.max_states.unwrap_or(DEFAULT_MAX_STATES),
        max_time: max_time.or(Some(story_analyze::query::DEFAULT_MAX_TIME)),
        prune,
        allowed_verbs: opts.slice.as_ref().map(|t| t.verbs().clone()),
        time_budget: opts.time_budget,
        ..Default::default()
    };
    if let Some(inv) = extra_invariant {
        cfg.invariant = Some(inv.clone());
    }
    cfg
}

fn truncated_note(truncated: bool, bounded: bool) -> &'static str {
    if truncated {
        "TRUNCATED — findings are bounds-conditional"
    } else if bounded {
        "within bounds"
    } else {
        "space exhausted within bounds"
    }
}

// -- slice ------------------------------------------------------------------

pub(super) fn slice_section(
    out: &mut String,
    target: &SliceTarget,
    boundary: &InterfaceBoundary,
    assumes: &[AssumptionArg],
) {
    outln!(out);
    match target {
        SliceTarget::Verbs(vs) => {
            let names: Vec<&str> = vs.iter().map(|v| v.as_str()).collect();
            if names.len() == 1 {
                outln!(out, "== slice: verb \"{}\" ==", names[0]);
            } else {
                outln!(out, "== slice: arc [{}] ==", names.join(", "));
            }
        }
    }
    outln!(out, "interface facts:");
    outln!(out, "  assumes (inputs from outside):");
    if boundary.inputs.is_empty() {
        outln!(out, "    (none — self-contained on inputs)");
    } else {
        for f in &boundary.inputs {
            outln!(out, "    - {f}");
        }
    }
    outln!(out, "  guarantees (outputs to outside):");
    if boundary.outputs.is_empty() {
        outln!(out, "    (none — no external products)");
    } else {
        for f in &boundary.outputs {
            outln!(out, "    - {f}");
        }
    }
    outln!(out, "  internal:");
    if boundary.internal.is_empty() {
        outln!(out, "    (none)");
    } else {
        for f in &boundary.internal {
            outln!(out, "    - {f}");
        }
    }
    if !assumes.is_empty() {
        outln!(out, "applied assumptions:");
        for a in assumes {
            match a {
                AssumptionArg::Card(id, count) => outln!(out, "  - card:{id}={count}"),
                AssumptionArg::Legacy(key, value) => outln!(out, "  - legacy:{key}={value}"),
                AssumptionArg::Aspect(card, key, value) => {
                    outln!(out, "  - aspect:{card}.{key}={value}")
                }
                AssumptionArg::When(cond) => outln!(out, "  - when {cond}"),
            }
        }
    }
}

pub(super) fn describe_slice(target: &SliceTarget) -> String {
    match target {
        SliceTarget::Verbs(vs) => {
            let names: Vec<&str> = vs.iter().map(|v| v.as_str()).collect();
            if names.len() == 1 {
                format!("verb \"{}\"", names[0])
            } else {
                format!("arc [{}]", names.join(", "))
            }
        }
    }
}

pub(super) fn describe_assumes(assumes: &[AssumptionArg]) -> String {
    assumes
        .iter()
        .map(|a| match a {
            AssumptionArg::Card(name, cnt) => format!("card:{name}={cnt}"),
            AssumptionArg::Legacy(k, v) => format!("legacy:{k}={v}"),
            AssumptionArg::Aspect(c, k, v) => format!("aspect:{c}.{k}={v}"),
            AssumptionArg::When(w) => format!("when {w}"),
        })
        .collect::<Vec<_>>()
        .join(",")
}

// -- reach ------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(super) fn reach_section(
    out: &mut String,
    analyzer: &mut Analyzer,
    desc: &str,
    goal: GoalPredicate,
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
    extra_invariant: Option<&GoalPredicate>,
    continue_traces: &[story_analyze::PlayTrace],
) -> (bool, Vec<story_analyze::PlayTrace>) {
    let _span = tracing::info_span!("reach").entered();
    outln!(out);
    outln!(out, "== reach: {desc} ==");
    let mut query = ReachQuery::new(goal);
    query.rng_mode = opts.rng;
    query.max_time = max_time.or(Some(story_analyze::query::DEFAULT_MAX_TIME));
    query.prune = prune;
    query.time_budget = opts.time_budget;
    query.seed_traces.extend(continue_traces.iter().cloned());
    if let Some(n) = opts.max_states {
        query.max_states = n;
    }
    if let Some(k) = opts.diverse {
        query.objective = SearchObjective::DiverseWays { max_ways: k };
    }
    if let Some(target) = &opts.slice {
        query.allowed_verbs = Some(target.verbs().clone());
    }
    if let Some(inv) = extra_invariant {
        query.invariant = Some(inv.clone());
    }
    if let Some(n) = opts.samples {
        query.seed_samples = n;
    }
    if let Some(path) = &opts.seed_trace {
        match std::fs::read_to_string(path) {
            Ok(content) => match story_analyze::PlayTrace::from_json(&content) {
                Ok(trace) => query.seed_trace = Some(trace),
                Err(e) => {
                    outln!(out, "seed trace `{path}` parse error: {e}");
                }
            },
            Err(e) => {
                outln!(out, "seed trace `{path}` file error: {e}");
            }
        }
    }
    if let Some(k) = opts.radius {
        query.tube_radius = Some(k);
    }
    query.envelope = opts.envelope;
    let outcome = analyzer.solve(&query);
    print_solve(out, &outcome, opts, max_time);
    (outcome.budget_depleted, outcome.resume_traces)
}

fn print_solve(out: &mut String, outcome: &SolveOutcome, opts: &Options, max_time: Option<Time>) {
    let status = if !outcome.witnesses.is_empty() {
        format!("REACHABLE — {} witness(es)", outcome.witnesses.len())
    } else if outcome.budget_depleted {
        "INCONCLUSIVE — time budget depleted; run with --continue to resume".to_string()
    } else if outcome.pruned > 0 {
        format!(
            "NO WITNESS UNDER PRUNING — {pruned} branch(es) looked impossible and were \
             not explored",
            pruned = outcome.pruned
        )
    } else if outcome.truncated {
        "INCONCLUSIVE — the search hit its bounds before exhausting the space".to_string()
    } else {
        "UNREACHABLE — no state within the bounds satisfies the goal".to_string()
    };
    outln!(out, "outcome: {status}");
    outln!(
        out,
        "expansions: {}{}",
        outcome.expansions,
        if outcome.budget_depleted {
            " (stopped at time budget)".to_string()
        } else if outcome.truncated {
            format!(
                " (truncated at max_states {})",
                opts.max_states.unwrap_or(DEFAULT_MAX_STATES)
            )
        } else {
            String::new()
        }
    );
    if outcome.budget_depleted {
        outln!(
            out,
            "bounds: time budget depleted — progress saved; run with --continue to resume"
        );
    } else if outcome.truncated {
        outln!(
            out,
            "bounds: truncated at max_states {}{} — a no-witness answer is bounds-conditional",
            opts.max_states.unwrap_or(DEFAULT_MAX_STATES),
            max_time.map_or_else(String::new, |t| format!(", horizon {t}m"))
        );
    }
    if outcome.pruned > 0 {
        outln!(
            out,
            "pruned: {} — heuristic feasibility; witnesses are unaffected",
            outcome.pruned
        );
    }
    if outcome.witnesses_dropped_by_the_oracle > 0 {
        outln!(
            out,
            "witnesses dropped by the replay oracle: {}",
            outcome.witnesses_dropped_by_the_oracle
        );
    }
    let total = outcome.witnesses.len();
    for (i, witness) in outcome.witnesses.iter().enumerate() {
        outln!(
            out,
            "witness {} of {} — {} action(s), {} transition(s), final clock {}m, {} (replay-verified)",
            i + 1,
            total,
            witness.actions,
            witness.depth,
            witness.final_clock(),
            probability_text(witness.probability),
        );
        print_witness(out, witness);
    }
    if opts.envelope {
        if let Some(env) = &outcome.envelope {
            outln!(out);
            outln!(out, "explored envelope:");
            outln!(
                out,
                "  states: {} evaluated, {} canonical states discovered, time: [{}m, {}m]",
                env.states_evaluated,
                env.unique_canonical_states,
                env.time_span.0,
                env.time_span.1
            );
            outln!(
                out,
                "  mechanics: {} verbs, {} recipes completed ({:.0}%)",
                env.verbs_used.len(),
                env.recipes_completed.len(),
                env.recipe_coverage_ratio * 100.0
            );
            if !env.aspect_ranges.is_empty() {
                let aspects: Vec<String> = env
                    .aspect_ranges
                    .iter()
                    .take(6)
                    .map(|((c, k), (min, max))| format!("{c}.{k}: [{min}, {max}]"))
                    .collect();
                outln!(out, "  aspects: {}", aspects.join(", "));
            }
            if env.total_causal_landmarks > 0 {
                let ratio = env.causal_landmarks_reached as f64
                    / env.total_causal_landmarks as f64
                    * 100.0;
                outln!(
                    out,
                    "  landmarks: {}/{} causal landmarks grounded ({:.0}%)",
                    env.causal_landmarks_reached,
                    env.total_causal_landmarks,
                    ratio
                );
            }
            outln!(
                out,
                "  topology: {} seed trajectory(ies), radius {}, {} frontier edges",
                env.seed_trajectories,
                if env.exhaustive_radius == usize::MAX {
                    "inf".to_string()
                } else {
                    env.exhaustive_radius.to_string()
                },
                env.frontier_boundary_edges
            );
        }
    }
}

/// The numbered step list: verb/recipe/clock per step and the decisive
/// narration line of each step's event stream.
fn print_witness(out: &mut String, witness: &WitnessPath) {
    let mut state = witness.start.clone();
    for (i, record) in witness.records.iter().enumerate() {
        let step = match &record.step {
            PlayerStep::Commit { verb, bindings } => {
                let bound: Vec<String> = bindings
                    .iter()
                    .map(|(slot, entity)| {
                        let name = state
                            .entities
                            .get(entity)
                            .map(|e| e.name.clone())
                            .unwrap_or_else(|| format!("#{}", entity.0));
                        format!("{slot}={name}#{entity}")
                    })
                    .collect();
                let recipe = record
                    .recipe
                    .as_ref()
                    .map(|r| format!(" -> {r}"))
                    .unwrap_or_default();
                format!("commit {verb} [{}]{}", bound.join(", "), recipe)
            }
            PlayerStep::Bind { sit, slot, entity } => {
                let name = state
                    .entities
                    .get(entity)
                    .map(|e| e.name.clone())
                    .unwrap_or_else(|| format!("#{}", entity.0));
                format!("bind sit {sit} {slot} = {name}#{entity}")
            }
            PlayerStep::Advance { dt } => format!("advance {dt}m"),
        };
        outln!(
            out,
            "  {}. [{}m -> {}m] {}",
            i + 1,
            record.time_before,
            record.time_after,
            step
        );
        if let Some(text) = record.events.iter().rev().find_map(|ev| match ev {
            Event::Narration { text } => Some(text.as_str()),
            _ => None,
        }) {
            outln!(out, "       \"{text}\"");
        }
        state = record.state_after.clone();
    }
    if let Some(ended) = &witness.final_state().ended {
        outln!(out, "  final: the story ends \"{ended}\"");
    }
}

// -- classify ---------------------------------------------------------------

pub(super) fn classify_section(
    out: &mut String,
    analyzer: &mut Analyzer,
    goal: Option<&GoalPredicate>,
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
    extra_invariant: Option<&GoalPredicate>,
) {
    let _span = tracing::info_span!("classify").entered();
    outln!(out);
    outln!(out, "== classify ==");
    let mut config = ClassifyConfig {
        explore: explore_config(opts, prune, max_time, extra_invariant),
        winning_endings: goal.and_then(|g| match g {
            GoalPredicate::Ending(id) => Some(ending_set(id.as_ref())),
            _ => None,
        })
        .unwrap_or_default(),
    };
    if prune {
        // Pruning expands toward the borrowed goal, so the closure becomes
        // an under-approximation — stated on the report.
        config.explore.stop_at = goal.cloned();
    }
    let note = if config.winning_endings.is_empty() {
        "none configured — winnable is empty (pass --reach ending:<ID> to set one); the \
         softlock set still measures reachability of any ending"
            .to_string()
    } else {
        config
            .winning_endings
            .iter()
            .map(|e| format!("\"{e}\""))
            .collect::<Vec<String>>()
            .join(", ")
    };
    outln!(out, "winning endings: {note}");
    let c = classify(analyzer, &config);
    let note = if c.graph.budget_depleted {
        "TRUNCATED — time budget depleted"
    } else {
        truncated_note(c.graph.truncated, max_time.is_some())
    };
    outln!(
        out,
        "explored: {} states, {} edges ({})",
        c.graph.nodes.len(),
        c.graph.edges.len(),
        note,
    );
    if prune && goal.is_some() {
        outln!(
            out,
            "bounds: feasibility pruning toward the goal — the closure is an \
             under-approximation"
        );
    }
    outln!(
        out,
        "winnable states: {}{}",
        c.winnable.len(),
        if c.winnable.contains(&c.graph.start) {
            " (the start is winnable)"
        } else {
            " (the start is NOT winnable)"
        }
    );
    let mut ponr = c.point_of_no_return_edges.clone();
    ponr.sort_unstable();
    outln!(out, "points of no return: {} crossing edge(s)", ponr.len());
    print_capped(out, ponr.iter().map(|(u, v)| {
        format!(
            "node {u} -> node {v} (the state leaves winnable play at clock {}m)",
            c.graph.nodes[*v].state.now
        )
    }));
    outln!(out, "softlocked live states: {}", c.softlock.len());
    print_capped(out, c.softlock.iter().map(|v| {
        format!(
            "node {v} (clock {}m, no ending reachable)",
            c.graph.nodes[*v].state.now
        )
    }));
    let mut loops = c.infinite_loops.clone();
    for l in &mut loops {
        l.sort_unstable();
    }
    loops.sort();
    outln!(out, "infinite loops: {}", loops.len());
    print_capped(out, loops.iter().map(|l| {
        let nodes: Vec<String> = l.iter().map(|v| v.to_string()).collect();
        format!("loop over {} state(s): nodes {}", l.len(), nodes.join(", "))
    }));
}

// -- mdp --------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(super) fn mdp_section(
    out: &mut String,
    analyzer: &Analyzer,
    desc: &str,
    goal: GoalPredicate,
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
    extra_invariant: Option<&GoalPredicate>,
) {
    let _span = tracing::info_span!("mdp").entered();
    outln!(out);
    outln!(out, "== mdp: {desc} ==");
    let mut config = MdpConfig::new(goal);
    config.rng_mode = opts.rng;
    config.max_time = max_time;
    config.prune = prune;
    config.time_budget = opts.time_budget;
    // --max-states is the expansion bound everywhere: the game graph's node
    // budget here. max_actions/max_depth keep the crate defaults.
    config.max_nodes = opts.max_states.unwrap_or(DEFAULT_MAX_STATES);
    if let Some(target) = &opts.slice {
        config.allowed_verbs = Some(target.verbs().clone());
    }
    if let Some(inv) = extra_invariant {
        config.invariant = Some(inv.clone());
    }
    let outcome = story_analyze::mdp::solve(analyzer, &config);
    print_mdp(out, &outcome);
}

fn print_mdp(out: &mut String, outcome: &MdpOutcome) {
    outln!(
        out,
        "V_max     = {}  (maximum goal probability under optimal play)",
        fraction_or_decimal(outcome.start.v_max)
    );
    outln!(
        out,
        "V_minimax = {}  (worst-case guarantee over positive-probability branches)",
        fraction_or_decimal(outcome.start.v_minimax)
    );
    outln!(
        out,
        "V_unif    = {}  (uniform-play baseline)",
        fraction_or_decimal(outcome.start.v_unif)
    );
    outln!(
        out,
        "expected actions under the optimal policy: {}",
        fraction_or_decimal(outcome.start.expected_actions)
    );
    outln!(
        out,
        "game graph: {} nodes{}",
        outcome.nodes,
        if outcome.complete {
            " (complete)".to_string()
        } else {
            " (TRUNCATED — cut subtrees score 0; the values are bounds-conditional)".to_string()
        }
    );
}

// -- pareto -----------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(super) fn pareto_section(
    out: &mut String,
    analyzer: &Analyzer,
    goal_desc: &str,
    goal: GoalPredicate,
    objectives: &[(String, Objective)],
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
    extra_invariant: Option<&GoalPredicate>,
) {
    let _span = tracing::info_span!("pareto").entered();
    let obj_desc: Vec<&str> = objectives.iter().map(|(text, _)| text.as_str()).collect();
    let only: Vec<Objective> = objectives.iter().map(|(_, o)| o.clone()).collect();
    outln!(out);
    outln!(
        out,
        "== pareto: goal {goal_desc}, objectives {} ==",
        obj_desc.join(", ")
    );
    let mut config = ParetoConfig::new(goal, only);
    config.explore = explore_config(opts, prune, max_time, extra_invariant);
    let report = pareto_analyze(analyzer, &config);
    if report.frontier.is_empty() {
        outln!(out, "frontier: empty — no explored state satisfies the goal within the bounds");
    } else {
        outln!(out, "frontier: {} point(s)", report.frontier.len());
        for (i, point) in report.frontier.iter().enumerate() {
            let values: Vec<String> = point.values.iter().map(|v| fraction_or_decimal(*v)).collect();
            outln!(
                out,
                "  point {} of {}: [{}] — {} action(s), final clock {}m, {}",
                i + 1,
                report.frontier.len(),
                values.join(", "),
                point.witness.actions,
                point.witness.final_clock(),
                probability_text(point.witness.probability),
            );
            print_witness(out, &point.witness);
        }
    }
    outln!(out, "mandatory recipes: {}", joined_or_dash(report.mandatory_recipes.iter()));
    outln!(out, "mandatory cards: {}", joined_or_dash(report.mandatory_cards.iter()));
    outln!(
        out,
        "minimal cuts: {}",
        cuts_text(&report.minimal_recipe_cuts, &report.verified_cuts)
    );
    if report.truncated {
        outln!(
            out,
            "bounds: parts of the analysis truncated — the frontier stands, cut results may be \
             incomplete"
        );
    }
}

fn cuts_text(cuts: &[std::collections::BTreeSet<story_core::RecipeId>], verified: &[std::collections::BTreeSet<story_core::RecipeId>]) -> String {
    if cuts.is_empty() {
        return "-".to_string();
    }
    let lines: Vec<String> = cuts
        .iter()
        .map(|c| {
            let recipes: Vec<String> = c.iter().map(|r| r.to_string()).collect();
            let mark = if verified.contains(c) {
                " (verified: no alternative path within the bounds)"
            } else {
                ""
            };
            format!("{{{}}}{}", recipes.join(", "), mark)
        })
        .collect();
    lines.join("; ")
}

fn joined_or_dash<T: std::fmt::Display>(items: impl Iterator<Item = T>) -> String {
    let all: Vec<String> = items.map(|i| i.to_string()).collect();
    if all.is_empty() {
        "-".to_string()
    } else {
        all.join(", ")
    }
}

// -- audit ------------------------------------------------------------------

pub(super) fn audit_section(
    out: &mut String,
    analyzer: &mut Analyzer,
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
    extra_invariant: Option<&GoalPredicate>,
) {
    let _span = tracing::info_span!("audit").entered();
    outln!(out);
    outln!(out, "== audit ==");
    let report = audit(analyzer, &explore_config(opts, prune, max_time, extra_invariant));
    outln!(
        out,
        "explored: {} states ({})",
        report.explored_nodes,
        truncated_note(report.truncated, max_time.is_some())
    );
    let mut shadowing = report.shadowing.clone();
    shadowing.sort_by(|a, b| {
        (&a.verb, &a.shadower, &a.shadowed).cmp(&(&b.verb, &b.shadower, &b.shadowed))
    });
    outln!(out, "shadowing: {} finding(s)", shadowing.len());
    print_capped(
        out,
        shadowing.iter().map(|s| {
            let verb = s.verb.as_ref().map_or_else(String::new, |v| format!("verb {v}: "));
            let cond = if s.conditional {
                " (conditional — the shadower is spent after firing)"
            } else {
                ""
            };
            format!("{verb}\"{}\" shadows \"{}\"{cond}", s.shadower, s.shadowed)
        }),
    );
    let mut unreachable = report.unreachable.clone();
    unreachable.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    outln!(out, "unreachable content: {} finding(s)", unreachable.len());
    print_capped(
        out,
        unreachable.iter().map(|u| {
            let evidence = match u.evidence {
                Evidence::StaticallyProved => "statically proved",
                Evidence::NotWithinBounds => "not within the exploration bounds (bounds-conditional)",
            };
            format!("{} {} — {evidence}", u.kind, u.id)
        }),
    );
    let mut faults: Vec<&FaultWitness> = report.faults.iter().collect();
    faults.sort_by_key(|f| fault_key(f));
    outln!(out, "reachable faults: {} finding(s)", faults.len());
    for (i, f) in faults.iter().enumerate() {
        if i >= LIST_CAP {
            outln!(out, "  ... and {} more", faults.len() - LIST_CAP);
            break;
        }
        let event = match &f.event {
            FaultEvent::ResolutionFailed { reason } => format!("resolution failed ({reason})"),
            FaultEvent::StartSkipped { reason } => format!("start skipped ({reason})"),
        };
        outln!(
            out,
            "  fault {} of {}: {event} — witness {} step(s), final clock {}m",
            i + 1,
            faults.len(),
            f.witness.records.len(),
            f.witness.final_clock()
        );
        print_witness(out, &f.witness);
    }
}

fn fault_key(f: &FaultWitness) -> (u8, String, usize, Time) {
    let kind = match &f.event {
        FaultEvent::ResolutionFailed { reason } => (0u8, reason.clone()),
        FaultEvent::StartSkipped { reason } => (1u8, reason.clone()),
    };
    (
        kind.0,
        kind.1,
        f.witness.records.len(),
        f.witness.final_clock(),
    )
}

fn print_capped(out: &mut String, lines: impl Iterator<Item = String>) {
    let all: Vec<String> = lines.collect();
    for line in all.iter().take(LIST_CAP) {
        outln!(out, "  {line}");
    }
    if all.len() > LIST_CAP {
        outln!(out, "  ... and {} more", all.len() - LIST_CAP);
    }
}
