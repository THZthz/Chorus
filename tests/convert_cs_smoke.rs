//! The converted Cultist Simulator core (scripts/cultist-full.txt) — a load
//! smoke for the tools/convert_cs.py output. The script is GENERATED DATA:
//! embedded with include_str! and compiled fresh per test (compile_script is
//! pure). This pins the acceptance bar of the converter: the full core
//! LOADS, lints clean (0 findings), and the census is accounted —
//! 1,566 elements (declared entities + prototypes + the aspect echo),
//! 1,686 compiled recipes over the seven mapped verbs, 103 decks (67 CS
//! decks + 36 internaldecks), 48 endings — plus one engine step proving the
//! WAIT station's wheel entry actually fires.
//!
//! The counts are the converter's coverage-report numbers (its own asserts
//! account every input recipe/element/deck/ending); this side pins that the
//! GENERATED FILE carries exactly those totals, so converter drift or a
//! half-regenerated file fails loudly here.

use chorus::content::script::{compile_script, lint_script};
use chorus::core::dict::core_dictionary;

/// The bundled conversion, read as text.
const FULL: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/cultist-full.txt"
));

const EXPECTED_RECIPES: usize = 1447 + 238 + 1; // mains + alt/run copies + the MANSUS default
const EXPECTED_VERBS: usize = 7;
const EXPECTED_DECKS: usize = 67 + 36; // CS decks + internaldecks
const EXPECTED_ENDINGS: usize = 48;
const EXPECTED_ENTITIES: usize = 102 + 2; // declared cards + THE_CITY + THE_ASPIRANT
const EXPECTED_PROTOTYPES: usize = 1133 + 1; // minted cards + CS_ASPECT_ECHO

#[test]
fn the_converted_core_loads_lints_clean_and_matches_the_census() {
    let dict = core_dictionary();

    // load: the whole file compiles
    let compiled = compile_script(FULL, &dict).expect("cultist-full.txt must compile");

    // lint: zero findings (dead recipes, shadowed whens, unwritten aspect
    // reads, unstackable deck entries, non-lexeme names)
    let warnings = lint_script(FULL, &dict).expect("lint runs on a compiling script");
    assert!(
        warnings.is_empty(),
        "lint must be clean, got {} findings:\n{}",
        warnings.len(),
        warnings.join("\n")
    );

    // census: recipes over the seven mapped verbs
    assert_eq!(compiled.content.verbs.len(), EXPECTED_VERBS);
    let recipes: usize = compiled
        .content
        .verbs
        .values()
        .map(|v| v.recipes.len())
        .sum();
    assert_eq!(recipes, EXPECTED_RECIPES, "compiled recipe count drifted");

    // census: decks, endings, declared world, prototypes
    assert_eq!(compiled.content.decks.len(), EXPECTED_DECKS);
    assert_eq!(compiled.content.endings.len(), EXPECTED_ENDINGS);
    assert_eq!(compiled.state.entities.len(), EXPECTED_ENTITIES);
    assert_eq!(compiled.content.prototypes.len(), EXPECTED_PROTOTYPES);

    // determinism: the converter's fixed seed rode into the content
    assert_eq!(compiled.content.seed, 1837);

    // the wheel's entry is wired: needs is a compiled recipe of WAIT, and
    // the job/ingredient bootstrap RUN targets resolve to it
    let wait = compiled.content.verbs.get("WAIT").expect("WAIT configured");
    assert!(
        wait.recipes.iter().any(|r| r.id == "wait.needs"),
        "wait.needs must be compiled"
    );
    assert!(
        wait.recipes.iter().any(|r| r.id == "wait.discardseason"),
        "wait.discardseason must be compiled"
    );
}

/// One turn of play: a clock-prefixed WAIT selects wait.needs (the
/// chain-bucket entry — bootstrap targets sort first), the window completes,
/// and its RUN ladder hands the cycle on — a follow-up WAIT window opens on
/// the same station. The season machine is live, not just loadable.
#[test]
fn a_bare_wait_fires_the_wheel_entry_and_chains() {
    use chorus::session::game::{Game, TurnKind};

    let mut game = Game::from_script(FULL, core_dictionary()).unwrap();

    let turn = game.handle_line("0:0 @ THE_ASPIRANT WAIT").unwrap();
    assert_ne!(turn.kind, TurnKind::Error, "the WAIT action must accept");

    // advance past the 1h window: the completion applies
    let turn = game.handle_line("0:2").unwrap();
    assert_ne!(turn.kind, TurnKind::Error);
    let journal: String = game
        .log
        .records
        .iter()
        .flat_map(|r| r.recipes.iter().cloned())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        journal.contains("wait.needs"),
        "wait.needs must have been selected and completed: {journal}"
    );

    // the RUN ladder opened a follow-up window on the WAIT station
    assert!(
        game.state.windows.values().any(|w| w.verb == "WAIT"),
        "the wheel must hand the cycle on: a WAIT window stays open"
    );
}
