//! The round-trip acceptance (plan.md M0): the compiled `Content`
//! serializes, deserializes, and refingerprints identically; recompiling the
//! same source is fingerprint-stable too.

use story_content::ir::EffectNode;

const SRC: &str = r#"
game "Roundtrip"
  seed 42
  turn 10m
  home deck_zone

zones
  deck_zone

aspects
  charge: 0

predicates
  predicate charged = core.charge >= 1

cards
  card core
    kind engine  unique
    aspects charge 1
    start in deck_zone

  card cell
    kind part  stackable
    aspects charge 2

effectset charge_up(n)
  effects
    set core.charge += n

decks
  deck parts
    entries cell, cell, core
    wrap

legacies
  cycles: 0

endings
  ending done "Done" grand
    text "Cycles counted: {legacy.cycles}."

on event tick
  rule t once when charged
    effects
      legacy cycles += 1

verbs
  verb work committable instances 1
    title "Work"
    recipe go
      craftable
      duration 5m
      slots
        s: 1..1 consuming aspect:charge
      effects
        charge_up(2)
        roll 1d6 + core.charge as r
        if r >= 4
          spawn cell count 2 as fresh
          move fresh into deck_zone
        signal tick
"#;

#[test]
fn content_json_round_trip_preserves_value_and_fingerprint() {
    let out = story_lang::compile_source("roundtrip.sw", SRC);
    assert!(
        out.diagnostics.is_empty(),
        "fixture must compile clean: {:?}",
        out.diagnostics
    );
    let content = out.content.unwrap();

    let bytes = serde_json::to_vec(&content).unwrap();
    let back: story_content::Content = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(content, back);
    assert_eq!(content.fingerprint(), back.fingerprint());
}

#[test]
fn recompile_is_fingerprint_stable() {
    let a = story_lang::compile_source("roundtrip.sw", SRC)
        .content
        .unwrap()
        .fingerprint();
    let b = story_lang::compile_source("roundtrip.sw", SRC)
        .content
        .unwrap()
        .fingerprint();
    assert_eq!(a, b);
}

#[test]
fn expanded_ir_carries_no_macro_or_predicate() {
    // The kernel never sees a predicate or an effectset (spec.md §7): the
    // recipe stores expanded effect lists only.
    let out = story_lang::compile_source("roundtrip.sw", SRC);
    let content = out.content.unwrap();
    let go = &content.verbs["work"].recipes["go"];
    // charge_up(2) expands to the single `set`; roll, branch, signal follow.
    assert_eq!(go.effects.len(), 4);
    assert!(matches!(go.effects[0], EffectNode::Set { .. }));
    assert!(matches!(go.effects[1], EffectNode::Roll { .. }));
    assert!(matches!(go.effects[2], EffectNode::Branch { .. }));
    assert!(matches!(go.effects[3], EffectNode::Signal { .. }));
    // The event book's rule carries inlined effects and its once flag.
    let book = &content.events["tick"];
    assert_eq!(book.rules.len(), 1);
    assert!(book.rules[0].once);
    assert!(matches!(
        book.rules[0].effects[0],
        EffectNode::Legacy { .. }
    ));
}
