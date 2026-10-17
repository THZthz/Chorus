//! Lint bundled scripts: the load-time findings of lint_script (§10's
//! "fail loudly", warn-loudly sibling), now severity-classified (E11):
//!
//!   - ERROR — the pre-existing finding set (dead recipes, shadowed whens,
//!     count gates the slot cardinality forbids, unknown template tokens,
//!     unwritten aspect reads, unstackable deck entries, non-lexeme names).
//!     Errors keep today's behavior: counted in the finding total, and the
//!     exit code discipline is unchanged (constant 0 for findings — compile
//!     errors are `compile_script`'s job, hard input errors exit 1).
//!   - WARNING — a recipe whose `when` is PROVABLY never-matching (false for
//!     every §6.6 selection binding; E11). Warnings print `[warning]` with
//!     the recipe id + reason and never fail the exit: a dead `when` is
//!     sometimes the point — cultist-full.txt carries 76 warnings, pinned as
//!     a composition in tests/convert_cs_smoke.rs (36 deliberate dead-marker
//!     twins under `NOT (count(actors) …)` whens, plus 24 genuinely-CS-dead
//!     and 16 converter-induced NOT-alt total contradictions on TALK).
//!
//!   cargo run --bin lint-scripts                  # everything under scripts/
//!   cargo run --bin lint-scripts -- my-game.txt   # or name files
//!
//! Q15 honesty: the default file list comes from reading the `scripts`
//! dir RELATIVE TO THE CWD; a missing scripts dir names itself and the cwd
//! on stderr and exits 1 (warnings advise, exit 0; hard input errors exit 1),
//! and a missing named file is the same hard-error family.

use chorus::content::script::{lint_script_findings, LintSeverity};
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
        // readdir order is machine-dependent; a deterministic tool reports
        // the files in a sorted order
        names.sort();
        names
    };

    let mut findings = 0usize;
    let mut warnings = 0usize;
    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                // an unreadable file is a hard error → exit 1
                eprintln!("Error: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        let linted = match lint_script_findings(&text, &dict) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Error: {e}");
                return std::process::ExitCode::from(1);
            }
        };
        if linted.is_empty() {
            println!("{file}: clean");
            continue;
        }
        findings += linted.len();
        warnings += linted.iter().filter(|f| f.severity == LintSeverity::Warning).count();
        println!("{file}:");
        for f in &linted {
            match f.severity {
                // the ERROR class keeps today's line shape exactly
                LintSeverity::Error => println!("  - {}", f.message),
                LintSeverity::Warning => println!("  - [warning] {}", f.message),
            }
        }
    }
    if warnings > 0 {
        println!(
            "\n{} file(s), {} finding(s), {} warning(s) — warnings advise, the exit code stays 0",
            files.len(),
            findings,
            warnings
        );
    } else {
        println!("\n{} file(s), {} finding(s)", files.len(), findings);
    }
    std::process::ExitCode::SUCCESS
}
