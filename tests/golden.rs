//! The Rust-self regression digest (plan P0, phase 0) — the measurement net
//! for the idiomatization work. One driver plays a FIXED set of transcripts
//! against the two bundled scripts (`scripts/the-nightjar-flyer.txt` +
//! `tests/demo-world.txt`) through the real binaries and the real library,
//! and compares every byte of every artifact against the committed baseline
//! in `tests/golden/`.
//!
//! THIS IS THE ZERO-DRIFT GATE for plan phases P1/P2 (the C-class internal
//! re-shapes): before/after any refactor commit the digest must not move.
//! It complements the assertion suite by catching drift no assertion
//! reaches — exact CLI stdout/stderr bytes, exit codes, JSONL log bytes on
//! disk, and the full-state snap projection.
//!
//! P3 (the settled B fixes) intentionally changes behavior: when a commit
//! moves observable bytes, re-baseline with
//!
//!     CHORUS_DIGEST_BLESS=1 cargo test --test golden
//!
//! as a BEHAVIOR-CHANGE commit — the moved artifacts enumerated in the
//! commit message. Never bless silently, and never bless to make a
//! P1/P2 commit pass.
//!
//! Generation and verification are THE SAME CODE PATH: the case list below
//! produces artifacts; bless mode writes them, verify mode (default) reads
//! the baseline and compares byte for byte. There is no second
//! implementation to drift against.
//!
//! Determinism clause (plan §Phase 0, verified 2026-10-14):
//!   - every path is REPO-RELATIVE (bins run with cwd = CARGO_MANIFEST_DIR
//!     and repo-relative argv; script texts are include_str! data);
//!   - temp files for JSONL round-trips live under `target/digest-tmp/`
//!     and their paths never enter an artifact — only the bytes written
//!     INTO them are hashed;
//!   - no wall-clock reaches the digest: `t_real` is `None` on the whole
//!     chain and `snap()` emits timestamp-free pure JSON;
//!   - ONE registered normalization: `reach-fuzz`'s summary line ends with
//!     a wall-clock `{:.1}s` field; the digest strips exactly that trailing
//!     field (see `strip_elapsed_field`) and hashes the rest verbatim;
//!   - `reach-scale` is NOT digested: its artifacts are temp files and its
//!     NameStat carries an `ms` timing field.

mod common;

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use common::snap::snap;

use chorus::content::script::compile_script;
use chorus::core::dict::core_dictionary;
use chorus::core::parse::parse_sentence;
use chorus::core::types::clone_state;
use chorus::engine::engine::{execute_step, ExecOptions, Step};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::session::game::{Game, TurnKind};

// ---------------------------------------------------------------------------
// fixed data
// ---------------------------------------------------------------------------

/// repo-relative script paths — the digest keys, and the exact argv the
/// bins receive (their outputs echo these spellings, e.g. lint's
/// `<file>: clean` lines)
const NIGHTJAR: &str = "scripts/the-nightjar-flyer.txt";
const DEMO: &str = "tests/demo-world.txt";

const NIGHTJAR_TEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/the-nightjar-flyer.txt"
));
const DEMO_TEXT: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/demo-world.txt"));

const BASELINE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
const TMP_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/digest-tmp");

/// the scripted win path (tests/nightjar.rs's own list) — seed 1899 is
/// calibrated so the inquest completes its confession through the
/// auto-settle adapter
const WIN_PATH: [&str; 9] = [
    "wait",                        // 22:00 — the body is found
    "explore the first cabin",     // 23:00 — the solicitor's letter
    "read the solicitor letter",   // 0:00  — border stop; the telegram lands
    "explore the firebox",         // 1:00  — the wax cylinder
    "read the vane ledger",        // 2:00  — read in the same hour it burns
    "talk with the spiritualist",  // meet Seraphine
    "seraphine read the wax cylinder",
    "seraphine read the telegram",
    "use on the inquest with the embezzlement the stolen voice the vienna warning",
];

/// the demo's six canonical steps (tests/demo-world.txt shapes), driven
/// through the RAW engine with the demo's frozen `x{id}_{n}` idGen —
/// including the logged REFUSE of the magick gate
const DEMO_STEPS: [&str; 6] = [
    "0:28 @ ANA AT HOUSE READ THE_FORGETTING",
    "0:30 @ ANA, BERTHA READ FAIRY_TALES", // ends 16:00 → dusk_falls fires
    "0:32 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
    "0:34 @ BERTHA READ MAGICK_BOOK", // logged REFUSE — the magick gate
    "0:34 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ MAGICK_BOOK",
    "0:40 @ ANA WAIT",
];

/// a load error must be a hard exit-1 error line, path-free — this text is
/// written to `target/digest-tmp/` and fed to the bins by argv
const BROKEN_SCRIPT: &str = "script: Broken\n\nentities:\n  X is character at NOWHERE\n";

// ---------------------------------------------------------------------------
// the driver — generation (bless) and verification are this one loop
// ---------------------------------------------------------------------------

struct Case {
    name: &'static str,
    run: fn() -> Vec<(&'static str, Vec<u8>)>,
}

const CASES: &[Case] = &[
    // ── the nightjar flyer: the chorus bin (piped REPL sessions) ──
    Case { name: "nightjar-repl-session", run: nightjar_repl_session },
    Case { name: "nightjar-repl-refusals", run: nightjar_repl_refusals },
    Case { name: "nightjar-repl-endings", run: nightjar_repl_endings },
    // ── the nightjar flyer: the library (play + JSONL + snap + replay) ──
    Case { name: "nightjar-play-log", run: nightjar_play_log },
    // ── the demo world: the chorus bin ──
    Case { name: "demo-repl-session", run: demo_repl_session },
    // ── the demo world: the raw engine transcript ──
    Case { name: "demo-engine-transcript", run: demo_engine_transcript },
    // ── reach CLI: --all / card / --demand over both scripts ──
    Case { name: "reach-all-nightjar", run: || run_bin(Bin::Reach, &[NIGHTJAR, "--all"], &[]) },
    Case {
        name: "reach-card-nightjar",
        run: || run_bin(Bin::Reach, &[NIGHTJAR, "THE_WAX_CYLINDER", "--usable"], &[]),
    },
    Case {
        name: "reach-demand-nightjar",
        run: || run_bin(Bin::Reach, &[NIGHTJAR, "--demand", "aspect:secret>=2"], &[]),
    },
    Case { name: "reach-all-demo", run: || run_bin(Bin::Reach, &[DEMO, "--all"], &[]) },
    Case {
        name: "reach-card-demo",
        run: || run_bin(Bin::Reach, &[DEMO, "THE_PALE_LADY", "--usable"], &[]),
    },
    // ── reach CLI: the frozen error shapes ──
    Case { name: "reach-usage-error", run: || run_bin(Bin::Reach, &[NIGHTJAR], &[]) },
    Case {
        name: "reach-bad-demand-error",
        run: || run_bin(Bin::Reach, &[NIGHTJAR, "--demand", "aspect:secret >= 3"], &[]),
    },
    Case {
        name: "reach-missing-file-error",
        run: || run_bin(Bin::Reach, &["nosuch-script.txt", "ANY_CARD"], &[]),
    },
    // ── lint CLI ──
    Case { name: "lint-nightjar", run: || run_bin(Bin::LintScripts, &[NIGHTJAR], &[]) },
    Case { name: "lint-demo", run: || run_bin(Bin::LintScripts, &[DEMO], &[]) },
    Case {
        name: "lint-missing-file-error",
        run: || run_bin(Bin::LintScripts, &["nosuch-script.txt"], &[]),
    },
    // ── reach-fuzz: fixed seed windows (the only normalized artifact) ──
    Case {
        name: "fuzz-seeds-1-5",
        run: || {
            run_bin_fuzz(&["--seeds", "5", "--from", "1", "--steps", "2", "--states", "60"])
        },
    },
    Case {
        name: "fuzz-seeds-42-44",
        run: || {
            run_bin_fuzz(&["--seeds", "3", "--from", "42", "--steps", "3", "--states", "40"])
        },
    },
    Case { name: "fuzz-zero-seeds", run: || run_bin_fuzz(&["--seeds", "0"]) },
    // ── chorus bin: the frozen error shapes ──
    Case { name: "chorus-usage-error", run: || run_bin(Bin::Chorus, &[], &[]) },
    Case {
        name: "chorus-missing-script-error",
        run: || run_bin(Bin::Chorus, &["nosuch-script.txt"], &[]),
    },
    Case { name: "chorus-broken-script-error", run: chorus_broken_script_error },
];

#[test]
fn golden_digest_matches_the_committed_baseline() {
    // the hand-rolled sha256 (no new dependencies) must be the real one
    // before any hash line is trusted
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );

    let bless = std::env::var("CHORUS_DIGEST_BLESS").ok().as_deref() == Some("1");

    // fresh scratch space for the JSONL round-trips (gitignored, and the
    // paths never enter an artifact)
    let _ = std::fs::remove_dir_all(TMP_DIR);
    std::fs::create_dir_all(TMP_DIR).expect("tmp dir creates");
    std::fs::create_dir_all(BASELINE_DIR).expect("baseline dir creates");

    let mut index = String::new();
    index.push_str("# chorus regression digest — the plan-P0 measurement net.\n");
    index.push_str("# Regenerated ONLY by: CHORUS_DIGEST_BLESS=1 cargo test --test golden\n");
    index.push_str("# (a behavior-change commit, P3; never for P1/P2 refactors).\n");
    index.push_str("# Format: <sha256> <bytes> <case>.<artifact>  — the .<artifact>\n");
    index.push_str("# files beside this index hold the actual bytes.\n");

    let mut checked = 0usize;
    for case in CASES {
        let artifacts = (case.run)();
        assert!(
            !artifacts.is_empty(),
            "case {} produced no artifacts",
            case.name
        );
        for (artifact, bytes) in artifacts {
            let rel = format!("{}.{}", case.name, artifact);
            let path = Path::new(BASELINE_DIR).join(&rel);
            if bless {
                std::fs::write(&path, &bytes)
                    .unwrap_or_else(|e| panic!("baseline `{rel}` writes: {e}"));
            }
            let baseline = std::fs::read(&path).unwrap_or_else(|e| {
                panic!("baseline `{rel}` unreadable ({e}) — regenerate with CHORUS_DIGEST_BLESS=1 cargo test --test golden")
            });
            compare_bytes(&rel, &baseline, &bytes);
            index.push_str(&format!("{} {:>7} {}\n", sha256_hex(&bytes), bytes.len(), rel));
            checked += 1;
        }
    }

    // the index is derived from the same bytes — it must agree with the
    // committed copy (a hand-edited index is drift too)
    let index_path = Path::new(BASELINE_DIR).join("digest.txt");
    if bless {
        std::fs::write(&index_path, &index).expect("digest.txt writes");
    }
    let committed = std::fs::read_to_string(&index_path).unwrap_or_else(|e| {
        panic!("tests/golden/digest.txt unreadable ({e}) — regenerate with CHORUS_DIGEST_BLESS=1 cargo test --test golden")
    });
    assert_eq!(
        committed, index,
        "tests/golden/digest.txt disagrees with the byte files — regenerate the index (bless), never hand-edit it"
    );

    if bless {
        println!("blessed {checked} artifacts into tests/golden/");
    }
}

/// the only failure message shape: which case, which artifact, how far in,
/// and the bytes around the first divergence
fn compare_bytes(rel: &str, baseline: &[u8], now: &[u8]) {
    if baseline == now {
        return;
    }
    let first = baseline
        .iter()
        .zip(now.iter())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| baseline.len().min(now.len()));
    let context = |b: &[u8]| -> String {
        let lo = first.saturating_sub(32);
        let hi = (first + 32).min(b.len());
        String::from_utf8_lossy(&b[lo..hi]).replace('\n', "\\n")
    };
    panic!(
        "digest drift in `{rel}`: baseline {} bytes, now {} bytes; \
         first differing byte at offset {}\n  baseline …{}…\n  now      …{}…\n\
         an intentional behavior change re-baselines with: \
         CHORUS_DIGEST_BLESS=1 cargo test --test golden",
        baseline.len(),
        now.len(),
        first,
        context(baseline),
        context(now),
    );
}

// ---------------------------------------------------------------------------
// bin cases — the real subprocesses, cwd = repo root, repo-relative argv
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Bin {
    Chorus,
    Reach,
    ReachFuzz,
    LintScripts,
}

impl Bin {
    fn exe(&self) -> &'static str {
        match self {
            Bin::Chorus => env!("CARGO_BIN_EXE_chorus"),
            Bin::Reach => env!("CARGO_BIN_EXE_reach"),
            Bin::ReachFuzz => env!("CARGO_BIN_EXE_reach-fuzz"),
            Bin::LintScripts => env!("CARGO_BIN_EXE_lint-scripts"),
        }
    }
}

/// run one bin invocation → (stdout, stderr, exit code) artifacts
fn run_bin(bin: Bin, args: &[&str], stdin_lines: &[&str]) -> Vec<(&'static str, Vec<u8>)> {
    let mut command = Command::new(bin.exe());
    command
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if !stdin_lines.is_empty() {
        command.stdin(Stdio::piped());
    }
    let mut child = command
        .spawn()
        .unwrap_or_else(|e| panic!("the {bin:?} bin spawns: {e}"));
    if !stdin_lines.is_empty() {
        let mut stdin = child.stdin.take().expect("piped stdin");
        for line in stdin_lines {
            stdin.write_all(line.as_bytes()).expect("write input");
            stdin.write_all(b"\n").expect("write newline");
        }
    }
    let out = child.wait_with_output().expect("the bin finishes");
    let exit = format!("{}\n", out.status.code().unwrap_or(-1));
    vec![
        ("stdout", out.stdout),
        ("stderr", out.stderr),
        ("exit", exit.into_bytes()),
    ]
}

/// reach-fuzz's summary ends with a wall-clock `{:.1}s` field — the ONE
/// registered normalization: strip exactly that trailing field and hash the
/// rest verbatim (everything else is a pure function of the seed window)
fn run_bin_fuzz(args: &[&str]) -> Vec<(&'static str, Vec<u8>)> {
    let mut artifacts = run_bin(Bin::ReachFuzz, args, &[]);
    for (name, bytes) in &mut artifacts {
        if *name == "stdout" {
            *bytes = strip_elapsed_field(bytes);
        }
    }
    artifacts
}

fn strip_elapsed_field(stdout: &[u8]) -> Vec<u8> {
    let s = String::from_utf8(stdout.to_vec()).expect("fuzz stdout is utf-8");
    match s.rfind(", ") {
        // the summary line is the only one carrying `seeds ×`; its final
        // `, <n.n>s` field is the timing
        Some(pos) if s.as_bytes()[..pos].windows(8).any(|w| w == b"seeds \xc3\x97") => {
            format!("{}\n", &s[..pos]).into_bytes()
        }
        _ => stdout.to_vec(),
    }
}

fn chorus_broken_script_error() -> Vec<(&'static str, Vec<u8>)> {
    // written under target/ (never committed); the argv stays repo-relative
    // and the compile error is path-free (compile_script sees text only)
    let path = Path::new(TMP_DIR).join("broken-script.txt");
    std::fs::write(&path, BROKEN_SCRIPT).expect("broken script writes");
    run_bin(Bin::Chorus, &["target/digest-tmp/broken-script.txt"], &[])
}

// ---------------------------------------------------------------------------
// the piped REPL sessions (the chorus bin)
// ---------------------------------------------------------------------------

/// every dispatcher branch: the meta views, a lenient miss, the inspect
/// prefix (named + bare Q11 form), a fall-through word, a real turn,
/// wait, undo to exhaustion
fn nightjar_repl_session() -> Vec<(&'static str, Vec<u8>)> {
    run_bin(
        Bin::Chorus,
        &[NIGHTJAR],
        &[
            "look",
            "cards",
            "journal",
            "story",
            "frobnicate", // handle_line → error turn + suggestions
            "inspect the wax cylinder",
            "inspect", // QUIRK(Q11): bare inspect shows the first visible card
            "inspectorish note", // \b fails inside a word → handle_line
            "explore the first cabin", // a real turn (opens a window)
            "wait",    // settles it
            "undo",
            "undo", // …until nothing is left
            "quit",
        ],
    )
}

/// the two refusal faces: travel.no_link (the corridor does not bend) and
/// inquest.short (a wobbly case) — then undo takes the wait turn back
fn nightjar_repl_refusals() -> Vec<(&'static str, Vec<u8>)> {
    run_bin(
        Bin::Chorus,
        &[NIGHTJAR],
        &[
            "wait", // 22:00 — the body is found, the aether-seal exists
            "travel to the dining car", // REFUSE: that car is not next door
            "use on the inquest with the aether seal", // REFUSE: one proof < 3
            "undo",
            "quit",
        ],
    )
}

/// the win path with naive interleave-waits: both competing endings fire in
/// one night — the inquest's completion and vienna_arrival — and
/// first-ENDING-wins hands the story to QUIET_NIGHT; the post-ending step
/// then rejects naming that title (§9.35)
fn nightjar_repl_endings() -> Vec<(&'static str, Vec<u8>)> {
    let mut lines: Vec<&str> = Vec::new();
    for (i, line) in WIN_PATH.into_iter().enumerate() {
        lines.push(line);
        if i + 1 < WIN_PATH.len() {
            lines.push("wait"); // naive settle — NOT the calibrated adapter
        }
    }
    lines.push("wait"); // settle the inquest — vienna wins the race
    lines.push("look"); // views still work after the ending
    lines.push("explore the guard van"); // post-ending rejection names the title
    lines.push("quit");
    run_bin(Bin::Chorus, &[NIGHTJAR], &lines)
}

/// the demo world: normal step + completion, the magick-gate REFUSE,
/// canonical clocked waits into the dusk trigger, undo OVER the trigger's
/// event rows (re-fold), the re-fire, and the story/journal/cards views
fn demo_repl_session() -> Vec<(&'static str, Vec<u8>)> {
    run_bin(
        Bin::Chorus,
        &[DEMO],
        &[
            "look",
            "read the forgetting",
            "wait", // settles READ → the CLUE spawns
            "bertha read the magick book", // REFUSE: the letters swim
            "0:30 @ ANA WAIT",
            "0:32 @ ANA WAIT", // dusk_falls fires
            "undo",            // drops the wait AND the trigger's event rows
            "0:32 @ ANA WAIT", // …and re-fires
            "story",
            "journal",
            "cards",
            "quit",
        ],
    )
}

// ---------------------------------------------------------------------------
// library cases — play, JSONL bytes on disk, snap, and the replay law
// ---------------------------------------------------------------------------

/// the auto-settle adapter (tests/nightjar.rs's): after a committed action
/// with open windows, the settle wait appends to the same night
fn auto_settle(game: &mut Game, line: &str) {
    let t = game.handle_line(line).expect("win-path turn executes");
    if t.kind == TurnKind::Action && !game.state.windows.is_empty() {
        game.wait().expect("the settle wait executes");
    }
}

fn joined(lines: &[String]) -> Vec<u8> {
    format!("{}\n", lines.join("\n")).into_bytes()
}

/// save the log, read the BYTES back, reload it from disk, and fold the
/// reload over the log's initial state — the JSONL round-trip IS the replay
/// law's proof surface
fn save_reload_fold(
    records: &[chorus::engine::engine::ActionRecord],
    initial: &chorus::core::types::State,
    content: &chorus::content::content::CompiledContent,
    file: &str,
) -> (Vec<u8>, Vec<u8>) {
    let path = Path::new(TMP_DIR).join(file);
    save_log(&path, records).expect("the log saves");
    let bytes = std::fs::read(&path).expect("the log bytes read back");
    let reloaded = load_log(&path).expect("the log reloads from disk");
    let folded = fold(&reloaded, initial, &core_dictionary(), content)
        .expect("fold(log) replays the state");
    (bytes, snap(&folded).into_bytes())
}

/// the calibrated win path through the library Game (the tests' own
/// adapter): the inquest completes with the confession — both ENDING rows
/// land in the log, and first-ENDING-wins (§9.35) gives the story to
/// QUIET_NIGHT; the log is digested as JSONL bytes + snap + the folded
/// replay's snap + the journal, and a post-ending attempt names the title
fn nightjar_play_log() -> Vec<(&'static str, Vec<u8>)> {
    let mut game = Game::from_script(NIGHTJAR_TEXT, core_dictionary()).expect("nightjar compiles");
    for line in WIN_PATH {
        auto_settle(&mut game, line);
    }
    let after = game
        .handle_line("explore the guard van")
        .expect("the post-ending turn executes");
    let (jsonl, fold_snap) = save_reload_fold(
        &game.log.records,
        &game.log.initial_state,
        &game.content,
        "nightjar-win.jsonl",
    );
    vec![
        ("jsonl", jsonl),
        ("snap", snap(&game.state).into_bytes()),
        ("fold-snap", fold_snap),
        ("journal", joined(&game.journal())),
        ("post-ending", joined(&after.lines)),
    ]
}

/// the demo's six canonical steps through the RAW engine (no session layer)
/// with the demo's frozen `x{id}_{n}` idGen — the log's JSONL bytes, the
/// played state's snap, and the disk round-trip replay's snap must all
/// stay byte-still
fn demo_engine_transcript() -> Vec<(&'static str, Vec<u8>)> {
    let dict = core_dictionary();
    let compiled = compile_script(DEMO_TEXT, &dict).expect("demo-world compiles");
    let mut state = compiled.state;
    let initial = clone_state(&state);
    let mut records = Vec::new();
    let mut id: i64 = 1;
    let mut n: usize = 0;
    for src in DEMO_STEPS {
        let s = parse_sentence(src, &dict).expect("the canonical step parses");
        let id_now = id;
        let mut id_gen = || {
            n += 1;
            format!("x{id_now}_{n}")
        };
        let r = execute_step(
            &compiled.content,
            &dict,
            state,
            Step {
                start_min: None,
                clock: Some(s.clock.expect("demo steps carry clocks")),
                clauses: s.clauses,
                t_real: None,
            },
            ExecOptions { first_record_id: id, id_gen: Some(&mut id_gen) },
        )
        .expect("the step executes");
        state = r.state;
        id += r.records.len() as i64;
        records.extend(r.records);
    }
    let (jsonl, fold_snap) = save_reload_fold(&records, &initial, &compiled.content, "demo.jsonl");
    vec![
        ("jsonl", jsonl),
        ("snap", snap(&state).into_bytes()),
        ("fold-snap", fold_snap),
    ]
}

// ---------------------------------------------------------------------------
// sha256 — hand-rolled (the dependency set is frozen at four crates); the
// FIPS vectors are asserted at the top of the test before any hash is
// compared. The hash feeds only the human-readable index — the byte files
// are compared byte for byte.
// ---------------------------------------------------------------------------

fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bitlen = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());

    for block in msg.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = String::with_capacity(64);
    for word in h {
        for byte in word.to_be_bytes() {
            out.push_str(&format!("{byte:02x}"));
        }
    }
    out
}
