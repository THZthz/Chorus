//! The reach analyzer's ambient rule (§6.9/§13.1): an ambient verb's
//! recipes are mints available WITHOUT player action — the soundness rule
//! is decided BOTH ways here:
//! - a card minted only by an ambient recipe whose gate is static-true is
//!   Obtainable (proven) — the one failing direction a blind analyzer
//!   would claim unobtainable;
//! - a card whose only source sits behind an ambient gate that decides
//!   False (a world form the world cannot satisfy, or a role form — the
//!   empty scope) never proves — no false obtainable.

use indexmap::IndexMap;

use chorus::content::{compile_content, ContentRaw, PrototypeDef, RecipeDef, VerbContent};
use chorus::core::dict::core_dictionary;
use chorus::core::types::{add_entity, new_state, AspectVal, Entity, State};
use chorus::reach::{analyze_card, CardOptions, CardVerdict};

/// One fixture entity.
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

/// The analyzer's card face over the dictionary carrying TIME.
fn verdict(
    dict: &chorus::core::dict::Dictionary,
    content: &chorus::content::CompiledContent,
    state: &State,
    target: &str,
) -> CardVerdict {
    analyze_card(content, state, dict, target, CardOptions::default())
        .expect("the analyze never errors on fixture content")
        .verdict
}

/// One ambient verb with one gated recipe minting PRIZE (a stackable
/// prototype), plus an unused core verb (the analyzer's walk needs a
/// committable verb somewhere for the usable face; the ambient rule reads
/// none of it). TIME registers into the test's dictionary — the fuzz
/// campaign's register-if-unknown discipline.
fn dict_with_time() -> chorus::core::dict::Dictionary {
    let mut dict = core_dictionary();
    if !dict.is_verb("TIME") {
        dict.register_verb("TIME").expect("TIME registers (no reserved word)");
    }
    dict
}

fn ambient_raw(gate: Option<&str>) -> ContentRaw {
    let ambient_verb = VerbContent {
        slots: Vec::new(),
        duration: 10,
        recipes: vec![RecipeDef {
            id: Some("time.prize".to_string()),
            when: gate.map(str::to_string),
            ..RecipeDef::default()
        }],
        ambient: true,
    };
    ContentRaw {
        seed: None,
        prototypes: Some(
            [(
                "PRIZE".to_string(),
                PrototypeDef {
                    kind: "idea".to_string(),
                    stackable: true,
                    ..PrototypeDef::default()
                },
            )]
            .into_iter()
            .collect(),
        ),
        verbs: [
            ("TIME".to_string(), ambient_verb),
            ("WORK".to_string(), VerbContent { duration: 30, ..VerbContent::default() }),
        ]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    }
}

#[test]
fn a_card_minted_only_by_an_ambient_recipe_is_obtainable_when_its_gate_is_static_true() {
    let raw = {
        let mut r = ambient_raw(None);
        // the recipe needs its outcome — set via the raw verb's recipe list
        let v = r.verbs.get_mut("TIME").unwrap();
        v.recipes[0].out = vec!["SPAWN PRIZE".to_string()];
        r
    };
    let dict = dict_with_time();
    let content = compile_content(&raw, &dict, &[]).unwrap();
    let state = state_with(vec![ent("ANA", "character", &[])]);
    assert_eq!(verdict(&dict, &content, &state, "PRIZE"), CardVerdict::Obtainable);
}

#[test]
fn a_card_behind_a_static_true_ambient_gate_proves_the_same() {
    // `exists(ANA)` decides True over the initial world — the gate holds,
    // the recipe is a proven source
    let raw = {
        let mut r = ambient_raw(Some("exists(ANA)"));
        let v = r.verbs.get_mut("TIME").unwrap();
        v.recipes[0].out = vec!["SPAWN PRIZE".to_string()];
        r
    };
    let dict = dict_with_time();
    let content = compile_content(&raw, &dict, &[]).unwrap();
    let state = state_with(vec![ent("ANA", "character", &[])]);
    assert_eq!(verdict(&dict, &content, &state, "PRIZE"), CardVerdict::Obtainable);
}

#[test]
fn a_card_behind_a_static_false_ambient_gate_never_proves() {
    // `exists(GHOST)` decides False — GHOST can never exist — so the recipe
    // never opens and PRIZE stays unproven (no false obtainable; the live
    // site keeps the verdict at the honest candidate face)
    let raw = {
        let mut r = ambient_raw(Some("exists(GHOST)"));
        let v = r.verbs.get_mut("TIME").unwrap();
        v.recipes[0].out = vec!["SPAWN PRIZE".to_string()];
        r
    };
    let dict = dict_with_time();
    let content = compile_content(&raw, &dict, &[]).unwrap();
    let state = state_with(vec![ent("ANA", "character", &[])]);
    let v = verdict(&dict, &content, &state, "PRIZE");
    assert_ne!(v, CardVerdict::Obtainable, "{v:?}");
}

#[test]
fn a_role_gated_ambient_recipe_decides_false_in_the_empty_scope_model() {
    // the §6.9 sweep binds NOTHING: a role-reading gate never matches at
    // runtime (lenient false over empty bindings), so the analyzer must
    // decide it False — claiming available here would be a false obtainable
    // the differential campaign would catch
    let raw = {
        let mut r = ambient_raw(Some("count(actors) >= 1"));
        let v = r.verbs.get_mut("TIME").unwrap();
        v.recipes[0].out = vec!["SPAWN PRIZE".to_string()];
        r
    };
    let dict = dict_with_time();
    let content = compile_content(&raw, &dict, &[]).unwrap();
    let state = state_with(vec![ent("ANA", "character", &[])]);
    let v = verdict(&dict, &content, &state, "PRIZE");
    assert_ne!(v, CardVerdict::Obtainable, "{v:?}");
}

#[test]
fn an_ambient_gate_on_an_undecidable_world_form_stays_available() {
    // `count(aspect:season>=1) >= 4` over a pool the player can pump
    // WITHOUT BOUND (an uncapped when-less recipe minting a season-bearing
    // stackable card) reads Unknown — the ambient mint stays available
    // (the walk may hold once the pool grows), and the verdict is the true
    // one: pump the pool, the sweep opens, PRIZE mints
    let mut raw = ambient_raw(Some("count(aspect:season>=1) >= 4"));
    raw.prototypes.as_mut().unwrap().insert(
        "SEASONCARD".to_string(),
        PrototypeDef {
            kind: "idea".to_string(),
            stackable: true,
            aspects: Some([("season".to_string(), AspectVal::Num(1))].into_iter().collect()),
            ..PrototypeDef::default()
        },
    );
    let v = raw.verbs.get_mut("WORK").unwrap();
    v.recipes = vec![RecipeDef {
        id: Some("work.pump".to_string()),
        out: vec!["SPAWN SEASONCARD".to_string()],
        ..RecipeDef::default()
    }];
    let tv = raw.verbs.get_mut("TIME").unwrap();
    tv.recipes[0].out = vec!["SPAWN PRIZE".to_string()];
    let dict = dict_with_time();
    let content = compile_content(&raw, &dict, &[]).unwrap();
    let state = state_with(vec![ent("ANA", "character", &[])]);
    assert_eq!(verdict(&dict, &content, &state, "PRIZE"), CardVerdict::Obtainable);
}
