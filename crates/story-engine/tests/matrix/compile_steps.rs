//! Entries 18, 19, 20 — compile-step machinery: effectset expansion, include splicing, meta pass-through.

use super::*;

// matrix 18
/// An `effectset` call expands at its call site: parameters splice into
/// the expanded effects, and macros may call macros.
#[test]
fn matrix_18_effectset_pos() {
    let src = r#"
game "Stoke"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  heat: 0
  coal: 0

cards
  card coal
    stackable
    aspects coal 1
    start in hall count 4

  card beacon
    unique
    start in hall

effectset stoke(n)
  effects
    consume aspect:coal n
    set beacon.heat += n

effectset double_stoke(n)
  effects
    stoke(n)
    stoke(n)

verbs
  verb stoker committable instances 1
    recipe work
      duration 0
      effects
        stoke(2)
        double_stoke(1)
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "stoker", &[]).expect("works");
    let beacon = common::named(&e, "beacon");
    assert_eq!(
        aspect(&e, beacon, "heat"),
        4,
        "2 + 1 + 1 through two macros"
    );
    assert_eq!(count_named(&e, "coal"), 0, "2 + 1 + 1 coals burned");
}

// matrix 18
/// An integer-expression argument splices into positions that parse full
/// expressions (set), and must expand whole everywhere — the language
/// reference markets `stoke(1 + total(...))` as legal (language.md §6).
/// Pins the spec reading; see the findings list in the M2 report.
#[test]
fn matrix_18_effectset_expr_argument() {
    let src = r#"
game "Stoke"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  heat: 0
  coal: 0

cards
  card coal
    stackable
    aspects coal 1
    start in hall count 4

  card beacon
    unique
    start in hall

effectset stoke(n)
  effects
    consume aspect:coal n
    set beacon.heat += n

verbs
  verb stoker committable instances 1
    recipe work
      duration 0
      effects
        stoke(1 + 1)
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "stoker", &[]).expect("works");
    assert_eq!(
        count_named(&e, "coal"),
        2,
        "the full expression is the count"
    );
    let beacon = common::named(&e, "beacon");
    assert_eq!(aspect(&e, beacon, "heat"), 2);
}

// matrix 18
/// Clause termination: a count expression ends at a clause keyword on
/// the same line (`spawn ... count 1 + 1 into hall`) and at end of line
/// (`consume aspect:coal 2 - 1`), and the next line still parses as its
/// own effect.
#[test]
fn matrix_18_expr_clause_termination() {
    let src = r#"
game "Kiln"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  coal: 0

cards
  card coal
    stackable
    aspects coal 1
    start in hall count 4

verbs
  verb stoker committable instances 1
    recipe work
      duration 0
      effects
        spawn coal count 1 + 1 into hall
        consume aspect:coal 2 - 1
        start helper

  verb helper uncommittable instances any
    recipe side
      chain-only
      duration 10m
      effects
        narrate "helper"
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "stoker", &[]).expect("works");
    assert_eq!(
        count_named(&e, "coal"),
        5,
        "4 + (1 + 1) spawned, (2 - 1) consumed"
    );
    assert_eq!(recipe_runs(&e, "helper.side"), 1, "the next line started");
}

// matrix 18
/// Unknown macros and wrong arities are refused at compile time.
#[test]
fn matrix_18_effectset_neg() {
    let base = r#"
game "Stoke"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  heat: 0

cards
  card coal
    stackable
    start in hall count 4

  card beacon
    unique
    start in hall

effectset stoke(n)
  effects
    consume aspect:coal n
    set beacon.heat += n

verbs
  verb stoker committable instances 1
    recipe work
      duration 0
      effects
        %CALL%
"#;
    assert!(
        compile_error_codes(&base.replace("%CALL%", "stoke_wrong(2)")).contains(&code::E233)
    );
    assert!(compile_error_codes(&base.replace("%CALL%", "stoke(1, 2)")).contains(&code::E234));
}

// matrix 19
/// An include splices its declarations into the one namespace: the
/// root's verbs read and bind what the included file declares.
#[test]
fn matrix_19_include_splicing_pos() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/matrix_inc_root.sw");
    let out = story_lang::compile_file(&path);
    let errors: Vec<String> = out
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect();
    assert!(errors.is_empty(), "clean splice: {errors:?}");
    let mut e = boot(
        std::sync::Arc::new(out.content.expect("no errors means content")),
        1,
    );
    assert_eq!(count_named(&e, "gem"), 2, "the included boot face applied");
    let events = common::commit_named(&mut e, "use_included", &[("gem", "gem")])
        .expect("the spliced card binds");
    assert_eq!(narration(&events), vec!["charged 3"]);
}

// matrix 19
/// A duplicate id across two included files is [E232] at the second
/// declaration site, and the diagnostic names both sites: the primary
/// location and the `first declared at` note.
#[test]
fn matrix_19_include_splicing_neg() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/matrix_inc_dup.sw");
    let out = story_lang::compile_file(&path);
    let d = out
        .diagnostics
        .iter()
        .find(|d| d.number == code::E232 && d.is_error())
        .expect("E232 expected")
        .clone();
    assert!(
        d.file.ends_with("matrix_inc_dup_b.sw") && d.line == 3,
        "the primary location is the second site: {d}"
    );
    assert!(
        d.notes
            .iter()
            .any(|n| n.contains("matrix_inc_dup_a.sw") && n.contains(":3")),
        "the note names the first site: {d}"
    );
}

// matrix 20
/// Meta annotations ride the compiled content and never touch the
/// kernel: the engine boots and plays meta-carrying content normally.
#[test]
fn matrix_20_meta_passthrough_pos() {
    let src = r#"
game "Meta"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall count 2
    meta ui_icon "coin"

verbs
  verb poker committable instances 1
    meta ui_hint "poke it"
    recipe jab
      duration 0
      slots
        s: 1..1 aspect:spark
      effects
        narrate "poked {s}"
"#;
    let content = compile(src);
    assert_eq!(
        content.cards[&story_core::CardId::from("coin")]
            .meta
            .get("ui_icon")
            .map(String::as_str),
        Some("coin"),
        "the kernel stores what it never reads"
    );
    assert_eq!(
        content.verbs["poker"]
            .meta
            .get("ui_hint")
            .map(String::as_str),
        Some("poke it")
    );
    let mut e = boot(content, 1);
    let events = common::commit_named(&mut e, "poker", &[("s", "coin")]).expect("plays");
    assert_eq!(narration(&events), vec!["poked coin"]);
}

// matrix 20
/// A repeated meta key is a compile refusal, and meta changes no
/// behavior: two contents differing only in meta reach identical states.
#[test]
fn matrix_20_meta_passthrough_neg() {
    let plain = r#"
game "Meta"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall count 2

verbs
  verb poker committable instances 1
    recipe jab
      duration 0
      slots
        s: 1..1 aspect:spark
      effects
        narrate "poked"
"#;
    let dup = plain.replace(
        "    start in hall count 2",
        "    start in hall count 2\n    meta ui_icon \"coin\"\n    meta ui_icon \"coin\"",
    );
    assert!(compile_error_codes(&dup).contains(&code::E236));

    let meta = plain.replace(
        "    start in hall count 2",
        "    start in hall count 2\n    meta ui_icon \"coin\"",
    );
    let mut a = boot(compile(plain), 1);
    let mut b = boot(compile(&meta), 1);
    common::commit_named(&mut a, "poker", &[("s", "coin")]).expect("plays");
    common::commit_named(&mut b, "poker", &[("s", "coin")]).expect("plays");
    assert_eq!(
        state_bytes(&a),
        state_bytes(&b),
        "meta never reaches the state"
    );
}
