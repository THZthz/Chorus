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

use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use chorus::core::dict::core_dictionary;
use chorus::core::text::dsl_trim;
use chorus::session::game::Game;

fn main() -> ExitCode {
    // the script path is the one CLI argument — the script's own `script:`
    // line names the game
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first().filter(|f| !f.is_empty()) else {
        eprintln!("usage: chorus <script.txt>\n       a line REPL: type what happens, or help/look/cards/story/journal/undo/wait/inspect/quit");
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

/// one dispatched line — the single routing point (the inspect prefix and
/// the meta commands, converged here).
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
