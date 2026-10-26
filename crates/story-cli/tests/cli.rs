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
const HORIZON: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/story-cli/tests/fixtures/horizon.sw"
);
const HORIZON_BAD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../crates/story-cli/tests/fixtures/horizon-bad.sw"
);

fn run(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(BIN)
        .args(args)
        .output()
        .expect("run story-cli");
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
    assert!(
        out.contains("tap committable instances 3: knock (priority 100, 10m)"),
        "{out}"
    );
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
    assert!(
        err.contains("story-cli analyze <file.sw> [options]"),
        "{err}"
    );

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
        &["analyze"],                                           // no file
        &["analyze", TINY],                                     // no selector
        &["analyze", TINY, "--reach", "ending:lit", "--bogus"], // unknown flag
        &["analyze", TINY, "--reach"],                          // missing value
        &["analyze", TINY, "--reach", "bogus:lit"],             // bad goal form
        &["analyze", TINY, "--reach", "ending:"],               // empty ending id
        &["analyze", TINY, "--pareto", "min(clock)"],           // pareto without a goal
        &["analyze", TINY, "--mdp", "--rng", "adversarial"],    // mdp without any goal
        &["analyze", TINY, "--audit", "--max-states", "0"],     // nonpositive bound
        &["analyze", TINY, "--audit", "--max-time", "0"],       // nonpositive horizon
        &["analyze", TINY, "--audit", "--rng", "wild"],         // bad rng mode
        &["analyze", TINY, "--audit", "a.sw", "b.sw"],          // two files
    ];
    for case in cases {
        let (code, _, err) = run(case);
        assert_eq!(code, 2, "expected usage exit 2 for {case:?}");
        assert!(
            err.contains("story-cli analyze:"),
            "missing error for {case:?}: {err}"
        );
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
        assert!(
            out.contains("story-cli analyze <file.sw> [options]"),
            "{flag}: {out}"
        );
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
    assert!(
        err.contains("declared endings: despair, gray_dawn, survived"),
        "{err}"
    );

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
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "card:stone",
        "--max-states",
        "100",
    ]);
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
    assert!(
        out.contains("winnable states: 5 (the start is winnable)"),
        "{out}"
    );
    assert!(out.contains("softlocked live states: 0"), "{out}");
    assert!(
        out.contains("explored: 5 states, 4 edges (within bounds)"),
        "{out}"
    );
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
    assert!(
        out.contains("expected actions under the optimal policy:"),
        "{out}"
    );
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
        out.contains("== pareto: goal ending \"lit\", objectives max(legacy.luck), min(clock) =="),
        "{out}"
    );
    assert!(out.contains("frontier: 1 point(s)"), "{out}");
    assert!(
        out.contains("mandatory recipes: gamble.flip, seal.light"),
        "{out}"
    );
    assert!(
        out.contains("verified: no alternative path within the bounds"),
        "{out}"
    );
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
fn the_content_horizon_bounds_the_default_query() {
    // No --max-time: the game meta's horizon is the query envelope, and the
    // authored deadline recipe fires within it.
    let (code, out, _) = run(&["analyze", HORIZON, "--reach", "ending:dawn"]);
    assert_eq!(code, 0);
    assert!(out.contains("horizon 360m (content)"), "{out}");
    assert!(out.contains("outcome: REACHABLE"), "{out}");
    assert!(out.contains("final: the story ends \"dawn\""), "{out}");
}

#[test]
fn max_time_overrides_the_content_horizon() {
    let (code, out, _) = run(&[
        "analyze",
        HORIZON,
        "--reach",
        "ending:dawn",
        "--max-time",
        "30m",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("max-time 30m"), "{out}");
    assert!(!out.contains("horizon 360m"), "{out}");
}

#[test]
fn an_ill_spelled_horizon_warns_and_runs_bounded() {
    let (code, out, _) = run(&[
        "analyze",
        HORIZON_BAD,
        "--reach",
        "ending:dawn",
        "--max-states",
        "200",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("ignoring game meta `horizon`"), "{out}");
    assert!(out.contains("outcome: INCONCLUSIVE"), "{out}");
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
    assert!(
        out.contains("verb log: \"write\" shadows \"close\""),
        "{out}"
    );
    assert!(
        out.contains("verb tend: \"feed\" shadows \"polish\""),
        "{out}"
    );
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

// ---------------------------------------------------------------------------
// modular slicing
// ---------------------------------------------------------------------------

#[test]
fn analyze_slice_verb_reports_interface_boundaries() {
    let (code, out, _) = run(&["analyze", EMBER, "--slice", "verb:tend"]);
    assert_eq!(code, 0);
    assert!(out.contains("== slice: verb \"tend\" =="), "{out}");
    assert!(out.contains("guarantees (outputs to outside):"), "{out}");
    assert!(out.contains("card \"beacon\" has aspect \"heat\""), "{out}");
}

#[test]
fn analyze_slice_with_assume_and_reach() {
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--slice",
        "verb:omen_sight",
        "--assume",
        "card:portent=1",
        "--reach",
        "when warden.dread >= 1",
        "--max-states",
        "100",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("== slice: verb \"omen_sight\" =="), "{out}");
    assert!(out.contains("assumes (inputs from outside):"), "{out}");
    assert!(out.contains("card \"portent\" live"), "{out}");
    assert!(out.contains("applied assumptions:"), "{out}");
    assert!(out.contains("card:portent=1"), "{out}");
    assert!(out.contains("== reach: when warden.dread >= 1 =="), "{out}");
}

#[test]
fn analyze_slice_arc_reports_internal_and_output_facts() {
    let (code, out, _) = run(&["analyze", TINY, "--slice", "arc:gamble,seal"]);
    assert_eq!(code, 0);
    assert!(out.contains("== slice: arc [gamble, seal] =="), "{out}");
    assert!(out.contains("internal:"), "{out}");
    assert!(out.contains("legacy \"luck\" can change"), "{out}");
    assert!(out.contains("guarantees (outputs to outside):"), "{out}");
    assert!(out.contains("ending \"lit\" reached"), "{out}");
}

#[test]
fn analyze_slice_semantic_isolation_blocks_out_of_slice_verbs() {
    // Seal alone cannot reach ending:lit without gamble or luck.
    let (code_unreach, out_unreach, _) = run(&[
        "analyze",
        TINY,
        "--slice",
        "verb:seal",
        "--reach",
        "ending:lit",
        "--max-time",
        "1h",
    ]);
    assert_eq!(code_unreach, 0);
    assert!(out_unreach.contains("UNREACHABLE"), "{out_unreach}");

    // Providing the assumed legacy precondition allows seal alone to reach ending:lit.
    let (code_reach, out_reach, _) = run(&[
        "analyze",
        TINY,
        "--slice",
        "verb:seal",
        "--assume",
        "legacy:luck=1",
        "--reach",
        "ending:lit",
        "--max-time",
        "1h",
    ]);
    assert_eq!(code_reach, 0);
    assert!(out_reach.contains("outcome: REACHABLE"), "{out_reach}");
    assert!(out_reach.contains("1 action(s)"), "{out_reach}");
}

#[test]
fn analyze_slice_with_comma_condition_assumption() {
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--slice",
        "verb:seal",
        "--assume",
        "when count(world, kind:matter) >= 1, legacy:luck=1",
        "--reach",
        "ending:lit",
        "--max-time",
        "1h",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");
    assert!(out.contains("when count(world, kind:matter) >= 1"), "{out}");
    assert!(out.contains("legacy:luck=1"), "{out}");
}

#[test]
fn analyze_slice_unknown_verb_fails() {
    let (code, _, err) = run(&["analyze", EMBER, "--slice", "verb:nonexistent"]);
    assert_eq!(code, 2);
    assert!(
        err.contains("unknown verb `nonexistent` in --slice"),
        "{err}"
    );
}

#[test]
fn analyze_assume_unknown_card_fails() {
    let (code, _, err) = run(&[
        "analyze",
        EMBER,
        "--slice",
        "verb:tend",
        "--assume",
        "card:nonexistent",
    ]);
    assert_eq!(code, 2);
    assert!(
        err.contains("unknown card `nonexistent` in --assume"),
        "{err}"
    );
}

#[test]
fn analyze_trajectory_guided_search_with_envelope() {
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "ending:lit",
        "--samples",
        "3",
        "--radius",
        "1",
        "--envelope",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");
    assert!(out.contains("explored envelope:"), "{out}");
    assert!(out.contains("states:"), "{out}");
    assert!(out.contains("time: ["), "{out}");
    assert!(out.contains("mechanics:"), "{out}");
    assert!(out.contains("topology:"), "{out}");
}

#[test]
fn play_record_and_seed_trace_in_analyze() {
    let temp_trace = std::env::temp_dir().join(format!("story_trace_{}.json", std::process::id()));
    let temp_trace_str = temp_trace.to_str().expect("valid utf-8 path");

    let script = "commit gamble\ncommit gamble\ncommit gamble\nquit\n";
    let (code, _) = play_with_stdin(&["play", TINY, "--record", temp_trace_str], script);
    assert_eq!(code, 0);
    assert!(temp_trace.exists(), "trace file should be written");

    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "ending:lit",
        "--seed-trace",
        temp_trace_str,
        "--radius",
        "1",
        "--envelope",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");
    assert!(out.contains("explored envelope:"), "{out}");

    let _ = std::fs::remove_file(temp_trace);
}

#[test]
fn time_budget_stops_analysis_and_continue_resumes() {
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "ending:survived",
        "--time-budget",
        "5ms",
    ]);
    assert_eq!(code, 0);
    assert!(
        out.contains("outcome: INCONCLUSIVE — time budget depleted; run with --continue to resume"),
        "{out}"
    );
    assert!(out.contains("time-budget 5ms"), "{out}");

    // Running with --continue resumes from saved progress
    let (code, out, _) = run(&[
        "analyze",
        EMBER,
        "--reach",
        "ending:survived",
        "--time-budget",
        "15ms",
        "--continue",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("continue on"), "{out}");

    // Clean up residual checkpoint in temp dir
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("story_resume_")
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

#[test]
fn time_budget_caches_cold_boot_and_causal_graph() {
    let (code, out, _) = run(&["analyze", TINY, "--reach", "ending:lit", "--profile"]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");

    // Subsequent run skips causal graph construction
    let (code, _, err) = run(&["analyze", TINY, "--reach", "ending:lit", "--profile"]);
    assert_eq!(code, 0);
    assert!(
        !err.contains("causal_build"),
        "cached run should skip causal_build"
    );
}

#[test]
fn time_budget_multiple_flags_and_zero_budget() {
    // When multiple flags are passed, the last one takes precedence.
    // A 0 budget stops immediately at 0 expansions.
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "ending:lit",
        "--time-budget",
        "10s",
        "--time-budget",
        "0s",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("time-budget 0ms"), "{out}");
    assert!(
        out.contains("outcome: INCONCLUSIVE — time budget depleted"),
        "{out}"
    );
    assert!(
        out.contains("expansions: 0 (stopped at time budget)"),
        "{out}"
    );

    // Clean up the checkpoint created by this test
    let (code, out, _) = run(&["analyze", TINY, "--reach", "ending:lit", "--continue"]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");
}

#[test]
fn time_budget_resumes_multiple_times_and_cleans_up() {
    // 1st run: stops at budget
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "ending:lit",
        "--time-budget",
        "0s",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: INCONCLUSIVE"), "{out}");

    // 2nd run: resumes with another zero budget
    let (code, out, _) = run(&[
        "analyze",
        TINY,
        "--reach",
        "ending:lit",
        "--time-budget",
        "0s",
        "--continue",
    ]);
    assert_eq!(code, 0);
    assert!(out.contains("continue on"), "{out}");
    assert!(out.contains("outcome: INCONCLUSIVE"), "{out}");

    // 3rd run: completes to REACHABLE and removes resume checkpoint
    let (code, out, _) = run(&["analyze", TINY, "--reach", "ending:lit", "--continue"]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");
}

#[test]
fn cold_boot_recovers_from_corrupted_and_empty_cache() {
    // Ensure cache exists
    let (code, _, _) = run(&["analyze", TINY, "--reach", "ending:lit"]);
    assert_eq!(code, 0);

    // Corrupt all cache files in temp dir matching story_cache_
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("story_cache_")
            {
                let _ = std::fs::write(entry.path(), b"{\"corrupt\": [");
            }
        }
    }

    // Run again: recovers from corrupted cache and succeeds
    let (code, out, _) = run(&["analyze", TINY, "--reach", "ending:lit"]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");

    // Empty the cache files
    if let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) {
        for entry in entries.flatten() {
            if entry
                .file_name()
                .to_string_lossy()
                .starts_with("story_cache_")
            {
                let _ = std::fs::write(entry.path(), b"");
            }
        }
    }

    // Run again: recovers from empty cache file and succeeds
    let (code, out, _) = run(&["analyze", TINY, "--reach", "ending:lit"]);
    assert_eq!(code, 0);
    assert!(out.contains("outcome: REACHABLE"), "{out}");
}

// ---------------------------------------------------------------------------
// play: interactive help, mode toggling, and verbose output
// ---------------------------------------------------------------------------

#[test]
fn play_help_prints_subcommand_options_and_exits_zero() {
    for flag in ["--help", "-h"] {
        let (code, out, err) = run(&["play", flag]);
        assert_eq!(code, 0, "{flag}");
        assert!(err.is_empty(), "{flag}: {err}");
        assert!(
            out.contains("story-cli play <file.sw> [options]"),
            "{flag}: {out}"
        );
        assert!(out.contains("--verbose, -v"), "{flag}: {out}");
        assert!(out.contains("--available, -a"), "{flag}: {out}");
        assert!(out.contains("INTERACTIVE COMMANDS:"), "{flag}: {out}");
        assert!(out.contains("GAME LOOP:"), "{flag}: {out}");
    }
}

#[test]
fn play_verbose_flag_and_bare_alias_launch_in_verbose_mode() {
    let script = "quit\n";
    let (code, out) = play_with_stdin(&["play", TINY, "--verbose"], script);
    assert_eq!(code, 0);
    assert!(out.contains("[verbose]"), "{out}");
    assert!(out.contains("Initial Board State:"), "{out}");
    assert!(out.contains("Available Actions:"), "{out}");

    // Non-positional flag before filename
    let (code2, out2) = play_with_stdin(&["play", "--verbose", TINY], script);
    assert_eq!(code2, 0);
    assert!(out2.contains("[verbose]"), "{out2}");

    // Bare alias with -v
    let (code3, out3) = play_with_stdin(&[TINY, "-v"], script);
    assert_eq!(code3, 0);
    assert!(out3.contains("[verbose]"), "{out3}");
}

#[test]
fn play_interactive_help_and_subcommand_help() {
    let script = "help\nhelp commit\nhelp advance\nquit\n";
    let (code, out) = play_with_stdin(&["play", TINY], script);
    assert_eq!(code, 0);
    assert!(out.contains("Game Loop:"), "{out}");
    assert!(out.contains("commit <verb> [slot=card ...]"), "{out}");
    assert!(out.contains("advance [n]"), "{out}");
}

#[test]
fn play_in_repl_mode_switching_and_verbose_verbs() {
    let script = "mode verbose\nverbs\nmode normal\nquit\n";
    let (code, out) = play_with_stdin(&["play", EMBER], script);
    assert_eq!(code, 0);
    assert!(out.contains("mode set to verbose"), "{out}");
    // Verbose verbs display slot criteria
    assert!(out.contains("fuel: 1..2 consuming (aspect:coal)"), "{out}");
    assert!(
        out.contains("morsel: 1..2 consuming (aspect:restore)"),
        "{out}"
    );
    assert!(out.contains("mode set to normal"), "{out}");
}

#[test]
fn play_verbose_action_feedback() {
    let script = "commit sup morsel=bread#10\nadvance\nquit\n";
    let (code, out) = play_with_stdin(&["play", EMBER, "--verbose"], script);
    assert_eq!(code, 0);
    assert!(
        out.contains("[sit 3] sup.eat started (10m, ends at 10m)"),
        "{out}"
    );
    assert!(out.contains("morsel <- entity 10"), "{out}");
    assert!(
        out.contains("advanced 1 turn(s) (10m) -> clock is now 10m"),
        "{out}"
    );
    assert!(out.contains("sup.eat completed"), "{out}");
}

#[test]
fn play_filter_unrunnable_recipes_tiny() {
    let script = "\
verbs
verbs available
verbs -a
filter on
verbs
verbs all
filter off
verbs
quit
";
    let (code, out) = play_with_stdin(&["play", TINY], script);
    assert_eq!(code, 0);

    let parts: Vec<&str> = out.split('>').collect();
    // parts[0] is startup banner
    // parts[1] is output of `verbs` (unfiltered)
    assert!(
        parts[1].contains("gamble committable instances 3: flip"),
        "{out}"
    );
    assert!(
        parts[1].contains("tap committable instances 3: knock"),
        "{out}"
    );
    assert!(
        parts[1].contains("seal committable instances 1: light"),
        "{out}"
    );

    // parts[2] is output of `verbs available`
    // tap.knock (requires 99 matter) and seal.light (chain-only) must not appear
    assert!(
        parts[2].contains("gamble committable instances 3: flip"),
        "{out}"
    );
    assert!(!parts[2].contains("knock"), "{out}");
    assert!(!parts[2].contains("tap"), "{out}");
    assert!(!parts[2].contains("light"), "{out}");
    assert!(!parts[2].contains("seal"), "{out}");

    // parts[3] is output of `verbs -a`
    assert!(
        parts[3].contains("gamble committable instances 3: flip"),
        "{out}"
    );
    assert!(!parts[3].contains("knock"), "{out}");

    // parts[4] is output of `filter on`
    assert!(
        parts[4].contains("recipe filtering is on (only runnable recipes shown)"),
        "{out}"
    );

    // parts[5] is output of `verbs` with filter on
    assert!(
        parts[5].contains("gamble committable instances 3: flip"),
        "{out}"
    );
    assert!(!parts[5].contains("knock"), "{out}");
    assert!(!parts[5].contains("seal"), "{out}");

    // parts[6] is output of `verbs all` (overrides filter)
    assert!(
        parts[6].contains("gamble committable instances 3: flip"),
        "{out}"
    );
    assert!(
        parts[6].contains("tap committable instances 3: knock"),
        "{out}"
    );
    assert!(
        parts[6].contains("seal committable instances 1: light"),
        "{out}"
    );

    // parts[7] is output of `filter off`
    assert!(
        parts[7].contains("recipe filtering is off (all recipes shown)"),
        "{out}"
    );

    // parts[8] is output of `verbs` with filter off
    assert!(
        parts[8].contains("gamble committable instances 3: flip"),
        "{out}"
    );
    assert!(
        parts[8].contains("tap committable instances 3: knock"),
        "{out}"
    );
    assert!(
        parts[8].contains("seal committable instances 1: light"),
        "{out}"
    );
}

#[test]
fn play_available_flag_and_emberwatch_filtering() {
    // When booted with --available, salvage (needs debris) and distress (needs 3 peril) are filtered out
    let script = "verbs\nquit\n";
    let (code, out) = play_with_stdin(&["play", EMBER, "--available"], script);
    assert_eq!(code, 0);

    assert!(out.contains("tend committable instances 1:"), "{out}");
    assert!(out.contains("sup committable instances 1: eat"), "{out}");
    assert!(!out.contains("salvage"), "{out}");
    assert!(!out.contains("distress"), "{out}");

    // Also works with bare alias and -a
    let (code2, out2) = play_with_stdin(&[EMBER, "-a"], script);
    assert_eq!(code2, 0);
    assert!(out2.contains("tend committable instances 1:"), "{out2}");
    assert!(out2.contains("sup committable instances 1: eat"), "{out2}");
    assert!(!out2.contains("salvage"), "{out2}");
    assert!(!out2.contains("distress"), "{out2}");
}
