//! Smoke tests for tests/common (snap/settle/seed/explorer_lib) — these
//! prove the four libs are alive and behaviorally pinned where it matters:
//! - snap: the FIVE `a<b?-1:1` sorts + fired sort are UTF-16-order and
//!   insertion-order-insensitive;
//! - snap+fold: fold(load-free) equality on a played step (law 6);
//! - settle: a settled turn carries begin+complete, act/done/refusal route;
//! - seed: the fixture compiles and carries the §6.4 world;
//! - explorer_lib: the win path plays end-to-end and the sampled walk
//!   harvests states (LCG 'NIGH' entry).

mod common;

use common::explorer_lib::{candidates_for, explorer_world, sampled_walk_states, win_path_states, Depth};
use common::seed::{seed_content, seed_state};
use common::settle::{act, done, refusal, settle_step};
use common::snap::snap;

use chorus::content::content::{compile_content, RecipeDef, SlotDef, VerbContent};
use chorus::core::clock::Clock;
use chorus::core::dict::core_dictionary;
use chorus::core::parse::parse_sentence;
use chorus::core::types::{
    add_entity, clone_state, new_state, AspectVal, AttrValue, Attrs, Entity, State,
};
use chorus::engine::engine::Step;
use chorus::engine::fold::fold;
use indexmap::IndexMap;

/// The store.rs test rig's PACE shape: one actor, one 30-minute window.
fn pace_rig() -> (chorus::core::dict::Dictionary, chorus::content::content::CompiledContent) {
    let mut dict = core_dictionary();
    dict.register_verb("PACE").expect("PACE registers");
    let raw = chorus::content::content::ContentRaw {
        verbs: [(
            "PACE".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".into(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".into()),
                    shown: None,
                }],
                duration: 30,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE \"d.\"".to_string()],
                }],
            },
        )]
        .into_iter()
        .collect(),
        ..chorus::content::content::ContentRaw::default()
    };
    let content = compile_content(&raw, &dict, &[]).expect("pace compiles");
    (dict, content)
}

fn solo(name: &str) -> State {
    let mut s = new_state(0);
    let mut e = Entity {
        id: name.to_lowercase(),
        kind: "character".into(),
        name: name.into(),
        aspects: IndexMap::new(),
        attrs: Attrs::new(),
        born_action: None,
        dead_action: None,
        stackable: false,
    };
    e.attrs
        .insert("container".into(), AttrValue::Str("house".into()));
    add_entity(&mut s, e).expect("fixture entity");
    s
}

// ------------------------------- snap --------------------------------------

#[test]
fn snap_hides_map_insertion_order_and_sorts_by_utf16_order() {
    // two states with the same CONTENT but different insertion orders must
    // snap identically (the five `a<b?-1:1` sorts + the fired `.sort()`)
    let mk = |order: &[&str], fired: &[&str]| {
        let mut s = solo("MARCH");
        for id in order {
            let e = Entity {
                id: (*id).to_string(),
                kind: "item".into(),
                name: (*id).to_string(),
                aspects: IndexMap::new(),
                attrs: Attrs::new(),
                born_action: Some(1),
                dead_action: None,
                stackable: false,
            };
            add_entity(&mut s, e).unwrap();
        }
        for f in fired {
            s.fired.insert((*f).to_string());
        }
        s
    };
    let a = mk(&["zeta", "alpha", "mid"], &["t_b", "t_a"]);
    let b = mk(&["mid", "zeta", "alpha"], &["t_a", "t_b"]);
    assert_eq!(snap(&a), snap(&b), "insertion order must not show");
    // and the emitted ents array is ascending: alpha < mid < zeta
    let s = snap(&a);
    let ia = s.find(r#""ents":[["alpha""#).expect("alpha first");
    let im = s.find(r#"["mid""#).expect("mid second");
    let iz = s.find(r#"["zeta""#).expect("zeta last");
    assert!(ia < im && im < iz, "ents must sort ascending: {s}");
    // fired ascending too
    let fa = s.find(r#""fired":["t_a","t_b"]"#).expect("fired sorted");
    let _ = fa;
}

#[test]
fn snap_sorts_keys_by_utf16_code_units_not_rust_bytes() {
    // U+10000 (surrogate pair, first unit 0xD800) sorts BEFORE U+E000 in UTF-16
    // `<` order but AFTER it in Rust byte order — the exact trap this
    // pin exists for
    let mut s = solo("MARCH");
    for (id, name) in [("\u{e000}", "E000"), ("\u{10000}", "PLANE1")] {
        let e = Entity {
            id: id.to_string(),
            kind: "item".into(),
            name: name.into(),
            aspects: IndexMap::new(),
            attrs: Attrs::new(),
            born_action: None,
            dead_action: None,
            stackable: false,
        };
        add_entity(&mut s, e).unwrap();
    }
    let s = snap(&s);
    let hi = s.find("\u{10000}").expect("non-bmp id present");
    let e0 = s.find("\u{e000}").expect("bmp id present");
    assert!(hi < e0, "UTF-16 order puts the surrogate pair first: {s}");
}

#[test]
fn snap_matches_play_and_fold_through_a_settled_step() {
    let (dict, content) = pace_rig();
    let state = solo("ANA");
    let sentence = parse_sentence("0:0 @ ANA PACE", &dict).unwrap();
    let r = settle_step(
        &content,
        &dict,
        state,
        Step {
            start_min: None,
            clock: sentence.clock,
            clauses: sentence.clauses,
            t_real: None,
        },
        1,
        None,
    )
    .unwrap();
    assert!(r.rejected.is_none());
    let played = r.state;
    let folded = fold(&r.records, &solo("ANA"), &dict, &content).unwrap();
    assert_eq!(snap(&folded), snap(&played), "fold equality (law 6)");
}

// ------------------------------- settle ------------------------------------

#[test]
fn settle_completes_the_window_and_act_done_route() {
    let (dict, content) = pace_rig();
    let sentence = parse_sentence("0:0 @ ANA PACE", &dict).unwrap();
    let r = settle_step(
        &content,
        &dict,
        solo("ANA"),
        Step {
            start_min: None,
            clock: sentence.clock,
            clauses: sentence.clauses,
            t_real: None,
        },
        1,
        None,
    )
    .unwrap();
    assert!(r.rejected.is_none());
    let kinds: Vec<&str> = r.records.iter().map(|x| x.kind.as_str()).collect();
    assert!(kinds.contains(&"action"), "begin row present: {kinds:?}");
    assert!(kinds.contains(&"complete"), "settled completion present: {kinds:?}");
    // the begin row is the spine; the completion carries the narrate
    assert_eq!(act(&r).unwrap().kind, "action");
    assert_eq!(done(&r).unwrap().kind, "complete");
    assert_eq!(refusal(&r), None);
    // a settled turn ends at start + Σdurations — the same minute by construction
    assert_eq!(done(&r).unwrap().advanced_min, 0);
    assert_eq!(act(&r).unwrap().advanced_min, 30);
}

#[test]
fn refusal_reads_the_authored_gate_and_the_turn_rolls_back() {
    let mut dict = core_dictionary();
    dict.register_verb("SULK").expect("SULK registers");
    let raw = chorus::content::content::ContentRaw {
        verbs: [(
            "SULK".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".into(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".into()),
                    shown: None,
                }],
                duration: 30,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["REFUSE(\"not in the mood.\")".to_string()],
                }],
            },
        )]
        .into_iter()
        .collect(),
        ..chorus::content::content::ContentRaw::default()
    };
    let content = compile_content(&raw, &dict, &[]).unwrap();
    let base = solo("ANA");
    let sentence = parse_sentence("0:0 @ ANA SULK", &dict).unwrap();
    let r = settle_step(
        &content,
        &dict,
        clone_state(&base),
        Step {
            start_min: None,
            clock: sentence.clock,
            clauses: sentence.clauses,
            t_real: None,
        },
        1,
        None,
    )
    .unwrap();
    assert_eq!(refusal(&r).as_deref(), Some("not in the mood."));
    // a refused turn logs its BEGIN attempt only (the ids are REUSED next
    // round) and the state rolls back whole
    assert_eq!(r.records.len(), 1);
    assert_eq!(r.records[0].kind, "action");
    assert_eq!(snap(&r.state), snap(&base), "the state rolls back whole");
}

// -------------------------------- seed -------------------------------------

#[test]
fn seed_content_compiles_and_seed_state_carries_the_world() {
    let dict = core_dictionary();
    let content = compile_content(&seed_content(), &dict, &[])
        .expect("the seed content compiles against the core dictionary");
    // the twelve-verb core set is authored (§4.3)
    for v in [
        "READ", "STUDY", "TALK", "TRAVEL_TO", "USE_ON", "WAIT", "WORK", "EXPLORE", "MANSUS",
        "DREAM", "LOCATE_AT", "ACQUIRE",
    ] {
        assert!(content.verbs.contains_key(v), "{v} compiled");
    }
    assert_eq!(content.templates.len(), 12);
    assert_eq!(content.triggers.len(), 1);
    assert!(content.prototypes.contains_key("CLUE"));

    let s = seed_state();
    assert_eq!(s.entities.len(), 15);
    let ana = &s.entities["ana"];
    assert!(matches!(ana.aspects.get("occult"), Some(AspectVal::Num(2))));
    // the study card's self-named aspect (§9.14 reserved-lexeme dodge)
    assert!(matches!(
        s.entities["study"].aspects.get("study"),
        Some(AspectVal::Num(1))
    ));
    // the pale lady carries a sub-map aspect (Q13 flat model)
    assert!(matches!(
        s.entities["pale"].aspects.get("language"),
        Some(AspectVal::Sub(_))
    ));
    // ANA is BERTHA's parent — the one social edge, under its canonical key
    assert!(s
        .relations
        .contains_key("ana|bertha|parent_child"));
}

// ---------------------------- explorer_lib ---------------------------------

#[test]
fn the_win_path_plays_end_to_end_through_lenient_input() {
    let world = explorer_world();
    let (states, records) = win_path_states(&world).expect("the seed-1899 win path holds");
    assert_eq!(states.len(), WIN_LEN, "a snapshot per step plus start");
    assert_eq!(states[0].0, "start");
    assert!(!records.is_empty());
    // the case is closed: the story ended through the inquest
    assert!(states.last().unwrap().1.ended.is_some());
    // and the fold of the whole path reproduces the final state byte-for-byte
    let replayed = fold(
        &records,
        &clone_state(&world.base),
        &world.dict,
        &world.content,
    )
    .unwrap();
    assert_eq!(snap(&replayed), snap(&states.last().unwrap().1));
}

const WIN_LEN: usize = 10; // 'start' + 9 WIN_PATH lines

#[test]
fn candidate_generator_offers_the_pinned_probes() {
    let world = explorer_world();
    let cands = candidates_for(
        &world.content,
        &world.dict,
        &world.base,
        &world.player,
        Depth::Sampled,
    );
    let tags: Vec<&str> = cands.iter().map(|c| c.tag.as_str()).collect();
    assert!(tags.contains(&"multi-clause"));
    assert!(tags.iter().any(|t| t.ends_with(" min-fill")));
    assert!(tags.iter().any(|t| t.contains("wrong-kind")));
    assert!(tags.iter().all(|t| *t != "clock rewind"), "now < 60");
    // every candidate step carries the front-door clock (§9.28)
    assert_eq!(
        cands[0].step.clock,
        Some(Clock { day: 0, halfhour: 42 })
    );
}

#[test]
fn the_sampled_walk_harvests_states_deterministically() {
    let world = explorer_world();
    let a = sampled_walk_states(&world).expect("the walk folds cleanly");
    let b = sampled_walk_states(&world).expect("the walk folds cleanly");
    assert!(!a.is_empty(), "the walk harvests up to ten states");
    assert_eq!(a.len(), b.len());
    for (xa, xb) in a.iter().zip(b.iter()) {
        assert_eq!(xa.0, xb.0, "same labels");
        assert_eq!(snap(&xa.1), snap(&xb.1), "the LCG 'NIGH' walk is deterministic");
    }
}
