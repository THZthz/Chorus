//! Stackable cards (21 tests): multiple
//! same-name instances (spec §4.2). Declared with `stackable`
//! on entity/prototype lines; the name becomes a POOL of living copies
//! addressed oldest-first; engine-written expiry destroys go by id while
//! authored name-addressed outcomes hit the oldest.
//!
//! Every test builds a FRESH Dictionary (the module-level dict pair becomes
//! per-test constructors — `pool_dict()`
//! registers the rig's own lexemes STEW/PROBE/CALM/DECAY). Error-message
//! patterns are copied CHARACTER FOR CHARACTER from the frozen literals.
//! object-identity semantics (`r.state === pooled`) have no Rust observable:
//! `settle_step` hands the ORIGINAL (never-mutated) state back on a refusal,
//! as a value — `snap()` equality IS the semantic content of those checks
//! (noted at each site, never weakened).

#[path = "../common/mod.rs"]
mod common;

use common::harness::assert_err_msg;
use common::seed::seed_state;
use common::settle::{act, done, refusal, settle_step};
use common::snap::snap;

use chorus::content::content::{
    compile_content, CompiledContent, ContentRaw, PrototypeDef, PrototypeDerive, RecipeDef,
    SlotDef, VerbContent,
};
use chorus::content::script::{compile_script, parse_script};
use chorus::core::clock::Clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::predicate::{eval_pred, parse_predicate, PredCtx, RoleBindings};
use chorus::core::r#ref::Ref;
use chorus::core::sentence::Arg;
use chorus::core::types::{
    add_entity, clone_state, new_state, AspectVal, Aspects, AttrValue, Attrs, Entity, State,
};
use chorus::engine::engine::{ActionRecord, ExecResult, Step};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::session::game::Game;
use chorus::session::lenient::{parse_player_input, LenientCtx, ParsedInput};

// ---------------------------------------------------------------- rigs ------

/// The rig dictionary: core verbs + the
/// rig's own lexemes, so every parse below works. Per-test fresh.
fn pool_dict() -> Dictionary {
    let mut d = core_dictionary();
    d.register_verb("STEW").unwrap();
    d.register_verb("PROBE").unwrap();
    d.register_verb("CALM").unwrap();
    d.register_verb("DECAY").unwrap();
    d
}

/// The rig script
/// exercises the §10 `stackable` keyword end to end: a stackable prototype
/// (SPAWN SUSPICION pools), a destroy-by-name verb, and a probe that binds
/// up to two same-named cards into its object role.
const POOL_SCRIPT: &str = r#"
script: Pools
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE

prototypes:
  SUSPICION is state stackable with weight:1

verbs:
  STEW:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> SPAWN SUSPICION, NARRATE "suspicion stirs."
  PROBE:
    actors: 1 kind:character
    object: 0..2 kind:state
    duration: 0m
    recipes:
      probe.clear when count(object) >= 1 -> SET(OBJECT, aspect:noted, +1), DESTROY OBJECT
      default -> NARRATE "{actors} probe."
  CALM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DESTROY SUSPICION
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "{actors} wait."
  ACQUIRE:
    actors: 1 kind:character
    object: 0..1
    duration: 0m
    recipes:
      default -> NARRATE "{actors} acquires."

templates:
  STEW says "{actors} stews."
  PROBE says "{actors} probe."
  CALM says "{actors} calms."
  WAIT says "{actors} wait."
"#;

fn rig(dict: &Dictionary) -> (CompiledContent, State) {
    let compiled = compile_script(POOL_SCRIPT, dict).unwrap();
    (compiled.content, compiled.state)
}

/// settle_step with the exact id-generator
/// closure: `x{first_record_id}_{n}`, the counter incrementing per minted id.
fn run(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    src: &str,
    first_record_id: i64,
) -> ExecResult {
    let s = parse_sentence(src, dict).unwrap();
    let mut n: i64 = 0;
    let mut id_gen = || {
        n += 1;
        format!("x{first_record_id}_{n}")
    };
    settle_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        first_record_id,
        Some(&mut id_gen),
    )
    .unwrap()
}

fn stew(content: &CompiledContent, dict: &Dictionary, mut state: State, times: usize) -> State {
    let mut id = 1i64;
    for _ in 0..times {
        let r = run(content, dict, state, "0:0 @ ANA STEW", id);
        assert_eq!(r.rejected, None, "stew must not reject");
        id += r.records.len() as i64;
        state = r.state;
    }
    state
}

/// `refusal(r)` matches `re`.
fn assert_refusal(r: &ExecResult, pattern: &str) {
    let msg =
        refusal(r).unwrap_or_else(|| panic!("expected a refusal matching /{pattern}/, got None"));
    let re = regex::Regex::new(pattern).unwrap();
    assert!(re.is_match(&msg), "refusal {msg:?} does not match /{pattern}/");
}

/// Temp log path, unique per process and tag.
fn tmp_log_path(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("chorus-pool-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("log.jsonl")
}

// ---------------------------------------------------------------------------
// the stackable keyword (§4.2, §10)
// ---------------------------------------------------------------------------

#[test]
fn parses_on_entity_and_prototype_lines_and_lands_in_the_compiled_sets() {
    let dict = core_dictionary();
    let parsed = parse_script(
        r#"
entities:
  SUSPICION is state stackable with weight:1
  WALL is place stackable in HOUSE
  HOUSE is place
  PLAIN is idea

prototypes:
  RUMOUR is idea stackable deriving name from object
  QUIET is idea
"#,
        &dict,
    )
    .unwrap();
    assert!(parsed.entities.iter().find(|e| e.name == "SUSPICION").unwrap().stackable);
    assert!(parsed.entities.iter().find(|e| e.name == "WALL").unwrap().stackable);
    assert!(!parsed.entities.iter().find(|e| e.name == "PLAIN").unwrap().stackable);
    let protos = parsed.content.prototypes.as_ref().unwrap();
    assert!(protos.get("RUMOUR").unwrap().stackable);
    // the absent optional; the Rust `bool` models
    // "absent = not stackable" exactly (the field is optional in the frozen
    // shape only because every read is a truthiness test)
    assert!(!protos.get("QUIET").unwrap().stackable);

    let compiled = compile_script(
        r#"
entities:
  SUSPICION is state stackable with weight:1
  WALL is place stackable
  HOUSE is place

prototypes:
  RUMOUR is idea stackable deriving name from object
  QUIET is idea
"#,
        &dict,
    )
    .unwrap();
    let content = &compiled.content;
    let state = &compiled.state;
    assert!(content.stackable_names.contains("SUSPICION"));
    assert!(content.stackable_names.contains("RUMOUR"));
    assert!(!content.stackable_names.contains("QUIET"));
    assert!(content.stackable_kinds.contains("state")); // kinds of stackable declarations
    assert!(content.stackable_kinds.contains("idea"));
    // entity-line cards mint stackable per their declaration
    assert!(state.entities.get("s_suspicion").unwrap().stackable);
    assert!(state.entities.get("s_wall").unwrap().stackable);
    assert!(!state.entities.get("s_house").unwrap().stackable);
}

#[test]
fn the_keyword_must_sit_before_with_deriving_anything_else_fails_the_line() {
    let dict = core_dictionary();
    assert!(parse_script("entities:\n  X is place with a:1 stackable\n", &dict).is_err());
    assert!(parse_script("prototypes:\n  X is idea deriving name from object stackable\n", &dict)
        .is_err());
}

// ---------------------------------------------------------------------------
// spawn pools and oldest-copy addressing (§4.2)
// ---------------------------------------------------------------------------

#[test]
fn three_spawns_stack_the_pool_holds_three_ids_in_birth_order_exists_sees_the_pool() {
    let dict = pool_dict();
    let (content, state) = rig(&dict);
    let r3state = stew(&content, &dict, state, 3);
    let pool = r3state.by_name.get("SUSPICION").unwrap();
    assert_eq!(pool.len(), 3);
    // insertion order = birth order; v0.5 mints stamp the completion rows
    let borns: Vec<i64> =
        pool.iter().map(|id| r3state.entities.get(id).unwrap().born_action.unwrap()).collect();
    assert!(borns[0] < borns[1]);
    assert!(borns[1] < borns[2]);
    let p = parse_predicate("exists(SUSPICION)").unwrap();
    let ctx = PredCtx { b: &RoleBindings::new(), state: &r3state, strict: false, roll: None };
    assert!(eval_pred(&p, &ctx).unwrap());
}

#[test]
fn the_player_surface_lists_every_copy_inspect_reads_the_oldest() {
    let mut game = Game::from_script(POOL_SCRIPT, pool_dict()).unwrap();
    for _ in 0..3 {
        game.handle_line("stew").unwrap();
        game.wait().unwrap(); // v0.5: the spawn lands at the completion
    }
    assert_eq!(game.cards().into_iter().filter(|l| l.starts_with("SUSPICION")).count(), 3); // cosmetic duplicates OK
    let face = game.inspect("suspicion");
    assert_eq!(face[0], "SUSPICION  [state]");
    let oldest_born = game
        .state
        .entities
        .get(&game.state.by_name.get("SUSPICION").unwrap()[0])
        .unwrap()
        .born_action;
    assert!(
        face.join("\n").contains(&format!("entered play at action {}", oldest_born.unwrap())),
        "{face:?}"
    ); // first-visible = oldest
}

#[test]
fn destroy_by_name_takes_the_oldest_then_the_next_oldest_the_last_release_frees_the_name() {
    let dict = pool_dict();
    let (content, state) = rig(&dict);
    let mut cur = stew(&content, &dict, state, 3);
    let mut id = 4i64;
    for expected in [2usize, 1, 0] {
        let r = run(&content, &dict, cur, "0:0 @ ANA CALM", id);
        assert_eq!(r.rejected, None);
        // the authored name-addressed destroy LOGS THE NAME (oldest resolved at apply)
        assert_eq!(
            done(&r).unwrap().outcomes[0].args[0],
            Arg::Ref { r#ref: Ref::Name { kind: None, name: "SUSPICION".into() } }
        );
        assert_eq!(
            r.state.by_name.get("SUSPICION").map_or(0, |v| v.len()),
            expected // the pool reads as empty once the name frees (the empty fallback)
        );
        cur = r.state;
        id += r.records.len() as i64;
    }
    assert!(!cur.by_name.contains_key("SUSPICION"));
    // the freed name accepts a non-stackable mint again (§5.1, among the living)
    let plain = compile_content(
        &ContentRaw {
            verbs: [(
                "ACQUIRE".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        out: vec!["SPAWN SUSPICION".to_string()],
                    }],
                },
            )]
            .into_iter()
            .collect(),
            prototypes: Some(
                [(
                    "SUSPICION".to_string(),
                    PrototypeDef { kind: "state".into(), ..Default::default() }, // NOT stackable
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        &core_dictionary(),
        &[],
    )
    .unwrap();
    let r = run(&plain, &dict, cur, "0:0 @ ANA ACQUIRE", id);
    assert_eq!(r.rejected, None);
}

#[test]
fn a_stackable_spawn_onto_a_distinct_non_stackable_living_name_rejects_nothing_logged() {
    let dict = pool_dict();
    let poke = compile_content(
        &ContentRaw {
            verbs: [(
                "ACQUIRE".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        out: vec!["SPAWN CHALICE".to_string()],
                    }],
                },
            )]
            .into_iter()
            .collect(),
            prototypes: Some(
                [(
                    "CHALICE".to_string(),
                    PrototypeDef { kind: "item".into(), stackable: true, ..Default::default() }, // stackable, but CHALICE lives non-stackable
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        &core_dictionary(),
        &[],
    )
    .unwrap();
    let mut state = seed_state();
    let r = run(&poke, &dict, clone_state(&state), "0:0 @ ANA ACQUIRE", 1);
    assert_eq!(r.rejected, None);
    assert_refusal(&r, "already in use");
    // §6.4: the window registered at begin — the completion-time refusal
    // still counted (the one delta; nothing else moved, and play/fold agree)
    assert_eq!(r.state.executed.get("acquire.default"), Some(&1));
    let replayed = chorus::engine::fold::fold(&r.records, &state, &dict, &poke).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    // once the non-stackable holder dies the name accepts the stackable mint
    state.entities.get_mut("chalice").unwrap().dead_action = Some(99);
    state.by_name.remove("CHALICE"); // by_name is the order-pool HashMap (outer map is unordered)
    let r2 = run(&poke, &dict, state, "0:0 @ ANA ACQUIRE", 2);
    assert_eq!(r2.rejected, None);
}

#[test]
fn a_non_stackable_spawn_onto_an_existing_pool_name_rejects_the_shared_namespace() {
    let dict = pool_dict();
    let (content, state) = rig(&dict);
    let pooled = stew(&content, &dict, state, 2);
    let plain = compile_content(
        &ContentRaw {
            verbs: [(
                "ACQUIRE".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        out: vec!["SPAWN SUSPICION".to_string()],
                    }],
                },
            )]
            .into_iter()
            .collect(),
            prototypes: Some(
                [(
                    "SUSPICION".to_string(),
                    PrototypeDef { kind: "state".into(), ..Default::default() }, // NOT stackable
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        &core_dictionary(),
        &[],
    )
    .unwrap();
    let _before = snap(&pooled); // computed but never asserted (deliberate)
    let r = run(&plain, &dict, clone_state(&pooled), "0:0 @ ANA ACQUIRE", 1);
    assert_eq!(r.rejected, None);
    assert_refusal(&r, "already in use");
    assert_eq!(
        r.state.entities.get(&pooled.by_name.get("SUSPICION").unwrap()[0]).unwrap().dead_action,
        None // nothing applied
    );
}

#[test]
fn add_entity_enforces_the_law_itself_non_stackable_duplicates_throw_stackables_append() {
    let mint = |s: &mut State, id: &str, stackable: bool| {
        add_entity(
            s,
            Entity {
                id: id.to_string(),
                kind: "state".to_string(),
                name: "X".to_string(),
                aspects: Aspects::new(),
                attrs: Attrs::new(),
                born_action: Some(1),
                dead_action: None,
                stackable,
            },
        )
    };
    let mut s = new_state(0);
    mint(&mut s, "a", false).unwrap();
    assert_err_msg(mint(&mut s, "b", false), "duplicate name 'X'");
    assert_err_msg(mint(&mut s, "b", true), "duplicate name 'X'"); // a non-stackable holder blocks stackers too
    let mut s2 = new_state(0);
    mint(&mut s2, "a", true).unwrap();
    mint(&mut s2, "b", true).unwrap();
    assert_eq!(s2.by_name.get("X"), Some(&vec!["a".to_string(), "b".to_string()]));
    assert_err_msg(mint(&mut s2, "c", false), "duplicate name 'X'");
}

#[test]
fn a_stackable_prototype_with_derive_name_from_object_stacks_under_the_derived_name() {
    let dict = core_dictionary();
    let spy = compile_content(
        &ContentRaw {
            verbs: [(
                "ACQUIRE".to_string(),
                VerbContent {
                    slots: vec![
                        SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".into()),
                            shown: None,
                        },
                        SlotDef {
                            role: "object".into(),
                            min: 1,
                            max: Some(1),
                            query: None,
                            shown: None,
                        },
                    ],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        out: vec!["SPAWN RUMOUR".to_string()],
                    }],
                },
            )]
            .into_iter()
            .collect(),
            prototypes: Some(
                [(
                    "RUMOUR".to_string(),
                    PrototypeDef {
                        kind: "idea".into(),
                        stackable: true,
                        derive: Some(PrototypeDerive {
                            name_from: Some("object".into()),
                            aspects: None,
                        }),
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        &dict,
        &[],
    )
    .unwrap();
    let mut state = seed_state();
    let mut id = 1i64;
    for _ in 0..2 {
        let r = run(&spy, &dict, state, "0:0 @ ANA ACQUIRE CHALICE", id);
        assert_eq!(r.rejected, None);
        id += r.records.len() as i64;
        state = r.state;
    }
    assert_eq!(state.by_name.get("RUMOUR_CHALICE").unwrap().len(), 2);
}

#[test]
fn kind_level_stackability_a_plain_spawn_of_a_non_stackable_prototype_in_a_stackable_kind() {
    // the authored ref is a bare name (no kind); the concrete row always
    // carries the resolved kind — so the stackable flag must be computed from
    // the resolved KIND at play too, or the fold's recompute diverges
    let dict = pool_dict();
    let c = compile_content(
        &ContentRaw {
            verbs: [
                (
                    "ACQUIRE".to_string(),
                    VerbContent {
                        slots: vec![SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".into()),
                            shown: None,
                        }],
                        duration: 0,
                        recipes: vec![RecipeDef {
                            id: None,
                            when: None,
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            out: vec!["SPAWN HEARSAY".to_string()],
                        }],
                    },
                ),
                (
                    "DECAY".to_string(),
                    VerbContent {
                        slots: vec![
                            SlotDef {
                                role: "actors".into(),
                                min: 1,
                                max: Some(1),
                                query: Some("kind:character".into()),
                                shown: None,
                            },
                            SlotDef {
                                role: "object".into(),
                                min: 1,
                                max: Some(1),
                                query: None,
                                shown: None,
                            },
                        ],
                        duration: 0,
                        recipes: vec![RecipeDef {
                            id: None,
                            when: None,
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            out: vec!["REPLACE OBJECT WITH HEARSAY".to_string()],
                        }],
                    },
                ),
            ]
            .into_iter()
            .collect(),
            prototypes: Some(
                [
                    // RUMOUR makes 'idea' a stackable KIND; HEARSAY is NOT stackable by name
                    ("RUMOUR", true),
                    ("HEARSAY", false),
                ]
                .into_iter()
                .map(|(n, st)| {
                    (
                        n.to_string(),
                        PrototypeDef { kind: "idea".into(), stackable: st, ..Default::default() },
                    )
                })
                .collect(),
            ),
            ..ContentRaw::default()
        },
        &dict,
        &[],
    )
    .unwrap();
    let base = seed_state();
    let r1 = run(&c, &dict, clone_state(&base), "0:0 @ ANA ACQUIRE", 1);
    assert_eq!(r1.rejected, None);
    let h1 = r1.state.by_name.get("HEARSAY").unwrap()[0].clone();
    assert!(r1.state.entities.get(&h1).unwrap().stackable);
    // the REPLACE successor computes the same way
    let r2 = run(&c, &dict, r1.state, "0:0 @ ANA DECAY CHALICE", r1.records.len() as i64 + 1);
    assert_eq!(r2.rejected, None);
    let h2 = r2.state.by_name.get("HEARSAY").unwrap()[0].clone();
    assert!(r2.state.entities.get(&h2).unwrap().stackable);
    // replay agreement: snap serializes every entity (flag included), so any
    // play/fold divergence in the recompute shows here — and the fold replays
    // BOTH steps' concrete rows, so x1_1's flag is recomputed from its log too
    let both: Vec<ActionRecord> = r1.records.iter().chain(r2.records.iter()).cloned().collect();
    let replayed = fold(&both, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
}

// ---------------------------------------------------------------------------
// slot bindings hold same-named copies individually (§4.2)
// ---------------------------------------------------------------------------

#[test]
fn two_copies_bind_by_id_set_and_destroy_object_address_each_bound_card() {
    let dict = pool_dict();
    let (content, state) = rig(&dict);
    let pooled = stew(&content, &dict, state, 2);
    let pool = pooled.by_name.get("SUSPICION").unwrap();
    let elder = pool[0].clone();
    let younger = pool[1].clone();
    let r = run(&content, &dict, pooled, &format!("0:0 @ ANA PROBE #{elder}, #{younger}"), 3);
    assert_eq!(r.rejected, None);
    assert_eq!(act(&r).unwrap().recipes, ["probe.clear"]);
    // one concrete SET per bound card, then the destroy carries both ids
    assert!(
        done(&r).unwrap().sentence.contains(&format!(
            "-> SET(#{elder}, aspect:noted, 1), SET(#{younger}, aspect:noted, 1), DESTROY(#{elder}, #{younger})"
        )),
        "{}",
        done(&r).unwrap().sentence
    );
    assert_eq!(
        r.state.entities.get(&elder).unwrap().aspects.get("noted"),
        Some(&AspectVal::Num(1))
    );
    assert_eq!(
        r.state.entities.get(&younger).unwrap().aspects.get("noted"),
        Some(&AspectVal::Num(1))
    );
    assert!(r.state.entities.get(&elder).unwrap().dead_action.is_some());
    assert!(r.state.entities.get(&younger).unwrap().dead_action.is_some());
    assert!(!r.state.by_name.contains_key("SUSPICION")); // the last copy died: name freed
}

#[test]
fn lenient_input_resolves_a_pool_name_to_its_oldest_copy() {
    let mut game = Game::from_script(POOL_SCRIPT, pool_dict()).unwrap();
    game.handle_line("stew").unwrap();
    game.wait().unwrap(); // v0.5: the STEW window must complete for SUSPICION to mint
    game.handle_line("stew").unwrap();
    game.wait().unwrap();
    let pool = game.state.by_name.get("SUSPICION").unwrap();
    let eldest = pool[0].clone();
    let younger = pool[1].clone();
    let parsed = parse_player_input(
        "probe the suspicion",
        &LenientCtx {
            dict: &game.dict,
            content: &game.content,
            state: &game.state,
            player: Some("ANA"),
        },
    );
    let ParsedInput::Ok { step, .. } = parsed else {
        panic!("'probe the suspicion' must parse");
    };
    let r = settle_step(
        &game.content,
        &game.dict,
        clone_state(&game.state),
        step,
        game.log.next_record_id(),
        None,
    )
    .unwrap();
    assert_eq!(r.rejected, None);
    assert!(r.state.entities.get(&eldest).unwrap().dead_action.is_some()); // the oldest went
    assert_eq!(r.state.by_name.get("SUSPICION"), Some(&vec![younger.clone()]));
}

// ---------------------------------------------------------------------------
// expiry under a pool (§4.2)
// ---------------------------------------------------------------------------

#[test]
fn engine_expiry_addresses_by_id_the_younger_expires_and_dies_no_re_fire_later() {
    let dict = pool_dict();
    let (content, state) = rig(&dict);
    let pooled = stew(&content, &dict, state, 2);
    let pool = pooled.by_name.get("SUSPICION").unwrap();
    let eldest = pool[0].clone();
    let younger = pool[1].clone();
    let mut pooled = pooled;
    pooled
        .entities
        .get_mut(&younger)
        .unwrap()
        .attrs
        .insert("expiresAt".to_string(), AttrValue::Num(30)); // crosses slot 1 (0:30)
    let r = run(&content, &dict, pooled, "0:0 @ ANA WAIT", 1); // 1h: crosses slots 1 and 2
    assert_eq!(r.rejected, None);
    let events: Vec<&ActionRecord> = r.records.iter().filter(|x| x.kind == "event").collect();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].trigger.as_deref(), Some("expire:SUSPICION"));
    assert_eq!(events[0].t_game, "0:1");
    // the engine-written destroy is keyed to the EXPIRING copy: DESTROY #id
    assert_eq!(events[0].outcomes[0].args[0], Arg::Ref { r#ref: Ref::Id { id: younger.clone() } });
    assert!(r.state.entities.get(&younger).unwrap().dead_action.is_some());
    assert_eq!(r.state.entities.get(&eldest).unwrap().dead_action, None); // the elder survives
    assert_eq!(r.state.by_name.get("SUSPICION"), Some(&vec![eldest.clone()]));
    // later crossings never re-fire: the survivor carries no expires
    let r2 = run(&content, &dict, r.state, "0:2 @ ANA WAIT", r.records.len() as i64 + 1);
    assert_eq!(r2.rejected, None);
    assert_eq!(r2.records.iter().filter(|x| x.kind == "event").count(), 0);
}

#[test]
fn three_copies_past_their_expires_cross_one_two_slot_advance_oldest_dies_first_all_by_id() {
    let dict = pool_dict();
    let (content, state) = rig(&dict);
    let pooled = stew(&content, &dict, state, 3);
    let pool: Vec<String> = pooled.by_name.get("SUSPICION").unwrap().clone(); // birth order
    let mut pooled = pooled;
    for pid in &pool {
        pooled
            .entities
            .get_mut(pid)
            .unwrap()
            .attrs
            .insert("expiresAt".to_string(), AttrValue::Num(10)); // past BOTH crossed slots
    }
    let base = clone_state(&pooled);
    let r = run(&content, &dict, pooled, "0:0 @ ANA WAIT", 1); // 1h: crosses slots 1 and 2
    assert_eq!(r.rejected, None);
    let events: Vec<&ActionRecord> = r.records.iter().filter(|x| x.kind == "event").collect();
    // the expiry snapshot iterates entities in birth order: all three fire at
    // the FIRST crossed slot (0:1), oldest first; slot 2 finds nobody alive
    let t_games: Vec<&str> = events.iter().map(|e| e.t_game.as_str()).collect();
    assert_eq!(t_games, ["0:1", "0:1", "0:1"]);
    let dead_ids: Vec<String> = events
        .iter()
        .map(|e| match &e.outcomes[0].args[0] {
            Arg::Ref { r#ref: Ref::Id { id } } => id.clone(),
            _ => "(not an id ref)".to_string(),
        })
        .collect();
    assert_eq!(dead_ids, pool);
    assert!(!r.state.by_name.contains_key("SUSPICION"));
    // every row is id-keyed, so the fold replays the triple destroy exactly
    // (the fold base is the pre-play `pooled` value)
    let replayed = fold(&r.records, &base, &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn authored_expiry_asymmetry_fires_per_crossing_copy_its_name_outcomes_hit_the_oldest() {
    let dict = pool_dict();
    let asym = compile_content(
        &ContentRaw {
            verbs: [
                (
                    "STEW".to_string(),
                    VerbContent {
                        slots: vec![SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".into()),
                            shown: None,
                        }],
                        duration: 0,
                        recipes: vec![RecipeDef {
                            id: None,
                            when: None,
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            out: vec!["SPAWN SUSPICION".to_string()],
                        }],
                    },
                ),
                (
                    "WAIT".to_string(),
                    VerbContent {
                        slots: vec![SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".into()),
                            shown: None,
                        }],
                        duration: 30, // one slot per wait: each advance crosses exactly one expiry crossing
                        recipes: vec![RecipeDef {
                            id: None,
                            when: None,
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            out: vec!["NARRATE(\"{actors} wait.\")".to_string()],
                        }],
                    },
                ),
            ]
            .into_iter()
            .collect(),
            triggers: Some(vec![chorus::content::content::TriggerDef::Expire(
                chorus::content::content::ExpireTriggerDef {
                    id: "sus_burns".into(),
                    on_expire_of: "SUSPICION".into(),
                    out: vec![
                        "DESTROY SUSPICION".to_string(),
                        "NARRATE(\"a suspicion burns away.\")".to_string(),
                    ],
                },
            )]),
            prototypes: Some(
                [(
                    "SUSPICION".to_string(),
                    PrototypeDef { kind: "state".into(), stackable: true, ..Default::default() },
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        &dict,
        &[],
    )
    .unwrap();
    let (_, state) = rig(&dict);
    let pooled = stew(&asym, &dict, state, 2);
    let pool = pooled.by_name.get("SUSPICION").unwrap();
    let elder = pool[0].clone();
    let younger = pool[1].clone();
    let mut pooled = pooled;
    // the YOUNGER carries the expires. The stews cost no time, so the wait's
    // event span reaches back to 0:0 (§6.6 dead air): expiresAt 60 (slot 2)
    // crosses exactly once during the 0:1 wait
    pooled
        .entities
        .get_mut(&younger)
        .unwrap()
        .attrs
        .insert("expiresAt".to_string(), AttrValue::Num(60));
    let r = run(&asym, &dict, pooled, "0:1 @ ANA WAIT", 3);
    assert_eq!(r.rejected, None);
    let triggers: Vec<&str> = r
        .records
        .iter()
        .filter(|x| x.kind == "event")
        .map(|e| e.trigger.as_deref().unwrap())
        .collect();
    assert_eq!(triggers, ["sus_burns"]);
    // …but its name-addressed DESTROY resolved to the OLDEST copy — the asymmetry:
    // the card that died is the ELDER, not the copy whose expires crossed
    assert!(r.state.entities.get(&elder).unwrap().dead_action.is_some());
    assert_eq!(r.state.entities.get(&younger).unwrap().dead_action, None); // the crossing copy still stands
    assert_eq!(r.state.by_name.get("SUSPICION"), Some(&vec![younger.clone()]));
    // the crossing copy is past its expires: it re-fires (§6.6) on the next
    // crossing — and being now the pool's head, the second DESTROY takes IT
    let r2 = run(&asym, &dict, r.state, "0:2 @ ANA WAIT", r.records.len() as i64 + 3);
    let triggers: Vec<&str> = r2
        .records
        .iter()
        .filter(|x| x.kind == "event")
        .map(|e| e.trigger.as_deref().unwrap())
        .collect();
    assert_eq!(triggers, ["sus_burns"]);
    assert!(r2.state.entities.get(&younger).unwrap().dead_action.is_some());
    assert!(!r2.state.by_name.contains_key("SUSPICION"));
}

// ---------------------------------------------------------------------------
// REPLACE X WITH X succeeds for stackables (§4.2)
// ---------------------------------------------------------------------------

fn decay_rig(dict: &Dictionary, stackable: bool) -> CompiledContent {
    compile_content(
        &ContentRaw {
            verbs: [(
                "DECAY".to_string(),
                VerbContent {
                    slots: vec![
                        SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".into()),
                            shown: None,
                        },
                        SlotDef {
                            role: "object".into(),
                            min: 1,
                            max: Some(1),
                            query: None,
                            shown: None,
                        },
                    ],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        out: vec!["REPLACE OBJECT WITH SUSPICION".to_string()],
                    }],
                },
            )]
            .into_iter()
            .collect(),
            prototypes: Some(
                [(
                    "SUSPICION".to_string(),
                    PrototypeDef { kind: "state".into(), stackable, ..Default::default() },
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        dict,
        &[],
    )
    .unwrap()
}

#[test]
fn spawn_appends_while_the_dying_card_lives_then_the_destroy_pops_it() {
    let dict = pool_dict();
    let c = decay_rig(&dict, true);
    let (content, state) = rig(&dict);
    let pooled = stew(&content, &dict, state, 1); // one copy alive, container s_house
    let elder = pooled.by_name.get("SUSPICION").unwrap()[0].clone();
    let pooled_base = clone_state(&pooled);
    let r = run(&c, &dict, pooled, &format!("0:0 @ ANA DECAY #{elder}"), 2);
    assert_eq!(r.rejected, None);
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["SPAWN", "DESTROY"]);
    let successor: Vec<String> = r.state.by_name.get("SUSPICION").unwrap().clone();
    assert_eq!(successor.len(), 1);
    assert_ne!(successor[0], elder);
    assert!(r.state.entities.get(&elder).unwrap().dead_action.is_some());
    assert_eq!(
        r.state.entities.get(&successor[0]).unwrap().attrs.get("container"),
        Some(&AttrValue::Str("s_house".into())) // place inherited
    );
    assert!(r.state.entities.get(&successor[0]).unwrap().stackable);
    // the fold replays the concrete SPAWN+DESTROY pair exactly (§4.4)
    let replayed = fold(&r.records, &pooled_base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    // and the next-oldest (now the only) replaces again just the same
    let r2 = run(
        &c,
        &dict,
        r.state,
        &format!("0:0 @ ANA DECAY #{}", successor[0]),
        r.records.len() as i64 + 2,
    );
    assert_eq!(r2.rejected, None);
    assert_eq!(r2.state.by_name.get("SUSPICION").unwrap().len(), 1);
}

#[test]
fn a_non_stackable_replace_onto_a_stackable_pool_still_rejects() {
    let dict = pool_dict();
    let c = decay_rig(&dict, false); // NOT stackable
    let (content, state) = rig(&dict);
    let pooled = stew(&content, &dict, state, 2);
    let elder = pooled.by_name.get("SUSPICION").unwrap()[0].clone();
    let r = run(&c, &dict, clone_state(&pooled), &format!("0:0 @ ANA DECAY #{elder}"), 3); // the successor's name collides with the pool
    assert_eq!(r.rejected, None);
    assert_refusal(&r, "already in use");
    assert_eq!(r.state.entities.get(&elder).unwrap().dead_action, None); // nothing applied
                                                                         // §6.4: the window registered at begin — the completion-time refusal
                                                                         // still counted (the one delta; nothing else moved, and play/fold agree)
    assert_eq!(r.state.executed.get("decay.default"), Some(&1));
    let replayed = chorus::engine::fold::fold(&r.records, &pooled, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ---------------------------------------------------------------------------
// REFUSE after a pool destroy leaves the real pool untouched (§6.5, law 6)
// ---------------------------------------------------------------------------

#[test]
fn the_draft_spliced_a_clone_of_the_pool_array_the_real_state_keeps_both_copies() {
    let dict = pool_dict();
    let refuse = compile_content(
        &ContentRaw {
            verbs: [(
                "CALM".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        out: vec![
                            "DESTROY SUSPICION".to_string(),
                            "REFUSE(\"not yet\")".to_string(),
                        ],
                    }],
                },
            )]
            .into_iter()
            .collect(),
            prototypes: Some(
                [(
                    "SUSPICION".to_string(),
                    PrototypeDef { kind: "state".into(), stackable: true, ..Default::default() },
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        &dict,
        &[],
    )
    .unwrap();
    let (content, state) = rig(&dict);
    let pooled = stew(&content, &dict, state, 2);
    let before = snap(&pooled);
    let pool = pooled.by_name.get("SUSPICION").unwrap();
    let eldest = pool[0].clone();
    let younger = pool[1].clone();
    let r = run(&refuse, &dict, clone_state(&pooled), "0:0 @ ANA CALM", 3);
    assert_eq!(r.rejected, None);
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["DESTROY", "REFUSE"]);
    assert_eq!(act(&r).unwrap().advanced_min, 0);
    // `r.state === pooled`: same value, untouched — the destroy ran on the
    // discarded draft (identity → snap equality)
    assert_eq!(snap(&r.state), before);
    // a shallow clone_state would have shared the array: the draft's splice
    // would have dropped the elder from the REAL pool too
    assert_eq!(pooled.by_name.get("SUSPICION"), Some(&vec![eldest.clone(), younger.clone()]));
    assert_eq!(pooled.entities.get(&eldest).unwrap().dead_action, None);
    assert_eq!(pooled.entities.get(&younger).unwrap().dead_action, None);
}

// ---------------------------------------------------------------------------
// fold / JSONL equality incl. the multimap (§7)
// ---------------------------------------------------------------------------

/// the completion row's SPAWN mint id (the `as`-bound `#id`).
fn mint_of(r: &ExecResult) -> String {
    let sp = r
        .records
        .iter()
        .find(|x| x.kind == "complete")
        .unwrap()
        .outcomes
        .iter()
        .find(|o| o.op == "SPAWN")
        .unwrap();
    match sp.args.iter().find_map(|a| match a {
        Arg::AsId { id } => Some(id.clone()),
        _ => None,
    }) {
        Some(id) => id,
        None => panic!("no mint in the completion"),
    }
}

#[test]
fn the_log_alone_reproduces_the_pooled_state_byname_arrays_included() {
    let dict = pool_dict();
    let (content, state) = rig(&dict);
    // the expiry enters THROUGH the log (a concrete SET materializes expiresAt),
    // so the fold sees exactly what play saw
    let expire_set = compile_content(
        &ContentRaw {
            verbs: [(
                "ACQUIRE".to_string(),
                VerbContent {
                    slots: vec![
                        SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".into()),
                            shown: None,
                        },
                        SlotDef {
                            role: "object".into(),
                            min: 0,
                            max: Some(1),
                            query: None,
                            shown: None,
                        },
                    ],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        out: vec!["SET(OBJECT, expires, 30m)".to_string()],
                    }],
                },
            )]
            .into_iter()
            .collect(),
            ..ContentRaw::default()
        },
        &dict,
        &[],
    )
    .unwrap();
    let mut cur = state;
    let mut id = 1i64;
    let mut all: Vec<ActionRecord> = Vec::new();
    let mut mints: Vec<String> = Vec::new(); // the pool copies in birth order, ids from the log
    let steps: [(&str, &str); 7] = [
        ("STEW", "0:0 @ ANA STEW"),  // mint 1
        ("STEW", "0:0 @ ANA STEW"),  // mint 2
        ("STEW", "0:0 @ ANA STEW"),  // mint 3
        ("EXPIRE", ""),              // logs SET(#third, expires, 30m) → expiresAt 30
        ("STEW", "0:0 @ ANA WAIT"),  // crosses slot 1: expire:SUSPICION DESTROY the third (by id)
        ("STEW", "0:2 @ ANA CALM"),  // authored destroy of the then-oldest (by name)
        ("STEW", "0:2 @ ANA PROBE"), // slot-bound SET+DESTROY of the last copy
    ];
    for (verb, step) in steps {
        let src = if verb == "EXPIRE" {
            format!("0:0 @ ANA ACQUIRE #{}", mints[2])
        } else if verb == "STEW" && step.contains("PROBE") {
            format!("0:2 @ ANA PROBE #{}", mints[1])
        } else {
            step.to_string()
        };
        let r = if verb == "STEW" {
            run(&content, &dict, cur, &src, id)
        } else {
            run(&expire_set, &dict, cur, &src, id)
        };
        assert_eq!(r.rejected, None);
        if verb == "STEW" && src.contains("STEW") {
            mints.push(mint_of(&r));
        }
        id += r.records.len() as i64;
        all.extend(r.records.iter().cloned());
        cur = r.state;
    }
    assert!(!cur.by_name.contains_key("SUSPICION")); // the pool drained
    let path = tmp_log_path("fold");
    save_log(&path, &all).unwrap();
    let loaded = load_log(&path).unwrap();
    let (base2_content, base2_state) = rig(&dict);
    let replayed = fold(&loaded, &base2_state, &dict, &base2_content).unwrap();
    assert_eq!(snap(&replayed), snap(&cur)); // snap includes the by_name multimap (shared helper)
    let _ = std::fs::remove_file(&path);
}

// ---------------------------------------------------------------------------
// counting the pool: count(<query>) (§4.5)
// ---------------------------------------------------------------------------

fn gate(dict: &Dictionary) -> CompiledContent {
    compile_content(
        &ContentRaw {
            verbs: [(
                "STEW".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 0,
                    recipes: vec![
                        RecipeDef {
                            id: Some("stew.enough".into()),
                            when: Some("count(name:SUSPICION) >= 2".into()),
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            out: vec!["NARRATE(\"the pot boils.\")".to_string()],
                        },
                        RecipeDef {
                            id: Some("stew.default".into()),
                            when: None,
                            shown: None,
                            hint: None,
                            max: None,
                            aspects: Vec::new(),
                            out: vec!["SPAWN SUSPICION".to_string()],
                        },
                    ],
                },
            )]
            .into_iter()
            .collect(),
            prototypes: Some(
                [(
                    "SUSPICION".to_string(),
                    PrototypeDef { kind: "state".into(), stackable: true, ..Default::default() },
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        dict,
        &[],
    )
    .unwrap()
}

#[test]
fn the_pool_count_grows_per_copy_and_flips_the_branch_at_the_threshold() {
    let dict = pool_dict();
    let c = gate(&dict);
    let (_, state) = rig(&dict);
    let mut cur = state;
    let mut picks: Vec<String> = Vec::new();
    let mut id = 1i64;
    for _ in 0..3 {
        let r = run(&c, &dict, cur, "0:0 @ ANA STEW", id);
        assert_eq!(r.rejected, None);
        picks.extend(act(&r).unwrap().recipes.iter().cloned());
        id += r.records.len() as i64;
        cur = r.state;
    }
    assert_eq!(picks, ["stew.default", "stew.default", "stew.enough"]);
    assert_eq!(cur.by_name.get("SUSPICION").unwrap().len(), 2); // the threshold branch narrates, it does not spawn
}

#[test]
fn count_dread_and_total_dread_are_load_errors_naming_the_valid_forms() {
    let dict = core_dictionary();
    let wait_raw = |when: &str| ContentRaw {
        verbs: [(
            "WAIT".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".into(),
                    min: 1,
                    max: Some(1),
                    query: None,
                    shown: None,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: None,
                    when: Some(when.to_string()),
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec!["NARRATE(\"x\")".to_string()],
                }],
            },
        )]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&wait_raw("count(DREAD) >= 3"), &dict, &[]),
        r"count\(\) argument 'DREAD' is neither a role",
    );
    assert_err_msg(
        compile_content(&wait_raw("total(DREAD, aspect:weight) >= 3"), &dict, &[]),
        r"total\(\) first argument 'DREAD' is not a declared role",
    );
    // the query form compiles — the lint only fences the role-looking typo
    let _ = compile_content(&wait_raw("count(name:DREAD) >= 3"), &dict, &[]); // compiles clean
                                                                              // a comparison glued INSIDE the term would bake 'DREAD>=1' in as the NAME
                                                                              // (a silent 0) — the name: term rejects it at load instead
    assert_err_msg(
        compile_content(&wait_raw("count(name:DREAD >= 1) >= 3"), &dict, &[]),
        "bad name term",
    );
    // total(<query>, …) is not offered — named as such, not a cryptic parse error
    assert_err_msg(
        compile_content(&wait_raw("total(name:DREAD, aspect:x) >= 3"), &dict, &[]),
        "total\\(\\) pools a bound role",
    );
}
