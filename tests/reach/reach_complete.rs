//! Completion triggers (`on complete`, §6.7) in the reach analyzer: the
//! `site_proven` Complete arm under the Wave-3 tightened model. The Wave-1
//! arm ("home obtainable ⇒ the window is arrangeable") was over-generous —
//! the fuzz generator v2 exposed three classes of false OBTAINABLE, and the
//! arm now demands a CONSTRUCTIBLE completion: a committable verb whose
//! committed-role slots can lock the home (the window rung), under a
//! selectable completable recipe that declares the aspect (the aspect
//! rung), with the body's mints colliding with nothing the completion draft
//! holds (the draft rung — EXISTENTIAL over the (verb, recipe) witnesses:
//! one clean pair releases, only every pair dirty demotes). The support
//! lines here are byte-frozen (the render contract).

use indexmap::IndexMap;

use chorus::content::{
    compile_content, CompleteTriggerDef, ContentRaw, PrototypeDef, RecipeDef, SlotDef,
    SlotTriggerDef, TriggerDef, VerbContent,
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

/// WORK with one gated, completable recipe: the committed object slot locks
/// the home, and the `when` is a world-form count gate over the funds pool —
/// the shape where the window rung's selectability decides the trigger.
fn gated_work(gate: &str) -> VerbContent {
    VerbContent {
        slots: vec![
            SlotDef {
                role: "actors".to_string(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".to_string()),
                shown: None,
                greedy: false,
                consuming: false,
            },
            SlotDef {
                role: "object".to_string(),
                min: 1,
                max: Some(1),
                query: None,
                shown: None,
                greedy: false,
                consuming: false,
            },
        ],
        duration: 30,
        recipes: vec![RecipeDef {
            id: Some("work.r0".to_string()),
            when: Some(gate.to_string()),
            shown: None,
            hint: None,
            max: None,
            aspects: Vec::new(),
            dur: None,
            out: vec![],
        }],
        ambient: false,
    }
}

/// A stackable funds-bearing prototype.
fn funds_proto() -> PrototypeDef {
    let mut p = proto("resource");
    p.stackable = true;
    p.aspects = Some(IndexMap::from([("funds".to_string(), AspectVal::Num(1))]));
    p
}

/// The gated-window fixture: WORK carries `gate`, `sources` are the extra
/// verbs, `triggers` the trigger list (the completion trigger included by
/// the caller).
fn gated_window_raw(
    gate: &str,
    sources: Vec<(String, VerbContent)>,
    triggers: Vec<TriggerDef>,
    protos: IndexMap<String, PrototypeDef>,
) -> ContentRaw {
    let mut verbs = IndexMap::from([("WORK".to_string(), gated_work(gate))]);
    verbs.extend(sources);
    ContentRaw {
        prototypes: Some(protos),
        verbs,
        triggers: Some(triggers),
        ..ContentRaw::default()
    }
}

/// The shared completion-state probe: X_CARD must stay a CANDIDATE — the
/// honest demotion — never a proven mint over a gate that cannot select.
fn gate_never_proves(raw: &ContentRaw, extra: Vec<Entity>) {
    let content = compile(raw);
    let mut world = vec![ent("ANA", "character", &[]), ent("HOME_CARD", "item", &[])];
    world.extend(extra);
    let state = state_with(world);
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert_eq!(rep.existence, Existence::Candidate);
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
                    greedy: false,
                    consuming: false,
                },
                SlotDef {
                    role: "object".to_string(),
                    min: 1,
                    max: Some(1),
                    query: None,
                    shown: None,
                    greedy: false,
                    consuming: false,
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
                dur: None,
                out: vec![],
            }],
            ambient: false,
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
    // the module-level analyze_card re-parses the --demand grammar against the
    // analyzer's dictionary; these fixtures declare no vocabulary, so the core
    // seed is the load-effective one
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
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
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
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
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
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
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
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
                slot: 6,
                once: None,
                out: vec!["SPAWN HOME_CARD".to_string()],
            }),
            complete_trigger("HOME_CARD", None),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep
        .chain
        .join("\n")
        .contains("trigger complete:HOME_CARD fires when a window completes binding 'HOME_CARD'"));
}

/// The draft rung is EXISTENTIAL over the (verb, recipe) witnesses — a
/// complete trigger fires on every completion (no once), so ONE clean pair
/// lets the mint land: WORK's work.dirty mints the trigger body's own
/// X_CARD (that pair's completion draft collides — and the pair's own site
/// even demotes on its MOVE, so the trigger is the card's only proven
/// server), while EXPLORE's explore.r0 mints nothing (that pair is clean).
/// The trigger proves by walking the CLEAN pair — the support line names
/// EXPLORE/explore.r0, never the dirty witness.
#[test]
fn a_complete_trigger_proves_through_one_clean_witness_pair_and_names_it() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
        ])),
        verbs: IndexMap::from([
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
                            greedy: false,
                            consuming: false,
                        },
                        SlotDef {
                            role: "object".to_string(),
                            min: 1,
                            max: Some(1),
                            query: None,
                            shown: None,
                            greedy: false,
                            consuming: false,
                        },
                    ],
                    duration: 30,
                    recipes: vec![RecipeDef {
                        id: Some("work.dirty".to_string()),
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: vec![],
                        dur: None,
                        // the MOVE keeps the dirty recipe's own site off the
                        // card (a throwing op demotes the list) without
                        // touching its draft — X_CARD is still an
                        // unconditional mint of the completion
                        out: vec!["SPAWN X_CARD".to_string(), "MOVE ANA INTO ROOM".to_string()],
                    }],
                    ambient: false,
                },
            ),
            (
                "EXPLORE".to_string(),
                VerbContent {
                    slots: vec![
                        SlotDef {
                            role: "actors".to_string(),
                            min: 1,
                            max: Some(1),
                            query: Some("kind:character".to_string()),
                            shown: None,
                            greedy: false,
                            consuming: false,
                        },
                        SlotDef {
                            role: "object".to_string(),
                            min: 1,
                            max: Some(1),
                            query: None,
                            shown: None,
                            greedy: false,
                            consuming: false,
                        },
                    ],
                    duration: 30,
                    recipes: vec![RecipeDef {
                        id: Some("explore.r0".to_string()),
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: vec![],
                        dur: None,
                        out: vec![],
                    }],
                    ambient: false,
                },
            ),
        ]),
        triggers: Some(vec![complete_trigger("HOME_CARD", None)]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![
        ent("ANA", "character", &[]),
        ent("ROOM", "place", &[]),
        ent("HOME_CARD", "item", &[]),
    ]);
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(rep.existence, Existence::Proven);
    // the support walk names the CLEAN witness (EXPLORE), not the dirty one
    assert!(
        rep.chain.join("\n").contains(
            "trigger complete:HOME_CARD fires when a window completes binding 'HOME_CARD' — \
             completion is arrangeable (commit EXPLORE locking 'HOME_CARD' in its object slot; \
             recipe explore.r0 completes the window)"
        ),
        "{:?}",
        rep.chain
    );
    assert!(
        !rep.chain.join("\n").contains("work.dirty"),
        "the dirty witness must not render: {:?}",
        rep.chain
    );
}

/// A min-0 committed slot (0..1) is NEVER a forced lock — the player can
/// leave it empty and commit the completion without binding the home at
/// all. WORK's only home-fitting slot is the optional `with` slot (the
/// world's lone item is HOME_CARD, so pre-correction every commit
/// "force-locked" it): work.r0's own X_CARD mint collides with the trigger
/// body's X_CARD mint on every completion, and the recipe falsely demoted
/// (CompleteDraftCollision). With the min≥1 rule the recipe proves — the
/// player commits actors only; no window fires the trigger.
#[test]
fn an_optional_min0_slot_never_force_locks_so_the_completing_recipe_proves() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
        ])),
        verbs: IndexMap::from([(
            "WORK".to_string(),
            VerbContent {
                slots: vec![
                    SlotDef {
                        role: "actors".to_string(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".to_string()),
                        shown: None,
                        greedy: false,
                        consuming: false,
                    },
                    SlotDef {
                        role: "with".to_string(),
                        min: 0,
                        max: Some(1),
                        query: Some("kind:item".to_string()),
                        shown: None,
                        greedy: false,
                        consuming: false,
                    },
                ],
                duration: 30,
                recipes: vec![RecipeDef {
                    id: Some("work.r0".to_string()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: vec![],
                    dur: None,
                    out: vec!["SPAWN X_CARD".to_string()],
                }],
                ambient: false,
            },
        )]),
        triggers: Some(vec![complete_trigger("HOME_CARD", None)]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[]), ent("HOME_CARD", "item", &[])]);
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(rep.existence, Existence::Proven);
    // the proven server is the RECIPE (its completion can simply not fill
    // the optional slot); the trigger's own witness stays draft-dirty
    assert!(
        rep.chain[0].contains("SPAWN mints 'X_CARD' — recipe work.r0 of WORK"),
        "{:?}",
        rep.chain
    );
    assert!(rep
        .chain
        .join("\n")
        .contains("WORK committable: actors ×1 ← initial entity ANA (character)"));
}

/// An UNKNOWN completing-recipe gate (an unbounded-pool count form — the
/// stackable PURSE mints unconditionally, so the world count is a floor,
/// never a ceiling) must NOT read as a deterministic refusal: the recipe
/// may select once the pool is raised, the window-refuse retry is
/// existential (§6.7), so the witness pair survives the selectability rung
/// and the trigger proves.
#[test]
fn an_unknown_count_gate_keeps_the_completing_recipe_selectable() {
    let work = VerbContent {
        slots: vec![
            SlotDef {
                role: "actors".to_string(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".to_string()),
                shown: None,
                greedy: false,
                consuming: false,
            },
            SlotDef {
                role: "object".to_string(),
                min: 1,
                max: Some(1),
                query: None,
                shown: None,
                greedy: false,
                consuming: false,
            },
        ],
        duration: 30,
        recipes: vec![RecipeDef {
            id: Some("work.r0".to_string()),
            when: Some("count(aspect:funds>=1) >= 40".to_string()),
            shown: None,
            hint: None,
            max: None,
            aspects: Vec::new(),
            dur: None,
            out: vec![],
        }],
        ambient: false,
    };
    let mut purse = proto("resource");
    purse.stackable = true;
    purse.aspects = Some(IndexMap::from([("funds".to_string(), AspectVal::Num(1))]));
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
            ("PURSE".to_string(), purse),
        ])),
        verbs: IndexMap::from([
            (
                "EXPLORE".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".to_string(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".to_string()),
                        shown: None,
                        greedy: false,
                        consuming: false,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: Some("explore.mint".to_string()),
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: vec!["SPAWN PURSE".to_string()],
                    }],
                    ambient: false,
                },
            ),
            ("WORK".to_string(), work),
        ]),
        triggers: Some(vec![complete_trigger("HOME_CARD", None)]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[]), ent("HOME_CARD", "item", &[])]);
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "X_CARD", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(rep.existence, Existence::Proven);
}

// ---------------------------------------------------------------------------
// the bounded-source demotions: each funds pool below is BOUNDED, so the
// completing recipe's high gate decides FALSE over the honest pool and the
// window never assembles — the trigger demotes to the candidate tier.
// Flagging any of these pools unbounded (or reading the capped weight as
// the count) would prove X_CARD over a gate that cannot select.
// ---------------------------------------------------------------------------

/// A once slot trigger is a BOUNDED source: it fires exactly once and mints
/// one PURSE, so `>= 40` is false at the honest pool — never a selectably
/// Unknown gate over a pool that cannot exist.
#[test]
fn a_once_trigger_funds_source_keeps_the_gated_completion_a_candidate() {
    let raw = gated_window_raw(
        "count(aspect:funds>=1) >= 40",
        vec![],
        vec![
            TriggerDef::Slot(SlotTriggerDef {
                id: "allowance".to_string(),
                slot: 6,
                once: Some(true),
                out: vec!["SPAWN PURSE".to_string()],
            }),
            complete_trigger("HOME_CARD", None),
        ],
        IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
            ("PURSE".to_string(), funds_proto()),
        ]),
    );
    gate_never_proves(&raw, vec![]);
}

/// A REPLACE consumes a live target per application — one initial POT means
/// a pool of at most one PURSE — so the replacing site never arms the
/// unbounded face and the `>= 40` gate stays decided-false.
#[test]
fn a_replace_source_keeps_the_gated_completion_a_candidate() {
    let raw = gated_window_raw(
        "count(aspect:funds>=1) >= 40",
        vec![(
            "MANSUS".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".to_string()),
                    shown: None,
                    greedy: false,
                    consuming: false,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: Some("swap.purse".to_string()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["REPLACE POT WITH PURSE".to_string()],
                }],
                ambient: false,
            },
        )],
        vec![complete_trigger("HOME_CARD", None)],
        IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
            ("PURSE".to_string(), funds_proto()),
        ]),
    );
    gate_never_proves(&raw, vec![ent("POT", "resource", &[("funds", 3)])]);
}

/// A `max: 1` recipe draws its endless deck exactly once — the draw arms
/// consult the same cap the SPAWN arm does, so the defaultcard is a bounded
/// fact and the `>= 40` gate stays decided-false.
#[test]
fn a_max_capped_draw_keeps_the_gated_completion_a_candidate() {
    let raw = gated_window_raw(
        "count(aspect:funds>=1) >= 40",
        vec![(
            "ACQUIRE".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".to_string()),
                    shown: None,
                    greedy: false,
                    consuming: false,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: Some("draw.once".to_string()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: Some(1),
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["DRAW D1".to_string()],
                }],
                ambient: false,
            },
        )],
        vec![complete_trigger("HOME_CARD", None)],
        IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
            ("COIN".to_string(), funds_proto()),
        ]),
    );
    let mut raw = raw;
    raw.decks = Some(vec![chorus::content::DeckRaw {
        id: "D1".to_string(),
        spec: vec!["COIN".to_string()],
        defaultcard: Some("COIN".to_string()),
        reset: true,
    }]);
    gate_never_proves(&raw, vec![]);
}

/// A bare uncapped recipe whose body DESTROYS its own actors filler
/// consumes its own selection: the single initial character fills the
/// actors slot exactly once, the first application kills it, and the second
/// commit refuses — the SPAWN is a bounded source (one mint), so the
/// `>= 32` gate decides false over the honest pool and the trigger demotes
/// to the candidate tier. Arming the mint unbounded would flatten the fact
/// to the 32-weight floor and flip the gate to decided-true — the false
/// proven this test pins out.
#[test]
fn a_recipe_that_destroys_its_own_filler_keeps_the_gated_completion_a_candidate() {
    let raw = gated_window_raw(
        "count(aspect:funds>=1) >= 32",
        vec![(
            "DREAM".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".to_string()),
                    shown: None,
                    greedy: false,
                    consuming: false,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: Some("dream.coin".to_string()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["DESTROY actors".to_string(), "SPAWN COIN".to_string()],
                }],
                ambient: false,
            },
        )],
        vec![complete_trigger("HOME_CARD", None)],
        IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
            ("COIN".to_string(), funds_proto()),
        ]),
    );
    gate_never_proves(&raw, vec![]);
}

/// A threshold below the flatten weight cap but above the honest pool: the
/// once-trigger source mints ONE purse and the fact says so, so a `>= 5`
/// gate decides false and the window never assembles.
#[test]
fn a_threshold_inside_the_capped_weight_region_demotes_to_candidate() {
    let raw = gated_window_raw(
        "count(aspect:funds>=1) >= 5",
        vec![],
        vec![
            TriggerDef::Slot(SlotTriggerDef {
                id: "allowance".to_string(),
                slot: 6,
                once: Some(true),
                out: vec!["SPAWN PURSE".to_string()],
            }),
            complete_trigger("HOME_CARD", None),
        ],
        IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
            ("PURSE".to_string(), funds_proto()),
        ]),
    );
    gate_never_proves(&raw, vec![]);
}

/// A closed comparison the pool's floor already violates is FALSE, never
/// may-select Unknown: the unbounded PURSE pool can only grow above its
/// floor, so `count(aspect:funds>=1) < 1` holds at no reachable pool — the
/// completing recipe never selects and the trigger demotes. (The invariant:
/// Unknown may only widen toward candidate, never produce proven on its
/// own.)
#[test]
fn a_floor_violated_closed_gate_never_selects_the_completing_recipe() {
    let raw = gated_window_raw(
        "count(aspect:funds>=1) < 1",
        vec![(
            "EXPLORE".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".to_string(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".to_string()),
                    shown: None,
                    greedy: false,
                    consuming: false,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: Some("explore.mint".to_string()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["SPAWN PURSE".to_string()],
                }],
                ambient: false,
            },
        )],
        vec![complete_trigger("HOME_CARD", None)],
        IndexMap::from([
            ("HOME_CARD".to_string(), proto("item")),
            ("X_CARD".to_string(), proto("item")),
            ("PURSE".to_string(), funds_proto()),
        ]),
    );
    gate_never_proves(&raw, vec![]);
}

// --------------------------------- Wave-4 (v0.9.8): the completing recipe kills the locked home ----

/// The completing recipe's own outcomes kill the locked home BEFORE the
/// trigger phase, and the phase iterates the commit-time locked ids skipping
/// dead cards — a pair whose recipe destroys/replaces the home (by name, or
/// by a role the home can be locked in) can never witness. Judged per pair
/// beside the draft-collision dirt; every pair dying demotes the site at
/// `CompleteHomeKilled`.
#[test]
fn a_completing_recipe_that_kills_the_locked_home_voids_the_witness_pair() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("LEDGER".to_string(), {
                let mut p = proto("idea");
                p.aspects = Some(IndexMap::from([("a".to_string(), AspectVal::Num(1))]));
                p
            }),
            ("P1".to_string(), proto("idea")),
        ])),
        verbs: IndexMap::from([(
            "TALK".to_string(),
            VerbContent {
                slots: vec![
                    SlotDef {
                        role: "actors".to_string(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".to_string()),
                        shown: None,
                        greedy: false,
                        consuming: false,
                    },
                    SlotDef {
                        role: "with".to_string(),
                        min: 1,
                        max: Some(1),
                        query: Some("aspect:a>=1".to_string()),
                        shown: None,
                        greedy: false,
                        consuming: false,
                    },
                ],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: Some("talk.r0".to_string()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["DESTROY LEDGER".to_string()],
                }],
                ambient: false,
            },
        )]),
        triggers: Some(vec![TriggerDef::Complete(CompleteTriggerDef {
            id: "complete:LEDGER".to_string(),
            home: "LEDGER".to_string(),
            aspect: None,
            out: vec!["SPAWN P1".to_string()],
        })]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[]), ent("LEDGER", "idea", &[("a", 1)])]);
    let dict = core_dictionary();
    let rep = analyze_card(&content, &state, &dict, "P1", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert_eq!(rep.existence, Existence::Candidate);
    // the --why face: the trigger's SPAWN site stays visible with the
    // home-kill blocker
    let analyzer = chorus::reach::Analyzer::new(&content, &state, &dict);
    let rows = analyzer.card_blockers("P1");
    assert_eq!(rows.len(), 1, "exactly the trigger's SPAWN P1 site serves P1");
    assert_eq!(rows[0].owner, "trigger complete:LEDGER");
    assert_eq!(
        rows[0].blocker,
        chorus::reach::fixpoint::CardBlocker::CompleteHomeKilled { home: "LEDGER".to_string() }
    );
}
