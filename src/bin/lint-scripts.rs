//! Lint bundled scripts — ts: tools/lint-cli.ts, ported byte for byte: the
//! load-time warnings of lintScript (§10's "fail loudly", warn-loudly
//! sibling). Exit code stays 0: warnings advise, errors are compileScript's
//! job.
//!
//!   cargo run --bin lint-scripts                  # everything under scripts/
//!   cargo run --bin lint-scripts -- my-game.txt   # or name files
//!
//! Frozen quirks (verification 03 CLI-05): the default file list comes from
//! `readdirSync('scripts')` RELATIVE TO THE CWD; a missing scripts dir or a
//! missing named file is an uncaught throw → exit 1 (contradicting the header
//! comment's "exit 0" — the CURRENT behavior is what is frozen, message line
//! plus exit code; the JS stack after it is not reproducible and is a
//! recorded divergence).

use chorus::content::script::lint_script;
use chorus::core::dict::core_dictionary;

fn main() -> std::process::ExitCode {
    let dict = core_dictionary();
    // ts: lint-cli.ts:13-16
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files: Vec<String> = if !args.is_empty() {
        args
    } else {
        // ts: readdirSync('scripts').filter(f => f.endsWith('.txt'))
        //     .map(f => join('scripts', f))
        let dir = match std::fs::read_dir("scripts") {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Error: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let mut names: Vec<String> = Vec::new();
        for entry in dir {
            match entry {
                Ok(e) => {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.ends_with(".txt") {
                        names.push(format!("scripts/{name}"));
                    }
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    return std::process::ExitCode::from(1);
                }
            }
        }
        names
    };

    let mut findings = 0usize;
    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                // ts: readFileSync throws uncaught → exit 1
                eprintln!("Error: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let warnings = match lint_script(&text, &dict) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Error: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        if warnings.is_empty() {
            println!("{file}: clean");
            continue;
        }
        findings += warnings.len();
        println!("{file}:");
        for w in &warnings {
            println!("  - {w}");
        }
    }
    // ts: lint-cli.ts:29 — console.log(`\n${files.length} file(s), ${findings} finding(s)`)
    println!("\n{} file(s), {} finding(s)", files.len(), findings);
    std::process::ExitCode::SUCCESS
}
