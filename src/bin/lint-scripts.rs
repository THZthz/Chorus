//! Lint bundled scripts: the load-time warnings of lint_script (§10's
//! "fail loudly", warn-loudly sibling). Exit code stays 0: warnings advise,
//! compile errors are `compile_script`'s job.
//!
//!   cargo run --bin lint-scripts                  # everything under scripts/
//!   cargo run --bin lint-scripts -- my-game.txt   # or name files
//!
//! Q15 honesty: the default file list comes from reading the `scripts`
//! dir RELATIVE TO THE CWD; a missing scripts dir names itself and the cwd
//! on stderr and exits 1 (warnings advise, exit 0; hard input errors exit 1),
//! and a missing named file is the same hard-error family.

use chorus::content::script::lint_script;
use chorus::core::dict::core_dictionary;

fn main() -> std::process::ExitCode {
    let dict = core_dictionary();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files: Vec<String> = if !args.is_empty() {
        args
    } else {
        // the default list: every scripts/*.txt relative to the CWD
        let dir = match std::fs::read_dir("scripts") {
            Ok(d) => d,
            Err(e) => {
                // Q15: name what failed and where — the bare io error
                // ("No such file or directory (os error 2)") never
                // mentioned the directory. Exit code unchanged (a hard
                // input error → 1).
                let cwd = std::env::current_dir()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "<cwd unavailable>".to_string());
                eprintln!(
                    "Error: no 'scripts' directory under '{cwd}' ({e}) — run from the repo root or name files: lint-scripts [file.txt ...]"
                );
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
                // an unreadable file is a hard error → exit 1
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
    println!("\n{} file(s), {} finding(s)", files.len(), findings);
    std::process::ExitCode::SUCCESS
}
