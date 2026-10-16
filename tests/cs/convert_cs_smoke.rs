//! The converted Cultist Simulator core (scripts/cultist-full.txt) — a load
//! smoke for the tools/convert_cs.py output. The script is GENERATED DATA:
//! embedded with include_str! and compiled fresh per test (compile_script is
//! pure). This pins the acceptance bar of the converter: the full core
//! LOADS, lints with 0 errors and exactly the converter's deliberate
//! dead-marker warnings (E11 — pinned below), and the census is accounted —
//! 1,566 elements (declared entities + prototypes; the one structural skip,
//! `credits.loc.zh-hans.`, lives in the converter, not the script),
//! 2,380 compiled recipes over the TWELVE configured verbs (7 CS stations +
//! 5 carrier verbs carrying the 464 rehomed non-station recipes, hidden),
//! 104 decks (67 CS decks + 37 internaldecks), 48 endings — plus one engine
//! step proving the WAIT station's wheel entry actually fires, and two
//! surface checks proving the carrier stations never reach the player (no
//! suggestions; a typed attempt rejects at the guarded actors slot).
//!
//! The counts are the converter's coverage-report numbers (its own asserts
//! account every input recipe/element/deck/ending); this side pins that the
//! GENERATED FILE carries exactly those totals, so converter drift or a
//! half-regenerated file fails loudly here.

use chorus::content::script::{
    compile_script, lint_script_findings, LintFinding, LintSeverity,
};
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

/// E11's warning tolerance on the generated core, pinned as a composition.
/// First, 36 DELIBERATE dead-marker twins: the converter emits CS's
/// requirement-less station recipes data-dead under `NOT (count(actors)
/// <op> <k>)` whens (the header's "twins" convention — CS cannot fire them
/// either), every one an `.x2` id on one of the five stations whose
/// `actors` slot takes exactly 1. Second, the NOT-alt total contradictions
/// (all TALK, none `.x2`), split by WHICH side is dead — the contradiction
/// in both is `apply_alt_gates`' NOT(parent-requirements) conjunct stacked
/// against a `total(with, aspect:X)` pin, but the two families are NOT the
/// same honesty claim (E12-style accounting). 24 are GENUINELY CS-DEAD
/// (society family: `society >= 1` vs a NOT-alt pinning it < 1) — the
/// recipe's own positive requirement contradicts its naming parent's
/// precondition (`talktofollower` runs only under `society < 1`), so even
/// CS's alt routing can never reach it. 16 are CONVERTER-INDUCED (summoned
/// family: `summoned >= 1` vs a NOT-alt pinning it < 1) — the recipe's own
/// requirements are satisfiable, but it is the fallback alt of TWO MUTUALLY
/// EXCLUSIVE parents (`talktofollower` {summoned:-1} and `talktospirit`
/// {summoned:1}), and the converter conjoins one NOT(parent) per naming
/// parent (convert_cs.py apply_alt_gates) — either parent alone satisfies
/// the target, the stacked negations never do: a routing fidelity loss the
/// port carries, not CS deadness. Any drift (a regenerated file, a
/// converter change, a detector change, a new spelling) fails loudly here.
/// The hand-authored scripts carry none (tests/lint.rs pins them
/// 0-warning).
const EXPECTED_MARKERS: usize = 36;
const EXPECTED_CS_DEAD: usize = 24;
const EXPECTED_CONVERTER_INDUCED: usize = 16;

/// The E12 recipe-spawn half, armed (findings ledger): every recipe-effect
/// spawn of a CS lifetime bearer rides the authored `SPAWN X expires <dur>`
/// modifier — the fresh copy arms at birth, per instance, pooled or not.
/// The converter dedupes the armed SITES per (recipe, element) and asserts
/// that count itself (497 over 114 elements); the GENERATED FILE carries one
/// modifier per minted copy — 582 instances on 446 recipe lines (the sites
/// weighted by effect magnitude, re-emitted on alt/run copies) — and that
/// file-visible count is what pins here. The deck-entry half stays
/// E12-ledgered (159 lifetime bearers served from decks; DRAW has no
/// expires arg — converter-side assert, nothing file-visible changes).
const EXPECTED_ARMED_MINTS: usize = 582;

/// Armed mint instances in a generated file: `SPAWN <NAME> expires <dur>`
/// outcomes on recipe lines (the SET-form `, expires, ` of the on-expire /
/// on-complete trigger bodies and the header comments never match).
fn count_armed_mints(text: &str) -> usize {
    text.lines()
        .filter(|l| l.contains("SPAWN ") && !l.trim_start().starts_with('#'))
        .map(|l| l.matches(" expires ").count())
        .sum()
}

#[test]
fn the_converted_core_loads_lints_clean_and_matches_the_census() {
    let dict = core_dictionary();

    // load: the whole file compiles
    let compiled = compile_script(FULL, &dict).expect("cultist-full.txt must compile");

    // lint: 0 errors; the only warnings are the converter's deliberate
    // dead-marker twins (E11) — every one an `.x2` id flagged by the
    // never-matching-`when` detector
    let findings = lint_script_findings(FULL, &dict).expect("lint runs on a compiling script");
    let warnings: Vec<&LintFinding> = findings
        .iter()
        .filter(|f| f.severity == LintSeverity::Warning)
        .collect();
    let errors: Vec<&LintFinding> = findings
        .iter()
        .filter(|f| f.severity == LintSeverity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "lint must have 0 errors, got {}:\n{}",
        errors.len(),
        errors
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(
        warnings.len(),
        EXPECTED_MARKERS + EXPECTED_CS_DEAD + EXPECTED_CONVERTER_INDUCED,
        "the never-matching-when warning count drifted:\n{}",
        warnings
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let markers = warnings
        .iter()
        .filter(|f| f.message.contains(".x2"))
        .count();
    // the NOT-alt families split on the pinned total's aspect: the
    // society-family contradictions are CS-dead, the summoned-family ones
    // are converter-induced (stacked NOT(parent) of mutually exclusive
    // parents — see the const doc above)
    let cs_dead = warnings
        .iter()
        .filter(|f| f.message.contains("pins total(with, aspect:society)"))
        .count();
    let converter_induced = warnings
        .iter()
        .filter(|f| f.message.contains("pins total(with, aspect:summoned)"))
        .count();
    assert_eq!(
        markers, EXPECTED_MARKERS,
        "the .x2 dead-marker twins drifted"
    );
    assert_eq!(
        cs_dead, EXPECTED_CS_DEAD,
        "the society-family (genuinely CS-dead) count drifted"
    );
    assert_eq!(
        converter_induced, EXPECTED_CONVERTER_INDUCED,
        "the summoned-family (converter-induced) count drifted"
    );
    assert_eq!(
        markers + cs_dead + converter_induced,
        warnings.len(),
        "every warning must be one of the three accounted families:\n{}",
        warnings
            .iter()
            .map(|f| f.message.as_str())
            .collect::<Vec<_>>()
            .join("\n")
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

    // E12 recipe-spawn half: the armed mints (see EXPECTED_ARMED_MINTS), and
    // one of them on the actual playable starvation path the wheel falls to
    assert_eq!(
        count_armed_mints(FULL),
        EXPECTED_ARMED_MINTS,
        "the armed-mint instance count drifted"
    );
    assert!(
        FULL.contains("-> DESTROY HEALTH, SPAWN HUNGER expires 1h,"),
        "the starvation mint (wait.alt.starvation) must arm at birth"
    );

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
