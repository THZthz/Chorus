//! Lint bundled scripts: the load-time findings of lint_script (§10's
//! "fail loudly" sibling), severity-classified (E11):
//!
//!   - ERROR — the load-time finding set (dead recipes, shadowed whens,
//!     count gates the slot cardinality forbids, unknown template tokens,
//!     unwritten aspect reads, unstackable deck entries, non-lexeme names).
//!     Errors are counted in the finding total and the exit code discipline
//!     holds: constant 0 for findings — compile errors are
//!     `compile_script`'s job, hard input errors exit 1.
//!   - WARNING — a recipe whose `when` is PROVABLY never-matching (false for
//!     every §6.6 selection binding; E11). Warnings print `[warning]` with
//!     the recipe id + reason and never fail the exit: a dead `when` is
//!     sometimes the point (a deliberately parked clause documents a branch
//!     the world can no longer reach), so this class only advises.
//!
//!   cargo run --bin lint-scripts
//!   cargo run --bin lint-scripts -- my-game.txt
//!   cargo run --bin lint-scripts -- --reach my-game.txt
//!
//! `--reach` (anywhere in the arguments, combinable with named files)
//! appends the lint bridge's findings (§13.1 → §13.2): the reach analyzer
//! run over that script's compiled (content, initial) — ONE `Analyzer` per
//! script, so on large scripts the flag adds a moment of
//! analysis — re-rendered as WARNING-class design smells the per-site lint
//! cannot see (dead verbs, self-gated lists, risky owners, frozen deck
//! cursors, the freeze boundary, REFUSE-doomed sites). Each line prints
//! prefixed `[reach]` so the two layers read apart in the one stream; the
//! summary names them separately. A reach finding is advisory by the §13.2
//! rule — it never changes the exit code — and without the flag the output
//! is byte-identical to the plain lint.
//!
//! The argv surface is the single-purpose Options grammar (no
//! subcommands, the cargo fmt shape): `lint-scripts [OPTIONS] [FILES...]`.
//! `--help`/`-h` anywhere prints the full help on stdout at exit 0; an
//! unknown `-`-flag is the argv error (exit 2, stderr, naming the flag —
//! never a silent fallthrough into the file list and a bogus read error,
//! the Q15 honesty tradition) while ANY non-flag word stays a file path,
//! the empty list meaning the `scripts/` default. The exit discipline is
//! unchanged: findings (errors included) exit 0, hard input errors exit 1,
//! argv errors exit 2.
//!
//! `--color` runs the one opt-in styling switch (`cli::Styles`, the same
//! switch `reach --color` flips): the per-file title bold, each finding
//! row's bullet + severity marker in the severity color (errors red,
//! `[warning]` yellow, `[reach]` magenta — the message text untouched), the
//! summary's non-zero counts in their severity colors and an all-zero
//! summary green whole. Without the flag the styler is the identity
//! transform and every face is byte-identical to the plain one.
//!
//! Q15 honesty: the default file list comes from reading the `scripts`
//! dir RELATIVE TO THE CWD; a missing scripts dir names itself and the cwd
//! on stderr and exits 1 (warnings advise, exit 0; hard input errors exit 1),
//! and a missing named file is the same hard-error family.

use std::io::Write;
use std::process::ExitCode;

use chorus::cli::Styles;
use chorus::content::script::{compile_script, lint_script_findings, LintFinding, LintSeverity};
use chorus::core::dict::core_dictionary;
use chorus::reach::reach_findings;

/// One parsed invocation: the two option flags plus the file list (any
/// non-flag word; the empty list meaning the `scripts/` default).
struct Options {
    reach: bool,
    color: bool,
    files: Vec<String>,
}

/// The short usage: the stderr face of the argv error (with the `--help`
/// hint — the cargo/uv convention).
const SHORT_USAGE: &str =
    "usage: lint-scripts [OPTIONS] [FILES...]\ntry 'lint-scripts --help' for the options";

/// The full help (stdout, exit 0): the args shape, the option table (each
/// flag one sentence) and three real invocations over the bundled scripts.
fn help_text() -> String {
    "\
lint-scripts — the script lint (§13.2) over named or bundled scripts

usage: lint-scripts [OPTIONS] [FILES...]

Args:
  FILES...    the scripts to lint; the default (no FILES) reads every scripts/*.txt relative to the CWD

Options:
  --reach     append the reach bridge's design-smell findings (the §13.1 → §13.2 six families; advisory — a reach finding never changes the exit code)
  --color     ANSI color (default off; when off the output is byte-identical to the plain face)
  -h, --help  print help

examples:
  lint-scripts
  lint-scripts --reach scripts/the-nightjar-flyer.txt
  lint-scripts --color scripts/the-nightjar-flyer.txt
"
    .to_string()
}

enum ParseOutcome {
    Help,
    Run(Options),
}

/// The grammar: `--help`/`-h` anywhere wins; the two option flags parse
/// position-free; any OTHER `-`-prefixed word is the argv error naming it,
/// and every non-flag word is a file path — the frozen semantics.
fn parse_args(args: &[String]) -> Result<ParseOutcome, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(ParseOutcome::Help);
    }
    let mut opts = Options { reach: false, color: false, files: Vec::new() };
    for a in args {
        match a.as_str() {
            "--reach" => opts.reach = true,
            "--color" => opts.color = true,
            // the bare `-` stays a file path; any longer `-`-word that is
            // not a known flag never reaches the file list
            other if other.starts_with('-') && other.len() > 1 => {
                return Err(format!("unknown flag '{other}'\n{SHORT_USAGE}"));
            }
            other => opts.files.push(other.to_string()),
        }
    }
    Ok(ParseOutcome::Run(opts))
}

/// The one stdout exit for every face: colorize (the identity transform
/// when --color is absent) then write. Called once on success and once
/// before every hard-error return — the pre-color bin printed line by
/// line, so the error face keeps the findings already on stdout. A failed
/// write is the hard-error family (exit 1), the reach bin's discipline.
fn flush_face(out: &mut dyn Write, styles: &Styles, face: &str) -> bool {
    let painted = styles.colorize(face);
    out.write_all(painted.as_bytes()).is_ok()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args) {
        Ok(ParseOutcome::Help) => {
            print!("{}", help_text());
            ExitCode::SUCCESS
        }
        Ok(ParseOutcome::Run(opts)) => run(opts),
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

fn run(opts: Options) -> ExitCode {
    let styles = Styles::new(opts.color);
    let mut out = std::io::stdout().lock();
    let mut face = String::new();

    // the file list: named files, empty meaning the default — every
    // scripts/*.txt relative to the CWD. No dictionary is built here: each
    // file is linted against its own fresh core dictionary (in the
    // loop).
    let files: Vec<String> = if !opts.files.is_empty() {
        opts.files
    } else {
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
                return ExitCode::from(1);
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
                    return ExitCode::from(1);
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
    let mut reach_findings_count = 0usize;
    for file in &files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(e) => {
                // an unreadable file is a hard error → exit 1
                let _ = flush_face(&mut out, &styles, &face);
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        };
        // a FRESH core dictionary per file: scripts declare
        // vocabulary at load, and linting N files with one shared dictionary
        // would make file isolation a property of the load's internal clone
        // discipline rather than of the tool — each file linted against its
        // own core dictionary can never see another file's declarations
        let dict = core_dictionary();
        let linted = match lint_script_findings(&text, &dict) {
            Ok(f) => f,
            Err(e) => {
                let _ = flush_face(&mut out, &styles, &face);
                eprintln!("Error: {e}");
                return ExitCode::from(1);
            }
        };
        // the bridge layer: the lint only runs on scripts that already
        // compile (lint_script_findings compiles internally), so this cannot
        // fail for a file that reached here — the error arm keeps the bin's
        // hard-error discipline anyway
        let bridged: Vec<LintFinding> = if opts.reach {
            // the bridge's analyzer reads the SAME fresh per-file dictionary
            // the lint used — a script's vocabulary is part of its compiled
            // face, so the bridge must see exactly what the lint saw
            match compile_script(&text, &dict) {
                Ok(cs) => reach_findings(&cs.content, &cs.state, &dict),
                Err(e) => {
                    let _ = flush_face(&mut out, &styles, &face);
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            }
        } else {
            Vec::new()
        };
        if linted.is_empty() && bridged.is_empty() {
            face.push_str(&format!("{file}: clean\n"));
            continue;
        }
        findings += linted.len();
        warnings += linted.iter().filter(|f| f.severity == LintSeverity::Warning).count();
        reach_findings_count += bridged.len();
        face.push_str(&format!("{file}:\n"));
        for f in &linted {
            match f.severity {
                // the ERROR class keeps today's line shape exactly
                LintSeverity::Error => face.push_str(&format!("  - {}\n", f.message)),
                LintSeverity::Warning => {
                    face.push_str(&format!("  - [warning] {}\n", f.message));
                }
            }
        }
        // the bridge's WARNINGs, marked so the two layers read apart
        for f in &bridged {
            face.push_str(&format!("  - [reach] {}\n", f.message));
        }
    }
    let summary = if reach_findings_count > 0 {
        format!(
            "\n{} file(s), {} finding(s), {} warning(s), {} reach finding(s) — warnings and \
             reach findings advise, the exit code stays 0\n",
            files.len(),
            findings,
            warnings,
            reach_findings_count
        )
    } else if warnings > 0 {
        format!(
            "\n{} file(s), {} finding(s), {} warning(s) — warnings advise, the exit code stays 0\n",
            files.len(),
            findings,
            warnings
        )
    } else {
        format!("\n{} file(s), {} finding(s)\n", files.len(), findings)
    };
    face.push_str(&summary);
    if !flush_face(&mut out, &styles, &face) {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
