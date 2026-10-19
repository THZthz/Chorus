//! The ambient verb's load face (§6.9/§10): the bare `ambient` line parses
//! anywhere in the block before `recipes:`, duplicates are the uniform load
//! error, slots are a compile error (held included), a top-level REFUSE in
//! an ambient recipe is a load error (branch-internal REFUSE keeps the
//! §6.7 window-refuse), and the dead-ambient verb joins the §13.2
//! dead-verb error family.

use chorus::content::script::{compile_script, lint_script_findings, LintSeverity};
use chorus::core::dict::core_dictionary;

use crate::common::harness::assert_err_msg;

/// The shared base: an ambient verb needs no slots and no player.
fn base(verbs: &str) -> String {
    format!(
        "
script: Ambient Load
player: ANA
start: 0:0

entities:
  ANA is character at HOUSE
  SEASON is idea with season:1 at HOUSE
  HOUSE is place

verbs:
{verbs}
"
    )
}

#[test]
fn the_bare_ambient_line_compiles() {
    let text = base(
        "  TIME:
    ambient
    duration: 1h
    recipes:
      time.pulse dur 10m when total(table, aspect:season) >= 1 -> NARRATE \"spring.\"
      default -> NARRATE \"turn.\"
",
    );
    let compiled = compile_script(&text, &core_dictionary()).unwrap();
    let v = compiled.content.verbs.get("TIME").unwrap();
    assert!(v.ambient, "the flag lands on the compiled verb");
    assert!(v.slots.is_empty());
}

#[test]
fn the_ambient_line_sits_anywhere_before_recipes() {
    // after `duration:`, beside the (absent) slots — order inside the block
    // is free
    let text = base(
        "  TIME:
    duration: 1h
    ambient
    recipes:
      default -> NARRATE \"turn.\"
",
    );
    let compiled = compile_script(&text, &core_dictionary()).unwrap();
    assert!(compiled.content.verbs.get("TIME").unwrap().ambient);
}

#[test]
fn a_duplicate_ambient_line_is_a_load_error() {
    let text = base(
        "  TIME:
    ambient
    ambient
    duration: 1h
    recipes:
      default -> NARRATE \"turn.\"
",
    );
    assert_err_msg(
        compile_script(&text, &core_dictionary()),
        r#"duplicate 'ambient' line in verb TIME"#,
    );
}

#[test]
fn a_kv_shaped_ambient_is_the_slot_role_error() {
    // `ambient: …` is the kv shape and dies on the slot role vocabulary —
    // the bare token is the whole grammar
    let text = base(
        "  TIME:
    ambient: true
    duration: 1h
    recipes:
      default -> NARRATE \"turn.\"
",
    );
    assert_err_msg(compile_script(&text, &core_dictionary()), r#"bad slot role 'ambient'"#);
}

#[test]
fn an_ambient_verb_declaring_a_slot_is_a_compile_error() {
    let text = base(
        "  TIME:
    ambient
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE \"turn.\"
",
    );
    assert_err_msg(
        compile_script(&text, &core_dictionary()),
        r#"verb TIME: an ambient verb declares no slots .*\(got slot 'actors'\)"#,
    );
}

#[test]
fn a_held_slot_on_an_ambient_verb_is_a_load_error() {
    // the held slot is a slot — the same compile error names it
    let text = base(
        "  TIME:
    ambient
    held: 0..1 kind:idea
    duration: 1h
    recipes:
      default -> NARRATE \"turn.\"
",
    );
    assert_err_msg(
        compile_script(&text, &core_dictionary()),
        r#"an ambient verb declares no slots .*\(got slot 'held'\)"#,
    );
}

#[test]
fn a_top_level_refuse_in_an_ambient_recipe_is_a_load_error() {
    let text = base(
        "  TIME:
    ambient
    duration: 1h
    recipes:
      time.gate when total(table, aspect:season) >= 1 -> REFUSE(\"no.\")
      default -> NARRATE \"turn.\"
",
    );
    assert_err_msg(
        compile_script(&text, &core_dictionary()),
        r#"recipe time.gate: a top-level REFUSE has no step to refuse on an ambient verb .* gate with `when`"#,
    );
}

#[test]
fn a_refuse_inside_an_if_branch_compiles() {
    // the branch-internal REFUSE keeps the §6.7 window-refuse — only the
    // top level is banned (its home is the table, which the sweep never
    // sees)
    let text = base(
        "  TIME:
    ambient
    duration: 1h
    recipes:
      default -> IF total(table, aspect:season) >= 1 -> ( NARRATE \"spring.\" ) ELSE -> ( REFUSE(\"winter.\") )
",
    );
    let compiled = compile_script(&text, &core_dictionary());
    assert!(compiled.is_ok(), "{compiled:?}");
}

#[test]
fn an_ambient_verb_with_no_recipes_is_the_dead_verb_error_family() {
    // a compile-clean script; the lint ERRORS (the §13.2 family a verb with
    // slots and no recipes already sits in)
    let text = base(
        "  TIME:
    ambient
    duration: 1h
  WORK:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE \"worked.\"
",
    );
    let findings = lint_script_findings(&text, &core_dictionary()).unwrap();
    let dead: Vec<_> = findings
        .iter()
        .filter(|f| f.severity == LintSeverity::Error && f.message.contains("TIME"))
        .map(|f| f.message.as_str())
        .collect();
    assert_eq!(dead.len(), 1, "{dead:?}");
    assert!(dead[0].contains("declares no recipes"), "{dead:?}");
    assert!(dead[0].contains("never opens a window"), "{dead:?}");
}

#[test]
fn the_when_families_apply_to_ambient_verbs_as_is() {
    // the held-scope read on a verb with no held slot (an ambient verb
    // never declares one) stays the existing dead-content ERROR
    let text = base(
        "  TIME:
    ambient
    duration: 1h
    recipes:
      time.held when count(held) >= 1 -> NARRATE \"turn.\"
      default -> NARRATE \"turn.\"
",
    );
    let findings = lint_script_findings(&text, &core_dictionary()).unwrap();
    assert!(
        findings.iter().any(|f| f.message.contains("held scope")),
        "{:?}",
        findings.iter().map(|f| &f.message).collect::<Vec<_>>()
    );
}
