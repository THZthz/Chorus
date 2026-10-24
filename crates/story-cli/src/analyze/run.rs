//! Running the requested analyses: one compile and boot, then the section
//! jobs fan out onto bounded parallel threads and print in fixed order.

use std::path::Path;
use std::sync::Arc;

use story_analyze::lang::parse_cond;
use story_analyze::pareto::{Direction, Objective};
use story_analyze::query::GoalPredicate;
use story_analyze::search::Analyzer;
use story_analyze::slice::{apply_assumption, slice_content, Assumption, InterfaceBoundary};
use story_content::ir::{QueryTerm, Scope, SeedSpec};
use story_content::Content;
use story_core::{CardId, EndingId, MintPolicy, State, Time};
use story_engine::EffectRegistry;

use super::cli::parse_duration;
use super::fmt::rng_name;
use super::sections::{
    audit_section, classify_section, describe_assumes, describe_slice, mdp_section,
    pareto_section, reach_section, slice_section,
};
use super::{AssumptionArg, Failure, GoalArg, ObjArg, Options};

// ---------------------------------------------------------------------------
// The analyses
// ---------------------------------------------------------------------------

pub(super) fn run_analyses(opts: &Options) -> Result<(), Failure> {
    // Compile (the same face the play driver runs; diagnostics are traffic).
    let out = {
        let _span = tracing::info_span!("compile").entered();
        story_lang::compile_file(Path::new(&opts.path))
    };
    let mut errors = 0;
    for d in &out.diagnostics {
        println!("{d}");
        if d.is_error() {
            errors += 1;
        }
    }
    let Some(mut content) = out.content else {
        return Err(Failure::Run(format!(
            "compilation failed with {errors} error(s)"
        )));
    };
    if errors > 0 {
        return Err(Failure::Run(format!(
            "compilation reported {errors} error(s)"
        )));
    }
    validate_endings(opts, &content)?;
    if let Some(target) = &opts.slice {
        for verb in target.verbs() {
            if !content.verbs.contains_key(verb.as_str()) {
                return Err(Failure::Usage(format!("unknown verb `{verb}` in --slice")));
            }
        }
    }
    for a in &opts.assumes {
        match a {
            AssumptionArg::Card(name, _) => {
                if !content.cards.contains_key(&CardId::from(name.as_str())) {
                    return Err(Failure::Usage(format!("unknown card `{name}` in --assume")));
                }
            }
            AssumptionArg::Legacy(key, _) => {
                if !content.legacies.contains_key(key) {
                    return Err(Failure::Usage(format!("unknown legacy `{key}` in --assume")));
                }
            }
            AssumptionArg::Aspect(card, key, _) => {
                if !content.cards.contains_key(&CardId::from(card.as_str())) {
                    return Err(Failure::Usage(format!("unknown card `{card}` in --assume")));
                }
                if !content.aspects.contains_key(key) {
                    return Err(Failure::Usage(format!("unknown aspect `{key}` in --assume")));
                }
            }
            AssumptionArg::When(_) => {}
        }
    }
    if let (Some(turn), Some(game)) = (opts.turn, content.game.as_mut()) {
        game.turn = turn;
    }
    let content = Arc::new(content);
    // A content-fixed seed overrides the CLI default; `seed host` content
    // takes the argument (the play driver's law).
    let seed = match content.game.as_ref().map(|g| g.seed.clone()) {
        Some(SeedSpec::Fixed(n)) => n,
        _ => opts.seed,
    };
    // The sequential boot is the diagnostic face: it reports boot traffic
    // once, gates the run on error diagnostics, and answers whether
    // heuristic pruning is available (host ops make the causal graph
    // incomplete). The section threads boot privately and silently below.
    let (boot_analyzer, diags) = {
        let _span = tracing::info_span!("boot").entered();
        Analyzer::boot(content.clone(), EffectRegistry::new(), seed, None)
    };
    for d in &diags {
        println!("boot: {d}");
    }
    if diags.iter().any(|d| d.is_error()) {
        return Err(Failure::Run("boot failed with error diagnostic(s)".to_string()));
    }
    let prune = opts.prune && !boot_analyzer.host_opaque();
    // The content may declare the analysis horizon itself (`meta horizon
    // "36h"` on the game header); the flag overrides it.
    let content_horizon = content
        .game
        .as_ref()
        .and_then(|g| g.meta.get("horizon"))
        .and_then(|v| {
            let h = parse_duration(v);
            if h.is_none() && !v.trim().is_empty() {
                println!("story-cli analyze: ignoring game meta `horizon` — {v:?} is not a duration spelling (<n>m, <n>h, <n>h<m>m)");
            }
            h
        });
    let max_time = opts.max_time.or(content_horizon);
    if let Some(game) = content.game.as_ref() {
        println!("{} — seed {}, turn {}m", game.title, seed, game.turn);
    }
    let mut settings = Vec::new();
    if let Some(target) = &opts.slice {
        settings.push(format!("slice {}", describe_slice(target)));
    }
    if !opts.assumes.is_empty() {
        settings.push(format!("assume {}", describe_assumes(&opts.assumes)));
    }
    settings.push(format!("rng {}", rng_name(opts.rng)));
    settings.push(format!(
        "max-states {}",
        opts.max_states.unwrap_or(DEFAULT_MAX_STATES)
    ));
    if let Some(t) = opts.max_time {
        settings.push(format!("max-time {t}m"));
    } else if let Some(h) = content_horizon {
        settings.push(format!("horizon {h}m (content)"));
    }
    if let Some(k) = opts.diverse {
        settings.push(format!("diverse {k}"));
    }
    if let Some(t) = opts.turn {
        settings.push(turn_setting(t, content.game.is_some()));
    }
    if opts.prune {
        if prune {
            settings.push("prune on".to_string());
        } else {
            settings.push("prune unavailable (host ops)".to_string());
        }
    }
    println!("settings: {}", settings.join(" | "));

    // Goal conditions compile against the content's own declarations.
    let decls = decl_blocks(&content);
    let mut reach_goals: Vec<(String, GoalPredicate)> = Vec::new();
    for arg in &opts.reaches {
        let goal = goal_predicate(arg, &decls).map_err(Failure::Usage)?;
        reach_goals.push((describe_goal(arg), goal));
    }
    let shared_goal = reach_goals.first().cloned();
    let mdp_goal = match &opts.mdp {
        Some(Some(arg)) => {
            let goal = goal_predicate(arg, &decls).map_err(Failure::Usage)?;
            Some((describe_goal(arg), goal))
        }
        Some(None) => shared_goal.clone(),
        None => None,
    };
    let pareto_objectives = match &opts.pareto {
        Some(objs) => {
            let mut resolved: Vec<(String, Objective)> = Vec::new();
            for o in objs {
                let (direction, expr) = parse_objective_text(o, &decls).map_err(Failure::Usage)?;
                resolved.push((o.expr.clone(), Objective { expr, direction }));
            }
            Some(resolved)
        }
        None => None,
    };

    let mut assumptions = Vec::new();
    let mut assumption_invariants = Vec::new();
    for a in &opts.assumes {
        match a {
            AssumptionArg::Card(name, count) => {
                assumptions.push(Assumption::Card {
                    id: CardId::from(name.as_str()),
                    count: *count,
                });
            }
            AssumptionArg::Legacy(key, val) => {
                assumptions.push(Assumption::Legacy {
                    key: key.clone(),
                    value: *val,
                });
            }
            AssumptionArg::Aspect(card, key, val) => {
                assumptions.push(Assumption::Aspect {
                    card: CardId::from(card.as_str()),
                    key: key.clone(),
                    value: *val,
                });
            }
            AssumptionArg::When(cond) => {
                let goal = goal_predicate(&GoalArg::When(cond.clone()), &decls).map_err(Failure::Usage)?;
                assumptions.push(Assumption::Invariant(goal.clone()));
                assumption_invariants.push(goal);
            }
        }
    }
    let extra_invariant = match assumption_invariants.len() {
        0 => None,
        1 => Some(assumption_invariants.remove(0)),
        _ => Some(GoalPredicate::And(assumption_invariants)),
    };
    let mut start_state = boot_analyzer.start().clone();
    for a in &assumptions {
        apply_assumption(&mut start_state, &content, a).map_err(Failure::Run)?;
    }
    let (active_content, slice_boundary) = if let Some(target) = &opts.slice {
        let b = InterfaceBoundary::extract(boot_analyzer.causal(), target);
        (Arc::new(slice_content(&content, target)), Some(b))
    } else {
        (content.clone(), None)
    };

    // The sections run on parallel threads — at most `opts.jobs` at a time,
    // each with a private analyzer boot; the buffers print in the fixed
    // section order after all of them settle, so the output stays
    // deterministic.
    let content = &active_content;
    let start_state = &start_state;
    let extra_inv = extra_invariant.as_ref();
    let mut sections: Vec<SectionJob<'_>> = Vec::new();
    if let (Some(target), Some(boundary)) = (&opts.slice, &slice_boundary) {
        let target = target.clone();
        let boundary = boundary.clone();
        let assumes = opts.assumes.clone();
        sections.push(Box::new(move || {
            let mut out = String::new();
            slice_section(&mut out, &target, &boundary, &assumes);
            out
        }));
    }
    if !reach_goals.is_empty() {
        let goals = &reach_goals;
        sections.push(Box::new(move || {
            let mut analyzer = boot_quiet(content, seed, start_state);
            let mut out = String::new();
            for (desc, goal) in goals {
                reach_section(&mut out, &mut analyzer, desc, goal.clone(), opts, prune, max_time, extra_inv);
            }
            out
        }));
    }
    if opts.classify {
        let goal = shared_goal.clone();
        sections.push(Box::new(move || {
            let mut analyzer = boot_quiet(content, seed, start_state);
            let mut out = String::new();
            classify_section(&mut out, &mut analyzer, goal.as_ref().map(|(_, g)| g), opts, prune, max_time, extra_inv);
            out
        }));
    }
    if let Some((desc, goal)) = &mdp_goal {
        let desc = desc.clone();
        let goal = goal.clone();
        sections.push(Box::new(move || {
            let analyzer = boot_quiet(content, seed, start_state);
            let mut out = String::new();
            mdp_section(&mut out, &analyzer, &desc, goal, opts, prune, max_time, extra_inv);
            out
        }));
    }
    if let (Some((desc, goal)), Some(objectives)) = (&shared_goal, &pareto_objectives) {
        let desc = desc.clone();
        let goal = goal.clone();
        let objectives = objectives.clone();
        sections.push(Box::new(move || {
            let analyzer = boot_quiet(content, seed, start_state);
            let mut out = String::new();
            pareto_section(&mut out, &analyzer, &desc, goal, &objectives, opts, prune, max_time, extra_inv);
            out
        }));
    }
    if opts.audit {
        sections.push(Box::new(move || {
            let mut analyzer = boot_quiet(content, seed, start_state);
            let mut out = String::new();
            audit_section(&mut out, &mut analyzer, opts, prune, max_time, extra_inv);
            out
        }));
    }
    let reports = run_sections(opts.jobs, sections);
    for report in &reports {
        print!("{report}");
    }
    Ok(())
}

/// One buffered section job: runs a section against a private analyzer and
/// returns its report text.
type SectionJob<'a> = Box<dyn FnOnce() -> String + Send + 'a>;

/// Run the buffered section jobs, at most `jobs` at a time, submitting in
/// order and collecting each result under its submission index; the
/// returned texts keep the fixed section order.
fn run_sections(jobs: usize, sections: Vec<SectionJob<'_>>) -> Vec<String> {
    let total = sections.len();
    if total == 0 {
        return Vec::new();
    }
    let mut results: Vec<Option<String>> = (0..total).map(|_| None).collect();
    std::thread::scope(|scope| {
        let (tx, rx) = std::sync::mpsc::channel::<(usize, String)>();
        let mut queue: Vec<(usize, SectionJob<'_>)> =
            sections.into_iter().enumerate().collect();
        let mut inflight = 0usize;
        let mut done = 0usize;
        while done < total {
            while inflight < jobs.max(1) && !queue.is_empty() {
                let (i, job) = queue.remove(0);
                let tx = tx.clone();
                scope.spawn(move || {
                    let _ = tx.send((i, job()));
                });
                inflight += 1;
            }
            let (i, text) = rx
                .recv()
                .unwrap_or_else(|_| panic!("an analyze section thread panicked"));
            results[i] = Some(text);
            inflight -= 1;
            done += 1;
        }
    });
    results.into_iter().map(|r| r.expect("section result")).collect()
}

/// A private analyzer for a section thread: the sequential boot above
/// already reported the diagnostics, so this one is silent.
fn boot_quiet(content: &Arc<Content>, seed: u64, start: &State) -> Analyzer {
    let (analyzer, _) = Analyzer::boot(content.clone(), EffectRegistry::new(), seed, None);
    analyzer.with_start_state(start.clone())
}

/// Every `ending:<ID>` goal must name a declared ending: a typo'd id would
/// otherwise answer "unreachable" over a goal that can never be authored.
/// A usage error naming what the content does declare.
fn validate_endings(opts: &Options, content: &Content) -> Result<(), Failure> {
    let mut goal_args: Vec<&GoalArg> = opts.reaches.iter().collect();
    if let Some(Some(g)) = &opts.mdp {
        goal_args.push(g);
    }
    let unknown: Vec<&str> = goal_args
        .into_iter()
        .filter_map(|arg| match arg {
            GoalArg::Ending(id) if !content.endings.contains_key(id) => Some(id.as_str()),
            _ => None,
        })
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    let declared = content
        .endings
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let note = if declared.is_empty() {
        "the content declares no endings".to_string()
    } else {
        format!("declared endings: {declared}")
    };
    Err(Failure::Usage(format!(
        "unknown ending `{}` — {note}",
        unknown.join("`, `")
    )))
}

/// The settings-line entry for `--turn`: echoed as applied, or honestly
/// noted when there was no game turn to override.
pub(super) fn turn_setting(turn: Time, applied: bool) -> String {
    if applied {
        format!("turn {turn}m")
    } else {
        format!("turn {turn}m not applied — no game turn to override")
    }
}

pub(super) const DEFAULT_MAX_STATES: usize = 100_000;

// ---------------------------------------------------------------------------
// Goal and objective compilation against the content's declarations
// ---------------------------------------------------------------------------

fn goal_predicate(arg: &GoalArg, decls: &[String]) -> Result<GoalPredicate, String> {
    match arg {
        GoalArg::Ending(id) => Ok(GoalPredicate::Ending(Some(EndingId::from(id.as_str())))),
        GoalArg::Card(name) => Ok(GoalPredicate::CardMatch {
            scope: Scope::Anywhere,
            prototype: Some(CardId::from(name.as_str())),
            kind: None,
            terms: vec![QueryTerm::Name(CardId::from(name.as_str()))],
            min_count: 1,
            min_ttl: None,
        }),
        GoalArg::When(cond) => {
            let refs: Vec<&str> = decls.iter().map(String::as_str).collect();
            parse_cond(&refs, &[], cond)
                .map(GoalPredicate::Expr)
                .map_err(|e| format!("the goal condition failed to compile: {e}"))
        }
    }
}

fn parse_objective_text(
    o: &ObjArg,
    decls: &[String],
) -> Result<(Direction, story_content::ir::Expr), String> {
    let open = match o.direction {
        Direction::Maximize => "max(",
        Direction::Minimize => "min(",
    };
    let inner = o
        .expr
        .strip_prefix(open)
        .and_then(|rest| rest.strip_suffix(')'))
        .ok_or_else(|| format!("objective `{}` must be {open}<expression>)", o.expr))?
        .trim();
    // The lang face lowers conditions (booleans); an objective is a numeric
    // expression, so lower `<expr> >= 0` and keep the expression side.
    let refs: Vec<&str> = decls.iter().map(String::as_str).collect();
    let cond = format!("{inner} >= 0");
    let lowered = parse_cond(&refs, &[], &cond)
        .map_err(|e| format!("the objective `{}` failed to compile: {e}", inner))?;
    match lowered {
        story_content::ir::Expr::Cmp { left, .. } => Ok((o.direction, *left)),
        _ => Err(format!(
            "the objective `{}` did not lower to a numeric expression",
            inner
        )),
    }
}

fn describe_goal(arg: &GoalArg) -> String {
    match arg {
        GoalArg::Ending(id) => format!("ending \"{id}\""),
        GoalArg::Card(name) => format!("card \"{name}\""),
        GoalArg::When(cond) => format!("when {cond}"),
    }
}

/// The declaration blocks (zones, aspects, cards, legacies, relations) the
/// content declares, rendered back in `.sw` syntax: the query condition
/// compiler splices them into its synthetic target so name paths, aspect
/// keys, kinds, zones, legacies, and relation kinds validate exactly as
/// they do in the real content.
fn decl_blocks(content: &Content) -> Vec<String> {
    let mut out = Vec::new();
    if !content.zones.is_empty() {
        let mut s = String::from("zones");
        for z in content.zones.values() {
            s.push_str(&format!("\n  {}", z.name));
            if let Some(parent) = &z.parent {
                s.push_str(&format!(" in {parent}"));
            }
        }
        out.push(s);
    }
    if !content.aspects.is_empty() {
        let mut s = String::from("aspects");
        for a in content.aspects.values() {
            s.push_str(&format!("\n  {}: {}", a.key, a.default));
        }
        out.push(s);
    }
    if !content.cards.is_empty() {
        let mut s = String::from("cards");
        for c in content.cards.values() {
            s.push_str(&format!("\n  card {}", c.id));
            if let Some(kind) = &c.kind {
                s.push_str(&format!("\n    kind {kind}"));
            }
            match &c.mint {
                MintPolicy::Stackable => s.push_str("\n    stackable"),
                MintPolicy::Unique => s.push_str("\n    unique"),
                MintPolicy::Group(g) => s.push_str(&format!("\n    group {g}")),
                MintPolicy::SingleCopy => {}
            }
        }
        out.push(s);
    }
    if !content.legacies.is_empty() {
        let mut s = String::from("legacies");
        for l in content.legacies.values() {
            s.push_str(&format!("\n  {}: {}", l.key, l.default));
        }
        out.push(s);
    }
    if !content.relation_kinds.is_empty() {
        let mut s = String::from("relations");
        for r in content.relation_kinds.values() {
            s.push_str(&format!("\n  {}", r.name));
            if r.symmetric {
                s.push_str(" symmetric");
            }
            if r.exclusive {
                s.push_str(" exclusive");
            }
        }
        out.push(s);
    }
    out
}
