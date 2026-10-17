//! Completion triggers (`on complete`, §6.7) in the reach analyzer: the
//! `site_proven` Complete arm under the Wave-3 tightened model. The Wave-1
//! arm ("home obtainable ⇒ the window is arrangeable") was over-generous —
//! the fuzz generator v2 exposed three classes of false OBTAINABLE, and the
//! arm now demands a CONSTRUCTIBLE completion: a committable verb whose
//! committed-role slots can lock the home (the window rung), under a
//! selectable completable recipe that declares the aspect (the aspect
//! rung), with the body's mints colliding with nothing the completion draft
//! holds (the draft rung). The support lines here are byte-frozen (the
//! render contract).

use std::process::Command;

use indexmap::IndexMap;

use chorus::content::{
    compile_content, CompleteTriggerDef, ContentRaw, PrototypeDef, RecipeDef, SlotDef, TriggerDef,
    VerbContent,
};
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

/// Compile a fixture (one fresh dictionary per test; no verbs registered).
fn compile(raw: &ContentRaw) -> chorus::content::CompiledContent {
    let dict = core_dictionary();
    compile_content(raw, &dict, &[]).expect("fixture content compiles")
}

fn proto(kind: &str) -> PrototypeDef {
    PrototypeDef { kind: kind.to_string(), ..PrototypeDef::default() }
}

fn complete_trigger(home: &str, aspect: Option<&str>) -> TriggerDef {
    TriggerDef::Complete(CompleteTriggerDef {
        id: format!("complete:{home}"),
        home: home.to_string(),
        aspect: aspect.map(str::to_string),
        out: vec!["SPAWN X_CARD".to_string()],
    })
}

/// The Wave-3 window witness, as a raw verb (a core-dictionary name): the
/// committed `object` slot
/// accepts the home (no query), the recipe is selectable, completable, and
/// (optionally) declares an aspect. NO duration condition exists — the
/// engine completes duration-0 windows like any other.
fn binder_verb(aspects: Vec<String>) -> (String, VerbContent) {
    (
        "WORK".to_string(),
        VerbContent {
            slots: vec![
                SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".to_string()),
                    shown: None,
                },
                SlotDef {
                    role: "object".to_string(),
                    min: 1,
                    max: Some(1),
                    query: None,
                    shown: None,
                },
            ],
            duration: 30,
            recipes: vec![RecipeDef {
                id: Some("work.r0".to_string()),
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects,
                out: vec![],
            }],
        },
    )
}

fn mint_fixture(aspect: Option<&str>) -> ContentRaw {
    ContentRaw {
        prototypes: Some(IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
        ])),
        verbs: IndexMap::from([binder_verb(aspect.map(str::to_string).into_iter().collect())]),
        triggers: Some(vec![complete_trigger("HOME_CARD", aspect)]),
        ..ContentRaw::default()
    }
}

/// The happy path: an INITIAL home card is lockable by the committable verb
/// under its completable recipe, so the completion trigger proves — and the
/// support line is byte-exact (the window witness names verb, slot and
/// recipe).
#[test]
fn a_complete_trigger_on_an_initial_home_proves_its_mint_with_the_exact_support_line() {
    let content = compile(&mint_fixture(None));
    let state = state_with(vec![ent("ANA", "character", &[]), ent("HOME_CARD", "item", &[])]);
    let rep = analyze_card(&content, &state, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(rep.existence, Existence::Proven);
    assert_eq!(
        rep.chain,
        vec![
            "SPAWN mints 'X_CARD' — trigger complete:HOME_CARD".to_string(),
            "  · trigger complete:HOME_CARD fires when a window completes binding 'HOME_CARD' — completion is arrangeable (commit WORK locking 'HOME_CARD' in its object slot; recipe work.r0 completes the window)"
                .to_string(),
        ]
    );
}

/// The aspect form (`on complete <aspect>`): the engine fires it only when
/// the completed recipe DECLARES the aspect (advance.rs matches
/// `recipe.aspects`), so the witness pair must be a recipe carrying it —
/// the support line names that recipe.
#[test]
fn an_aspect_complete_trigger_carries_the_filter_in_its_exact_support_line() {
    let content = compile(&mint_fixture(Some("labor")));
    let state = state_with(vec![ent("ANA", "character", &[]), ent("HOME_CARD", "item", &[])]);
    let rep = analyze_card(&content, &state, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(
        rep.chain,
        vec![
            "SPAWN mints 'X_CARD' — trigger complete:HOME_CARD".to_string(),
            "  · trigger complete:HOME_CARD fires when a window completes binding 'HOME_CARD' — completion is arrangeable (commit WORK locking 'HOME_CARD' in its object slot; recipe work.r0 completes the window and declares the 'labor' aspect)"
                .to_string(),
        ]
    );
}

/// The control: an unobtainable home can never complete a binding, so the
/// trigger never fires — but the site only DEMOTES to the candidate ladder
/// (the conservative direction: 'unobtainable' stays structural).
#[test]
fn a_complete_trigger_whose_home_is_unobtainable_stays_unproven_candidate() {
    let content = compile(&mint_fixture(None));
    let state = state_with(vec![ent("ANA", "character", &[])]);
    let rep = analyze_card(&content, &state, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert_eq!(rep.existence, Existence::Candidate);
}

/// The window rung fires when no committable verb can lock the home: the
/// same fixture MINUS the binder verb. The support line's arrangeability
/// claim is gone; the trigger's mint demotes (Wave-3 tightening: a home
/// obtainable on its own is NOT arrangeability).
#[test]
fn a_complete_trigger_without_any_home_locking_verb_stays_unproven() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
        ])),
        triggers: Some(vec![complete_trigger("HOME_CARD", None)]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[]), ent("HOME_CARD", "item", &[])]);
    let rep = analyze_card(&content, &state, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert_eq!(rep.existence, Existence::Candidate);
}

/// A PRODUCED home works too: once the fixpoint proves the home card's own
/// mint (here a slot trigger), the avail lookup arms the completion trigger
/// — the expiry_support desc-lookup shape, second branch.
#[test]
fn a_complete_trigger_on_a_produced_home_proves_after_the_home_is_proven() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
        ])),
        verbs: IndexMap::from([binder_verb(vec![])]),
        triggers: Some(vec![
            TriggerDef::Slot(chorus::content::SlotTriggerDef {
                id: "seed".to_string(),
                slot: 2,
                once: None,
                out: vec!["SPAWN HOME_CARD".to_string()],
            }),
            complete_trigger("HOME_CARD", None),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);
    let rep = analyze_card(&content, &state, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep
        .chain
        .join("\n")
        .contains("trigger complete:HOME_CARD fires when a window completes binding 'HOME_CARD'"));
}

/// The integration smoke: the cultist slice's THE_TURNING is minted ONLY by
/// `on complete` clauses (VIGOR#2's `on complete job`, the two posts'), and
/// the probe must still prove it — the tightened window witness resolves to
/// WORK's `with` slot (0..1: an optional fill the player includes) under
/// work.vigor (aspects labor, job), exit 0, OBTAINABLE, the byte-exact
/// support line — and `--all` must sweep the whole slice without a panic.
#[test]
fn cultist_slice_the_turning_proves_via_its_complete_triggers_and_all_exits_0() {
    let probe = Command::new(env!("CARGO_BIN_EXE_reach"))
        .args(["scripts/cultist-slice.txt", "THE_TURNING"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the reach bin spawns");
    assert_eq!(probe.status.code(), Some(0));
    let out = String::from_utf8_lossy(&probe.stdout);
    assert!(out.contains("verdict: OBTAINABLE"), "{out}");
    assert!(
        out.contains(
            "trigger complete:VIGOR#2 fires when a window completes binding 'VIGOR' — completion is arrangeable (commit WORK locking 'VIGOR' in its with slot; recipe work.vigor completes the window and declares the 'job' aspect)"
        ),
        "{out}"
    );

    let all = Command::new(env!("CARGO_BIN_EXE_reach"))
        .args(["scripts/cultist-slice.txt", "--all"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the reach bin spawns");
    assert_eq!(all.status.code(), Some(0));
}
