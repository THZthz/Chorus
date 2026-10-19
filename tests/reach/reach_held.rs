//! The held slot (§6.7, v0.9.3) in the reach analyzer: the per-slot
//! BoundView shape — the held slot rides the `total(bound, …)` sum like any
//! optional slot — and the §13.1 arrangeable-hold model for `count(held)`
//! gates: ≥1 fitting descriptor reads the count as exactly 1 (arranged — a
//! require-held gate is provable), no fitting descriptor reads exactly 0
//! (the gate decides False — never Proven off a hold the world cannot
//! arrange), and no window model at all (the §6.7 chain-pinned verb — the C
//! ruling, held inherits it) reads the full-open pair, Unknown.
//!
//! Nothing here registers verbs — a fresh core dictionary per test.

use indexmap::IndexMap;

use chorus::content::{compile_content, ContentRaw, PrototypeDef, RecipeDef, SlotDef, VerbContent};
use chorus::core::dict::core_dictionary;
use chorus::core::types::{add_entity, new_state, AspectVal, Entity, State};
use chorus::reach::{analyze_card, CardOptions, CardVerdict, Existence};

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

fn state_with(es: Vec<Entity>) -> State {
    let mut s = new_state(0);
    for e in es {
        add_entity(&mut s, e).expect("fixture entities never collide (unique names)");
    }
    s
}

fn compile(raw: &ContentRaw) -> chorus::content::CompiledContent {
    let mut dict = core_dictionary();
    // the fixture lexemes register the §4.3 way before the content compiles
    for lexeme in ["RITE", "THINK"] {
        if !dict.is_verb(lexeme) {
            dict.register_verb(lexeme).expect("the fixture lexeme registers");
        }
    }
    compile_content(raw, &dict, &[]).expect("fixture content compiles")
}

fn analyze(
    content: &chorus::content::CompiledContent,
    state: &State,
    target: &str,
) -> chorus::reach::CardReport {
    let dict = core_dictionary();
    analyze_card(content, state, &dict, target, CardOptions::default())
        .expect("the fixture analyzes")
}

fn slot(role: &str, min: i64, max: Option<i64>, query: Option<&str>) -> SlotDef {
    SlotDef { role: role.to_string(), min, max, query: query.map(str::to_string), shown: None }
}

/// A RITE verb whose object is a rite and whose held slot takes a quested
/// item; the requiring recipe mints PRIZE only under the hold.
fn rite_with_held(held_query: Option<&str>, gate: &str) -> VerbContent {
    VerbContent {
        slots: vec![
            slot("actors", 1, Some(1), Some("kind:character")),
            slot("object", 1, Some(1), Some("kind:rite")),
            slot("held", 0, Some(1), held_query),
        ],
        duration: 0,
        recipes: vec![
            RecipeDef {
                id: Some("rite.prize".to_string()),
                when: Some(gate.to_string()),
                out: vec!["SPAWN PRIZE".to_string()],
                ..RecipeDef::default()
            },
            RecipeDef {
                id: None,
                when: None,
                out: vec!["NARRATE \"plain.\"".to_string()],
                ..RecipeDef::default()
            },
        ],
        ambient: false,
    }
}

fn raw_rite(verb: VerbContent) -> ContentRaw {
    ContentRaw {
        verbs: [("RITE".to_string(), verb)].into_iter().collect(),
        prototypes: Some(
            [(
                "PRIZE".to_string(),
                PrototypeDef { kind: "idea".to_string(), ..PrototypeDef::default() },
            )]
            .into_iter()
            .collect::<indexmap::IndexMap<_, _>>(),
        ),
        ..ContentRaw::default()
    }
}

/// A require-held recipe is PROVABLE when the hold is arrangeable: a fitting
/// card exists in the world, the analyzer arranges the hold like a commit,
/// and the gate decides True — the mint lands (obtainable).
#[test]
fn a_require_held_recipe_is_provable_when_the_hold_is_arrangeable() {
    let content =
        compile(&raw_rite(rite_with_held(Some("kind:item aspect:quest>=1"), "count(held) >= 1")));
    let state = state_with(vec![
        ent("MARCH", "character", &[]),
        ent("RITE_OF_DAWN", "rite", &[]),
        ent("CHALICE", "item", &[("quest", 1)]),
    ]);
    let report = analyze(&content, &state, "PRIZE");
    assert_eq!(report.verdict, CardVerdict::Obtainable, "{:?}", report.verdict);
    assert_eq!(report.existence, Existence::Proven);
}

/// The same recipe with NO fitting card anywhere — the hold alone cannot
/// satisfy the gate, and the analyzer never claims Proven off it: the gate
/// decides False and the mint stays unreachable.
#[test]
fn a_require_held_recipe_is_unprovable_when_the_fitting_card_is_unreachable() {
    // the held query names a quest level nothing in the world can carry
    let content =
        compile(&raw_rite(rite_with_held(Some("kind:item aspect:quest>=9"), "count(held) >= 1")));
    let state = state_with(vec![
        ent("MARCH", "character", &[]),
        ent("RITE_OF_DAWN", "rite", &[]),
        ent("CHALICE", "item", &[("quest", 1)]),
    ]);
    let report = analyze(&content, &state, "PRIZE");
    // the gate decides False (the hold can never arrange), so the site never
    // proves — the analyzer's conservative verdict for a dead GATE is the
    // honest candidate, never the structural 'unobtainable' (that face is
    // for absent mints, not unmatching gates)
    assert_eq!(report.verdict, CardVerdict::Unproven, "{:?}", report.verdict);
    assert_eq!(report.existence, Existence::Candidate);
}

/// The C ruling, held inherits it: a chain-pinned verb reads no window
/// model, so its held-reading gates stay Unknown — the recipe is a live
/// candidate (selectability treats an undecided gate as may-select) but
/// never proven.
#[test]
fn a_chain_pinned_held_gate_stays_unknown_and_the_site_unproven() {
    let mut verb = rite_with_held(Some("kind:item aspect:quest>=1"), "count(held) >= 1");
    // the chain edge: a THINK-free raw — the chain candidate lives in a
    // parent verb's body targeting rite.prize, making RITE chain-pinned
    let parent = VerbContent {
        slots: vec![slot("actors", 1, Some(1), Some("kind:character"))],
        duration: 0,
        recipes: vec![RecipeDef {
            id: None,
            when: None,
            out: vec!["RUN \"rite.prize\"".to_string()],
            ..RecipeDef::default()
        }],
        ambient: false,
    };
    let mut raw = raw_rite(verb);
    verb = parent;
    raw.verbs.insert("THINK".to_string(), verb);
    let content = compile(&raw);
    let state = state_with(vec![
        ent("MARCH", "character", &[]),
        ent("RITE_OF_DAWN", "rite", &[]),
        ent("CHALICE", "item", &[("quest", 1)]),
    ]);
    let report = analyze(&content, &state, "PRIZE");
    // the arrangeable world changes nothing here: the model is absent, the
    // gate Unknown, the site unproven — a candidate, never obtainable
    assert_eq!(report.verdict, CardVerdict::Unproven, "{:?}", report.verdict);
    assert_eq!(report.existence, Existence::Candidate);
}

/// The held slot rides the BoundView's per-slot shape: `total(bound,
/// aspect:k)` sums the held slot's ceiling like any optional slot — a gate
/// satisfiable only at the held card's level proves the arrangement face.
#[test]
fn the_held_slot_rides_the_bound_scope_sum() {
    // the rite demands pooled bound quest >= 1: without a held card the
    // window's sum is 0; holding CHALICE (quest 1) lifts the ceiling — and
    // the analyzer, modeling the hold as arrangeable, reaches it
    let content = compile(&raw_rite(rite_with_held(
        Some("kind:item aspect:quest>=1"),
        "total(bound, aspect:quest) >= 1",
    )));
    let state = state_with(vec![
        ent("MARCH", "character", &[]),
        ent("RITE_OF_DAWN", "rite", &[]),
        ent("CHALICE", "item", &[("quest", 1)]),
    ]);
    let report = analyze(&content, &state, "PRIZE");
    // total(bound, …) keeps its conservative face: the interval's FLOOR
    // (no hold) fails the gate, so it stays Unknown → the site unproven —
    // the widening-only invariant (Unknown never fabricates Proven)
    assert_eq!(report.verdict, CardVerdict::Unproven, "{:?}", report.verdict);
}
