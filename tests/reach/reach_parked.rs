//! The parked scope in the reach analyzer (§6.3, v0.10.0, §13.1): the parked
//! pool is a first-class face of the card world — a proven parked mint lifts
//! the `total(parked, …)` interval and NOT the world intervals, a same-list
//! `SET(…, state.parked, true)` flip moves the product between pools, and
//! proven parked products report through the world notes.
//!
//! Fixtures ride the reach.rs conventions: hand-built ContentRaw, a fresh
//! core dictionary per analyze, and the conservative direction as the
//! contract.

use indexmap::IndexMap;

use chorus::content::{compile_content, ContentRaw, PrototypeDef, RecipeDef, SlotDef, VerbContent};
use chorus::core::dict::core_dictionary;
use chorus::core::types::{add_entity, new_state, AspectVal, Entity, State};
use chorus::reach::{world_report, CardOptions, CardVerdict, Existence};

/// One fixture entity (id derived from the lowercased name).
fn ent(name: &str, kind: &str, aspects: &[(&str, i64)]) -> Entity {
    let mut a: IndexMap<String, AspectVal> = IndexMap::new();
    for (k, v) in aspects {
        a.insert((*k).to_string(), AspectVal::Num(*v));
    }
    Entity {
        id: format!("s_{}", name.to_lowercase()),
        kind: kind.to_string(),
        name: name.to_string(),
        aspects: a,
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
        desc: None,
    }
}

/// A state holding exactly these fixture entities.
fn state_with(es: Vec<Entity>) -> State {
    let mut s = new_state(0);
    for e in es {
        add_entity(&mut s, e).expect("fixture entities never collide (unique names)");
    }
    s
}

fn slot(role: &str, min: i64, max: i64, query: Option<&str>) -> SlotDef {
    SlotDef {
        role: role.to_string(),
        min,
        max: Some(max),
        query: query.map(str::to_string),
        shown: None,
        greedy: false,
        consuming: false,
    }
}

fn recipe(id: &str, when: Option<&str>, out: Vec<String>) -> RecipeDef {
    RecipeDef {
        id: Some(id.to_string()),
        when: when.map(str::to_string),
        shown: None,
        hint: None,
        max: None,
        aspects: Vec::new(),
        dur: None,
        out,
    }
}

fn verb(slots: Vec<SlotDef>, recipes: Vec<RecipeDef>) -> VerbContent {
    VerbContent { slots, duration: 0, recipes, ambient: false }
}

fn proto(
    name: &str,
    kind: &str,
    aspects: &[(&str, i64)],
    stackable: bool,
) -> (String, PrototypeDef) {
    let mut a: IndexMap<String, AspectVal> = IndexMap::new();
    for (k, v) in aspects {
        a.insert((*k).to_string(), AspectVal::Num(*v));
    }
    (
        name.to_string(),
        PrototypeDef {
            kind: kind.to_string(),
            aspects: Some(a),
            stackable,
            unique: false,
            derive: None,
            attrs: None,
            desc: None,
        },
    )
}

/// [`proto`] with the §4.2 unique declaration (a unique prototype cannot
/// declare stackable — the grammar rejects the pair, §4.2).
fn unique_proto(name: &str, kind: &str, aspects: &[(&str, i64)]) -> (String, PrototypeDef) {
    let (n, mut p) = proto(name, kind, aspects, false);
    p.unique = true;
    (n, p)
}

fn compile(raw: ContentRaw) -> chorus::content::CompiledContent {
    let mut dict = core_dictionary();
    // the fixture verbs register like any script `VERB:` header (§10)
    for v in raw.verbs.keys() {
        let _ = dict.register_verb(v);
    }
    compile_content(&raw, &dict, &[]).expect("fixture content compiles")
}

fn analyze_card(
    content: &chorus::content::CompiledContent,
    state: &State,
    target: &str,
) -> chorus::reach::CardReport {
    let dict = core_dictionary();
    chorus::reach::analyze_card(content, state, &dict, target, CardOptions::default()).unwrap()
}

/// The parked-mint world: PUMP mints DREAD parked (a bare proven SPAWN);
/// MARKER's two recipes gate on the parked scope (mints MARK) and on the
/// world form (mints SHADOW — parked-blind, never true). ANA commits.
fn parked_mint_world() -> (chorus::content::CompiledContent, State) {
    let mut prototypes = indexmap::IndexMap::new();
    let (n, p) = proto("DREAD", "idea", &[("dread", 1)], true);
    prototypes.insert(n, p);
    let (n, p) = proto("MARK", "idea", &[], false);
    prototypes.insert(n, p);
    let (n, p) = proto("SHADOW", "idea", &[], false);
    prototypes.insert(n, p);
    let mut verbs = indexmap::IndexMap::new();
    verbs.insert(
        "PUMP".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe("pump.park", None, vec!["SPAWN DREAD parked".to_string()])],
        ),
    );
    verbs.insert(
        "MARKER".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "marker.prove",
                Some("total(parked, aspect:dread) >= 1"),
                vec!["SPAWN MARK".to_string()],
            )],
        ),
    );
    verbs.insert(
        "GATER".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "gater.world",
                Some("exists(aspect:dread >= 1)"),
                vec!["SPAWN SHADOW".to_string()],
            )],
        ),
    );
    let raw = ContentRaw { verbs, prototypes: Some(prototypes), ..ContentRaw::default() };
    let content = compile(raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);
    (content, state)
}

/// T-P10, direction one: a proven parked mint lifts the PARKED interval —
/// the parked-gated MARK proves obtainable — while the world forms stay
/// parked-blind, so the world-gated SHADOW is structurally absent.
#[test]
fn a_proven_parked_mint_lifts_the_parked_interval_and_not_the_world() {
    let (content, state) = parked_mint_world();
    let mark = analyze_card(&content, &state, "MARK");
    assert_eq!(mark.verdict, CardVerdict::Obtainable, "{mark:?}");
    assert_eq!(mark.existence, Existence::Proven);
    // the world forms are parked-blind: the world-gated site cannot prove —
    // it demotes to a guarded candidate (the honest under-claim), never to
    // proven off mints it cannot see
    let shadow = analyze_card(&content, &state, "SHADOW");
    assert_eq!(shadow.verdict, CardVerdict::Unproven, "{shadow:?}");
    assert_eq!(shadow.existence, Existence::Candidate);
    assert!(
        shadow.candidates.iter().any(|c| c.reason.contains("when")),
        "the demotion is the gate: {:?}",
        shadow.candidates
    );
}

/// The parked product reports through the world notes (the destroyed-by
/// map's precedent — zero new analysis).
#[test]
fn the_parked_product_reports_in_the_world_notes() {
    let (content, state) = parked_mint_world();
    let report = world_report(&content, &state, &core_dictionary());
    assert!(
        report.notes.iter().any(|n| n.contains("parked:") && n.contains("DREAD")),
        "the parked note rides the notes: {:?}",
        report.notes
    );
}

/// The SET-flip world: FLIP mints WISP and parks it in the SAME list — the
/// census's `parked_in_list` face moves the product from the world pool to
/// the parked pool. MARKER gates on the parked scope (mints MARK) and on
/// the name's world-existence (mints SHADOW).
fn set_flip_world() -> (chorus::content::CompiledContent, State) {
    let mut prototypes = indexmap::IndexMap::new();
    let (n, p) = proto("WISP", "idea", &[("dread", 1)], true);
    prototypes.insert(n, p);
    let (n, p) = proto("MARK", "idea", &[], false);
    prototypes.insert(n, p);
    let (n, p) = proto("SHADOW", "idea", &[], false);
    prototypes.insert(n, p);
    let mut verbs = indexmap::IndexMap::new();
    verbs.insert(
        "FLIP".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "flip.park",
                None,
                vec!["SPAWN WISP".to_string(), "SET(WISP, state.parked, true)".to_string()],
            )],
        ),
    );
    verbs.insert(
        "MARKER".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "marker.prove",
                Some("total(parked, aspect:dread) >= 1"),
                vec!["SPAWN MARK".to_string()],
            )],
        ),
    );
    verbs.insert(
        "GATER".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "gater.world",
                Some("exists(name:WISP)"),
                vec!["SPAWN SHADOW".to_string()],
            )],
        ),
    );
    let raw = ContentRaw { verbs, prototypes: Some(prototypes), ..ContentRaw::default() };
    let content = compile(raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);
    (content, state)
}

/// T-P10, direction two: the same-list SET flip moves the product between
/// pools — the parked scope sees it (MARK obtainable), the world-existential
/// does not (SHADOW structurally absent).
#[test]
fn a_set_flip_moves_the_product_between_pools() {
    let (content, state) = set_flip_world();
    let mark = analyze_card(&content, &state, "MARK");
    assert_eq!(mark.verdict, CardVerdict::Obtainable, "{mark:?}");
    // the world-existential cannot see the parked product — the site demotes
    // to a guarded candidate, never proven
    let shadow = analyze_card(&content, &state, "SHADOW");
    assert_eq!(shadow.verdict, CardVerdict::Unproven, "{shadow:?}");
    assert_eq!(shadow.existence, Existence::Candidate);
}

/// The control twin: without the SET, the same mint stays world-visible —
/// the name existential proves and the parked scope is the empty side.
#[test]
fn without_the_flip_the_mint_stays_world_visible() {
    let mut prototypes = indexmap::IndexMap::new();
    let (n, p) = proto("WISP", "idea", &[("dread", 1)], true);
    prototypes.insert(n, p);
    let (n, p) = proto("MARK", "idea", &[], false);
    prototypes.insert(n, p);
    let (n, p) = proto("SHADOW", "idea", &[], false);
    prototypes.insert(n, p);
    let mut verbs = indexmap::IndexMap::new();
    verbs.insert(
        "FLIP".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe("flip.plain", None, vec!["SPAWN WISP".to_string()])],
        ),
    );
    verbs.insert(
        "MARKER".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "marker.prove",
                Some("total(parked, aspect:dread) >= 1"),
                vec!["SPAWN MARK".to_string()],
            )],
        ),
    );
    verbs.insert(
        "GATER".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "gater.world",
                Some("exists(name:WISP)"),
                vec!["SPAWN SHADOW".to_string()],
            )],
        ),
    );
    let raw = ContentRaw { verbs, prototypes: Some(prototypes), ..ContentRaw::default() };
    let content = compile(raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);
    // nothing parks: the parked scope is the empty side (the gate demotes
    // its site) and the world scope proves
    let mark = analyze_card(&content, &state, "MARK");
    assert_eq!(mark.verdict, CardVerdict::Unproven, "nothing parks: {mark:?}");
    assert_eq!(mark.existence, Existence::Candidate);
    let shadow = analyze_card(&content, &state, "SHADOW");
    assert_eq!(shadow.verdict, CardVerdict::Obtainable, "the world sees the mint");
    assert_eq!(shadow.existence, Existence::Proven);
}

/// An initial entity born with the flag pools the same way: the parked
/// scope reads it, the world forms do not.
#[test]
fn an_initial_parked_entity_pools_into_the_interior() {
    let (content, state) = {
        let mut prototypes = indexmap::IndexMap::new();
        let (n, p) = proto("MARK", "idea", &[], false);
        prototypes.insert(n, p);
        let (n, p) = proto("SHADOW", "idea", &[], false);
        prototypes.insert(n, p);
        let mut verbs = indexmap::IndexMap::new();
        verbs.insert(
            "MARKER".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                vec![recipe(
                    "marker.prove",
                    Some("total(parked, aspect:dread) >= 1"),
                    vec!["SPAWN MARK".to_string()],
                )],
            ),
        );
        verbs.insert(
            "GATER".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                vec![recipe(
                    "gater.world",
                    Some("exists(aspect:dread >= 1)"),
                    vec!["SPAWN SHADOW".to_string()],
                )],
            ),
        );
        let raw = ContentRaw { verbs, prototypes: Some(prototypes), ..ContentRaw::default() };
        let content = compile(raw);
        let mut ghost = ent("GHOST", "idea", &[("dread", 1)]);
        ghost.attrs.insert("state.parked".to_string(), chorus::core::types::AttrValue::Bool(true));
        let state = state_with(vec![ent("ANA", "character", &[]), ghost]);
        (content, state)
    };
    let mark = analyze_card(&content, &state, "MARK");
    assert_eq!(mark.verdict, CardVerdict::Obtainable, "{mark:?}");
    // the world form is parked-blind — the site demotes to a guarded
    // candidate, never proven
    let shadow = analyze_card(&content, &state, "SHADOW");
    assert_eq!(shadow.verdict, CardVerdict::Unproven, "{shadow:?}");
    assert_eq!(shadow.existence, Existence::Candidate);
}

/// The unique×parked world: two verbs each carry one `SPAWN UDREAD
/// parked` recipe — two independent proven sites, each registering a parked
/// fact of the UNIQUE name. MARKER1 gates the parked scope at 1, MARKER2 at 2.
fn unique_parked_world() -> (chorus::content::CompiledContent, State) {
    let mut prototypes = indexmap::IndexMap::new();
    let (n, p) = unique_proto("UDREAD", "idea", &[("dread", 1)]);
    prototypes.insert(n, p);
    let (n, p) = proto("MARK1", "idea", &[], false);
    prototypes.insert(n, p);
    let (n, p) = proto("MARK2", "idea", &[], false);
    prototypes.insert(n, p);
    let mut verbs = indexmap::IndexMap::new();
    verbs.insert(
        "PUMPA".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe("pump.park.a", None, vec!["SPAWN UDREAD parked".to_string()])],
        ),
    );
    verbs.insert(
        "PUMPB".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe("pump.park.b", None, vec!["SPAWN UDREAD parked".to_string()])],
        ),
    );
    verbs.insert(
        "MARKER1".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "marker1.prove",
                Some("total(parked, aspect:dread) >= 1"),
                vec!["SPAWN MARK1".to_string()],
            )],
        ),
    );
    verbs.insert(
        "MARKER2".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "marker2.prove",
                Some("total(parked, aspect:dread) >= 2"),
                vec!["SPAWN MARK2".to_string()],
            )],
        ),
    );
    let raw = ContentRaw { verbs, prototypes: Some(prototypes), ..ContentRaw::default() };
    let content = compile(raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);
    (content, state)
}

/// The parked-unique interval is 0-or-1: both mint faces skip while a LIVING
/// copy exists and the by_name read counts parked copies (a parked card is
/// alive), so a unique can never hold two interior copies — the second parked
/// mint of the same name skips against the first. The parked pool clamps the
/// same way the world pool does: one distinct instance, weight 1.
#[test]
fn a_unique_parks_to_at_most_one_interior_copy() {
    let (content, state) = unique_parked_world();
    // the floor: one parked copy is arrangeable (the first mint lands —
    // no living copy blocks it), so the >= 1 gate proves
    let mark1 = analyze_card(&content, &state, "MARK1");
    assert_eq!(mark1.verdict, CardVerdict::Obtainable, "{mark1:?}");
    assert_eq!(mark1.existence, Existence::Proven);
    // the ceiling: the second parked mint skips against the living parked
    // copy, so the parked scope can never sum to 2 — the >= 2 gate demotes
    // its site to a guarded candidate, never proven
    let mark2 = analyze_card(&content, &state, "MARK2");
    assert_eq!(mark2.verdict, CardVerdict::Unproven, "{mark2:?}");
    assert_eq!(mark2.existence, Existence::Candidate);
    assert!(
        mark2.candidates.iter().any(|c| c.reason.contains("when")),
        "the demotion is the gate: {:?}",
        mark2.candidates
    );
}

/// The park→unpark world: FLIP mints WISP and flips the flag TWICE in one
/// list — park, then unpark. MARKER gates the parked scope, GATER the world
/// existential.
fn park_unpark_world() -> (chorus::content::CompiledContent, State) {
    let mut prototypes = indexmap::IndexMap::new();
    let (n, p) = proto("WISP", "idea", &[("dread", 1)], true);
    prototypes.insert(n, p);
    let (n, p) = proto("MARK", "idea", &[], false);
    prototypes.insert(n, p);
    let (n, p) = proto("SHADOW", "idea", &[], false);
    prototypes.insert(n, p);
    let mut verbs = indexmap::IndexMap::new();
    verbs.insert(
        "FLIP".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "flip.roundtrip",
                None,
                vec![
                    "SPAWN WISP".to_string(),
                    "SET(WISP, state.parked, true)".to_string(),
                    "SET(WISP, state.parked, false)".to_string(),
                ],
            )],
        ),
    );
    verbs.insert(
        "MARKER".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "marker.prove",
                Some("total(parked, aspect:dread) >= 1"),
                vec!["SPAWN MARK".to_string()],
            )],
        ),
    );
    verbs.insert(
        "GATER".to_string(),
        verb(
            vec![slot("actors", 1, 1, Some("kind:character"))],
            vec![recipe(
                "gater.world",
                Some("exists(name:WISP)"),
                vec!["SPAWN SHADOW".to_string()],
            )],
        ),
    );
    let raw = ContentRaw { verbs, prototypes: Some(prototypes), ..ContentRaw::default() };
    let content = compile(raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);
    (content, state)
}

/// Parking is two-phase: the later non-`true` `state.parked` SET un-parks the
/// product, so at every commit boundary the engine's parked sum from this
/// list is 0 and the product ends world-side — the parked scope never sums
/// it (the gate demotes) and the world existential proves.
#[test]
fn a_later_unpark_set_keeps_the_product_world_side() {
    let (content, state) = park_unpark_world();
    let mark = analyze_card(&content, &state, "MARK");
    assert_eq!(mark.verdict, CardVerdict::Unproven, "{mark:?}");
    assert_eq!(mark.existence, Existence::Candidate);
    assert!(
        mark.candidates.iter().any(|c| c.reason.contains("when")),
        "the demotion is the gate: {:?}",
        mark.candidates
    );
    // the same list's product is world-visible: the unpark returned it
    let shadow = analyze_card(&content, &state, "SHADOW");
    assert_eq!(shadow.verdict, CardVerdict::Obtainable, "{shadow:?}");
    assert_eq!(shadow.existence, Existence::Proven);
}
