//! The four CLI bins, exercised as subprocesses — argv parsing (verbatim
//! including the error texts), stream discipline, exit codes and the Q15
//! frozen quirks. This file pins the WIRING (real argv → real process):
//! usage exit 2, the trailing-`--demand` vanish, bad-demand exit 1, fuzz
//! flags/streams/exit, the reach-scale `--worker` NameStat interface, and
//! lint-scripts' constant-0 + missing-input exit 1.

use std::process::Command;

mod common;

fn run(bin: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let exe = match bin {
        "reach" => env!("CARGO_BIN_EXE_reach"),
        "reach-fuzz" => env!("CARGO_BIN_EXE_reach-fuzz"),
        "reach-scale" => env!("CARGO_BIN_EXE_reach-scale"),
        "lint-scripts" => env!("CARGO_BIN_EXE_lint-scripts"),
        other => panic!("unknown bin {other}"),
    };
    let out = Command::new(exe)
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the bin spawns");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

const SCRIPT: &str = "scripts/the-nightjar-flyer.txt";

/// The usage gate: no card, no --all, no
/// --demand → exit 2, the two-line usage text on stderr, stdout empty.
#[test]
fn reach_usage_exits_2_with_the_exact_text() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"]\n       reach <script.txt> --all\n"
    );
}

/// Q15 — a trailing `--demand` reads argv[idx+1] = undefined: the
/// demand silently vanishes and the run degrades to name mode.
#[test]
fn reach_trailing_demand_vanishes_into_name_mode_q15() {
    let (plain_code, plain_out, _) = run("reach", &[SCRIPT, "THE_WAX_CYLINDER"]);
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "THE_WAX_CYLINDER", "--demand"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(code, plain_code, "{stderr}");
    assert_eq!(stdout, plain_out, "the vanished demand must not change the output");
}

/// `--demand ''`: the EMPTY string is FALSY in JS, so
/// `!demand` holds and the usage gate exits 2 with the
/// exact two-line text (Rust used to treat Some("") as a provided demand and
/// rendered a demand report).
#[test]
fn reach_empty_string_demand_is_falsy_usage_exits_2() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "--demand", ""]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        "usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"]\n       reach <script.txt> --all\n"
    );
}

/// with a card name, `--demand ''` degrades to plain
/// NAME mode (the falsy demand never reaches analyze_card/render: the header
/// prints the card name, the run is the no-demand run).
#[test]
fn reach_empty_string_demand_degrades_to_name_mode() {
    let (plain_code, plain_out, _) = run("reach", &[SCRIPT, "THE_WAX_CYLINDER"]);
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "THE_WAX_CYLINDER", "--demand", ""]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(code, plain_code);
    assert_eq!(stdout, plain_out, "the empty demand must not change the output");
}

/// a bad demand query is an uncaught parse error: the
/// message line and exit 1 are the frozen part (nothing is printed after it
/// — the recorded divergence).
#[test]
fn reach_bad_demand_exits_1_with_the_query_error() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "--demand", "aspect:secret >= 3"]);
    assert_eq!(code, Some(1));
    assert_eq!(stdout, "");
    assert!(
        stderr.starts_with("Error: bad query term '>=' (expected kind:/aspect:/state:/name:)"),
        "{stderr}"
    );
}

/// Flags default 200/7/400/1 (overridden here for a
/// fast run); progress/summary on stdout; exit 0 with no failures.
#[test]
fn reach_fuzz_flags_streams_and_exit_zero_when_sound() {
    let (code, stdout, _stderr) = run(
        "reach-fuzz",
        &["--seeds", "3", "--from", "1", "--steps", "2", "--states", "60"],
    );
    assert_eq!(code, Some(0), "{stdout}");
    // the summary line — the `×`/`≤` text is the frozen shape (the seconds
    // field is timing)
    assert!(
        stdout.contains("\n3 seeds × ≤2 steps × ≤60 states: 0 failure(s), 0 inconclusive, busiest world "),
        "{stdout}"
    );
    assert!(stdout.contains(" duplicate push(es) deduped, "), "{stdout}");
    // 3 seeds < 50: no progress line
    assert!(!stdout.contains("seeds, 0 failures\n"), "{stdout}");
}

/// the per-50 progress line — `… 50/50 seeds, …`.
#[test]
fn reach_fuzz_progress_line_every_50_seeds() {
    let (code, stdout, _stderr) = run(
        "reach-fuzz",
        &["--seeds", "50", "--from", "1", "--steps", "1", "--states", "20"],
    );
    assert_eq!(code, Some(0), "{stdout}");
    assert!(stdout.contains("… 50/50 seeds, 0 failures"), "{stdout}");
}

/// A clean bundled script: exit 0 (warnings advise), the
/// `file: clean` line and the final count line.
#[test]
fn lint_scripts_is_clean_on_the_bundled_script_and_exits_zero() {
    let (code, stdout, stderr) = run("lint-scripts", &[SCRIPT]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(
        stdout,
        "scripts/the-nightjar-flyer.txt: clean\n\n1 file(s), 0 finding(s)\n"
    );
}

/// a missing input is an uncaught throw: exit 1 (the header's "exit
/// code stays 0" applies to findings, not I/O; the CURRENT behavior is frozen).
#[test]
fn lint_scripts_missing_input_exits_1() {
    let (code, stdout, stderr) = run("lint-scripts", &["nosuch-script.txt"]);
    assert_eq!(code, Some(1));
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("Error: "), "{stderr}");
}

/// the `--worker i k P R` sharded interface: names `idx % k === i` of a
/// freshly rebuilt world, NameStat JSON (name/ms/verdict/existence/roots/
/// nodes/depth in this key order) in tmpdir, exit 0.
#[test]
fn reach_scale_worker_shard_and_namestat_interface() {
    let (code, stdout, stderr) = run("reach-scale", &["--worker", "1", "3", "20", "15"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "");
    let path = std::env::temp_dir().join("reach-scale-1.json");
    let json = std::fs::read_to_string(&path).expect("the worker dumps its shard");
    // cleanup regardless of the assertions below
    let stats: serde_json::Value = serde_json::from_str(&json).expect("the shard parses");
    let _ = std::fs::remove_file(&path);
    let arr = stats.as_array().expect("a JSON array of NameStat");
    assert!(!arr.is_empty(), "shard 1 of 3 over 20 protos is non-empty");
    // the frozen key ORDER
    const FIELDS: [&str; 7] = ["name", "ms", "verdict", "existence", "roots", "nodes", "depth"];
    for st in arr {
        let obj = st.as_object().expect("an object");
        let keys: Vec<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(keys, FIELDS, "the NameStat key order is the A/B interface");
    }
    // the shard discipline: regenerate the same world here and compare the
    // name set (idx % 3 === 1 over world.names)
    let world = common::reach_scale_lib::build_cs_scale(20, 15);
    let mine: Vec<String> = world
        .names
        .iter()
        .enumerate()
        .filter(|(idx, _)| idx % 3 == 1)
        .map(|(_, n)| n.clone())
        .collect();
    let worker_names: Vec<String> = arr
        .iter()
        .map(|st| st["name"].as_str().expect("name").to_string())
        .collect();
    assert_eq!(worker_names, mine);
}
