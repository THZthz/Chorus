//! Demo helper: play a few turns of a script through the library API and
//! save the action log as JSONL, so `reach --from-state` has a real midgame
//! save to analyze.
//!
//!   cargo run --example make-midgame-log -- scripts/cultist-slice.txt /tmp/midgame.jsonl
//!
//! The moves below: work twice, wait for the windows to settle, then wait
//! on — a small but non-initial world.

use std::path::PathBuf;

use chorus::core::dict::core_dictionary;
use chorus::session::game::Game;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let script = args.next().unwrap_or_else(|| "scripts/cultist-slice.txt".into());
    let out: PathBuf = args.next().unwrap_or_else(|| "/tmp/midgame.jsonl".into()).into();

    let src = std::fs::read_to_string(&script)?;
    let mut game = Game::from_script(&src, core_dictionary())?;

    for line in ["work with vigor", "work with wits", "wait", "wait"] {
        let turn = game.handle_line(line)?;
        if let Err(e) = game.wait() {
            eprintln!("(after `{line}`): {e}");
        }
        let _ = turn; // transcript noise is not wanted here; the log is the product
    }

    chorus::engine::store::save_log(&out, &game.log.records)?;
    println!("{} records -> {}", game.log.records.len(), out.display());
    Ok(())
}
