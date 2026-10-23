//! End-to-end tests for the story-cli binary: the play subcommand's
//! piped-stdin behavior (the bare file form is its alias), the analyze
//! subcommand's reports over the `tiny.sw` fixture and the `emberwatch.sw`
//! acceptance sample, usage errors and exit codes, and byte-identical
//! output across runs.

use std::io::Write;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_story-cli");
const TINY: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tiny.sw");
const EMBER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/emberwatch.sw");

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN).args(args).output().expect("run story-cli");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn play_with_stdin(args: &[&str], script: &str) -> (i32, String) {
    let mut child = Command::new(BIN)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn story-cli");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(script.as_bytes())
        .expect("write the play script");
    let out = child.wait_with_output().expect("wait for story-cli");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

// ---------------------------------------------------------------------------
// play
// ---------------------------------------------------------------------------

const SCRIPT: &str = "\
verbs
commit gamble
state
undo
state
quit
";

#[test]
fn play_drives_commit_state_and_undo_over_piped_stdin() {
    let (code, out) = play_with_stdin(&["play", TINY], SCRIPT);
    assert_eq!(code, 0);
    // The verb listing with recipe, priority, and duration.
    assert!(out.contains("tap committable instances 3: knock (priority 100, 10m)"), "{out}");
    // The commit settled its roll through the deterministic kernel path.
    assert!(out.contains("Nothing stirs (1)."), "{out}");
    assert!(out.contains("[sit 1] gamble.flip completed"), "{out}");
    // `state` sees the committed run; `undo` reverts the legacy write.
    assert!(out.contains("undone"), "{out}");
    assert!(out.contains("legacy: luck=0"), "{out}");
}

#[test]
fn the_bare_file_form_is_an_alias_for_play() {
    let (play_code, play_out) = play_with_stdin(&["play", TINY], SCRIPT);
    let (bare_code, bare_out) = play_with_stdin(&[TINY], SCRIPT);
    assert_eq!(play_code, 0);
    assert_eq!(bare_code, play_code);
    assert_eq!(bare_out, play_out);
}

// ---------------------------------------------------------------------------
// usage and exit codes
// ---------------------------------------------------------------------------

#[test]
fn usage_and_exit_codes() {
    // No arguments: usage on stderr, exit 2.
    let (code, out, err) = run(&[]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.contains("story-cli analyze <file.sw> [options]"), "{err}");

    // An unknown first token that names no file: usage, exit 2.
    let (code, _, err) = run(&["bogus"]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown subcommand"), "{err}");

    // help: usage on stdout, exit 0.
    let (code, out, _) = run(&["help"]);
    assert_eq!(code, 0);
    assert!(out.contains("story-cli play <file.sw>"), "{out}");
    assert!(out.contains("--reach <goal>"), "{out}");

    // analyze usage errors: exit 2, a message on stderr.
    let cases: &[&[&str]] = &[
        &["analyze"],                                             // no file
        &["analyze", TINY],                                       // no selector
        &["analyze", TINY, "--reach", "ending:lit", "--bogus"],   // unknown flag
        &["analyze", TINY, "--reach"],                            // missing value
        &["analyze", TINY, "--reach", "bogus:lit"],               // bad goal form
        &["analyze", TINY, "--reach", "ending:"],                 // empty ending id
        &["analyze", TINY, "--pareto", "min(clock)"],             // pareto without a goal
        &["analyze", TINY, "--mdp", "--rng", "adversarial"],      // mdp without any goal
        &["analyze", TINY, "--audit", "--max-states", "0"],       // nonpositive bound
        &["analyze", TINY, "--audit", "--max-time", "0"],         // nonpositive horizon
        &["analyze", TINY, "--audit", "--rng", "wild"],           // bad rng mode
        &["analyze", TINY, "--audit", "a.sw", "b.sw"],            // two files
    ];
    for case in cases {
        let (code, _, err) = run(case);
        assert_eq!(code, 2, "expected usage exit 2 for {case:?}");
        assert!(err.contains("story-cli analyze:"), "missing error for {case:?}: {err}");
    }
}

// ---------------------------------------------------------------------------
// analyze: per-subcommand help and goal validation
// ---------------------------------------------------------------------------

#[test]
fn analyze_help_prints_the_subcommand_options_and_exits_zero() {
    for flag in ["--help", "-h"] {
        let (code, out, err) = run(&["analyze", flag]);
        assert_eq!(code, 0, "{flag}");
        assert!(err.is_empty(), "{flag}: {err}");
        assert!(out.contains("story-cli analyze <file.sw> [options]"), "{flag}: {out}");
        assert!(out.contains("--reach <goal>"), "{flag}: {out}");
        assert!(out.contains("--mdp [<goal>]"), "{flag}: {out}");
        assert!(out.contains("--max-states <n>"), "{flag}: {out}");
        assert!(out.contains("--turn <duration>"), "{flag}: {out}");
        // --help wins over the other arguments.
        let (code, out, _) = run(&["analyze", flag, "--bogus"]);
        assert_eq!(code, 0, "{flag}");
        assert!(out.contains("--max-states <n>"), "{flag}: {out}");
    }
}

#[test]
fn an_undeclared_ending_id_is_a_usage_error_naming_the_declared_endings() {
    let (code, out, err) = run(&["analyze", EMBER, "--reach", "ending:nope"]);
    assert_eq!(code, 2);
    assert!(out.is_empty());
    assert!(err.contains("unknown ending `nope`"), "{err}");
    assert!(err.contains("declared endings: despair, gray_dawn, survived"), "{err}");

    // The --mdp goal selector validates against the same declarations.
    let (code, _, err) = run(&["analyze", EMBER, "--mdp", "ending:lit"]);
    assert_eq!(code, 2);
    assert!(err.contains("unknown ending `lit`"), "{err}");
}

// ---------------------------------------------------------------------------
// analyze: reach
// ---------------------------------------------------------------------------

#[test]
fn reach_finds_a_replay_verified_witness_on_emberwatch() {
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "ending:survived",
        "--max-states",
        "6000",
        "--max-time",
        "430m",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("Emberwatch — seed 7, turn 10m"), "{out}");
    assert!(out.contains("== reach: ending \"survived\" =="), "{out}");
    assert!(out.contains("outcome: REACHABLE"), "{out}");
    // The minimal line is one late blaze; every witness replays.
    assert!(out.contains("1 action(s)"), "{out}");
    assert!(out.contains("commit tend [] -> tend.blaze"), "{out}");
    assert!(out.contains("(replay-verified)"), "{out}");
    assert!(out.contains("final: the story ends \"survived\""), "{out}");
}

#[test]
fn condition_syntax_goals_compile_against_the_content_declarations() {
    // A condition over a unique card's aspect and the clock.
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "when beacon.heat >= 1 and clock >= 6h",
        "--max-states",
        "8000",
        "--max-time",
        "430m",
    ]);
    assert_eq!(code, 0);
    assert!(
        out.contains("== reach: when beacon.heat >= 1 and clock >= 6h =="),
        "{out}"
    );
    assert!(out.contains("outcome: REACHABLE"), "{out}");

    // A zone-scoped aggregate that already holds at boot: a zero-step
    // witness satisfies the goal at the start state.
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "when count(zone:tower, aspect:coal) >= 1",
        "--max-states",
        "100",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");
    assert!(out.contains("0 action(s), 0 transition(s)"), "{out}");
}

#[test]
fn an_unreachable_goal_exits_zero_and_answers_honestly() {
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "when legacy.luck >= 99",
        "--max-states",
        "5000",
        "--max-time",
        "30m",
    ]);
    assert_eq!(code, 0, "an unreachable answer is a completed analysis");
    assert!(
        out.contains("outcome: UNREACHABLE — no state within the bounds satisfies the goal"),
        "{out}"
    );
    assert!(out.contains("expansions:"), "{out}");
    // A card goal that holds at boot resolves with a zero-step witness.
    let (code, out, _) = run(&["analyze", TINY, "--reach", "card:stone", "--max-states", "100"]);
    assert_eq!(code, 0);
    assert!(out.contains("== reach: card \"stone\" =="), "{out}");
}

#[test]
fn diverse_reports_structurally_distinct_witnesses_with_exact_probabilities() {
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "ending:lit",
        "--rng",
        "optimistic",
        "--diverse",
        "5",
        "--max-states",
        "3000",
        "--max-time",
        "30m",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE — 3 witness(es)"), "{out}");
    assert!(out.contains("probability 1/2"), "{out}");
    assert!(out.contains("probability 1/4"), "{out}");
    assert!(out.contains("probability 1/8"), "{out}");
}

// ---------------------------------------------------------------------------
// analyze: classify
// ---------------------------------------------------------------------------

#[test]
fn classify_reports_winnability_on_tiny() {
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "ending:lit",
        "--classify",
        "--max-states",
        "2000",
        "--max-time",
        "30m",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("== classify =="), "{out}");
    assert!(out.contains("winning endings: \"lit\""), "{out}");
    assert!(out.contains("winnable states: 20 (the start is winnable)"), "{out}");
    assert!(out.contains("softlocked live states: 0"), "{out}");
    assert!(out.contains("explored: 20 states, 28 edges (within bounds)"), "{out}");
}

#[test]
fn classify_truncation_is_stated_never_silent() {
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "ending:survived",
        "--classify",
        "--max-states",
        "2000",
        "--max-time",
        "430m",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("TRUNCATED"), "{out}");
    assert!(out.contains("bounds-conditional"), "{out}");
}

// ---------------------------------------------------------------------------
// analyze: mdp and pareto
// ---------------------------------------------------------------------------

#[test]
fn mdp_reports_optimal_play_values_on_tiny() {
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--mdp",
        "when legacy.luck >= 1",
        "--rng",
        "optimistic",
        "--max-time",
        "30m",
    ]);
    assert_eq!(code, 0);
    // Three capped flips: the win probability under optimal play is 7/8.
    assert!(out.contains("== mdp: when legacy.luck >= 1 =="), "{out}");
    assert!(out.contains("V_max     = 7/8"), "{out}");
    assert!(out.contains("V_minimax"), "{out}");
    assert!(out.contains("V_unif"), "{out}");
    assert!(out.contains("expected actions under the optimal policy:"), "{out}");
    assert!(out.contains("(complete)"), "{out}");
}

#[test]
fn max_states_bounds_the_mdp_game_graph() {
    // --max-states is the game-graph node budget: the solver stops at the
    // bound and the report says the values are bounds-conditional.
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--mdp",
        "ending:survived",
        "--max-states",
        "500",
    ]);
    assert_eq!(code, 0);
    let line = out
        .lines()
        .find(|l| l.starts_with("game graph: "))
        .expect("the mdp report states its game-graph size");
    let nodes: usize = line
        .strip_prefix("game graph: ")
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .expect("a node count");
    assert!(nodes <= 500, "{line}");
    assert!(line.contains("TRUNCATED"), "{line}");
}

#[test]
fn pareto_reports_the_frontier_and_verified_cuts_on_tiny() {
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "ending:lit",
        "--pareto",
        "max(legacy.luck), min(clock)",
        "--rng",
        "optimistic",
        "--max-states",
        "2000",
        "--max-time",
        "30m",
    ]);
    assert_eq!(code, 0);
    assert!(
        out.contains(
            "== pareto: goal ending \"lit\", objectives max(legacy.luck), min(clock) =="
        ),
        "{out}"
    );
    assert!(out.contains("frontier: 1 point(s)"), "{out}");
    assert!(out.contains("mandatory recipes: gamble.flip, seal.light"), "{out}");
    assert!(out.contains("verified: no alternative path within the bounds"), "{out}");
}

// ---------------------------------------------------------------------------
// analyze: audit
// ---------------------------------------------------------------------------

#[test]
fn prune_flag_echoes_and_keeps_witnesses_honest() {
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "ending:survived",
        "--prune",
        "--max-states",
        "60000",
        "--max-time",
        "430m",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("prune on"), "{out}");
    // The witness-bearing answer is unaffected by the gate.
    assert!(out.contains("outcome: REACHABLE"), "{out}");
    assert!(out.contains("(replay-verified)"), "{out}");

    // Host-op-free content prunes; the report states the count when the
    // gate fired at all. (On this sample the survived line prunes nothing —
    // the assertion is the flag's honesty, not its fire rate.)
    let (code, out2, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "ending:survived",
        "--prune",
        "--max-states",
        "60000",
        "--max-time",
        "430m",
    ]);
    assert_eq!(code, 0);
    assert_eq!(out, out2, "prune output is deterministic");
}

#[test]
fn audit_reports_the_known_emberwatch_findings() {
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--audit",
        "--max-states",
        "8000",
        "--max-time",
        "430m",
    ]);
    assert_eq!(code, 0);
    // The report states the exploration size its findings are conditional on.
    let explored = out
        .lines()
        .find(|l| l.starts_with("explored: "))
        .expect("the audit report states its exploration size");
    assert!(explored.contains("states ("), "{explored}");
    assert!(explored.contains("TRUNCATED"), "{explored}");
    // Three semantic shadowing pairs: the bell's event rules, the log's
    // continue target, and feed ahead of polish.
    assert!(out.contains("shadowing: 3 finding(s)"), "{out}");
    assert!(out.contains("verb log: \"write\" shadows \"close\""), "{out}");
    assert!(out.contains("verb tend: \"feed\" shadows \"polish\""), "{out}");
    // The authored-dead recipes: the handbook (dawns_seen is only written
    // past the last dawn) and routine (polish wins the equal-priority scan).
    assert!(out.contains("recipe tend.handbook"), "{out}");
    assert!(out.contains("recipe tend.routine"), "{out}");
    // The gnaw-vs-decay race sits deeper in the night than this bound: no
    // fault is reachable within 8000 states, and the report says so.
    assert!(out.contains("reachable faults: 0 finding(s)"), "{out}");
}

// ---------------------------------------------------------------------------
// determinism and modifiers
// ---------------------------------------------------------------------------

#[test]
fn analyze_output_is_byte_identical_across_runs() {
    let args = [
        "analyze",
        EMBER,
        "--reach",
        "ending:survived",
        "--max-states",
        "6000",
        "--max-time",
        "430m",
    ];
    let (_, first, _) = run(&args);
    let (_, second, _) = run(&args);
    assert_eq!(first, second);
}

#[test]
fn audit_output_is_byte_identical_across_runs() {
    let args = [
        "analyze",
        EMBER,
        "--audit",
        "--max-states",
        "8000",
        "--max-time",
        "430m",
    ];
    let (_, first, _) = run(&args);
    let (_, second, _) = run(&args);
    assert_eq!(first, second);
}

#[test]
fn the_turn_override_reaches_the_boot_header() {
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "ending:gray_dawn",
        "--turn",
        "15m",
        "--max-states",
        "500",
        "--max-time",
        "500",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("Emberwatch — seed 7, turn 15m"), "{out}");
    assert!(out.contains("| turn 15m"), "{out}");
}
