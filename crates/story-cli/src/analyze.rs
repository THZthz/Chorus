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

use story_analyze::pareto::Direction;
use story_analyze::query::RngMode;
use story_analyze::slice::SliceTarget;
use story_core::Time;

mod cli;
mod fmt;
mod run;
mod sections;

use cli::parse_args;
use run::run_analyses;

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

/// An assumption in `--assume` position.
#[derive(Clone, Debug)]
enum AssumptionArg {
    Card(String, u32),
    Legacy(String, i64),
    Aspect(String, String, i64),
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
    slice: Option<SliceTarget>,
    assumes: Vec<AssumptionArg>,
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
    samples: Option<usize>,
    seed_trace: Option<String>,
    radius: Option<usize>,
    envelope: bool,
    time_budget: Option<std::time::Duration>,
    r#continue: bool,
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::fmt::fraction_or_decimal;
    use super::run::turn_setting;

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
