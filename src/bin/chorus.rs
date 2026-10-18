//! The chorus line REPL.
//!
//!   cargo run --bin chorus -- scripts/the-nightjar-flyer.txt
//!   printf 'look\ncards\nread the manifest\nwait\njournal\nquit\n' | cargo run --bin chorus -- scripts/the-nightjar-flyer.txt
//!
//! A plain line loop over a [`chorus::session::game::Game`], with the meta
//! views folded in as typed words. This layer is the UI face: everything
//! below the dispatcher is the semantic stack (lenient → engine → render),
//! pinned elsewhere, and the bin is routing only — every line it prints
//! comes from a `Game` method (tests/session/repl.rs replays a session
//! through the library and matches the bin's stdout byte for byte):
//!   - ONE dispatcher — inspect prefix, the meta words, everything
//!     else to `handle_line`; routing lives nowhere else.
//!   - the banner is `Game::banner` (title, goal, status, try row, verbs);
//!     a turn's lines are followed by `Game::after_turn` (the stations
//!     still processing and the try row), so the player always sees what
//!     is cooking and what to type next.
//!   - Q11: bare `inspect` is usage + the card list (Game::inspect's
//!     empty-name branch) — it never guesses the first visible card.
//!   - `IsTerminal` decides the prompt: a TTY draws `» ` before each read;
//!     a piped session instead ECHOES each line as `» line` so a
//!     transcript reads like a terminal session (input visible among the
//!     output).
//!
//! Dispatcher collisions (recorded, not accidental): the meta words are
//! typed views, so bare `wait` means game.wait() (advance the
//! clock), NOT the WAIT verb. The verb stays reachable as `march wait` /
//! `ANA WAIT` / canonical `0:42 @ MARCH WAIT`.
//!
//! The `save <file>` meta word writes the JSONL action log (Game::save) —
//! the file `reach --from-state` replays as a midgame world; a bare `save`
//! answers with the usage line, and a word like `saved` is no save at all
//! (the ASCII `\b` class) and falls through to handle_line.

use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use chorus::core::dict::core_dictionary;
use chorus::core::text::dsl_trim;
use chorus::session::game::Game;

/// The no-arguments face and the tail of every argv error: one usage line
/// with the meta words, plus the `chorus --help` hint (the cargo/uv
/// convention), stderr, exit 2.
const SHORT_USAGE: &str = "usage: chorus <script.txt>\n       a line REPL: type what happens, or help/look/cards/story/journal/save/undo/wait/inspect/quit\ntry 'chorus --help' for how to play\n";

/// The full `--help` text (stdout, exit 0) — the plain-text style of the
/// reach/lint-scripts helps: usage, the meta words, one option, examples.
const HELP: &str = "\
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

fn main() -> ExitCode {
    // one CLI argument — the script path (the script's own `script:` line
    // names the game). `--help`/`-h` anywhere wins; anything else starting
    // with `-` is an unknown flag, and a second positional word is named,
    // never silently dropped
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.is_empty()).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return ExitCode::from(0);
    }
    let mut script: Option<&String> = None;
    for a in &args {
        if a.starts_with('-') && a.len() > 1 {
            eprint!("unknown flag '{a}'\n{SHORT_USAGE}");
            return ExitCode::from(2);
        }
        if script.replace(a).is_some() {
            eprint!("unexpected argument '{a}'\n{SHORT_USAGE}");
            return ExitCode::from(2);
        }
    }
    let Some(file) = script else {
        eprint!("{SHORT_USAGE}");
        return ExitCode::from(2);
    };

    // read the script file; on failure print the message line on stderr and
    // exit 1 (bin convention)
    let dict = core_dictionary();
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let mut game = match Game::from_script(&text, dict) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    // the banner: title, goal, status, the try row, the verbs
    emit_all(game.banner(), &mut out);

    // a TTY draws the prompt; a piped session echoes the line instead
    let tty = std::io::stdin().is_terminal();

    let stdin = std::io::stdin();
    loop {
        if tty {
            let _ = out.write_all("\u{bb} ".as_bytes());
            let _ = out.flush();
        }
        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break, // EOF ends the session
            Ok(_) => {}
            Err(_) => return ExitCode::from(1),
        }
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }
        if !tty {
            emit(&mut out, &format!("\u{bb} {line}"));
        }
        if let Flow::Quit = dispatch(&mut game, &line, &mut out) {
            break;
        }
        if out.flush().is_err() {
            return ExitCode::from(1);
        }
    }
    ExitCode::from(0)
}

/// one dispatched line — the single routing point (the inspect and save
/// prefixes and the meta commands, converged here).
fn dispatch(game: &mut Game, line: &str, out: &mut impl Write) -> Flow {
    // the DSL whitespace class (Q8): a pasted BOM or a non-ASCII space
    // before `quit` must not turn the word into a failed action
    let trimmed = dsl_trim(line);
    // `/^inspect\b\s*(.*)$/` over the trimmed line; the
    // prefix wins over everything else (`inspector` does NOT match: \b)
    if let Some(rest) = trimmed.strip_prefix("inspect") {
        // the `\b` word-boundary class is ASCII (`\w` = [A-Za-z0-9_]): a following word
        // (`inspector`, `inspect5`, `inspect_x`) means NO boundary and the
        // line falls through to handle_line, exactly as the regex demands
        if rest.is_empty() || rest.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            // Q11: bare `inspect` is usage + the card list (Game::inspect's
            // empty-name branch) — no first-card guess — and `inspect  x`
            // trims the rest
            for l in game.inspect(rest.trim()) {
                emit(out, &l);
            }
            return Flow::Continue;
        }
        // a word like `inspector …` falls through to handle_line (the regex
        // would also reject it: \b fails inside a word)
    }
    // `/^save\b\s*(.*)$/` over the trimmed line; the SAME ASCII
    // word-boundary class as the inspect prefix: a following word
    // (`saved`, `saving`, `save5`) means NO boundary and the line falls
    // through to handle_line
    if let Some(rest) = trimmed.strip_prefix("save") {
        if rest.is_empty() || rest.starts_with(|c: char| !c.is_ascii_alphanumeric() && c != '_') {
            // bare `save` is the usage line (Game::save's empty-path
            // branch); `save  x` trims the rest — one line either way
            emit_all(game.save(rest.trim()).map(|l| vec![l]), out);
            return Flow::Continue;
        }
        // a word like `saved …` falls through (the boundary fails)
    }
    // the meta commands (the typed views); the turns — undo, wait, a line of
    // play — are followed by the live area (what is processing, what to try)
    match trimmed {
        "quit" => return Flow::Quit,
        "help" => emit_all(game.help(), out),
        "undo" => {
            emit_all(game.undo().map(|t| t.lines), out);
            emit_all(game.after_turn(), out);
        }
        "wait" => {
            emit_all(game.wait().map(|t| t.lines), out);
            emit_all(game.after_turn(), out);
        }
        "story" => emit_all(game.story(), out),
        "journal" => {
            for l in game.journal() {
                emit(out, &l);
            }
        }
        "look" => {
            for l in game.look() {
                emit(out, &l);
            }
        }
        "cards" => {
            for l in game.cards() {
                emit(out, &l);
            }
        }
        // everything else is a turn of play
        _ => {
            emit_all(game.handle_line(line).map(|t| t.lines), out);
            emit_all(game.after_turn(), out);
        }
    }
    Flow::Continue
}

enum Flow {
    Continue,
    Quit,
}

/// a turn's lines go to stdout verbatim — the `« … »` canonical echo is one
/// of them, preserved byte for byte; the view commands share the engine's
/// Result channel (story's prose rendering). An engine/render error is a
/// crash: the message line on stderr + exit 1 is the bin convention.
fn emit_all(t: chorus::error::Result<Vec<String>>, out: &mut impl Write) {
    match t {
        Ok(lines) => {
            for l in &lines {
                emit(out, l);
            }
        }
        Err(e) => {
            let _ = out.flush();
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

fn emit(out: &mut impl Write, line: &str) {
    let _ = out.write_all(line.as_bytes());
    let _ = out.write_all(b"\n");
}
