//! The `analyze` command line: the `--help` reference plus option, goal,
//! slice, and assumption parsing.

use std::collections::BTreeSet;

use story_analyze::pareto::Direction;
use story_analyze::query::RngMode;
use story_analyze::slice::SliceTarget;
use story_core::{Time, VerbId};

use super::{AssumptionArg, GoalArg, ObjArg, Options};

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
MODULAR SLICING:
    --slice <target>     isolate one verb or arc for modular analysis:
                         verb:<id>            e.g. --slice verb:tend
                         arc:<v1,v2,...>      e.g. --slice arc:tend,furnace
    --assume <facts>     boundary assumptions (comma-separated):
                         card:<id>[=<n>]      e.g. card:coal=2
                         legacy:<key>=<n>     e.g. legacy:dawns_seen=1
                         aspect:<c>.<k>=<n>   e.g. aspect:beacon.heat=2
                         when <condition>     filter states during exploration
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
    --samples <n>        seed reachability search with <n> runtime rollouts
    --seed-trace <file>  seed reachability search with an external play trace
    --radius <k>         neighborhood radius around seeds (default 1 when seeded)
    --envelope           display the explored envelope coverage report
    --time-budget <time> wall-clock budget (e.g. 500ms, 10s, 1m); stops analysis
                         and saves progress when depleted
    --continue           resume analysis from saved progress
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

pub(super) fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut opts = Options {
        path: String::new(),
        slice: None,
        assumes: Vec::new(),
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
        samples: None,
        seed_trace: None,
        radius: None,
        envelope: false,
        time_budget: None,
        r#continue: false,
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
            "--slice" => {
                let v = take_value(inline, args, &mut i, "--slice")?;
                let target = parse_slice_arg(&v)?;
                match &mut opts.slice {
                    Some(SliceTarget::Verbs(set)) => match target {
                        SliceTarget::Verbs(more) => set.extend(more),
                    },
                    None => opts.slice = Some(target),
                }
            }
            "--assume" => {
                let v = take_value(inline, args, &mut i, "--assume")?;
                for piece in split_top_level_commas(&v) {
                    let p = piece.trim();
                    if !p.is_empty() {
                        opts.assumes.push(parse_assume_arg(p)?);
                    }
                }
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
            "--samples" => {
                let v = take_value(inline, args, &mut i, "--samples")?;
                let n: usize = v
                    .parse()
                    .map_err(|_| format!("--samples needs an unsigned integer; got `{v}`"))?;
                opts.samples = Some(n);
            }
            "--seed-trace" => {
                let v = take_value(inline, args, &mut i, "--seed-trace")?;
                opts.seed_trace = Some(v);
            }
            "--radius" => {
                let v = take_value(inline, args, &mut i, "--radius")?;
                if v == "inf" || v == "none" {
                    opts.radius = Some(usize::MAX);
                } else {
                    let n: usize = v
                        .parse()
                        .map_err(|_| format!("--radius needs an integer or `inf`; got `{v}`"))?;
                    opts.radius = Some(n);
                }
            }
            "--envelope" => {
                no_inline(inline, "--envelope")?;
                opts.envelope = true;
            }
            "--time-budget" => {
                let v = take_value(inline, args, &mut i, "--time-budget")?;
                opts.time_budget = Some(parse_wall_duration("--time-budget", &v)?);
            }
            "--continue" => {
                no_inline(inline, "--continue")?;
                opts.r#continue = true;
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
        && opts.slice.is_none()
    {
        return Err(
            "no analysis requested: give a goal selector (--reach, --classify, --mdp, --pareto), \
             --audit, or --slice"
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

fn parse_slice_arg(s: &str) -> Result<SliceTarget, String> {
    if let Some(verb) = s.strip_prefix("verb:") {
        let v = verb.trim();
        if v.is_empty() {
            return Err("verb: needs a verb id".to_string());
        }
        return Ok(SliceTarget::single(VerbId::from(v)));
    }
    if let Some(arc) = s.strip_prefix("arc:") {
        let verbs: BTreeSet<VerbId> = arc
            .split(',')
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(VerbId::from)
            .collect();
        if verbs.is_empty() {
            return Err("arc: needs at least one verb id".to_string());
        }
        return Ok(SliceTarget::arc(verbs));
    }
    Err(format!(
        "unknown slice target `{s}` — use verb:<id> or arc:<v1,v2,...>"
    ))
}

fn parse_assume_arg(s: &str) -> Result<AssumptionArg, String> {
    let s = s.trim();
    if let Some(card_part) = s.strip_prefix("card:") {
        let (name, count) = match card_part.split_once('=') {
            Some((name, cnt)) => {
                let n: u32 = cnt
                    .trim()
                    .parse()
                    .map_err(|_| format!("invalid card count in `{s}`"))?;
                if n == 0 {
                    return Err("card count must be positive".to_string());
                }
                (name.trim(), n)
            }
            None => (card_part.trim(), 1),
        };
        if name.is_empty() {
            return Err("card: needs a card name".to_string());
        }
        return Ok(AssumptionArg::Card(name.to_string(), count));
    }
    if let Some(leg_part) = s.strip_prefix("legacy:") {
        let Some((key, val)) = leg_part.split_once('=') else {
            return Err(format!("legacy assumption `{s}` must be legacy:<key>=<n>"));
        };
        let v: i64 = val
            .trim()
            .parse()
            .map_err(|_| format!("invalid legacy value in `{s}`"))?;
        return Ok(AssumptionArg::Legacy(key.trim().to_string(), v));
    }
    if let Some(asp_part) = s.strip_prefix("aspect:") {
        let Some((target_key, val)) = asp_part.split_once('=') else {
            return Err(format!(
                "aspect assumption `{s}` must be aspect:<card>.<key>=<n>"
            ));
        };
        let Some((card, key)) = target_key.split_once('.') else {
            return Err(format!(
                "aspect assumption `{s}` must specify card and key, e.g. aspect:beacon.heat=2"
            ));
        };
        let v: i64 = val
            .trim()
            .parse()
            .map_err(|_| format!("invalid aspect value in `{s}`"))?;
        return Ok(AssumptionArg::Aspect(
            card.trim().to_string(),
            key.trim().to_string(),
            v,
        ));
    }
    if let Some(cond) = s.strip_prefix("when ") {
        let cond = cond.trim();
        if cond.is_empty() {
            return Err("when: needs a condition".to_string());
        }
        return Ok(AssumptionArg::When(cond.to_string()));
    }
    Err(format!(
        "unknown assumption form `{s}` — use card:<id>[=<n>], legacy:<k>=<v>, aspect:<c>.<k>=<v>, or 'when <condition>'"
    ))
}

fn split_top_level_commas(s: &str) -> Vec<String> {
    let mut pieces = Vec::new();
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
    pieces
}

fn split_objectives(s: &str) -> Result<Vec<ObjArg>, String> {
    let mut objs = Vec::new();
    for piece in split_top_level_commas(s) {
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
pub(super) fn parse_duration(s: &str) -> Option<i64> {
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

pub(super) fn parse_wall_duration(flag: &str, s: &str) -> Result<std::time::Duration, String> {
    let t = s.trim();
    if let Some(ms) = t.strip_suffix("ms") {
        let n: u64 = ms
            .parse()
            .map_err(|_| format!("{flag} needs positive milliseconds; got `{s}`"))?;
        return Ok(std::time::Duration::from_millis(n));
    }
    if let Some(secs) = t.strip_suffix('s') {
        let n: u64 = secs
            .parse()
            .map_err(|_| format!("{flag} needs positive seconds; got `{s}`"))?;
        return Ok(std::time::Duration::from_secs(n));
    }
    if let Some(mins) = t.strip_suffix('m') {
        let n: u64 = mins
            .parse()
            .map_err(|_| format!("{flag} needs positive minutes; got `{s}`"))?;
        return Ok(std::time::Duration::from_secs(n * 60));
    }
    if let Some(hours) = t.strip_suffix('h') {
        let n: u64 = hours
            .parse()
            .map_err(|_| format!("{flag} needs positive hours; got `{s}`"))?;
        return Ok(std::time::Duration::from_secs(n * 3600));
    }
    if let Ok(n) = t.parse::<u64>() {
        return Ok(std::time::Duration::from_secs(n));
    }
    Err(format!(
        "{flag} needs a duration (e.g. 500ms, 5s, 1m, 1h, or bare seconds); got `{s}`"
    ))
}
