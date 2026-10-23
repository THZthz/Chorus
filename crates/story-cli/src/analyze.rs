//! The `analyze` subcommand: boots compiled content (the same boot the play
//! driver runs, default effect registry) and drives story-analyze's public
//! engines — reachability with replay-verified witnesses, state-space
//! classification, the stochastic MDP solver, the design auditor, and the
//! Pareto/cut analyzer. Reports print to stdout in a fixed section order
//! (reach, classify, mdp, pareto, audit) with deterministic, sorted
//! findings; bounds truncation and inconclusive answers are stated, never
//! silently dropped.

use std::path::Path;
use std::sync::Arc;

use story_analyze::audit::{audit, Evidence, FaultEvent, FaultWitness};
use story_analyze::classify::{classify, ending_set, ClassifyConfig};
use story_analyze::lang::parse_cond;
use story_analyze::mdp::{MdpConfig, MdpOutcome};
use story_analyze::pareto::{analyze as pareto_analyze, Direction, Objective, ParetoConfig};
use story_analyze::query::{GoalPredicate, ReachQuery, RngMode, SearchObjective};
use story_analyze::search::{Analyzer, ExploreConfig, SolveOutcome};
use story_analyze::transitions::{PlayerStep, WitnessPath};
use story_content::ir::{QueryTerm, Scope, SeedSpec};
use story_content::Content;
use story_core::{CardId, EndingId, MintPolicy, Time};
use story_engine::{EffectRegistry, Event};

/// How many list entries a section prints before summarizing the rest.
const LIST_CAP: usize = 8;

/// The `analyze --help` text: the subcommand's own option reference.
const ANALYZE_USAGE: &str = "\
story-cli analyze — boot compiled content and run the design analyses

USAGE:
    story-cli analyze <file.sw> [options]

ANALYSES (at least one required unless --audit is given):
    --reach <goal>       reachability with replay-verified witnesses
    --classify           winnability, points of no return, softlocks
    --mdp [<goal>]       optimal-play values V_max / V_minimax / V_unif
    --audit              recipe shadowing, dead content, reachable faults
    --pareto <objs>      trade-off frontier and counterfactual cuts
GOAL FORMS (for --reach and --mdp):
    ending:<ID>          the story has ended with that ending; an id the
                         content does not declare is a usage error (exit 2)
    card:<name>          at least one live card of that name
    when <condition>     a condition in .sw syntax, e.g.
                         'when beacon.heat >= 1 and clock >= 6h'
OBJECTIVE FORMS (for --pareto; comma-separated):
    max(<expression>)    maximize the expression at the goal state
    min(<expression>)    minimize the expression at the goal state
MODIFIERS:
    --rng <mode>         deterministic | optimistic | adversarial
                         (default deterministic)
    --max-states <n>     expansion bound (default 100000): caps the explored
                         state space of --reach/--classify/--pareto and the
                         game-graph node budget of --mdp
    --max-time <time>    clock horizon in minutes; <n>m / <n>h accepted
    --diverse <k>        report up to k structurally distinct reach witnesses
    --turn <duration>    override the game's turn length; <n>m / <n>h; when
                         the content has no game turn to override, the
                         settings line says the override was not applied
    --seed N             host seed when the content declares `seed host`
    --help               this help (exit 0)
NOTES:
    --classify and --pareto take their goal from --reach; --pareto requires it.
    Analyses exit 0 even when the answer is \"unreachable\"; exit 1 on compile
    or boot failure; exit 2 on usage errors.";

// ---------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------

/// A goal in goal-selector position.
#[derive(Clone, Debug)]
enum GoalArg {
    Ending(String),
    Card(String),
    When(String),
}

/// One `--pareto` objective, direction plus the raw expression text.
#[derive(Clone, Debug)]
struct ObjArg {
    direction: Direction,
    expr: String,
}

/// The parsed `analyze` invocation.
#[derive(Debug)]
struct Options {
    path: String,
    reaches: Vec<GoalArg>,
    classify: bool,
    /// `--mdp` given; the optional goal argument.
    mdp: Option<Option<GoalArg>>,
    audit: bool,
    pareto: Option<Vec<ObjArg>>,
    rng: RngMode,
    max_states: Option<usize>,
    max_time: Option<Time>,
    diverse: Option<usize>,
    turn: Option<Time>,
    seed: u64,
}

/// Why `analyze` stopped: a usage problem (stderr, exit 2) or a run
/// failure such as a compile/boot error (stdout, exit 1).
enum Failure {
    Usage(String),
    Run(String),
}

pub fn run(args: &[String]) -> i32 {
    let opts = match parse_args(args) {
        Ok(opts) => opts,
        Err(msg) => return usage_error(msg),
    };
    match run_analyses(&opts) {
        Ok(()) => 0,
        Err(Failure::Usage(msg)) => usage_error(msg),
        Err(Failure::Run(msg)) => {
            println!("{msg}");
            1
        }
    }
}

fn usage_error(msg: String) -> i32 {
    eprintln!("story-cli analyze: {msg}");
    eprintln!("run `story-cli analyze --help` for the analyze options");
    2
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        path: String::new(),
        reaches: Vec::new(),
        classify: false,
        mdp: None,
        audit: false,
        pareto: None,
        rng: RngMode::Deterministic,
        max_states: None,
        max_time: None,
        diverse: None,
        turn: None,
        seed: 1,
    };
    let mut i = 0usize;
    while i < args.len() {
        let arg = args[i].as_str();
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (arg, None),
        };
        i += 1;
        match flag {
            "--help" | "-h" => {
                no_inline(inline, "--help")?;
                println!("{ANALYZE_USAGE}");
                std::process::exit(0);
            }
            "--reach" => {
                let v = take_value(inline, args, &mut i, "--reach")?;
                opts.reaches.push(parse_goal_arg(&v)?);
            }
            "--classify" => {
                no_inline(inline, "--classify")?;
                opts.classify = true;
            }
            "--mdp" => {
                // The goal argument is optional: a following flag-like token
                // belongs to the next option, not to --mdp.
                let goal = if let Some(v) = inline {
                    Some(parse_goal_arg(&v)?)
                } else if peek_value(args, i) {
                    let v = args[i].clone();
                    i += 1;
                    Some(parse_goal_arg(&v)?)
                } else {
                    None
                };
                opts.mdp = Some(goal);
            }
            "--audit" => {
                no_inline(inline, "--audit")?;
                opts.audit = true;
            }
            "--pareto" => {
                let v = take_value(inline, args, &mut i, "--pareto")?;
                opts.pareto = Some(split_objectives(&v)?);
            }
            "--rng" => {
                let v = take_value(inline, args, &mut i, "--rng")?;
                opts.rng = match v.as_str() {
                    "deterministic" => RngMode::Deterministic,
                    "optimistic" => RngMode::OptimisticBranching,
                    "adversarial" => RngMode::Adversarial,
                    other => {
                        return Err(format!(
                            "--rng must be deterministic, optimistic, or adversarial; got `{other}`"
                        ))
                    }
                };
            }
            "--max-states" => {
                let v = take_value(inline, args, &mut i, "--max-states")?;
                let n: usize = v
                    .parse()
                    .map_err(|_| format!("--max-states needs a positive integer; got `{v}`"))?;
                if n == 0 {
                    return Err("--max-states must be positive".to_string());
                }
                opts.max_states = Some(n);
            }
            "--max-time" => {
                let v = take_value(inline, args, &mut i, "--max-time")?;
                opts.max_time = Some(parse_time("--max-time", &v)?);
            }
            "--diverse" => {
                let v = take_value(inline, args, &mut i, "--diverse")?;
                let n: usize = v
                    .parse()
                    .map_err(|_| format!("--diverse needs a positive integer; got `{v}`"))?;
                if n == 0 {
                    return Err("--diverse must be positive".to_string());
                }
                opts.diverse = Some(n);
            }
            "--turn" => {
                let v = take_value(inline, args, &mut i, "--turn")?;
                opts.turn = Some(parse_time("--turn", &v)?);
            }
            "--seed" => {
                let v = take_value(inline, args, &mut i, "--seed")?;
                opts.seed = v
                    .parse()
                    .map_err(|_| format!("--seed needs an unsigned integer; got `{v}`"))?;
            }
            other if other.starts_with("--") => return Err(format!("unknown option `{other}`")),
            other => {
                if opts.path.is_empty() {
                    opts.path = other.to_string();
                } else {
                    return Err(format!("unexpected argument `{other}` (one content file only)"));
                }
            }
        }
    }
    if opts.path.is_empty() {
        return Err("no content file given".to_string());
    }
    let mdp_goal = match &opts.mdp {
        Some(Some(g)) => Some(g.clone()),
        Some(None) => opts.reaches.first().cloned(),
        None => None,
    };
    if opts.mdp.is_some() && mdp_goal.is_none() {
        return Err(
            "--mdp needs a goal: pass one to --mdp or to --reach".to_string(),
        );
    }
    if opts.pareto.is_some() && opts.reaches.is_empty() {
        return Err("--pareto needs a goal: pass --reach <goal>".to_string());
    }
    if opts.reaches.is_empty()
        && !opts.classify
        && opts.mdp.is_none()
        && opts.pareto.is_none()
        && !opts.audit
    {
        return Err(
            "no analysis requested: give a goal selector (--reach, --classify, --mdp, --pareto) \
             or --audit"
                .to_string(),
        );
    }
    Ok(opts)
}

/// Consume the flag's value: the inline `--flag=value` form, else the next
/// token when it is not flag-like.
fn take_value(
    inline: Option<String>,
    args: &[String],
    i: &mut usize,
    flag: &str,
) -> Result<String, String> {
    if let Some(v) = inline {
        return Ok(v);
    }
    if peek_value(args, *i) {
        let v = args[*i].clone();
        *i += 1;
        return Ok(v);
    }
    Err(format!("{flag} needs a value"))
}

fn peek_value(args: &[String], i: usize) -> bool {
    args.get(i).is_some_and(|v| !v.starts_with("--"))
}

fn no_inline(inline: Option<String>, flag: &str) -> Result<(), String> {
    inline.map_or(Ok(()), |v| Err(format!("{flag} takes no value; got `{v}`")))
}

fn parse_goal_arg(s: &str) -> Result<GoalArg, String> {
    if let Some(id) = s.strip_prefix("ending:") {
        if id.is_empty() {
            return Err("ending: needs an ending id".to_string());
        }
        return Ok(GoalArg::Ending(id.to_string()));
    }
    if let Some(name) = s.strip_prefix("card:") {
        if name.is_empty() {
            return Err("card: needs a card name".to_string());
        }
        return Ok(GoalArg::Card(name.to_string()));
    }
    if let Some(cond) = s.strip_prefix("when ") {
        let cond = cond.trim();
        if cond.is_empty() {
            return Err("when: needs a condition".to_string());
        }
        return Ok(GoalArg::When(cond.to_string()));
    }
    Err(format!(
        "unknown goal form `{s}` — use ending:<ID>, card:<name>, or 'when <condition>'"
    ))
}

fn split_objectives(s: &str) -> Result<Vec<ObjArg>, String> {
    let mut pieces: Vec<String> = Vec::new();
    let mut depth = 0usize;
    let mut cur = String::new();
    for ch in s.chars() {
        match ch {
            '(' => {
                depth += 1;
                cur.push(ch);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(ch);
            }
            ',' if depth == 0 => pieces.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    pieces.push(cur);
    let mut objs = Vec::new();
    for piece in pieces {
        let p = piece.trim();
        if p.is_empty() {
            return Err("empty objective in --pareto".to_string());
        }
        let direction = if p.starts_with("max(") {
            Direction::Maximize
        } else if p.starts_with("min(") {
            Direction::Minimize
        } else {
            return Err(format!(
                "objective `{p}` must be max(<expression>) or min(<expression>)"
            ));
        };
        if !p.ends_with(')') {
            return Err(format!(
                "objective `{p}` must be max(<expression>) or min(<expression>)"
            ));
        }
        objs.push(ObjArg {
            direction,
            expr: p.to_string(),
        });
    }
    if objs.is_empty() {
        return Err("--pareto needs at least one objective".to_string());
    }
    Ok(objs)
}

fn parse_time(flag: &str, s: &str) -> Result<Time, String> {
    let t = s.trim();
    let (num, mult) = match t.strip_suffix('m') {
        Some(n) => (n, 1i64),
        None => match t.strip_suffix('h') {
            Some(n) => (n, 60i64),
            None => (t, 1i64),
        },
    };
    let n: i64 = num.parse().map_err(|_| format!("{flag} needs minutes, <n>m, or <n>h; got `{s}`"))?;
    let v = n
        .checked_mul(mult)
        .ok_or_else(|| format!("{flag} overflows"))?;
    if v <= 0 {
        return Err(format!("{flag} must be positive; got `{s}`"));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// The analyses
// ---------------------------------------------------------------------------

fn run_analyses(opts: &Options) -> Result<(), Failure> {
    // Compile (the same face the play driver runs; diagnostics are traffic).
    let out = story_lang::compile_file(Path::new(&opts.path));
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
    let (mut analyzer, diags) =
        Analyzer::boot(content.clone(), EffectRegistry::new(), seed, None);
    for d in &diags {
        println!("boot: {d}");
    }
    if diags.iter().any(|d| d.is_error()) {
        return Err(Failure::Run("boot failed with error diagnostic(s)".to_string()));
    }
    if let Some(game) = content.game.as_ref() {
        println!("{} — seed {}, turn {}m", game.title, seed, game.turn);
    }
    let mut settings = vec![
        format!("rng {}", rng_name(opts.rng)),
        format!(
            "max-states {}",
            opts.max_states.unwrap_or(DEFAULT_MAX_STATES)
        ),
    ];
    if let Some(t) = opts.max_time {
        settings.push(format!("max-time {t}m"));
    }
    if let Some(k) = opts.diverse {
        settings.push(format!("diverse {k}"));
    }
    if let Some(t) = opts.turn {
        settings.push(turn_setting(t, content.game.is_some()));
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

    for (desc, goal) in &reach_goals {
        reach_section(&mut analyzer, desc, goal.clone(), opts);
    }
    if opts.classify {
        classify_section(&mut analyzer, shared_goal.as_ref().map(|(_, g)| g), opts);
    }
    if let Some((desc, goal)) = &mdp_goal {
        mdp_section(&analyzer, desc, goal.clone(), opts);
    }
    if let (Some((desc, goal)), Some(objectives)) = (&shared_goal, &pareto_objectives) {
        pareto_section(&analyzer, desc, goal.clone(), objectives, opts);
    }
    if opts.audit {
        audit_section(&mut analyzer, opts);
    }
    Ok(())
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
fn turn_setting(turn: Time, applied: bool) -> String {
    if applied {
        format!("turn {turn}m")
    } else {
        format!("turn {turn}m not applied — no game turn to override")
    }
}

const DEFAULT_MAX_STATES: usize = 100_000;

fn explore_config(opts: &Options) -> ExploreConfig {
    ExploreConfig {
        rng_mode: opts.rng,
        max_states: opts.max_states.unwrap_or(DEFAULT_MAX_STATES),
        max_time: opts.max_time,
        ..Default::default()
    }
}

fn truncated_note(truncated: bool, opts: &Options) -> &'static str {
    if truncated {
        "TRUNCATED — findings are bounds-conditional"
    } else if opts.max_time.is_some() {
        "within bounds"
    } else {
        "space exhausted within bounds"
    }
}

// -- reach ------------------------------------------------------------------

fn reach_section(analyzer: &mut Analyzer, desc: &str, goal: GoalPredicate, opts: &Options) {
    println!();
    println!("== reach: {desc} ==");
    let mut query = ReachQuery::new(goal);
    query.rng_mode = opts.rng;
    query.max_time = opts.max_time;
    if let Some(n) = opts.max_states {
        query.max_states = n;
    }
    if let Some(k) = opts.diverse {
        query.objective = SearchObjective::DiverseWays { max_ways: k };
    }
    let outcome = analyzer.solve(&query);
    print_solve(&outcome, opts);
}

fn print_solve(outcome: &SolveOutcome, opts: &Options) {
    let status = if !outcome.witnesses.is_empty() {
        format!("REACHABLE — {} witness(es)", outcome.witnesses.len())
    } else if outcome.truncated {
        "INCONCLUSIVE — the search hit its bounds before exhausting the space".to_string()
    } else {
        "UNREACHABLE — no state within the bounds satisfies the goal".to_string()
    };
    println!("outcome: {status}");
    println!(
        "expansions: {}{}",
        outcome.expansions,
        if outcome.truncated {
            format!(
                " (truncated at max_states {})",
                opts.max_states.unwrap_or(DEFAULT_MAX_STATES)
            )
        } else {
            String::new()
        }
    );
    if outcome.truncated {
        println!(
            "bounds: truncated at max_states {}{} — a no-witness answer is bounds-conditional",
            opts.max_states.unwrap_or(DEFAULT_MAX_STATES),
            opts.max_time.map_or_else(String::new, |t| format!(", max-time {t}m"))
        );
    }
    if outcome.witnesses_dropped_by_the_oracle > 0 {
        println!(
            "witnesses dropped by the replay oracle: {}",
            outcome.witnesses_dropped_by_the_oracle
        );
    }
    let total = outcome.witnesses.len();
    for (i, witness) in outcome.witnesses.iter().enumerate() {
        println!(
            "witness {} of {} — {} action(s), {} transition(s), final clock {}m, {} (replay-verified)",
            i + 1,
            total,
            witness.actions,
            witness.depth,
            witness.final_clock(),
            probability_text(witness.probability),
        );
        print_witness(witness);
    }
}

/// The numbered step list: verb/recipe/clock per step and the decisive
/// narration line of each step's event stream.
fn print_witness(witness: &WitnessPath) {
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
        println!(
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
            println!("       \"{text}\"");
        }
        state = record.state_after.clone();
    }
    if let Some(ended) = &witness.final_state().ended {
        println!("  final: the story ends \"{ended}\"");
    }
}

// -- classify ---------------------------------------------------------------

fn classify_section(analyzer: &mut Analyzer, goal: Option<&GoalPredicate>, opts: &Options) {
    println!();
    println!("== classify ==");
    let config = ClassifyConfig {
        explore: explore_config(opts),
        winning_endings: goal.and_then(|g| match g {
            GoalPredicate::Ending(id) => Some(ending_set(id.as_ref())),
            _ => None,
        })
        .unwrap_or_default(),
    };
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
    println!("winning endings: {note}");
    let c = classify(analyzer, &config);
    println!(
        "explored: {} states, {} edges ({})",
        c.graph.nodes.len(),
        c.graph.edges.len(),
        truncated_note(c.graph.truncated, opts),
    );
    println!(
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
    println!("points of no return: {} crossing edge(s)", ponr.len());
    print_capped(ponr.iter().map(|(u, v)| {
        format!(
            "node {u} -> node {v} (the state leaves winnable play at clock {}m)",
            c.graph.nodes[*v].state.now
        )
    }));
    println!("softlocked live states: {}", c.softlock.len());
    print_capped(c.softlock.iter().map(|v| {
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
    println!("infinite loops: {}", loops.len());
    print_capped(loops.iter().map(|l| {
        let nodes: Vec<String> = l.iter().map(|v| v.to_string()).collect();
        format!("loop over {} state(s): nodes {}", l.len(), nodes.join(", "))
    }));
}

// -- mdp --------------------------------------------------------------------

fn mdp_section(analyzer: &Analyzer, desc: &str, goal: GoalPredicate, opts: &Options) {
    println!();
    println!("== mdp: {desc} ==");
    let mut config = MdpConfig::new(goal);
    config.rng_mode = opts.rng;
    config.max_time = opts.max_time;
    // --max-states is the expansion bound everywhere: the game graph's node
    // budget here. max_actions/max_depth keep the crate defaults.
    config.max_nodes = opts.max_states.unwrap_or(DEFAULT_MAX_STATES);
    let outcome = story_analyze::mdp::solve(analyzer, &config);
    print_mdp(&outcome);
}

fn print_mdp(outcome: &MdpOutcome) {
    println!(
        "V_max     = {}  (maximum goal probability under optimal play)",
        fraction_or_decimal(outcome.start.v_max)
    );
    println!(
        "V_minimax = {}  (worst-case guarantee over positive-probability branches)",
        fraction_or_decimal(outcome.start.v_minimax)
    );
    println!(
        "V_unif    = {}  (uniform-play baseline)",
        fraction_or_decimal(outcome.start.v_unif)
    );
    println!(
        "expected actions under the optimal policy: {}",
        fraction_or_decimal(outcome.start.expected_actions)
    );
    println!(
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

fn pareto_section(
    analyzer: &Analyzer,
    goal_desc: &str,
    goal: GoalPredicate,
    objectives: &[(String, Objective)],
    opts: &Options,
) {
    let obj_desc: Vec<&str> = objectives.iter().map(|(text, _)| text.as_str()).collect();
    let only: Vec<Objective> = objectives.iter().map(|(_, o)| o.clone()).collect();
    println!();
    println!(
        "== pareto: goal {goal_desc}, objectives {} ==",
        obj_desc.join(", ")
    );
    let mut config = ParetoConfig::new(goal, only);
    config.explore = explore_config(opts);
    let report = pareto_analyze(analyzer, &config);
    if report.frontier.is_empty() {
        println!("frontier: empty — no explored state satisfies the goal within the bounds");
    } else {
        println!("frontier: {} point(s)", report.frontier.len());
        for (i, point) in report.frontier.iter().enumerate() {
            let values: Vec<String> = point.values.iter().map(|v| fraction_or_decimal(*v)).collect();
            println!(
                "  point {} of {}: [{}] — {} action(s), final clock {}m, {}",
                i + 1,
                report.frontier.len(),
                values.join(", "),
                point.witness.actions,
                point.witness.final_clock(),
                probability_text(point.witness.probability),
            );
            print_witness(&point.witness);
        }
    }
    println!("mandatory recipes: {}", joined_or_dash(report.mandatory_recipes.iter()));
    println!("mandatory cards: {}", joined_or_dash(report.mandatory_cards.iter()));
    println!(
        "minimal cuts: {}",
        cuts_text(&report.minimal_recipe_cuts, &report.verified_cuts)
    );
    if report.truncated {
        println!(
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

fn audit_section(analyzer: &mut Analyzer, opts: &Options) {
    println!();
    println!("== audit ==");
    let report = audit(analyzer, &explore_config(opts));
    println!(
        "explored: {} states ({})",
        report.explored_nodes,
        truncated_note(report.truncated, opts)
    );
    let mut shadowing = report.shadowing.clone();
    shadowing.sort_by(|a, b| {
        (&a.verb, &a.shadower, &a.shadowed).cmp(&(&b.verb, &b.shadower, &b.shadowed))
    });
    println!("shadowing: {} finding(s)", shadowing.len());
    for s in &shadowing {
        let verb = s.verb.as_ref().map_or_else(String::new, |v| format!("verb {v}: "));
        println!(
            "  {verb}\"{}\" shadows \"{}\"{}",
            s.shadower,
            s.shadowed,
            if s.conditional {
                " (conditional — the shadower is spent after firing)"
            } else {
                ""
            }
        );
    }
    let mut unreachable = report.unreachable.clone();
    unreachable.sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    println!("unreachable content: {} finding(s)", unreachable.len());
    for u in &unreachable {
        let evidence = match u.evidence {
            Evidence::StaticallyProved => "statically proved",
            Evidence::NotWithinBounds => "not within the exploration bounds (bounds-conditional)",
        };
        println!("  {} {} — {evidence}", u.kind, u.id);
    }
    let mut faults: Vec<&FaultWitness> = report.faults.iter().collect();
    faults.sort_by_key(|f| fault_key(f));
    println!("reachable faults: {} finding(s)", faults.len());
    for (i, f) in faults.iter().enumerate() {
        if i >= LIST_CAP {
            println!("  ... and {} more", faults.len() - LIST_CAP);
            break;
        }
        let event = match &f.event {
            FaultEvent::ResolutionFailed { reason } => format!("resolution failed ({reason})"),
            FaultEvent::StartSkipped { reason } => format!("start skipped ({reason})"),
        };
        println!(
            "  fault {} of {}: {event} — witness {} step(s), final clock {}m",
            i + 1,
            faults.len(),
            f.witness.records.len(),
            f.witness.final_clock()
        );
        print_witness(&f.witness);
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

fn print_capped(lines: impl Iterator<Item = String>) {
    let all: Vec<String> = lines.collect();
    for line in all.iter().take(LIST_CAP) {
        println!("  {line}");
    }
    if all.len() > LIST_CAP {
        println!("  ... and {} more", all.len() - LIST_CAP);
    }
}

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

// ---------------------------------------------------------------------------
// Number formatting
// ---------------------------------------------------------------------------

fn rng_name(rng: RngMode) -> &'static str {
    match rng {
        RngMode::Deterministic => "deterministic",
        RngMode::OptimisticBranching => "optimistic",
        RngMode::Adversarial => "adversarial",
    }
}

/// Exact fraction when the double is a small rational (the chance-point
/// class probabilities are), else a decimal. The scan runs on the absolute
/// value so negative rationals reduce too, with the sign reapplied; a
/// magnitude below the six-decimal resolution prints as `≈0` rather than a
/// misleading `±0.000000`.
fn fraction_or_decimal(x: f64) -> String {
    if !x.is_finite() {
        return format!("{x}");
    }
    if x == 0.0 {
        return "0".to_string();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let a = x.abs();
    if a < 1e-6 {
        return "≈0".to_string();
    }
    for q in 1..=1000u64 {
        let scaled = a * q as f64;
        let nearest = scaled.round();
        if nearest > 0.0 && (scaled - nearest).abs() < 1e-9 {
            let (mut n, mut d) = (nearest as u64, q);
            let g = gcd(n, d);
            n /= g;
            d /= g;
            return if d == 1 {
                format!("{sign}{n}")
            } else {
                format!("{sign}{n}/{d}")
            };
        }
    }
    format!("{sign}{a:.6}")
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn probability_text(p: Option<f64>) -> String {
    match p {
        None => "probability n/a (adversarial witnesses carry none)".to_string(),
        Some(x) => format!("probability {}", fraction_or_decimal(x)),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{fraction_or_decimal, turn_setting};

    #[test]
    fn fractions_reduce_on_the_absolute_value_and_keep_the_sign() {
        assert_eq!(fraction_or_decimal(0.0), "0");
        assert_eq!(fraction_or_decimal(1.0), "1");
        assert_eq!(fraction_or_decimal(0.5), "1/2");
        assert_eq!(fraction_or_decimal(0.125), "1/8");
        assert_eq!(fraction_or_decimal(-0.5), "-1/2");
        assert_eq!(fraction_or_decimal(-2.0), "-2");
        // A negative irrational keeps the sign through the decimal path.
        assert_eq!(fraction_or_decimal(-std::f64::consts::PI), "-3.141593");
        assert_eq!(fraction_or_decimal(std::f64::consts::PI), "3.141593");
    }

    #[test]
    fn tiny_magnitudes_print_an_honest_near_zero() {
        assert_eq!(fraction_or_decimal(1e-9), "≈0");
        assert_eq!(fraction_or_decimal(-1e-9), "≈0");
        assert_eq!(fraction_or_decimal(f64::INFINITY), "inf");
        assert_eq!(fraction_or_decimal(f64::NAN), "NaN");
    }

    #[test]
    fn the_turn_setting_distinguishes_applied_from_not_applied() {
        assert_eq!(turn_setting(15, true), "turn 15m");
        assert_eq!(
            turn_setting(15, false),
            "turn 15m not applied — no game turn to override"
        );
    }
}
