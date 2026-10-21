//! Include splicing (language.md §1): textual splice at the include point,
//! one namespace across every included file, cycle [E231], cross-file
//! duplicate [E232].

use std::path::Path;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn includes_splice_into_one_compilation_unit() {
    let out = story_lang::compile_file(&fixture("include_main.sw"));
    assert!(
        out.diagnostics.is_empty(),
        "clean compile expected: {:?}",
        out.diagnostics
    );
    let content = out.content.unwrap();
    // Declarations from the included files are in the one database.
    assert!(content
        .cards
        .contains_key(&story_core::CardId::from("ember")));
    assert_eq!(content.aspects["spark"].default, 0);
    assert_eq!(content.verbs["tap"].recipes["strike"].duration, 5);
    // Meta pass-through rides the spliced declaration (spec.md §7).
    assert_eq!(
        content.cards[&story_core::CardId::from("ember")]
            .meta
            .get("ui_icon")
            .map(String::as_str),
        Some("ember")
    );
}

#[test]
fn include_cycle_is_e231() {
    let out = story_lang::compile_file(&fixture("cycle_a.sw"));
    let hit = out
        .diagnostics
        .iter()
        .find(|d| d.number == story_content::code::E231);
    let d = hit.expect("E231 expected for the include cycle");
    assert!(d.message.contains("cycle"), "{}", d.message);
}

#[test]
fn cross_file_duplicate_id_is_e232_at_second_site() {
    let out = story_lang::compile_file(&fixture("dup_main.sw"));
    let hit = out
        .diagnostics
        .iter()
        .find(|d| d.number == story_content::code::E232);
    let d = hit.expect("E232 expected for the cross-file duplicate");
    assert!(
        d.file.ends_with("dup_b.sw"),
        "reported at the second site: {d}"
    );
    assert!(
        d.notes.join(" ").contains("dup_a.sw"),
        "both sites named: {d}"
    );
}
