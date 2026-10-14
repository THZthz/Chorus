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

