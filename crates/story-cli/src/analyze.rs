//! The `analyze` subcommand: boots compiled content (the same boot the play
//! driver runs, default effect registry) and drives story-analyze's public
//! engines — reachability with replay-verified witnesses, state-space
//! classification, the stochastic MDP solver, the design auditor, and the
//! Pareto/cut analyzer. The requested sections run on parallel threads,
//! each with its own analyzer boot; every section appends to its own
//! buffer, and the reports print to stdout in the fixed section order
//! (reach, classify, mdp, pareto, audit) once all of them settle —
//! deterministic output, sorted findings, and bounds truncation and
//! inconclusive answers stated, never silently dropped.

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
    --max-time <time>    clock horizon in minutes; <n>m / <n>h / <n>h<m>m
                         accepted; overrides the content's `meta horizon`
    --diverse <k>        report up to k structurally distinct reach witnesses
    --turn <duration>    override the game's turn length; <n>m / <n>h; when
                         the content has no game turn to override, the
                         settings line says the override was not applied
    --seed N             host seed when the content declares `seed host`
    --prune              skip branches that look impossible (goal facts
                         cannot be grounded even in the causal relaxation).
                         Heuristic: it can mis-prune decrease-chain routes,
                         so with no witness the answer reads NO WITNESS
                         UNDER PRUNING, never UNREACHABLE. Witness-bearing
                         answers are unaffected. Inert for host-op content.
    --profile            accumulate span wall times and print the table to
                         stderr when the run ends
    --jobs <n>           section concurrency cap (default 1); a full
                         exploration still holds gigabytes (one emberwatch
                         classify peaks near 6 GB), so raise this only for
                         content whose state space fits memory twice over
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
    profile: bool,
    prune: bool,
    /// Section concurrency cap. The explorations are allocation-heavy
    /// (~1 GB per full state space on emberwatch), so more concurrent
    /// sections trade wall time for memory pressure.
    jobs: usize,
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
    if opts.profile {
        crate::profile::install();
    }
    let code = {
        let _root = tracing::info_span!("analyze").entered();
        match run_analyses(&opts) {
            Ok(()) => 0,
            Err(Failure::Usage(msg)) => usage_error(msg),
            Err(Failure::Run(msg)) => {
                println!("{msg}");
                1
            }
        }
    };
    crate::profile::report();
    code
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
        profile: false,
        prune: false,
        jobs: 1,
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
            "--profile" => {
                no_inline(inline, "--profile")?;
                opts.profile = true;
            }
            "--prune" => {
                no_inline(inline, "--prune")?;
                opts.prune = true;
            }
            "--jobs" => {
                let v = take_value(inline, args, &mut i, "--jobs")?;
                let n: usize = v
                    .parse()
                    .map_err(|_| format!("--jobs needs a positive integer; got `{v}`"))?;
                if n == 0 {
                    return Err("--jobs must be positive".to_string());
                }
                opts.jobs = n;
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

/// Parse a duration spelling in the content language's own grammar:
/// `<n>m`, `<n>h`, `<n>h<m>m`, or bare minutes. Positive; minutes.
fn parse_duration(s: &str) -> Option<i64> {
    let t = s.trim();
    let (h_str, rest, had_h) = match t.split_once('h') {
        Some((h, rest)) => (h, rest, true),
        None => ("0", t, false),
    };
    // Bare minutes: no h part, no m suffix.
    if !had_h && !rest.ends_with('m') {
        let v: i64 = t.parse().ok()?;
        return (v > 0).then_some(v);
    }
    let h: i64 = h_str.parse().ok()?;
    if h < 0 {
        return None;
    }
    let m: i64 = if rest.is_empty() { 0 } else { rest.strip_suffix('m')?.parse().ok()? };
    if m < 0 {
        return None;
    }
    let v = h.checked_mul(60)?.checked_add(m)?;
    (v > 0).then_some(v)
}

fn parse_time(flag: &str, s: &str) -> Result<Time, String> {
    parse_duration(s).ok_or_else(|| {
        format!("{flag} needs minutes, <n>m, <n>h, or <n>h<m>m; got `{s}`")
    })
}

// ---------------------------------------------------------------------------
// The analyses
// ---------------------------------------------------------------------------

fn run_analyses(opts: &Options) -> Result<(), Failure> {
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
    let mut settings = vec![
        format!("rng {}", rng_name(opts.rng)),
        format!(
            "max-states {}",
            opts.max_states.unwrap_or(DEFAULT_MAX_STATES)
        ),
    ];
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

    // The sections run on parallel threads — at most `opts.jobs` at a time,
    // each with a private analyzer boot; the buffers print in the fixed
    // section order after all of them settle, so the output stays
    // deterministic.
    let content = &content;
    let mut sections: Vec<SectionJob<'_>> = Vec::new();
    if !reach_goals.is_empty() {
        let opts = opts;
        let max_time = max_time;
        let goals = &reach_goals;
        sections.push(Box::new(move || {
            let mut analyzer = boot_quiet(content, seed);
            let mut out = String::new();
            for (desc, goal) in goals {
                reach_section(&mut out, &mut analyzer, desc, goal.clone(), opts, prune, max_time);
            }
            out
        }));
    }
    if opts.classify {
        let opts = opts;
        let goal = shared_goal.clone();
        sections.push(Box::new(move || {
            let mut analyzer = boot_quiet(content, seed);
            let mut out = String::new();
            classify_section(&mut out, &mut analyzer, goal.as_ref().map(|(_, g)| g), opts, prune, max_time);
            out
        }));
    }
    if let Some((desc, goal)) = &mdp_goal {
        let opts = opts;
        let desc = desc.clone();
        let goal = goal.clone();
        sections.push(Box::new(move || {
            let analyzer = boot_quiet(content, seed);
            let mut out = String::new();
            mdp_section(&mut out, &analyzer, &desc, goal, opts, prune, max_time);
            out
        }));
    }
    if let (Some((desc, goal)), Some(objectives)) = (&shared_goal, &pareto_objectives) {
        let opts = opts;
        let desc = desc.clone();
        let goal = goal.clone();
        let objectives = objectives.clone();
        sections.push(Box::new(move || {
            let analyzer = boot_quiet(content, seed);
            let mut out = String::new();
            pareto_section(&mut out, &analyzer, &desc, goal, &objectives, opts, prune, max_time);
            out
        }));
    }
    if opts.audit {
        let opts = opts;
        sections.push(Box::new(move || {
            let mut analyzer = boot_quiet(content, seed);
            let mut out = String::new();
            audit_section(&mut out, &mut analyzer, opts, prune, max_time);
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
fn boot_quiet(content: &Arc<Content>, seed: u64) -> Analyzer {
    let (analyzer, _) = Analyzer::boot(content.clone(), EffectRegistry::new(), seed, None);
    analyzer
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

fn explore_config(opts: &Options, prune: bool, max_time: Option<Time>) -> ExploreConfig {
    ExploreConfig {
        rng_mode: opts.rng,
        max_states: opts.max_states.unwrap_or(DEFAULT_MAX_STATES),
        max_time,
        prune,
        ..Default::default()
    }
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

// -- reach ------------------------------------------------------------------

fn reach_section(
    out: &mut String,
    analyzer: &mut Analyzer,
    desc: &str,
    goal: GoalPredicate,
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
) {
    let _span = tracing::info_span!("reach").entered();
    outln!(out);
    outln!(out, "== reach: {desc} ==");
    let mut query = ReachQuery::new(goal);
    query.rng_mode = opts.rng;
    query.max_time = max_time;
    query.prune = prune;
    if let Some(n) = opts.max_states {
        query.max_states = n;
    }
    if let Some(k) = opts.diverse {
        query.objective = SearchObjective::DiverseWays { max_ways: k };
    }
    let outcome = analyzer.solve(&query);
    print_solve(out, &outcome, opts, max_time);
}

fn print_solve(out: &mut String, outcome: &SolveOutcome, opts: &Options, max_time: Option<Time>) {
    let status = if !outcome.witnesses.is_empty() {
        format!("REACHABLE — {} witness(es)", outcome.witnesses.len())
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

fn classify_section(
    out: &mut String,
    analyzer: &mut Analyzer,
    goal: Option<&GoalPredicate>,
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
) {
    let _span = tracing::info_span!("classify").entered();
    outln!(out);
    outln!(out, "== classify ==");
    let mut config = ClassifyConfig {
        explore: explore_config(opts, prune, max_time),
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
    outln!(
        out,
        "explored: {} states, {} edges ({})",
        c.graph.nodes.len(),
        c.graph.edges.len(),
        truncated_note(c.graph.truncated, max_time.is_some()),
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

fn mdp_section(
    out: &mut String,
    analyzer: &Analyzer,
    desc: &str,
    goal: GoalPredicate,
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
) {
    let _span = tracing::info_span!("mdp").entered();
    outln!(out);
    outln!(out, "== mdp: {desc} ==");
    let mut config = MdpConfig::new(goal);
    config.rng_mode = opts.rng;
    config.max_time = max_time;
    config.prune = prune;
    // --max-states is the expansion bound everywhere: the game graph's node
    // budget here. max_actions/max_depth keep the crate defaults.
    config.max_nodes = opts.max_states.unwrap_or(DEFAULT_MAX_STATES);
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

fn pareto_section(
    out: &mut String,
    analyzer: &Analyzer,
    goal_desc: &str,
    goal: GoalPredicate,
    objectives: &[(String, Objective)],
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
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
    config.explore = explore_config(opts, prune, max_time);
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

fn audit_section(
    out: &mut String,
    analyzer: &mut Analyzer,
    opts: &Options,
    prune: bool,
    max_time: Option<Time>,
) {
    let _span = tracing::info_span!("audit").entered();
    outln!(out);
    outln!(out, "== audit ==");
    let report = audit(analyzer, &explore_config(opts, prune, max_time));
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
    for s in &shadowing {
        let verb = s.verb.as_ref().map_or_else(String::new, |v| format!("verb {v}: "));
        outln!(
            out,
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
    outln!(out, "unreachable content: {} finding(s)", unreachable.len());
    for u in &unreachable {
        let evidence = match u.evidence {
            Evidence::StaticallyProved => "statically proved",
            Evidence::NotWithinBounds => "not within the exploration bounds (bounds-conditional)",
        };
        outln!(out, "  {} {} — {evidence}", u.kind, u.id);
    }
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
