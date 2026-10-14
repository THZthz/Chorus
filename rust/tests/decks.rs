//! Port of test/decks.test.ts — W0 subset (plan §4-W0 / §5.2.1): the four
//! permutation vectors (decks.test.ts:141-144, computed independently in
//! python3) and the drawFrom three-rule probe with hand-built decks; W2 adds
//! the same three rules through a COMPILED deck (compile_script →
//! CompiledContent, ts: test/decks.test.ts:169-177's compile(rig(...)) shape).
//! The engine-dependent parts — cursors, fold, drift — are W3.

use chorus::content::content::{draw_from, permute_positions, CompiledDeck, DeckCard};

/// A hand-built deck standing in for the compiled one; the kind is inert for
/// the hand-built draws (the mint that consumes it is W2/W3).
fn deck(id: &str, names: &[&str], defaultcard: Option<&str>, reset: bool) -> CompiledDeck {
    CompiledDeck {
        id: id.to_string(),
        order: names
            .iter()
            .map(|n| DeckCard {
                name: (*n).to_string(),
                kind: "idea".to_string(),
            })
            .collect(),
        defaultcard: defaultcard.map(|n| DeckCard {
            name: n.to_string(),
            kind: "idea".into(),
        }),
        reset,
    }
}

#[test]
fn permutation_is_pure_in_seed_and_deck_id_with_position_vectors() {
    // ts: test/decks.test.ts:134-144 — the shuffle keys are pinned splitmix64
    // values, so these are not probabilistic asserts; the n=7 and
    // negative-seed rows were computed independently (python3).
    assert_eq!(permute_positions(4, 0, "RUMOURS"), vec![0, 2, 1, 3]);
    assert_eq!(permute_positions(4, 42, "RUMOURS"), vec![3, 0, 1, 2]);
    assert_eq!(permute_positions(7, 0, "SEVEN"), vec![0, 6, 3, 1, 4, 5, 2]);
    assert_eq!(permute_positions(7, -3, "NEG"), vec![4, 0, 2, 6, 3, 1, 5]);
}

#[test]
fn draw_from_below_period_wraps_on_reset_and_serves_the_defaultcard() {
    // ts: test/decks.test.ts:169-178 — PLAN_DECK (decks.test.ts:99) compiles
    // (seed 0) to the order pinned by permutation_is_pure_in_seed_and_deck_id
    // _with_position_vectors above: RUMOUR, HEARSAY, RUMOUR, SCANDAL.
    let rumours = deck(
        "RUMOURS",
        &["RUMOUR", "HEARSAY", "RUMOUR", "SCANDAL"],
        Some("RUMOUR"),
        true,
    );
    assert_eq!(draw_from(&rumours, 0).unwrap().name, "RUMOUR");
    assert_eq!(draw_from(&rumours, 3).unwrap().name, "SCANDAL");
    assert_eq!(draw_from(&rumours, 4).unwrap().name, "RUMOUR"); // reset wraps: period 4
    assert_eq!(draw_from(&rumours, 9).unwrap().name, "HEARSAY"); // 9 % 4 = 1
    let solo = deck("SOLO", &["HEARSAY"], None, true);
    assert_eq!(draw_from(&solo, 17).unwrap().name, "HEARSAY"); // single-entry deck
}

#[test]
fn draw_from_exhausted_deck_fails_loud_with_the_verbatim_message() {
    // ts: src/content/content.ts:476 — the raw error carries the (§9.42)
    // suffix; decks.test.ts:410 pins it through a refusal as
    // /deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard'/
    // (the engine-side refusal wiring is W3).
    let singles = deck("SINGLES", &["HEARSAY"], None, false);
    let err = draw_from(&singles, 1).unwrap_err();
    assert_eq!(
        err.msg,
        "deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard' (§9.42)"
    );
}

// ---------------------------------------------------------------------------
// W2: the same three rules through a COMPILED deck (the draw rule shared by
// play and fold is the one content.rs built at load).
// ---------------------------------------------------------------------------

/// ts: test/decks.test.ts:60-98 (`rig`) — the minimal script shape with the
/// gossip prototypes and a DRAW-verb; seed 0 keeps the pinned order
/// RUMOUR, HEARSAY, RUMOUR, SCANDAL (the permutation vectors above).
fn rig(decks: &str) -> String {
    format!(
        r#"script: Probe

prototypes:
  RUMOUR is idea stackable
  HEARSAY is idea stackable
  SCANDAL is idea stackable

entities:
  ANA is character

decks:
{decks}

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DRAW RUMOURS
"#
    )
}

#[test]
fn compiled_deck_draws_obey_the_three_rules_below_period_reset_wrap_defaultcard_forever() {
    // ts: test/decks.test.ts:169-177 through compileScript, plus the
    // defaultcard-forever leg of decks.test.ts:390-394's describe (the
    // Game-refusal harness around it is W3).
    let dict = chorus::core::dict::core_dictionary();
    let game = chorus::content::script::compile_script(
        &rig("  RUMOURS spec RUMOUR, RUMOUR, HEARSAY, SCANDAL defaultcard RUMOUR reset\n  SOLO spec HEARSAY reset\n  GOSSIP spec RUMOUR, HEARSAY, SCANDAL defaultcard HEARSAY\n  SINGLES spec HEARSAY"),
        &dict,
    )
    .unwrap();
    let rumours = game.content.decks.get("RUMOURS").unwrap();
    assert_eq!(draw_from(rumours, 0).unwrap().name, "RUMOUR");
    assert_eq!(draw_from(rumours, 3).unwrap().name, "SCANDAL");
    assert_eq!(draw_from(rumours, 4).unwrap().name, "RUMOUR"); // reset wraps: period 4
    assert_eq!(draw_from(rumours, 9).unwrap().name, "HEARSAY"); // 9 % 4 = 1
    let solo = game.content.decks.get("SOLO").unwrap();
    assert_eq!(draw_from(solo, 17).unwrap().name, "HEARSAY");
    // defaultcard forever: past the spec every draw serves the default
    // (GOSSIP's seed-0 permutation is the identity [0,1,2], pinned here so
    // draw 0 below is the in-period first spec entry)
    let gossip = game.content.decks.get("GOSSIP").unwrap();
    assert_eq!(draw_from(gossip, 0).unwrap().name, "RUMOUR"); // in-period
    for cursor in [3, 10, 99] {
        assert_eq!(draw_from(gossip, cursor).unwrap().name, "HEARSAY");
    }
    // neither: the verbatim exhaustion error (content.ts:472-477)
    let singles = game.content.decks.get("SINGLES").unwrap();
    let err = draw_from(singles, 1).unwrap_err();
    assert_eq!(
        err.msg,
        "deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard' (§9.42)"
    );
}

