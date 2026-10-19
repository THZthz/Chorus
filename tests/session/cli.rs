//! The five CLI bins, exercised as subprocesses — argv parsing (verbatim
//! including the error texts), stream discipline, exit codes and the Q15
//! honesty fixes. This file pins the WIRING (real argv → real process):
//! the reach Commands+Options grammar (each subcommand dispatching, the
//! full `--help` text, the argv-error faces, the `--color` opt-in and its
//! byte-identity default), bad-demand exit 1, fuzz flags/streams/exit, the
//! reach-scale `--worker` NameStat interface, and lint-scripts' pieces: the
//! constant-0 + missing-input exits and the single-purpose Options grammar
//! (the full-help pin, the unknown-flag
//! exit-2 face, the `--color` opt-in over the lint faces, the sorted
//! default list). The chorus bin keeps the same family: `--help`/`-h`
//! anywhere, the short-usage argv errors, and the script path as the one
//! positional.

use std::process::{Command, Stdio};

fn run(bin: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let exe = match bin {
        "reach" => env!("CARGO_BIN_EXE_reach"),
        "reach-fuzz" => env!("CARGO_BIN_EXE_reach-fuzz"),
        "reach-scale" => env!("CARGO_BIN_EXE_reach-scale"),
        "lint-scripts" => env!("CARGO_BIN_EXE_lint-scripts"),
        "chorus" => env!("CARGO_BIN_EXE_chorus"),
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

/// The short usage — the no-arguments face and the tail of every argv
/// error: one usage line plus the `reach --help` hint (the cargo/uv
/// convention), stderr, exit 2.
const SHORT_USAGE: &str =
    "usage: reach <script.txt> <COMMAND> [OPTIONS]\ntry 'reach --help' for the command list\n";

/// lint-scripts' short usage — the tail of its argv error (the unknown
/// flag face), stderr, exit 2.
const LINT_SHORT_USAGE: &str =
    "usage: lint-scripts [OPTIONS] [FILES...]\ntry 'lint-scripts --help' for the options\n";

/// The six legal command words, in help order — what every
/// missing/unknown-command error lists.
const COMMAND_LIST: &str = "card, all, demand, verb, world, graph";

/// No arguments at all → the short usage, exit 2, stdout empty.
#[test]
fn reach_no_arguments_is_the_short_usage_exit_2() {
    let (code, stdout, stderr) = run("reach", &[]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(stderr, SHORT_USAGE);
}

/// The usage gate, new grammar: a script with NO command word → exit 2,
/// the command list named, the short usage following.
#[test]
fn reach_script_without_a_command_lists_the_commands() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("reach needs a command — expected one of: {COMMAND_LIST}\n{SHORT_USAGE}")
    );
}

/// An unknown command word → exit 2 naming it, the command list, the
/// short usage. A `-`-spelled unknown word takes the unknown-FLAG face
/// (reach_unknown_flag_names_the_word_and_exits_2) — a non-`-` word is
/// the unknown command.
#[test]
fn reach_unknown_command_names_the_word_and_lists_the_commands() {
    for word in ["frobnicate", "World", "cards"] {
        let (code, stdout, stderr) = run("reach", &[SCRIPT, word]);
        assert_eq!(code, Some(2), "{word}");
        assert_eq!(stdout, "", "{word}");
        assert_eq!(
            stderr,
            format!("unknown command '{word}' — expected one of: {COMMAND_LIST}\n{SHORT_USAGE}"),
            "{word}"
        );
    }
}

/// `--help` (and `-h`) anywhere in argv prints the full help on stdout and
/// exits 0 — pinned byte for byte (the lint-scripts plain-text style).
#[test]
fn reach_help_is_the_frozen_full_text_exit_0() {
    let expected = "\
reach — static card reachability over a compiled script

usage: reach <script.txt> <COMMAND> [OPTIONS]
try 'reach --help' for the command list

Commands:
  card <NAME>      one card's verdict: proven paths and bindable slots
  all              the verdict for every name
  demand <QUERY>   the cards whose aspects/properties match a slot query like aspect:secret>=2
  verb <NAME>      one verb's report
  world            the whole-world dashboard
  graph            the mint dependency graph as Graphviz DOT

Options:
  --usable             also require bindability into a committable verb slot
  --why                append the per-site blocker table (card/all/demand)
  --cost               append the cost/count section (card/all; ignored under demand)
  --json               machine-readable output (card/all/demand; world/verb/graph ignore it)
  --by <MINUTE>        bind every time-keyed judgment to absolute minute MINUTE
  --from-state <LOG>   replay a JSONL action log as the analysis initial world
  --color              ANSI color (default off; when off the output is byte-identical to the plain face)
  -h, --help           print help

  a value option (--by, --from-state) repeated on one line: the last occurrence wins

examples:
  reach scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --why
  reach scripts/the-nightjar-flyer.txt all --cost --by 600
  reach scripts/the-nightjar-flyer.txt world
";
    for argv in [vec!["--help"], vec!["-h"], vec![SCRIPT, "card", "X", "--help"]] {
        let (code, stdout, stderr) = run("reach", &argv);
        assert_eq!(code, Some(0), "{argv:?}");
        assert_eq!(stdout, expected, "{argv:?}");
        assert_eq!(stderr, "", "{argv:?}");
    }
}

/// The six subcommands each dispatch to their face over the bundled
/// script: exit 0 and the face's own head/marker on stdout.
#[test]
fn reach_card_command_dispatches_the_single_card_face() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "card", "THE_WAX_CYLINDER"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(
        stdout.contains("scripts/the-nightjar-flyer.txt · card THE_WAX_CYLINDER\n"),
        "{stdout}"
    );
    assert!(stdout.contains("  verdict: "), "{stdout}");
}

#[test]
fn reach_all_command_dispatches_the_walk() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "all"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("scripts/the-nightjar-flyer.txt · card "), "{stdout}");
    // the walk: more than one report in the sweep
    assert!(stdout.matches("  verdict: ").count() > 1, "{stdout}");
}

#[test]
fn reach_demand_command_dispatches_the_query_face() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "demand", "aspect:secret>=2"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains(" · card matching 'aspect:secret>=2'\n"), "{stdout}");
}

#[test]
fn reach_verb_command_dispatches_the_verb_report() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "verb", "WORK"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.starts_with("scripts/the-nightjar-flyer.txt · verb WORK\n"), "{stdout}");
}

#[test]
fn reach_world_command_dispatches_the_dashboard() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "world"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.starts_with("scripts/the-nightjar-flyer.txt · world\n"), "{stdout}");
    assert!(stdout.contains("  overview: "), "{stdout}");
    assert!(stdout.contains("  freeze: "), "{stdout}");
}

#[test]
fn reach_graph_command_dispatches_the_dot_face() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "graph"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.starts_with("digraph reach {\n"), "{stdout}");
}

/// The `--color` opt-in: ON → the same face with ANSI escapes; OFF → the
/// byte-identical plain face. The machine faces (graph, --json) never
/// color, even under --color.
#[test]
fn reach_color_is_opt_in_and_the_machine_faces_never_color() {
    // off (the default): no escapes anywhere, byte-identical to the plain face
    let (code, plain, stderr) = run("reach", &[SCRIPT, "card", "THE_WAX_CYLINDER"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(!plain.contains("\x1b["), "{plain}");
    let (code, on, _) = run("reach", &[SCRIPT, "card", "THE_WAX_CYLINDER", "--color"]);
    assert_eq!(code, Some(0));
    assert!(on.contains("\x1b["), "the colored face carries escapes");
    assert!(on.contains("verdict: "), "{on}");
    // the world face colors too
    let (code, world_on, _) = run("reach", &[SCRIPT, "world", "--color"]);
    assert_eq!(code, Some(0));
    assert!(world_on.contains("\x1b["), "{world_on}");
    // graph and --json stay their machine selves under --color
    let (code, graph_plain, _) = run("reach", &[SCRIPT, "graph"]);
    assert_eq!(code, Some(0));
    let (code, graph_color, _) = run("reach", &[SCRIPT, "graph", "--color"]);
    assert_eq!(code, Some(0));
    assert_eq!(graph_plain, graph_color, "the DOT face is never colorized");
    assert!(!graph_color.contains("\x1b["), "{graph_color}");
    let (code, json_color, _) =
        run("reach", &[SCRIPT, "card", "THE_WAX_CYLINDER", "--json", "--color"]);
    assert_eq!(code, Some(0));
    assert!(!json_color.contains("\x1b["), "{json_color}");
    assert!(json_color.trim_start().starts_with('{'), "{json_color}");
}

/// Q15 in the new grammar — a trailing `demand` command with no value slot
/// is the argv error naming the command, the short usage following, exit 2.
#[test]
fn reach_trailing_demand_is_an_argv_error_q15() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "demand"]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("demand needs a value: reach <script.txt> demand \"<query>\"\n{SHORT_USAGE}")
    );
}

/// `demand ''`: the EMPTY string value is FALSY (the frozen truthiness, now
/// per command) and reads as a missing value — the argv error, exit 2.
#[test]
fn reach_empty_string_demand_is_falsy_argv_error() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "demand", ""]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("demand needs a value: reach <script.txt> demand \"<query>\"\n{SHORT_USAGE}")
    );
    // the same truthiness for card and verb
    for (cmd, shape) in [("card", "<NAME>"), ("verb", "\"<NAME>\"")] {
        let (code, stdout, stderr) = run("reach", &[SCRIPT, cmd, ""]);
        assert_eq!(code, Some(2), "{cmd}");
        assert_eq!(stdout, "", "{cmd}");
        assert_eq!(
            stderr,
            format!("{cmd} needs a value: reach <script.txt> {cmd} {shape}\n{SHORT_USAGE}"),
            "{cmd}"
        );
    }
}

/// the value-flag errors keep the Q15 wordings: `--by` with no value and
/// with a non-integer value, `--from-state` with no value — exit 2 naming
/// the flag, the short usage following.
#[test]
fn reach_value_flag_errors_name_the_flag() {
    for (argv, head) in [
        (vec![SCRIPT, "--by"], "--by needs a value: --by <minute>"),
        (vec![SCRIPT, "--by", "soon"], "--by needs an integer minute: --by <minute>"),
        (vec![SCRIPT, "card", "X", "--by"], "--by needs a value: --by <minute>"),
        (vec![SCRIPT, "--from-state"], "--from-state needs a value: --from-state <log.jsonl>"),
    ] {
        let (code, stdout, stderr) = run("reach", &argv);
        assert_eq!(code, Some(2), "{argv:?}");
        assert_eq!(stdout, "", "{argv:?}");
        assert_eq!(stderr, format!("{head}\n{SHORT_USAGE}"), "{argv:?}");
    }
}

/// a bad demand query is an uncaught parse error: the
/// message line and exit 1 are the frozen part (nothing is printed after it
/// — the recorded divergence).
#[test]
fn reach_bad_demand_exits_1_with_the_query_error() {
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "demand", "aspect:secret >= 3"]);
    assert_eq!(code, Some(1));
    assert_eq!(stdout, "");
    assert!(
        stderr.starts_with("Error: bad query term '>=' (expected kind:/aspect:/state:/name:)"),
        "{stderr}"
    );
}

/// an unknown `-`-word in flag position is the argv error naming it — a
/// typo'd `--usablee` must never silently drop its flag and change the
/// face's semantics, and `--` is no end-of-options separator on this
/// surface (the lint-scripts alignment): every case exits 2 with the word
/// named and the short usage following.
#[test]
fn reach_unknown_flag_names_the_word_and_exits_2() {
    for (argv, word) in [
        (vec![SCRIPT, "card", "THE_WAX_CYLINDER", "--typo"], "--typo"),
        (vec![SCRIPT, "card", "THE_WAX_CYLINDER", "--usablee"], "--usablee"),
        (vec![SCRIPT, "--typo", "all"], "--typo"),
        (vec![SCRIPT, "all", "-x"], "-x"),
        (vec![SCRIPT, "card", "--", "--weird"], "--"),
    ] {
        let (code, stdout, stderr) = run("reach", &argv);
        assert_eq!(code, Some(2), "{argv:?}");
        assert_eq!(stdout, "", "{argv:?}");
        assert_eq!(
            stderr,
            format!("unknown flag '{word}' — expected one of: --usable, --why, --cost, --json, --by <MINUTE>, --from-state <LOG>, --color, -h/--help\n{SHORT_USAGE}"),
            "{argv:?}"
        );
    }
}

/// extra positionals beyond `<script> <COMMAND> [value]` are the argv
/// error naming the FIRST extra — no silently-ignored word beside the
/// command (`all extra --json`, `world extra stuff`, `card X Y`).
#[test]
fn reach_extra_positionals_are_argv_errors() {
    for (argv, extra) in [
        (vec![SCRIPT, "all", "extra", "--json"], "extra"),
        (vec![SCRIPT, "world", "extra", "stuff"], "extra"),
        (vec![SCRIPT, "graph", "bogus"], "bogus"),
        (vec![SCRIPT, "card", "X", "Y"], "Y"),
        (vec![SCRIPT, "demand", "kind:resource", "Z"], "Z"),
    ] {
        let (code, stdout, stderr) = run("reach", &argv);
        assert_eq!(code, Some(2), "{argv:?}");
        assert_eq!(stdout, "", "{argv:?}");
        assert_eq!(stderr, format!("unexpected argument '{extra}'\n{SHORT_USAGE}"), "{argv:?}");
    }
}

/// a value flag's next slot must be a VALUE, not another flag —
/// `--from-state --why` names the flag instead of reading a log named
/// `--why` (the baffling "cannot read --why" face). `--by` is the
/// deliberate exception: a negative minute is a value, not a flag.
#[test]
fn reach_value_slots_reject_flag_looking_tokens() {
    let (code, stdout, stderr) =
        run("reach", &[SCRIPT, "card", "THE_GEM", "--from-state", "--why"]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(
        stderr,
        format!("--from-state needs a value, not '--why': --from-state <log.jsonl>\n{SHORT_USAGE}")
    );
    // the --by exemption: a negative minute parses, binds, annotates the head
    let (code, stdout, stderr) = run("reach", &[SCRIPT, "card", "THE_LATE_LORD", "--by", "-5"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains(" · card THE_LATE_LORD  [by minute -5]\n"), "{stdout}");
}

// ---------------------------------------------------------------------------
// chorus — the line REPL's argv face (the same family grammar)
// ---------------------------------------------------------------------------

/// chorus' short usage — the no-arguments face and the tail of every argv
/// error: the usage line with the meta words, plus the `chorus --help`
/// hint, stderr, exit 2.
const CHORUS_SHORT_USAGE: &str = "usage: chorus <script.txt>\n       a line REPL: type what happens, or help/look/cards/story/journal/save/undo/wait/inspect/quit\ntry 'chorus --help' for how to play\n";

/// `--help` (and `-h`) anywhere in argv prints the full help on stdout and
/// exits 0 — pinned byte for byte, the reach/lint-scripts plain-text style.
#[test]
fn chorus_help_is_the_frozen_full_text_exit_0() {
    let expected = "\
chorus — the line REPL: a script file played as a text game

usage: chorus <script.txt>
try 'chorus --help' for how to play

The player types plain English-ish sentences; the banner's `try:` row
lists what the game understands right now. Meta words (typed as-is):

  look                the place, the clock, what is here and carried
  cards               the visible table
  inspect <card>      one card's face; a bare `inspect` prints usage + the table
  story               the prose so far
  journal             the canonical log
  save <file.jsonl>   write the action log — `reach --from-state <file>` replays it
  undo                take back the last turn
  wait                jump to the next due completion (or one hour)
  help                how to play
  quit                leave

Options:
  -h, --help  print help

examples:
  chorus scripts/the-nightjar-flyer.txt
  printf 'look\\nread the manifest\\nsave /tmp/log.jsonl\\nquit\\n' | chorus scripts/the-nightjar-flyer.txt
  reach scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --from-state /tmp/log.jsonl
";
    for argv in
        [vec!["--help"], vec!["-h"], vec![SCRIPT, "-h"], vec![SCRIPT, "card", "X", "--help"]]
    {
        let (code, stdout, stderr) = run("chorus", &argv);
        assert_eq!(code, Some(0), "{argv:?}");
        assert_eq!(stdout, expected, "{argv:?}");
        assert_eq!(stderr, "", "{argv:?}");
    }
}

/// No arguments at all → the short usage, exit 2, stdout empty.
#[test]
fn chorus_no_arguments_is_the_short_usage_exit_2() {
    let (code, stdout, stderr) = run("chorus", &[]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(stderr, CHORUS_SHORT_USAGE);
}

/// An unknown `-`-flag (longer than one char) is the argv error naming it —
/// never a silent fallthrough into a script read of the flag word.
#[test]
fn chorus_unknown_flag_names_the_word_and_exits_2() {
    for flag in ["--typo", "-x", "--color", "--reach"] {
        let (code, stdout, stderr) = run("chorus", &[flag]);
        assert_eq!(code, Some(2), "{flag}");
        assert_eq!(stdout, "", "{flag}");
        assert_eq!(stderr, format!("unknown flag '{flag}'\n{CHORUS_SHORT_USAGE}"), "{flag}");
        // the flag loses to nothing — the error fires wherever it sits
        let (code, stdout, stderr) = run("chorus", &[SCRIPT, flag]);
        assert_eq!(code, Some(2), "{flag}");
        assert_eq!(stdout, "", "{flag}");
        assert_eq!(stderr, format!("unknown flag '{flag}'\n{CHORUS_SHORT_USAGE}"), "{flag}");
    }
}

/// Extra positionals beyond the script path name the FIRST extra —
/// `chorus script.txt card X` is not a silent two-word drop.
#[test]
fn chorus_extra_positionals_are_argv_errors() {
    for (argv, extra) in [
        (vec![SCRIPT, "extra"], "extra"),
        (vec![SCRIPT, "card", "X"], "card"),
        (vec![SCRIPT, "extra", "stuff"], "extra"),
    ] {
        let (code, stdout, stderr) = run("chorus", &argv);
        assert_eq!(code, Some(2), "{argv:?}");
        assert_eq!(stdout, "", "{argv:?}");
        assert_eq!(
            stderr,
            format!("unexpected argument '{extra}'\n{CHORUS_SHORT_USAGE}"),
            "{argv:?}"
        );
    }
}

/// A save whose path cannot be written (the parent directory is missing) is
/// the bin's error face: the piped transcript stops after the echo, the
/// `Error: cannot write …` line goes to stderr, exit 1 — the emit_all
/// convention over `Game::save`'s `Result` channel.
#[test]
fn chorus_save_failure_is_the_error_line_exit_1() {
    let dir = std::env::temp_dir().join(format!("chorus-save-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir); // the save below needs the dir MISSING
    let log = dir.join("x.jsonl");
    let session = format!("look\nsave {}\nquit\n", log.display());
    let out = Command::new(env!("CARGO_BIN_EXE_chorus"))
        .arg(SCRIPT)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.as_mut().expect("piped stdin").write_all(session.as_bytes())?;
            child.wait_with_output()
        })
        .expect("the chorus bin runs");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(out.stderr).expect("stderr is UTF-8");
    assert!(stdout.contains(&format!("\u{bb} save {}", log.display())), "{stdout}");
    assert!(stderr.starts_with("Error: cannot write "), "{stderr}");
}

/// A bare `-` is one char long — no flag is one char long on this surface —
/// so it stays a script path (here: a missing one, the hard input error,
/// exit 1), never the unknown-flag face.
#[test]
fn chorus_single_dash_is_a_script_path_not_a_flag() {
    let (code, stdout, stderr) = run("chorus", &["-"]);
    assert_eq!(code, Some(1));
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("Error: "), "{stderr}");
}

/// An empty argument is filtered out before the positional scan — `chorus
/// ""` is the no-arguments face, not a doomed read of an empty path.
#[test]
fn chorus_empty_argument_is_the_no_arguments_face() {
    let (code, stdout, stderr) = run("chorus", &[""]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(stderr, CHORUS_SHORT_USAGE);
}

/// Flags default 200/7/400/1 (overridden here for a
/// fast run); progress/summary on stdout; exit 0 with no failures.
#[test]
fn reach_fuzz_flags_streams_and_exit_zero_when_sound() {
    let (code, stdout, _stderr) =
        run("reach-fuzz", &["--seeds", "3", "--from", "1", "--steps", "2", "--states", "60"]);
    assert_eq!(code, Some(0), "{stdout}");
    // the summary line — the `×`/`≤` text is the frozen shape (the seconds
    // field is timing)
    assert!(
        stdout.contains(
            "\n3 seeds × ≤2 steps × ≤60 states: 0 failure(s), 0 inconclusive, busiest world "
        ),
        "{stdout}"
    );
    assert!(stdout.contains(" duplicate push(es) deduped, "), "{stdout}");
    // 3 seeds < 50: no progress line
    assert!(!stdout.contains("seeds, 0 failures\n"), "{stdout}");
}

/// the per-50 progress line — `… 50/50 seeds, …` — plus the failure-set pin
/// over seeds 1..=50 under these bounds. The set held exactly {41} while the
/// Complete arm was over-generous ("home obtainable ⇒ the window is
/// arrangeable"); the Wave-3 tightening narrowed it to empty; the v8
/// generator stream (the recipe `dur` band re-drawing every later draw)
/// surfaced exactly {37} — a false-UNOBTAINABLE class judged PRE-EXISTING:
/// the seed-37 scenario fails identically with every recipe `dur` stripped
/// (a content shape where the per-recipe machinery is inert), so the class
/// lives in the analyzer's demotion model, not the dur work. The v9
/// generator stream (the §6.3 sum scopes re-drawing every `when`) leaves
/// this shallow window EMPTY again — the campaign still carries the class
/// at default bounds (failing seeds 169/236 over seeds 1..=300; seed 169's
/// failure reproduces identically with every sum-scope `when` stripped,
/// judged PRE-EXISTING the same way), so the set is the campaign's open
/// ledger: a non-empty set here (a new class, a widened one, or a class
/// surfacing inside the window) breaks this pin and must be re-judged,
/// never silently absorbed.
#[test]
fn reach_fuzz_progress_line_every_50_seeds() {
    let (code, stdout, stderr) =
        run("reach-fuzz", &["--seeds", "50", "--from", "1", "--steps", "1", "--states", "20"]);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    assert!(stdout.contains("… 50/50 seeds, 0 failures"), "{stdout}");
    assert!(
        stdout.contains("\n50 seeds × ≤1 steps × ≤20 states: 0 failure(s), 0 inconclusive, "),
        "{stdout}"
    );
    assert!(!stdout.contains("failing seeds:"), "{stdout}");
    assert!(!stderr.contains("FAIL "), "{stderr}");
}

/// A clean bundled script: exit 0 (warnings advise), the
/// `file: clean` line and the final count line.
#[test]
fn lint_scripts_is_clean_on_the_bundled_script_and_exits_zero() {
    let (code, stdout, stderr) = run("lint-scripts", &[SCRIPT]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, "scripts/the-nightjar-flyer.txt: clean\n\n1 file(s), 0 finding(s)\n");
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

/// Q15 — the default (no-argument) run reads `scripts/` relative to the
/// cwd; a missing directory NAMES itself and the cwd (the bare io error
/// never mentioned either). Exit 1 unchanged: a hard input error.
#[test]
fn lint_scripts_missing_scripts_dir_names_the_directory() {
    let dir = std::env::temp_dir().join(format!("chorus-lint-q15-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch cwd");
    // canonicalized: getcwd() reports the physical path (macOS /var is a
    // symlink to /private/var) and the bin's message quotes current_dir()
    let dir = dir.canonicalize().expect("the physical path");
    let out = Command::new(env!("CARGO_BIN_EXE_lint-scripts"))
        .current_dir(&dir)
        .output()
        .expect("the bin spawns");
    let _ = std::fs::remove_dir(&dir);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stdout), "");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.starts_with(&format!("Error: no 'scripts' directory under '{}'", dir.display())),
        "{stderr}"
    );
    assert!(stderr.contains("name files: lint-scripts [file.txt ...]"), "{stderr}");
}

/// v0.8.0 — lint-scripts' single-purpose Options grammar: `--help`/`-h`
/// anywhere prints the full help on stdout and exits 0 — pinned byte for
/// byte (the option table with the `--reach` advisory note, three real
/// examples).
#[test]
fn lint_scripts_help_is_the_frozen_full_text_exit_0() {
    let expected = "\
lint-scripts — the script lint (§13.2) over named or bundled scripts

usage: lint-scripts [OPTIONS] [FILES...]

Args:
  FILES...    the scripts to lint; the default (no FILES) reads every scripts/*.txt relative to the CWD

Options:
  --reach     append the reach bridge's design-smell findings (the §13.1 → §13.2 six families; advisory — a reach finding never changes the exit code)
  --color     ANSI color (default off; when off the output is byte-identical to the plain face)
  -h, --help  print help

examples:
  lint-scripts
  lint-scripts --reach scripts/the-nightjar-flyer.txt
  lint-scripts --color scripts/the-nightjar-flyer.txt
";
    for argv in [vec!["--help"], vec!["-h"], vec![SCRIPT, "--help"], vec!["--reach", "--help"]] {
        let (code, stdout, stderr) = run("lint-scripts", &argv);
        assert_eq!(code, Some(0), "{argv:?}");
        assert_eq!(stdout, expected, "{argv:?}");
        assert_eq!(stderr, "", "{argv:?}");
    }
}

/// v0.8.0 — an unknown `-`-flag is the argv error: exit 2 on stderr naming
/// the flag, the short usage following — never a silent fallthrough into
/// the file list and a bogus read error (the Q15 honesty tradition). Any
/// non-flag word is STILL a file path (the frozen semantics): a missing one
/// is the hard input error, exit 1, unchanged.
#[test]
fn lint_scripts_unknown_flag_is_an_argv_error_exit_2() {
    for flag in ["--foo", "--reac", "--color=always", "-x"] {
        let (code, stdout, stderr) = run("lint-scripts", &[flag]);
        assert_eq!(code, Some(2), "{flag}");
        assert_eq!(stdout, "", "{flag}");
        assert_eq!(stderr, format!("unknown flag '{flag}'\n{LINT_SHORT_USAGE}"), "{flag}");
    }
    // unknown flags lose to nothing — the error fires wherever they sit
    let (code, stdout, stderr) = run("lint-scripts", &["--reach", "--foo", SCRIPT]);
    assert_eq!(code, Some(2));
    assert_eq!(stdout, "");
    assert_eq!(stderr, format!("unknown flag '--foo'\n{LINT_SHORT_USAGE}"));
}

/// v0.8.0 — the `--color` opt-in over the lint faces: ON → the same faces
/// with ANSI escapes (the title bold, the `[reach]` marker in its color,
/// the all-zero summary green); OFF → the byte-identical plain face.
/// Stripping the escapes restores the plain bytes both ways.
#[test]
fn lint_scripts_color_is_opt_in_and_default_stays_plain() {
    // plain (the default): no escapes anywhere — the plain bytes
    let (code, plain, stderr) = run("lint-scripts", &[SCRIPT]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(!plain.contains("\x1b["), "{plain}");
    assert_eq!(plain, "scripts/the-nightjar-flyer.txt: clean\n\n1 file(s), 0 finding(s)\n");

    // colored: escapes; stripping them restores the plain bytes
    let (code, on, _) = run("lint-scripts", &[SCRIPT, "--color"]);
    assert_eq!(code, Some(0));
    assert!(on.contains("\x1b["), "{on}");
    assert_eq!(strip_ansi(&on), plain, "removing escapes restores the plain face");
    // the everything-clean face: the `…: clean` row plain, the summary
    // green whole
    assert!(on.starts_with("scripts/the-nightjar-flyer.txt: clean\n"), "{on}");
    assert!(on.contains("\x1b[38;5;10m1 file(s), 0 finding(s)\x1b[0m\n"), "{on}");

    // the bridge layer: the `[reach]` marker magenta
    let (code, on_reach, _) = run("lint-scripts", &[SCRIPT, "--reach", "--color"]);
    assert_eq!(code, Some(0), "{on_reach}");
    assert!(on_reach.contains("  - \x1b[38;5;13m[reach]\x1b[0m "), "{on_reach}");
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
    let world = chorus::reach::scale::build_cs_scale(20, 15);
    let mine: Vec<String> = world
        .names
        .iter()
        .enumerate()
        .filter(|(idx, _)| idx % 3 == 1)
        .map(|(_, n)| n.clone())
        .collect();
    let worker_names: Vec<String> =
        arr.iter().map(|st| st["name"].as_str().expect("name").to_string()).collect();
    assert_eq!(worker_names, mine);
}

/// v0.7.6 — script-declared verbs must not leak ACROSS linted files: the bin
/// builds a fresh core dictionary per file, so file two, which USES file
/// one's verb without declaring it, cannot compile (the templates gate) —
/// while with a shared dictionary file one's lint would have registered
/// INTERROGATE and file two would have passed (through the real argv →
/// process path).
#[test]
fn lint_scripts_files_are_isolated_for_script_declared_verbs() {
    let declares = "
script: Verbs Isolation One
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE
  THE_GUARD is character with duty:2 at HOUSE

verbs:
  INTERROGATE:
    actors: 1 kind:character
    object: 1 kind:character
    duration: 30m
    recipes:
      default -> NARRATE \"{actors} question {object}.\"
";
    // the user, not the declarer: the templates line reads the verb file one
    // declared — legal only if file one's registration leaked into this
    // file's dictionary
    let uses = "
script: Verbs Isolation Two
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE

templates:
  INTERROGATE says \"{actors} press {object}.\"
";
    let dir = std::env::temp_dir().join(format!("chorus-lint-verbs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch dir");
    let one = dir.join("verbs-one.txt");
    let two = dir.join("verbs-two.txt");
    std::fs::write(&one, declares).expect("write file one");
    std::fs::write(&two, uses).expect("write file two");
    // the declaring file lints clean on its own
    let (code, stdout, stderr) = run("lint-scripts", &[one.to_str().unwrap()]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.contains("verbs-one.txt: clean"), "{stdout}");
    // ... and the using file fails ALONE (no leak backward either)
    let (solo_code, solo_out, solo_err) = run("lint-scripts", &[two.to_str().unwrap()]);
    assert_eq!(solo_code, Some(1), "{solo_out}");
    assert!(solo_err.contains("template 'INTERROGATE' is not a registered verb"), "{solo_err}");
    // and in ONE invocation the leak would show exactly here: a shared
    // dictionary linted file one first, file two inherited INTERROGATE, and
    // the run exited 0. Per-file isolation: exit 1, file one clean, file two
    // named by the same compile error.
    let (code, stdout, stderr) =
        run("lint-scripts", &[one.to_str().unwrap(), two.to_str().unwrap()]);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stdout.contains("verbs-one.txt: clean"), "{stdout}");
    assert!(!stdout.contains("verbs-two.txt: clean"), "{stdout}");
    assert!(stderr.contains("template 'INTERROGATE' is not a registered verb"), "{stderr}");
}

/// A tiny SGR stripper for the `--color` on/off equality (test-only): an
/// escape, `[`, then up to the first ASCII alphabetic byte, dropped.
fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}
