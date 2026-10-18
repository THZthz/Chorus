//! The scripted REPL acceptance: fixed input lines are
//! piped into the REAL `chorus` bin (CARGO_BIN_EXE) and its stdout is
//! asserted self-consistently — Rust against Rust, no external fixtures.
//!
//! - the dispatcher: a session exercising every routing branch is replayed
//!   through the library Game and the full stdout must match byte for byte
//!   (the bin is routing only: `Game::banner` opens, each piped line is
//!   echoed as `» line`, a turn's lines are followed by `Game::after_turn`).
//!   The piped session is deterministic across runs, so the bin's bytes and
//!   the library's bytes are the same surface judged twice.

use std::io::Write;
use std::process::{Command, Stdio};

use chorus::core::dict::core_dictionary;
use chorus::session::game::{Game, TurnKind};

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

fn nightjar_game() -> Game {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/scripts/the-nightjar-flyer.txt"
    ))
    .expect("the nightjar script");
    Game::from_script(&text, core_dictionary()).expect("the script compiles")
}

/// The library replay of a piped session: `Game::banner` opens, every
/// line is echoed as `» line` (the piped-mode stand-in for a terminal's
/// prompt + typing), the meta views print their lines, and the turns —
/// a line of play, `wait`, `undo` — are followed by `Game::after_turn`.
fn replay(game: &mut Game, inputs: &[&str]) -> String {
    let mut expected = String::new();
    push_lines(&mut expected, &game.banner().expect("the banner renders"));
    for line in inputs {
        let trimmed = chorus::core::text::dsl_trim(line);
        push_lines(&mut expected, &[format!("\u{bb} {line}")]);
        match trimmed {
            "quit" => break,
            "help" => push_lines(&mut expected, &game.help().expect("help renders")),
            "undo" => {
                push_lines(&mut expected, &game.undo().expect("undo succeeds").lines);
                push_lines(&mut expected, &game.after_turn().expect("the live area renders"));
            }
            "wait" => {
                push_lines(&mut expected, &game.wait().expect("wait succeeds").lines);
                push_lines(&mut expected, &game.after_turn().expect("the live area renders"));
            }
            "story" => push_lines(&mut expected, &game.story().expect("story renders")),
            "journal" => push_lines(&mut expected, &game.journal()),
            "look" => push_lines(&mut expected, &game.look()),
            "cards" => push_lines(&mut expected, &game.cards()),
            _ if trimmed == "save"
                || (trimmed.starts_with("save")
                    && trimmed["save".len()..]
                        .starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_')) =>
            {
                // the save prefix: bare `save` is the usage line,
                // `save <file>` writes the log — one line either way
                let rest = trimmed.strip_prefix("save").unwrap_or("");
                let line = game.save(rest.trim()).expect("save renders");
                push_lines(&mut expected, &[line]);
            }
            _ if trimmed == "inspect" || trimmed.starts_with("inspect ") => {
                let rest = trimmed.strip_prefix("inspect").unwrap_or("");
                push_lines(&mut expected, &game.inspect(rest.trim()));
            }
            _ => {
                // handle_line: error turns AND committed turns alike — the
                // lines are the behavior either way, then the live area
                let turn = game.handle_line(line).expect("the turn executes");
                push_lines(&mut expected, &turn.lines);
                push_lines(&mut expected, &game.after_turn().expect("the live area renders"));
            }
        }
    }
    expected
}

/// The dispatcher is routing only: a session that exercises every
/// branch (meta words incl. help, the inspect and save prefixes incl. the
/// Q11 bare form, a fall-through word like `inspectorish` or `save5`, bad
/// input, handle_line turns, a BOM-prefixed meta word) replays through the
/// library Game, and the whole stdout matches byte for byte.
#[test]
fn repl_dispatcher_routing_matches_the_library_game_exactly() {
    let inputs = [
        "look",                     // meta → game.look()
        "cards",                    // meta → game.cards()
        "help",                     // meta → game.help()
        "journal",                  // meta → game.journal() (empty: the placeholder)
        "story",                    // meta → game.story() (empty: the placeholder)
        "frobnicate",               // handle_line → error turn + the live area
        "save",                     // the save prefix, no path → the usage line
        "saved the day",            // \b fails inside a word → handle_line
        "save5",                    // the digit shape: no boundary → handle_line
        "inspect the wax cylinder", // inspect prefix, named card
        "inspect",                  // Q11: bare inspect is usage + the card list
        "inspectorish note",        // \b fails inside a word → handle_line
        "explore the first cabin",  // a REFUSED turn (the seal, before the cry): logged, no window
        "\u{feff}wait",             // a pasted BOM: still the meta word (Q8 class)
        "undo",                     // meta → game.undo() truncates the turn
        "undo",                     // …until nothing is left
        "quit",
    ];
    let (code, stdout, stderr) = run_repl(NIGHTJAR_SCRIPT, &inputs);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stderr, "", "the session must run clean");

    let mut game = nightjar_game();
    let expected = replay(&mut game, &inputs);
    assert_eq!(stdout, expected, "the bin is routing only — the library is the behavior");
}

fn push_lines(out: &mut String, lines: &[String]) {
    for l in lines {
        out.push_str(l);
        out.push('\n');
    }
}

/// `save <file>` writes the JSONL action log from both surfaces: the bin
/// and the library replay run the SAME session, each saving to its OWN tmp
/// path (one writer per file); the transcripts match byte for byte (the
/// only difference — the echoed path — is swapped back), and the two files
/// are byte-equal. Nothing is left behind.
#[test]
fn repl_save_writes_the_log_and_bin_and_library_files_are_byte_equal() {
    let dir = std::env::temp_dir().join(format!("chorus-repl-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch dir");
    let bin_log = dir.join("bin.jsonl");
    let lib_log = dir.join("lib.jsonl");

    // the bin's session: a turn of play, then the save
    let bin_inputs: Vec<String> = vec![
        "look".into(),
        "read the manifest".into(),
        format!("save {}", bin_log.display()),
        "quit".into(),
    ];
    let bin_strs: Vec<&str> = bin_inputs.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = run_repl(NIGHTJAR_SCRIPT, &bin_strs);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stderr, "", "the session must run clean");

    // the library replay of the SAME session, saving to its own path
    let lib_inputs: Vec<String> = vec![
        "look".into(),
        "read the manifest".into(),
        format!("save {}", lib_log.display()),
        "quit".into(),
    ];
    let lib_strs: Vec<&str> = lib_inputs.iter().map(String::as_str).collect();
    let mut game = nightjar_game();
    let expected = replay(&mut game, &lib_strs);
    // the save line's echo and its confirmation both carry the path — swap
    // it back so the two transcripts judge byte-equal
    let expected = expected.replace(lib_log.to_str().unwrap(), bin_log.to_str().unwrap());
    assert_eq!(stdout, expected, "the bin is routing only");

    // the two saves are the same records
    let from_bin = std::fs::read(&bin_log).expect("the bin wrote its log");
    let from_lib = std::fs::read(&lib_log).expect("the replay wrote its log");
    assert!(!from_bin.is_empty(), "the played session logged records");
    assert_eq!(from_bin, from_lib, "both surfaces wrote the same records");
    // the confirmation line names the count: every written line parses as
    // one record, so the count is the file's line count
    let count = from_bin.iter().filter(|b| **b == b'\n').count();
    assert!(
        stdout.contains(&format!("saved: {count} record(s) -> {}", bin_log.display())),
        "{stdout}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Q11: bare `inspect` names the usage and lists the table — it does NOT
/// guess a card.
/// The bin bytes are the library bytes for the same call, and the body's
/// shape is pinned: usage line first, every following line indented.
#[test]
fn bare_inspect_is_usage_plus_the_card_list_never_the_first_card() {
    let inputs = ["inspect", "quit"];
    let (code, stdout, stderr) = run_repl(NIGHTJAR_SCRIPT, &inputs);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stderr, "", "the session must run clean");

    // the library replay of the SAME call
    let mut game = nightjar_game();
    let expected = replay(&mut game, &inputs);
    assert_eq!(stdout, expected, "the bin is routing only");

    // the shape: the banner, the echo, then usage, then the INDENTED list up
    // to the next echo
    let banner_len = game.banner().expect("banner").len();
    let body: Vec<&str> = expected.lines().skip(banner_len + 1).collect();
    assert_eq!(body[0], "usage: inspect <card name>");
    let list: Vec<&&str> = body[1..].iter().take_while(|l| !l.starts_with('\u{bb}')).collect();
    assert!(!list.is_empty(), "the nightjar table is non-empty — the list must follow the usage");
    assert!(
        list.iter().all(|l| l.starts_with("  ")),
        "the card list is indented — no bare card face lines"
    );
}

/// The opening screen says what the game is about and what to do: the
/// title, the script's goal line, where and when the player stands, who
/// shares the place, a try row of typeable actions, the verbs, the help
/// hint — and `help` explains the grammar in the game's own vocabulary.
#[test]
fn the_banner_and_help_tell_a_new_player_what_to_type() {
    let (code, stdout, _) = run_repl(NIGHTJAR_SCRIPT, &["help", "quit"]);
    assert_eq!(code, Some(0));
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "── Murder on the Nightjar Flyer ──");
    assert!(lines[1].starts_with("§ unmask Lord Vane's killer"));
    assert_eq!(lines[2], "you are MARCH · at GUARD VAN · day 0, 21:00");
    assert_eq!(lines[3], "here: THE GUARD · THE INQUEST");
    assert!(lines[4].starts_with("try: "));
    assert!(lines[5].starts_with("verbs: read · talk · explore"));
    assert_eq!(lines[6], "type `help` for how to play");
    assert_eq!(lines[7], "\u{bb} help");
    assert_eq!(lines[8], "how to play");
    assert!(stdout.contains("people go BEFORE the verb"));
    assert!(stdout.contains(
        "views: look · cards · inspect <card> · story · journal · save <file> · undo · help · quit"
    ));
    // every try-row entry is a line the game accepts as a real turn
    let mut game = nightjar_game();
    let try_row = lines[4].trim_start_matches("try: ").split(" · ");
    for action in try_row {
        let turn = game.handle_line(action).expect("the suggestion executes");
        assert_eq!(turn.kind, TurnKind::Action, "{action}: {:?}", turn.lines);
        game.undo().expect("undo");
    }
}
