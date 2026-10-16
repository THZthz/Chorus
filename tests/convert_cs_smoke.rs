//! The converted Cultist Simulator core (scripts/cultist-full.txt) — a load
//! smoke for the tools/convert_cs.py output. The script is GENERATED DATA:
//! embedded with include_str! and compiled fresh per test (compile_script is
//! pure). This pins the acceptance bar of the converter: the full core
//! LOADS, lints clean (0 findings), and the census is accounted —
//! 1,566 elements (declared entities + prototypes; the one structural skip,
//! `credits.loc.zh-hans.`, lives in the converter, not the script),
//! 2,380 compiled recipes over the TWELVE configured verbs (7 CS stations +
//! 5 carrier verbs carrying the 464 rehomed non-station recipes, hidden),
//! 104 decks (67 CS decks + 37 internaldecks), 48 endings — plus one engine
//! step proving the WAIT station's wheel entry actually fires, and two
//! surface checks proving the carrier stations never reach the player
//! (no suggestions; a typed attempt rejects at the guarded actors slot).
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

const EXPECTED_RECIPES: usize = 2080 + 294 + 5 + 1; // emitted + alt/run copies + 5 carrier REFUSE walls + the MANSUS default
const EXPECTED_VERBS: usize = 12; // 7 CS stations + READ TRAVEL_TO USE_ON ACQUIRE LOCATE_AT
const EXPECTED_DECKS: usize = 67 + 37; // CS decks + internaldecks
const EXPECTED_ENDINGS: usize = 48;
const EXPECTED_ENTITIES: usize = 102 + 2; // declared cards + THE_CITY + THE_ASPIRANT
const EXPECTED_PROTOTYPES: usize = 1463; // minted cards + the 330 isAspect carriers (no echo prototype — the aspect cards write every vocabulary key)

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

    // census: recipes over the twelve configured verbs
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

    // full coverage: the 464 non-station recipes rehomed onto the carriers —
    // a rite induction (USE_ON), an Exile strategy (TRAVEL_TO), a craving
    // (ACQUIRE), a body-and-law clock (LOCATE_AT), and a shadowed twin under
    // its deterministic .x2 suffix
    for (verb, id) in [
        ("USE_ON", "use_on.grailinduction"),
        ("TRAVEL_TO", "travel_to.long.executestrategy.assassination.begin"),
        ("ACQUIRE", "acquire.ambition"),
        ("LOCATE_AT", "locate_at.suspicion"),
        ("READ", "read.workvictory"),
    ] {
        let v = compiled.content.verbs.get(verb).expect("carrier configured");
        assert!(
            v.recipes.iter().any(|r| r.id == id),
            "{id} must be compiled on {verb}"
        );
    }
    let study = compiled.content.verbs.get("STUDY").expect("STUDY configured");
    assert!(
        study.recipes.iter().any(|r| r.id == "study.studyresearchsuccess.x2"),
        "shadowed twins must carry their .x2 suffix"
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

    // the surface held: not one carrier station reaches the suggestions,
    // before or after the wheel turned
    for s in game.suggestions().unwrap() {
        for c in ["read", "travel to", "use on", "acquire", "locate at"] {
            assert!(!s.starts_with(c), "carrier verb leaked into suggestions: {s}");
        }
    }
}

/// The player-visible surface is the seven stations: the five carrier verbs
/// (guarded actors slot: `aspect:ritual>=1` matches no character) never
/// reach the suggestion list, at game start.
#[test]
fn the_carrier_stations_never_surface_in_suggestions() {
    use chorus::session::game::Game;

    let game = Game::from_script(FULL, core_dictionary()).unwrap();
    let suggestions = game.suggestions().unwrap();
    assert!(
        !suggestions.is_empty(),
        "the seven stations must still suggest at game start"
    );
    for s in &suggestions {
        for c in ["read", "travel to", "use on", "acquire", "locate at"] {
            assert!(!s.starts_with(c), "carrier verb leaked into suggestions: {s}");
        }
    }
}

/// A carrier verb typed by hand rejects at the engine — the guarded actors
/// slot matches no character, so the carriers stay chain-addressed even
/// under explicit invocation.
#[test]
fn a_typed_carrier_attempt_rejects_at_the_engine() {
    use chorus::session::game::{Game, TurnKind};

    let mut game = Game::from_script(FULL, core_dictionary()).unwrap();
    let turn = game
        .handle_line("0:0 @ THE_ASPIRANT READ")
        .expect("the attempt parses; the engine must refuse it");
    assert_eq!(turn.kind, TurnKind::Error, "the carrier station must not accept");
    assert!(
        turn.lines
            .iter()
            .any(|l| l.contains("does not fit slot 'actors' of READ")),
        "the rejection must name the guarded slot: {:?}",
        turn.lines
    );
}
