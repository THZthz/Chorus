//! PR3-09 — the scripted REPL acceptance (plan §4-W6): fixed input lines are
//! piped into the REAL `chorus` bin (CARGO_BIN_EXE) and its stdout is diffed
//! against the W4 goldens.
//!
//! - the nightjar win path: nine lenient input lines (the test-local
//!   WIN_PATH of nightjar.test.ts:38-48) interleaved with the `wait` meta
//!   command — the explicit form of the auto-settle adapter (each commit's
//!   window completes before the next input). The final `journal` section is
//!   BYTE-IDENTICAL to goldens/nightjar-main.journal.txt — the TS Game
//!   (handleLine) golden the dump family recorded — and the first line enters
//!   in canonical form because bare `wait` is the wait BUTTON (game.wait())
//!   in the REPL dispatcher, not a verb commit (see bin/chorus.rs header).
//! - the demo: six canonical steps over tests/demo-world.txt (the seed world
//!   as a script). The story projection is byte-identical to
//!   goldens/demo.prose.txt modulo the story() join shape, and the journal
//!   matches goldens/demo.journal.txt under the REGISTERED id map documented
//!   in tests/demo-world.txt (script-compiler s_ ids + the engine-default
//!   e-mints where the TS example injected its own idGen) — asserted to be
//!   EXACTLY that map: any other divergence fails.
//! - the dispatcher: a session exercising every routing branch is replayed
//!   through the library Game and the full stdout must match byte for byte
//!   (UI-03: the bin is routing only).

use std::io::Write;
use std::process::{Command, Stdio};

const NIGHTJAR_SCRIPT: &str = "scripts/the-nightjar-flyer.txt";
const DEMO_SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/demo-world.txt");

/// pipe `lines` into the bin and collect (exit code, stdout, stderr)
fn run_repl(script: &str, lines: &[&str]) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_chorus"))
        .arg(script)
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the chorus bin spawns");
    {
        let stdin = child.stdin.as_mut().expect("piped stdin");
        for line in lines {
            stdin.write_all(line.as_bytes()).expect("write input");
            stdin.write_all(b"\n").expect("write newline");
        }
    }
    let out = child.wait_with_output().expect("the bin finishes");
    (
        out.status.code(),
        String::from_utf8(out.stdout).expect("stdout is UTF-8"),
        String::from_utf8(out.stderr).expect("stderr is UTF-8"),
    )
}

/// tui.tsx:451-452 — the runPlain banner: the meta title (UI-04: the
/// script's `script:` line) and the lowercased verb list
fn banner(title: &str, verbs: &[String]) -> String {
    format!("── {title} ──\nverbs: {}\n", verbs.join(" · "))
}

/// ts: nightjar.test.ts:38-48 (WIN_PATH) — the scripted win path. Line 1 is
/// the canonical spelling of the wait-verb commit: the REPL's bare `wait` is
/// the meta command (game.wait(), the TUI button semantics), and the
/// canonical sentence produces the identical step (pinned below by the byte
/// equality with the TS golden).
const WIN_PATH_SESSION: [&str; 18] = [
    "0:42 @ MARCH WAIT",
    "wait", // the auto-settle: game.wait() rides every commit's window
    "explore the first cabin",
    "wait",
    "read the solicitor letter",
    "wait",
    "explore the firebox",
    "wait",
    "read the vane ledger",
    "wait",
    "talk with the spiritualist",
    "wait",
    "seraphine read the wax cylinder",
    "wait",
    "seraphine read the telegram",
    "wait",
    "use on the inquest with the embezzlement the stolen voice the vienna warning",
    "wait",
];

/// the win path piped into the real bin ends byte-identical to the TS Game
/// golden: `journal` prints exactly goldens/nightjar-main.journal.txt, and
/// the whole session is deterministic across runs.
#[test]
fn repl_nightjar_win_path_journal_matches_the_ts_game_golden_byte_for_byte() {
    let inputs: Vec<&str> = WIN_PATH_SESSION
        .iter()
        .copied()
        .chain(["journal", "quit"])
        .collect();
    let (code, stdout, stderr) = run_repl(NIGHTJAR_SCRIPT, &inputs);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stderr, "", "the session must run clean");

    // the banner (UI-04: title from meta; tui.tsx:26-28 verbList)
    assert!(stdout.starts_with(&banner(
        "Murder on the Nightjar Flyer",
        &[
            "read", "talk", "explore", "use on", "travel to", "wait", "study", "work", "mansus",
            "locate at", "dream", "acquire",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect::<Vec<_>>(),
    )));

    // the journal section — everything from the first journal row on
    let golden = include_str!("goldens/nightjar-main.journal.txt");
    let first_row = golden.lines().next().expect("golden journal nonempty");
    let idx = stdout
        .find(&format!("\n{first_row}\n"))
        .expect("the journal section is present");
    assert_eq!(
        &stdout[idx + 1..],
        golden,
        "the REPL journal must equal the TS golden byte for byte"
    );

    // determinism: the same lines, twice, one stdout
    let (code2, stdout2, _) = run_repl(NIGHTJAR_SCRIPT, &inputs);
    assert_eq!(code2, Some(0));
    assert_eq!(stdout, stdout2, "piped sessions are deterministic");
}

/// deviation map for the demo journal (tests/demo-world.txt's header):
/// examples/demo.ts injected `x{id}_${++n}` spawn ids (demo.ts:33) where the
/// Game/handleLine path uses the engine default `e{firstRecordId}_{n}`
/// (engine.ts:179), and the seed fixture's bare ids become the script
/// compiler's `s_${lower(name)}` (types.ts:159). Applied to the TS golden
/// this yields EXACTLY the REPL's journal — asserted below, so any
/// divergence beyond this map fails.
fn map_demo_ids(golden: &str) -> String {
    let mut s = golden.to_string();
    for (from, to) in [
        ("#x3_1", "#e3_1"),   // CLUE_THE_FORGETTING — the run's first mint
        ("#x6_2", "#e6_1"),   // BONDING_MEMORY — the default mint counter resets per step
        ("#x6_3", "#e6_2"),   // DREAD (the dusk_falls event, same step)
        ("#x11_5", "#e11_1"), // PLAN_INFILTRATION
        ("#ana", "#s_ana"),
        ("#study", "#s_the_study"), // THE_STUDY → s_${lower(name)}
        ("#house", "#s_house"),
        ("#dining", "#s_dining_hall"), // DINING_HALL → s_${lower(name)}
        // container attrs ride inside the sentence, JSON-escaped
        ("\\\"house\\\"", "\\\"s_house\\\""),
        ("\\\"dining\\\"", "\\\"s_dining_hall\\\""),
    ] {
        s = s.replace(from, to);
    }
    s
}

/// the demo session: six canonical steps, then `story` and `journal`.
/// Story is byte-identical to goldens/demo.prose.txt modulo story()'s own
/// join shape (play.ts:150 indents a record's continuation lines by two
/// where the prose golden flattens them); the journal matches under exactly
/// the registered id map.
#[test]
fn repl_demo_session_story_and_journal_match_the_demo_goldens() {
    let inputs = [
        "0:28 @ ANA AT HOUSE READ THE_FORGETTING",
        "0:30 @ ANA, BERTHA READ FAIRY_TALES",
        "0:32 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
        "0:34 @ BERTHA READ MAGICK_BOOK",
        "0:34 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ MAGICK_BOOK",
        "0:40 @ ANA WAIT",
        "story",
        "journal",
        "quit",
    ];
    let (code, stdout, stderr) = run_repl(DEMO_SCRIPT, &inputs);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stderr, "", "the session must run clean");

    // the banner (UI-04): the title is the script's `script:` directive —
    // examples/play.ts's `^title:` regex was NOT ported
    assert!(stdout.starts_with(&banner(
        "The Forgotten House",
        &[
            "read", "study", "talk", "travel to", "use on", "wait", "work", "explore", "mansus",
            "dream", "locate at", "acquire",
        ]
        .iter()
        .map(|s| (*s).to_string())
        .collect::<Vec<_>>(),
    )));

    // ---- the story section vs the prose golden ----
    let story_start = stdout.find("\n· ").expect("the story section is present") + 1;
    let journal_first = "\n0:0 @ WAIT 14h\n";
    let journal_idx = stdout
        .find(journal_first)
        .expect("the journal section is present");
    let story_lines: Vec<&str> = stdout[story_start..journal_idx]
        .lines()
        .filter(|l| !l.is_empty())
        .collect();
    // rebuild the golden's per-record prose, then apply story()'s join shape:
    // ts: play.ts:150 — `'· ' + proseLines(r).join('\n  ')` per record, where
    // the prose golden (dump demoFamily) joins the same lines with '\n'
    let mut expected: Vec<String> = Vec::new();
    let mut open_record = false;
    for line in include_str!("goldens/demo.prose.txt").lines() {
        if let Some(rest) = line.strip_prefix("· ") {
            expected.push(format!("· {rest}"));
            open_record = true;
        } else if line.is_empty() {
            continue; // the golden's trailing newline
        } else {
            assert!(open_record, "a prose continuation without a '· ' record start");
            expected.push(format!("  {line}"));
            open_record = false;
        }
    }
    assert_eq!(
        story_lines,
        expected.iter().map(String::as_str).collect::<Vec<_>>(),
        "the story projection must equal the prose golden (story join shape)"
    );

    // ---- the journal section vs the golden under the registered map ----
    let journal_section = &stdout[journal_idx + 1..];
    assert_eq!(
        journal_section,
        map_demo_ids(include_str!("goldens/demo.journal.txt")),
        "the demo journal must equal the TS golden under EXACTLY the registered id map"
    );
}

/// UI-03 — the dispatcher is routing only: a session that exercises every
/// branch (meta words, inspect prefix incl. the Q11 bare form, a fall-through
/// word like `inspectorish`, bad input, handle_line turns) replays through
/// the library Game, and the whole stdout matches byte for byte.
#[test]
fn repl_dispatcher_routing_matches_the_library_game_exactly() {
    let inputs = [
        "look",                     // meta → game.look()
        "cards",                    // meta → game.cards()
        "journal",                  // meta → game.journal() (empty: the placeholder)
        "story",                    // meta → game.story() (empty: the placeholder)
        "frobnicate",               // handle_line → error turn + suggestions
        "inspect the wax cylinder", // inspect prefix, named card
        "inspect",                  // QUIRK(UI-06/Q11): bare inspect shows the first visible card
        "inspectorish note",        // \b fails inside a word → handle_line
        "explore the first cabin",  // a real turn (opens a window)
        "wait",                     // meta → game.wait() settles it
        "undo",                     // meta → game.undo() truncates the turn
        "undo",                     // …until nothing is left
        "quit",
    ];
    let (code, stdout, stderr) = run_repl(NIGHTJAR_SCRIPT, &inputs);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stderr, "", "the session must run clean");

    // the library replay of the SAME sequence through the SAME calls
    use chorus::core::dict::core_dictionary;
    use chorus::session::game::Game;
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scripts/the-nightjar-flyer.txt"
    ))
    .expect("the nightjar script");
    let mut game = Game::from_script(&text, core_dictionary()).expect("the script compiles");

    let title = game.title();
    let mut expected = banner(
        &title,
        &game
            .content
            .verbs
            .keys()
            .map(|v| v.to_lowercase().replace('_', " "))
            .collect::<Vec<_>>(),
    );
    for line in inputs {
        let trimmed = line.trim();
        match trimmed {
            "quit" => break,
            "undo" => push_lines(&mut expected, &game.undo().expect("undo succeeds").lines),
            "wait" => push_lines(&mut expected, &game.wait().expect("wait succeeds").lines),
            "story" => push_lines(&mut expected, &game.story().expect("story renders")),
            "journal" => push_lines(&mut expected, &game.journal()),
            "look" => push_lines(&mut expected, &game.look()),
            "cards" => push_lines(&mut expected, &game.cards()),
            _ if trimmed == "inspect" || trimmed.starts_with("inspect ") => {
                let rest = trimmed.strip_prefix("inspect").unwrap_or("");
                push_lines(&mut expected, &game.inspect(rest.trim()));
            }
            _ => {
                // handle_line: error turns AND committed turns alike — the
                // lines are the behavior either way
                let turn = game.handle_line(line).expect("the turn executes");
                push_lines(&mut expected, &turn.lines);
            }
        }
    }
    assert_eq!(
        stdout, expected,
        "the bin is routing only — the library is the behavior"
    );
}

fn push_lines(out: &mut String, lines: &[String]) {
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
}
