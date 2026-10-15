//! The scripted REPL acceptance: fixed input lines are
//! piped into the REAL `chorus` bin (CARGO_BIN_EXE) and its stdout is
//! asserted self-consistently — Rust against Rust, no external fixtures.
//!
//! - the dispatcher: a session exercising every routing branch is replayed
//!   through the library Game and the full stdout must match byte for byte
//!   (UI-03: the bin is routing only). The piped session is deterministic
//!   across runs, so the bin's bytes and the library's bytes are the same
//!   surface judged twice.

use std::io::Write;
use std::process::{Command, Stdio};

const NIGHTJAR_SCRIPT: &str = "scripts/the-nightjar-flyer.txt";

/// pipe `lines` into the bin and collect (exit code, stdout, stderr)
fn run_repl(script: &str, lines: &[&str]) -> (Option<i32>, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_chorus"))
        .arg(script)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
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

/// the runPlain banner: the meta title (UI-04: the
/// script's `script:` line) and the lowercased verb list
fn banner(title: &str, verbs: &[String]) -> String {
    format!("── {title} ──\nverbs: {}\n", verbs.join(" · "))
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
        "/scripts/the-nightjar-flyer.txt"
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
