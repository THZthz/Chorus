//! Every compile diagnostic in language.md §9 (plus the adjacent unnumbered
//! failure shapes) fires on a minimal fixture, asserted by number, location,
//! and message/note substring. The `# ERR` comment marks the expected
//! diagnostic line in each fixture.

#[path = "diagnostics/adjacent.rs"]
mod adjacent;
#[path = "diagnostics/decls.rs"]
mod decls;
#[path = "diagnostics/effects.rs"]
mod effects;
#[path = "diagnostics/relations.rs"]
mod relations;
#[path = "diagnostics/registry.rs"]
mod registry;
#[path = "diagnostics/slots.rs"]
mod slots;

use story_content::{Diagnostic, Severity};

fn compile(src: &str) -> Vec<Diagnostic> {
    story_lang::compile_source("t.sw", src).diagnostics
}

fn marker_line(src: &str) -> u32 {
    for (i, line) in src.lines().enumerate() {
        if line.contains("# ERR") {
            return i as u32 + 1;
        }
    }
    panic!("fixture has no # ERR marker");
}

fn expect_error(src: &str, number: u32, contains: &str, note_contains: Option<&str>) {
    let diags = compile(src);
    let line = marker_line(src);
    let hit = diags
        .iter()
        .find(|d| d.severity == Severity::Error && d.number == number && d.line == line);
    let d = hit.unwrap_or_else(|| {
        panic!(
            "expected E{number} at t.sw:{line}; got:\n{}",
            diags
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    let hay = format!("{} {}", d.message, d.notes.join(" "));
    assert!(
        hay.contains(contains),
        "E{number} message mismatch: `{hay}` does not contain `{contains}`"
    );
    if let Some(note) = note_contains {
        let notes = d.notes.join(" ");
        assert!(
            notes.contains(note),
            "E{number} note mismatch: `{notes}` does not contain `{note}`"
        );
    }
}

fn expect_warning(src: &str, number: u32, contains: &str) {
    let diags = compile(src);
    let line = marker_line(src);
    let hit = diags
        .iter()
        .find(|d| d.severity == Severity::Warning && d.number == number && d.line == line);
    let d = hit.unwrap_or_else(|| {
        panic!(
            "expected W{number} at t.sw:{line}; got:\n{}",
            diags
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    assert!(
        d.message.contains(contains),
        "W{number} message mismatch: `{}`",
        d.message
    );
}

const PREAMBLE: &str = "\
game \"t\"
  seed host
  turn 1m
  home a

zones
  a

aspects
  heat: 0
";

fn fixture(body: &str) -> String {
    format!("{PREAMBLE}\n{body}\n")
}
