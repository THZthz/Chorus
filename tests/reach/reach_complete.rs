//! Completion triggers (`on complete`, §6.7) in the reach analyzer: the
//! `site_proven` Complete arm. The contract mirrors the Expire arm's
//! generosity: the trigger is arrangeable exactly when its home card is
//! obtainable (initial or produced) — a window completing the binding is
//! player-schedulable, so nothing else gates the site. The support lines
//! here are byte-frozen (the render contract).

use std::process::Command;

use indexmap::IndexMap;

use chorus::content::{compile_content, CompleteTriggerDef, ContentRaw, PrototypeDef, TriggerDef};
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

fn mint_fixture(aspect: Option<&str>) -> ContentRaw {
    ContentRaw {
        prototypes: Some(IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
        ])),
        triggers: Some(vec![complete_trigger("HOME_CARD", aspect)]),
        ..ContentRaw::default()
    }
}

/// The happy path: an INITIAL home card makes the completion trigger
/// arrangeable, so its mint proves — and the support line is byte-exact.
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
            "  · trigger complete:HOME_CARD fires when a window completes binding 'HOME_CARD' — completion is arrangeable (begin a duration>0 window that binds the home card)"
                .to_string(),
        ]
    );
}

/// The aspect form (`on complete <aspect>`): the engine fires it only when
/// the completed recipe DECLARES the aspect (advance.rs matches
/// `recipe.aspects`), so the filter belongs in the support line — the
/// arrangeability argument is unchanged (begin a window whose recipe
/// carries the aspect).
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
            "  · trigger complete:HOME_CARD fires when a window completes binding 'HOME_CARD' — completion is arrangeable (begin a duration>0 window that binds the home card and declares the 'labor' aspect)"
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
/// the probe must now prove it — exit 0, OBTAINABLE, the byte-exact aspect
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
            "trigger complete:VIGOR#2 fires when a window completes binding 'VIGOR' — completion is arrangeable (begin a duration>0 window that binds the home card and declares the 'job' aspect)"
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
