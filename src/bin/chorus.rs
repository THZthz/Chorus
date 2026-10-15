//! The chorus line REPL.
//!
//!   cargo run --bin chorus -- scripts/the-nightjar-flyer.txt
//!   printf 'look\ncards\nread the manifest\nwait\njournal\nquit\n' | cargo run --bin chorus -- scripts/the-nightjar-flyer.txt
//!
//! A plain line loop over a [`chorus::session::game::Game`], with the button
//! views folded in as typed meta commands. This layer is the simplified UI
//! face: everything below the dispatcher is the frozen semantic stack
//! (lenient → engine → render), byte-pinned elsewhere. The pieces it
//! preserves:
//!   - ONE dispatcher — inspect prefix, the meta words, everything
//!     else to `handle_line`; routing lives nowhere else.
//!   - the title comes from `Game::from_script` → `meta.title`, i.e.
//!     the script's `script:` directive.
//!   - the `« … »` canonical echo rides each action turn —
//!     handle_line's own lines, printed verbatim.
//!   - Q11: bare `inspect` is usage + the card list (Game::inspect's
//!     empty-name branch) — it never guesses the first visible card.
//!   - non-TTY runs fine: `IsTerminal` only decides whether the `» ` prompt
//!     is drawn, so a piped session's stdout is byte-clean (a readline
//!     prompt would interleave).
//!
//! Dispatcher collisions (recorded, not accidental): the meta words are the
//! button semantics, so bare `wait` means game.wait() (advance the
//! clock), NOT the WAIT verb. The verb stays reachable as `march wait` /
//! `ANA WAIT` / canonical `0:42 @ MARCH WAIT` — the scripted nightjar win
//! path in tests/repl.rs enters its first line in canonical form for exactly
//! this reason (same records, test pinned).

use std::io::{IsTerminal, Write};
use std::process::ExitCode;

use chorus::core::dict::core_dictionary;
use chorus::session::game::{Game, Turn};

fn main() -> ExitCode {
    // the script path is the one CLI argument — the script's own `script:`
    // line names the game
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first().filter(|f| !f.is_empty()) else {
        eprintln!("usage: chorus <script.txt>\n       a line REPL: type what happens, or look/cards/story/journal/undo/wait/inspect/quit");
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
    // the banner: title from meta, verbs lowercased with underscores spaced
    let banner = format!(
        "── {} ──\nverbs: {}\n",
        game.title(),
        game.content
            .verbs
            .keys()
            .map(|v| v.to_lowercase().replace('_', " "))
            .collect::<Vec<_>>()
            .join(" · ")
    );
    if out.write_all(banner.as_bytes()).is_err() {
        return ExitCode::from(1);
    }

    // the prompt is a TTY courtesy only — piped sessions stay byte-clean
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
    let trimmed = line.trim();
    // `/^inspect\b\s*(.*)$/` over the trimmed line; the
    // prefix wins over everything else (`inspector` does NOT match: \b)
    if let Some(rest) = trimmed.strip_prefix("inspect") {
        // JS \b is ASCII (\w = [A-Za-z0-9_]): a following word character
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
    // the meta commands (the button labels, as typed)
    match trimmed {
        "quit" => return Flow::Quit,
        "undo" => turn(game.undo(), out),
        "wait" => turn(game.wait(), out),
        "story" => result(game.story(), out),
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
        _ => turn(game.handle_line(line), out),
    }
    Flow::Continue
}

enum Flow {
    Continue,
    Quit,
}

/// a Turn's lines go to stdout verbatim — the `« … »` canonical echo is one
/// of them, preserved byte for byte
fn turn(t: chorus::error::Result<Turn>, out: &mut impl Write) {
    match t {
        Ok(t) => {
            for l in &t.lines {
                emit(out, l);
            }
        }
        Err(e) => {
            // an engine/render error is a crash; the message line on stderr
            // + exit 1 is the bin convention
            let _ = out.flush();
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

/// view commands that share the engine's Result channel (story's prose
/// rendering) — same crash convention as [`turn`]
fn result(r: chorus::error::Result<Vec<String>>, out: &mut impl Write) {
    match r {
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
