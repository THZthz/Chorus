//! Static card-reachability (32 tests).
//! Both semantics — (a) existence and (b) usability — over hand-built
//! ContentRaw fixtures. The conservative direction is the contract:
//! 'unobtainable' only on structural absence; anything guarded demotes to
//! 'unproven'.
//!
//! Nothing in this file registers verbs, so a fresh core dictionary per
//! test is the exact isolation.

use indexmap::{IndexMap, IndexSet};

use chorus::content::content::{
    compile_content, ContentRaw, DeckRaw, PrototypeDef, PrototypeDerive, RecipeDef, SlotDef,
    SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::dict::core_dictionary;
use chorus::core::types::{add_entity, new_state, AspectVal, Entity, State};
use chorus::reach::{
    analyze_card, render_report, CardOptions, CardVerdict, Existence,
};

mod common;

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

/// Compile a fixture (one fresh dictionary per test).
fn compile(raw: &ContentRaw) -> chorus::content::content::CompiledContent {
    let dict = core_dictionary();
    compile_content(raw, &dict, &[]).expect("fixture content compiles")
}

fn ana() -> Entity {
    ent("ANA", "character", &[])
}
fn book() -> Entity {
    ent("THE_BOOK", "text", &[("secret", 2)])
}

fn slot(role: &str, min: i64, max: i64, query: Option<&str>) -> SlotDef {
    SlotDef {
        role: role.to_string(),
        min,
        max: Some(max),
        query: query.map(str::to_string),
        shown: None,
    }
}

fn verb(name_slots: Vec<SlotDef>, duration: i64, recipes: Vec<RecipeDef>) -> VerbContent {
    VerbContent {
        slots: name_slots,
        duration,
        recipes,
    }
}

fn recipe(out: &[&str]) -> RecipeDef {
    RecipeDef {
        id: None,
        when: None,
        shown: None,
        hint: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn recipe_when(when: &str, out: &[&str]) -> RecipeDef {
    RecipeDef {
        id: None,
        when: Some(when.to_string()),
        shown: None,
        hint: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn proto(kind: &str) -> PrototypeDef {
    PrototypeDef {
        kind: kind.to_string(),
        ..PrototypeDef::default()
    }
}

fn deck(id: &str, spec: &[&str]) -> DeckRaw {
    DeckRaw {
        id: id.to_string(),
        spec: spec.iter().map(|s| (*s).to_string()).collect(),
        defaultcard: None,
        reset: false,
    }
}

fn slot_trigger(id: &str, slot: i64, out: &[&str]) -> TriggerDef {
    TriggerDef::Slot(SlotTriggerDef {
        id: id.to_string(),
        slot,
        once: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    })
}

fn empty_raw() -> ContentRaw {
    ContentRaw::default()
}

// ---------- (a) existence --------------------------------------------------

#[test]
fn an_initial_entity_is_proven_obtainable_a_name_with_no_source_is_structurally_unobtainable() {
    let content = compile(&empty_raw());
    let state = state_with(vec![ana(), book()]);
    let rep = analyze_card(&content, &state, "THE_BOOK", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(rep.existence, Existence::Proven);
    assert!(rep.chain[0].contains("initial entities"));

    let ghost = analyze_card(&content, &state, "GHOST_CARD", CardOptions::default()).unwrap();
    assert_eq!(ghost.verdict, CardVerdict::Unobtainable);
}

#[test]
fn a_deck_hit_behind_a_slot_trigger_draw_is_proven_the_order_is_seed_fixed_at_load() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("A_CARD".to_string(), proto("item")),
            ("B_CARD".to_string(), proto("item")),
        ])),
        decks: Some(vec![deck("BOX", &["A_CARD", "B_CARD"])]),
        triggers: Some(vec![slot_trigger("deal", 2, &["DRAW BOX"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ana()]);
    let b = analyze_card(&content, &state, "B_CARD", CardOptions::default()).unwrap();
    assert_eq!(b.verdict, CardVerdict::Obtainable);
    assert!(b.chain[0].contains("deck BOX serves 'B_CARD'"));
    assert!(b.chain.join("\n").contains("trigger deal fires on slot 2"));

    let c = analyze_card(&content, &state, "C_CARD", CardOptions::default()).unwrap();
    assert_eq!(c.verdict, CardVerdict::Unobtainable);
}

#[test]
fn deck_capacity_one_once_trigger_draw_serves_only_order0_deeper_positions_are_never_served() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("A_CARD".to_string(), proto("item")),
            ("B_CARD".to_string(), proto("item")),
        ])),
        decks: Some(vec![deck("BOX", &["A_CARD", "B_CARD"])]),
        triggers: Some(vec![TriggerDef::Slot(SlotTriggerDef {
            id: "deal".to_string(),
            slot: 2,
            once: Some(true),
            out: vec!["DRAW BOX".to_string()],
        })]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let order = content.decks.get("BOX").unwrap().order.clone();
    let a = analyze_card(&content, &state_with(vec![ana()]), &order[0].name, CardOptions::default())
        .unwrap();
    assert_eq!(a.verdict, CardVerdict::Obtainable);
    // nothing else can draw, so the deeper position is structurally unserved
    let b = analyze_card(&content, &state_with(vec![ana()]), &order[1].name, CardOptions::default())
        .unwrap();
    assert_eq!(b.verdict, CardVerdict::Unobtainable);
}

#[test]
fn a_duplicate_non_stackable_mint_in_one_list_refuses_the_draft_every_site_in_it_demotes() {
    let mk = |dup: bool| {
        let recipes = vec![recipe(&[
            "DRAW D1",
            if dup { "SPAWN P1" } else { "SPAWN P2" },
        ])];
        ContentRaw {
            prototypes: Some(IndexMap::from([
                ("P1".to_string(), proto("item")),
                ("P2".to_string(), proto("idea")),
            ])),
            decks: Some(vec![deck("D1", &["P1"])]),
            verbs: IndexMap::from([(
                "MANSUS".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("object", 1, 1, Some("kind:text")),
                    ],
                    30,
                    recipes,
                ),
            )]),
            ..ContentRaw::default()
        }
    };
    // the DRAW deterministically mints P1 (order[0] of a single-card deck) →
    // the trailing SPAWN P1 collides → completion refusal → P1 never survives
    let risky = analyze_card(
        &compile(&mk(true)),
        &state_with(vec![ana(), book()]),
        "P1",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(risky.verdict, CardVerdict::Unproven);
    assert!(risky.notes.join(" ").contains("collide"));
    // control: the SPAWN mints a DIFFERENT name — the draw site stays proven
    let control = analyze_card(
        &compile(&mk(false)),
        &state_with(vec![ana(), book()]),
        "P1",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(control.verdict, CardVerdict::Obtainable);
}

#[test]
fn a_deck_nobody_draws_is_not_a_source() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("A_CARD".to_string(), proto("item"))])),
        decks: Some(vec![deck("BOX", &["A_CARD"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "A_CARD", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unobtainable);
}

#[test]
fn spawn_via_a_default_recipe_on_a_committable_verb_the_chain_names_the_slot_supports() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&["SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep.chain[0].contains("SPAWN mints"));
    assert!(rep
        .chain
        .join("\n")
        .contains("EXPLORE committable: actors ×1 ← initial entity ANA"));
}

#[test]
fn a_role_dependent_when_demotes_to_unproven_never_a_false_unobtainable() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe_when("actors.aspect.occult >= 1", &["SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert_eq!(rep.existence, Existence::Candidate);
    assert!(rep.notes.join(" ").contains("guarded"));
}

#[test]
fn a_world_static_when_that_already_holds_stays_proven() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe_when("exists(THE_BOOK)", &["SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(
        &content,
        &state_with(vec![ana(), book()]),
        "PILLAR",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
}

#[test]
fn an_uncommittable_verb_is_a_dead_producer_structural_absence() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![
                    slot("actors", 1, 1, Some("kind:character")),
                    slot("object", 1, 1, Some("kind:rite")),
                ],
                30,
                vec![recipe(&["SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unobtainable);
}

#[test]
fn the_fixpoint_chains_mints_through_slots_deck_card_to_slot_to_second_stage_mint() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("KEY_CARD".to_string(), proto("item")),
            ("PILLAR".to_string(), proto("item")),
        ])),
        decks: Some(vec![deck("BOX", &["KEY_CARD"])]),
        triggers: Some(vec![slot_trigger("deal", 2, &["DRAW BOX"])]),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![
                    slot("actors", 1, 1, Some("kind:character")),
                    slot("object", 1, 1, Some("kind:item")),
                ],
                30,
                vec![recipe(&["SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep.chain.join("\n").contains("KEY_CARD"));
}

#[test]
fn an_authored_refuse_upstream_kills_the_site_the_apply_loop_stops_there() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&["REFUSE(\"not yet\")", "SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unobtainable);
}

#[test]
fn a_derived_mint_shape_is_never_structurally_absent() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([(
            "RUMOUR".to_string(),
            PrototypeDef {
                kind: "idea".to_string(),
                derive: Some(PrototypeDerive {
                    name_from: Some("object".to_string()),
                    aspects: None,
                }),
                ..PrototypeDef::default()
            },
        )])),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(
        &content,
        &state_with(vec![ana()]),
        "RUMOUR_HOUSE",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert!(rep.notes.join(" ").contains("derived-mint"));
}

// ---------- mixed sources & segment discipline -----------------------------

#[test]
fn mixture_a_deck_fed_card_satisfies_an_if_exists_guard_gating_a_spawn() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("KEY_CARD".to_string(), proto("item")),
            ("PILLAR".to_string(), proto("item")),
        ])),
        decks: Some(vec![deck("BOX", &["KEY_CARD"])]),
        triggers: Some(vec![slot_trigger("deal", 2, &["DRAW BOX"])]),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&["IF exists(KEY_CARD) -> (SPAWN PILLAR)"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep.chain.join("\n").contains("SPAWN mints"));
}

#[test]
fn mixture_a_slot_needing_two_items_sums_across_sources_initial_plus_deck() {
    // without the deck there is only ROPE — one of two — so the verb never
    // commits; the site is not structurally dead either (a KEY_CARD proto
    // exists in the universe), and the honest verdict is unproven
    let base = |extra: bool| {
        let mut raw = ContentRaw {
            prototypes: Some(IndexMap::from([
                ("KEY_CARD".to_string(), proto("item")),
                ("PILLAR".to_string(), proto("item")),
            ])),
            verbs: IndexMap::from([(
                "EXPLORE".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("object", 2, 2, Some("kind:item")),
                    ],
                    30,
                    vec![recipe(&["SPAWN PILLAR"])],
                ),
            )]),
            ..ContentRaw::default()
        };
        if extra {
            raw.decks = Some(vec![deck("BOX", &["KEY_CARD"])]);
            raw.triggers = Some(vec![slot_trigger("deal", 2, &["DRAW BOX"])]);
        }
        raw
    };
    let no_deck = analyze_card(
        &compile(&base(false)),
        &state_with(vec![ana(), ent("ROPE", "item", &[])]),
        "PILLAR",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(no_deck.verdict, CardVerdict::Unproven);

    // add the deck: ROPE (initial) + KEY_CARD (drawn) crosses the min —
    // the chain names both sources from DIFFERENT entry paths
    let full = analyze_card(
        &compile(&base(true)),
        &state_with(vec![ana(), ent("ROPE", "item", &[])]),
        "PILLAR",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(full.verdict, CardVerdict::Obtainable);
    assert!(full.chain.join("\n").contains("ROPE"));
    assert!(full.chain.join("\n").contains("KEY_CARD"));
}

#[test]
fn segment_scoping_a_refuse_inside_an_if_arm_does_not_kill_the_outer_list() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&[
                    "IF exists(THE_BOOK) -> (REFUSE(\"blocked\"))",
                    "SPAWN PILLAR",
                ])],
            ),
        )]),
        ..ContentRaw::default()
    };
    // THE_BOOK is absent, so the arm never runs and the outer SPAWN applies
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
}

#[test]
fn a_top_level_refuse_kills_the_whole_list_earlier_spawns_included() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&["SPAWN PILLAR", "REFUSE(\"stop\")"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unobtainable);
}

#[test]
fn a_throwing_op_anywhere_in_the_list_demotes_every_site_in_it() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&["SPAWN PILLAR", "MOVE ANA INTO NOWHERE"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert!(rep.notes.join(" ").contains("throwing op"));
}

#[test]
fn replace_is_a_producer_of_its_with_target() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("ASH".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "USE_ON".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&["REPLACE THE_BOOK WITH ASH"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(
        &content,
        &state_with(vec![ana(), book()]),
        "ASH",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep.chain[0].contains("REPLACE mints"));
}

// ---------- (c) demand semantics: attribute thresholds ---------------------

/// The book-world fixture: ANA plus THE_BOOK (text, secret:2).
fn book_world() -> (chorus::content::content::CompiledContent, State) {
    let content = compile(&empty_raw());
    let state = state_with(vec![ana(), book()]);
    (content, state)
}

#[test]
fn a_threshold_satisfiable_by_a_static_descriptor_is_proven_across_comparison_ops() {
    for demand in [
        "aspect:secret>=2",
        "aspect:secret>1",
        "aspect:secret=2",
        "aspect:secret<=2",
        "aspect:secret!=3",
    ] {
        let (content, state) = book_world();
        let rep = analyze_card(
            &content,
            &state,
            "",
            CardOptions {
                demand: Some(demand.to_string()),
                ..CardOptions::default()
            },
        )
        .unwrap();
        assert_eq!(rep.verdict, CardVerdict::Obtainable, "{demand}");
        assert!(rep.chain[0].contains("THE_BOOK"), "{demand}");
    }
}

#[test]
fn a_bounded_threshold_nothing_can_write_is_soundly_unobtainable() {
    let (content, state) = book_world();
    let rep = analyze_card(
        &content,
        &state,
        "",
        CardOptions {
            demand: Some("aspect:secret<=1".to_string()),
            ..CardOptions::default()
        },
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unobtainable);
    assert_eq!(rep.existence, Existence::None);
}

#[test]
fn a_set_pump_keeps_an_unmatched_threshold_from_being_called_impossible() {
    let raw = ContentRaw {
        verbs: IndexMap::from([(
            "USE_ON".to_string(),
            verb(
                vec![
                    slot("actors", 1, 1, Some("kind:character")),
                    slot("object", 1, 1, Some("kind:item")),
                ],
                30,
                vec![recipe(&["SET(OBJECT, aspect:charged, +3)"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ana(), ent("LAMP", "item", &[("charged", 0)])]);
    let rep = analyze_card(
        &content,
        &state,
        "",
        CardOptions {
            demand: Some("aspect:charged>=2".to_string()),
            ..CardOptions::default()
        },
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert!(rep.notes.join(" ").contains("pumped"));
}

#[test]
fn a_pumped_threshold_un_deads_a_verb_slot_no_static_card_could_fill() {
    let mk = |with_pump: bool| {
        let mut verbs = IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![
                    slot("actors", 1, 1, Some("kind:character")),
                    slot("object", 1, 1, Some("kind:item aspect:charged>=3")),
                ],
                30,
                vec![recipe(&["SPAWN PILLAR"])],
            ),
        )]);
        if with_pump {
            verbs.insert(
                "USE_ON".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("object", 1, 1, Some("kind:item")),
                    ],
                    30,
                    vec![recipe(&["SET(OBJECT, aspect:charged, +3)"])],
                ),
            );
        }
        ContentRaw {
            prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
            verbs,
            ..ContentRaw::default()
        }
    };
    let state = state_with(vec![ana(), ent("LAMP", "item", &[("charged", 0)])]);
    // no SET writes aspect:charged anywhere → the threshold is bounded away → dead
    let without = analyze_card(
        &compile(&mk(false)),
        &state,
        "PILLAR",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(without.verdict, CardVerdict::Unobtainable);
    // the pump keeps the slot undecided → candidate, never a false unobtainable
    let with = analyze_card(&compile(&mk(true)), &state, "PILLAR", CardOptions::default()).unwrap();
    assert_eq!(with.verdict, CardVerdict::Unproven);
}

#[test]
fn a_derive_fed_threshold_cannot_be_bounded() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([(
            "RUMOUR".to_string(),
            PrototypeDef {
                kind: "idea".to_string(),
                derive: Some(PrototypeDerive {
                    name_from: None,
                    aspects: Some(IndexMap::from([(
                        "glimpse".to_string(),
                        chorus::content::content::DeriveAspect {
                            from_object_aspect: "secret".to_string(),
                            minus: 0,
                        },
                    )])),
                }),
                ..PrototypeDef::default()
            },
        )])),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(
        &content,
        &state_with(vec![ana()]),
        "",
        CardOptions {
            demand: Some("aspect:glimpse>=5".to_string()),
            ..CardOptions::default()
        },
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert!(rep.notes.join(" ").contains("derive"));
}

// ---------- (b) usability --------------------------------------------------

/// The readable-book fixture: READ binds a kind:text object.
fn readable_book() -> chorus::content::content::CompiledContent {
    let raw = ContentRaw {
        verbs: IndexMap::from([(
            "READ".to_string(),
            verb(
                vec![
                    slot("actors", 1, 1, Some("kind:character")),
                    slot("object", 1, 1, Some("kind:text")),
                ],
                30,
                vec![recipe(&["NARRATE \"read\""])],
            ),
        )]),
        ..ContentRaw::default()
    };
    compile(&raw)
}

#[test]
fn an_initial_card_bindable_through_a_committable_verb_slot_is_obtainable_under_usable() {
    let content = readable_book();
    let rep = analyze_card(
        &content,
        &state_with(vec![ana(), book()]),
        "THE_BOOK",
        CardOptions {
            usable: true,
            ..CardOptions::default()
        },
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep.usable_via.contains(&"READ.object (query matches)".to_string()));
}

#[test]
fn an_existing_card_no_slot_can_bind_is_unusable_under_usable_obtainable_without_the_flag() {
    let content = readable_book();
    let state = state_with(vec![ana(), ent("THE_WALL", "place", &[])]);
    let plain = analyze_card(&content, &state, "THE_WALL", CardOptions::default()).unwrap();
    assert_eq!(plain.verdict, CardVerdict::Obtainable);
    let rep = analyze_card(
        &content,
        &state,
        "THE_WALL",
        CardOptions {
            usable: true,
            ..CardOptions::default()
        },
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unusable);
    assert_eq!(rep.existence, Existence::Proven);
}

// ---------- path export (all proven acquisition paths, best-effort) --------

#[test]
fn every_proven_producer_is_exported_a_recipe_and_a_trigger_both_minting_yield_two_roots() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([(
            "PILLAR".to_string(),
            PrototypeDef {
                kind: "item".to_string(),
                stackable: true,
                ..PrototypeDef::default()
            },
        )])),
        triggers: Some(vec![slot_trigger("quake", 3, &["SPAWN PILLAR"])]),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                0,
                vec![recipe(&["SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(rep.paths.len(), 2);
    let explore = content.verbs.get("EXPLORE").unwrap();
    let recipe_id = &explore.recipes[0].id;
    let vias: IndexSet<&str> = rep.paths.iter().map(|p| p.via.as_str()).collect();
    assert!(vias.contains(&format!("recipe {recipe_id} of EXPLORE").as_str()));
    assert!(vias.contains(&"trigger quake"));
    for p in &rep.paths {
        assert_eq!(p.mints, "PILLAR");
    }
}

#[test]
fn an_initial_entity_exports_a_single_leaf_path() {
    let content = compile(&empty_raw());
    let rep = analyze_card(
        &content,
        &state_with(vec![ana(), book()]),
        "THE_BOOK",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.paths.len(), 1);
    let p = &rep.paths[0];
    assert_eq!(p.op, chorus::reach::PathOp::Initial);
    assert_eq!(p.mints, "THE_BOOK");
    assert_eq!(p.requires.len(), 0);
    assert!(p.supports[0].contains("initial entities"));
}

#[test]
fn requires_expands_recursively_a_slot_filler_carries_its_own_provenance() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![
                    slot("actors", 1, 1, Some("kind:character")),
                    slot("object", 1, 1, Some("kind:text")),
                ],
                0,
                vec![recipe(&["SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(
        &content,
        &state_with(vec![ana(), book()]),
        "PILLAR",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(rep.paths.len(), 1);
    let mut names: Vec<&str> = rep.paths[0].requires.iter().map(|q| q.name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["ANA", "THE_BOOK"]);
    for q in &rep.paths[0].requires {
        assert_eq!(q.paths.len(), 1);
        assert_eq!(q.paths[0].op, chorus::reach::PathOp::Initial);
    }
}

#[test]
fn requires_chains_through_produced_fillers_deck_card_to_slot_to_second_stage_mint() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("SEED".to_string(), proto("item")),
            ("PLANT".to_string(), proto("text")),
        ])),
        decks: Some(vec![deck("D1", &["SEED"])]),
        triggers: Some(vec![slot_trigger("sow", 1, &["DRAW D1"])]),
        verbs: IndexMap::from([(
            "STUDY".to_string(),
            verb(
                vec![
                    slot("actors", 1, 1, Some("kind:character")),
                    slot("object", 1, 1, Some("name:SEED")),
                ],
                0,
                vec![recipe(&["SPAWN PLANT"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PLANT", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert_eq!(rep.paths.len(), 1);
    let seed_req = rep.paths[0]
        .requires
        .iter()
        .find(|q| q.name == "SEED")
        .expect("SEED is a required filler");
    assert_eq!(seed_req.paths.len(), 1);
    assert_eq!(seed_req.paths[0].op, chorus::reach::PathOp::Draw);
    let deck = seed_req.paths[0].deck.as_ref().unwrap();
    assert_eq!(deck.id, "D1");
    assert_eq!(seed_req.paths[0].via, "trigger sow");
}

#[test]
fn demand_mode_exports_a_root_per_matching_source_not_just_the_first() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            (
                "RED".to_string(),
                PrototypeDef {
                    kind: "item".to_string(),
                    aspects: Some(IndexMap::from([(
                        "tint".to_string(),
                        AspectVal::Num(1),
                    )])),
                    ..PrototypeDef::default()
                },
            ),
            (
                "BLUE".to_string(),
                PrototypeDef {
                    kind: "idea".to_string(),
                    aspects: Some(IndexMap::from([(
                        "tint".to_string(),
                        AspectVal::Num(1),
                    )])),
                    ..PrototypeDef::default()
                },
            ),
        ])),
        triggers: Some(vec![slot_trigger("fall", 2, &["SPAWN BLUE"])]),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                0,
                vec![recipe(&["SPAWN RED"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(
        &content,
        &state_with(vec![ana()]),
        "",
        CardOptions {
            demand: Some("aspect:tint>=1".to_string()),
            ..CardOptions::default()
        },
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    let mut mints: Vec<&str> = rep.paths.iter().map(|p| p.mints.as_str()).collect();
    mints.sort();
    assert_eq!(mints, ["BLUE", "RED"]);
}

/// JS truthiness on the analyzer side: an EMPTY demand
/// string is NO demand —
/// `Some("")` must render byte-identical to `demand: None` (the card NAME in
/// the header, plain name-mode analysis); `parse_query("")` returning an
/// empty query without erroring would silently enter demand mode instead.
#[test]
fn an_empty_string_demand_is_falsy_renders_exactly_like_no_demand() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            (
                "RED".to_string(),
                PrototypeDef {
                    kind: "item".to_string(),
                    aspects: Some(IndexMap::from([(
                        "tint".to_string(),
                        AspectVal::Num(1),
                    )])),
                    ..PrototypeDef::default()
                },
            ),
            (
                "BLUE".to_string(),
                PrototypeDef {
                    kind: "idea".to_string(),
                    aspects: Some(IndexMap::from([(
                        "tint".to_string(),
                        AspectVal::Num(1),
                    )])),
                    ..PrototypeDef::default()
                },
            ),
        ])),
        triggers: Some(vec![slot_trigger("fall", 2, &["SPAWN BLUE"])]),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                0,
                vec![recipe(&["SPAWN RED"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ana()]);
    let plain = render_card("s.txt", &content, &state, "ANA", &CardOptions::default());
    let empty = render_card(
        "s.txt",
        &content,
        &state,
        "ANA",
        &CardOptions {
            demand: Some(String::new()),
            ..CardOptions::default()
        },
    );
    assert_eq!(empty, plain, "an empty demand string must be no demand at all");
    // the header carries the card NAME, never `matching ''`
    assert!(plain.starts_with("s.txt · card ANA\n"), "{plain}");
}

#[test]
fn static_if_guards_on_a_proven_path_render_into_the_guards_list() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("PILLAR".to_string(), proto("item")),
            ("KEY".to_string(), proto("item")),
        ])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                0,
                vec![recipe(&["IF exists(KEY) -> (SPAWN PILLAR)"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(
        &content,
        &state_with(vec![ana(), ent("KEY", "item", &[])]),
        "PILLAR",
        CardOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep.paths[0].guards.contains(&"exists(KEY)".to_string()));
}

#[test]
fn guarded_candidate_sources_are_listed_flat_in_candidates_never_as_paths() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([("PILLAR".to_string(), proto("item"))])),
        verbs: IndexMap::from([(
            "EXPLORE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                0,
                vec![recipe_when("actors.aspect.occult >= 1", &["SPAWN PILLAR"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let rep = analyze_card(&content, &state_with(vec![ana()]), "PILLAR", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert_eq!(rep.paths.len(), 0);
    assert!(!rep.candidates.is_empty());
    assert!(rep.candidates[0].via.contains("EXPLORE"));
    assert!(rep.candidates[0].reason.contains("when"));
}

// ---------------------------------------------------------------------------
// the same (content, State) analyzed twice must produce
// byte-identical reports (insertion order is output order — avail push
// order → first-fit proofs; proven order → chain first-hit and paths order).
// Exercised over gen_scenario's adversarial corpus: every probe of every seed,
// rendered through the frozen CLI shapes and compared byte for byte, plus one
// shared-Analyzer pass against two fresh-Analyzer passes.
// ---------------------------------------------------------------------------

fn render_card(
    file: &str,
    content: &chorus::content::content::CompiledContent,
    state: &State,
    target: &str,
    opts: &CardOptions,
) -> String {
    match analyze_card(content, state, target, opts.clone()) {
        Ok(rep) => {
            let mut out = String::new();
            render_report(
                &mut out,
                file,
                target,
                opts.demand.as_deref(),
                opts.usable,
                &rep,
            );
            out
        }
        Err(e) => format!("ERR {e}"),
    }
}

/// 同 (content, State) 两次分析输出逐字节一致 — the determinism pin.
#[test]
fn the_same_content_and_state_analyzed_twice_is_byte_identical_reach03() {
    let dict = core_dictionary();
    for seed in 1u32..=60 {
        let sc = common::reach_fuzz_lib::gen_scenario(seed);
        let content =
            compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("scenario compiles");
        let mut first = String::new();
        let mut second = String::new();
        for probe in &sc.probes {
            let opts = match probe.mode {
                common::reach_fuzz_lib::ProbeMode::Name => CardOptions {
                    usable: seed % 2 == 0,
                    demand: None,
                },
                common::reach_fuzz_lib::ProbeMode::Demand => CardOptions {
                    usable: false,
                    demand: Some(probe.q.clone()),
                },
            };
            first.push_str(&render_card("scenario", &content, &sc.initial, &probe.q, &opts));
            second.push_str(&render_card("scenario", &content, &sc.initial, &probe.q, &opts));
        }
        assert_eq!(first, second, "seed {seed}: two analyses diverged");
        assert!(!first.is_empty(), "seed {seed}: probes must render");
    }
}

/// A shared Analyzer (the world-cache shape) must agree
/// byte-for-byte with fresh-per-call analyzers.
#[test]
fn a_shared_analyzer_matches_fresh_per_call_analyzers_reach07() {
    let dict = core_dictionary();
    for seed in 1u32..=30 {
        let sc = common::reach_fuzz_lib::gen_scenario(seed);
        let content =
            compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("scenario compiles");
        let shared = chorus::reach::Analyzer::new(&content, &sc.initial);
        for probe in &sc.probes {
            let opts = match probe.mode {
                common::reach_fuzz_lib::ProbeMode::Name => CardOptions {
                    usable: seed % 3 == 0,
                    demand: None,
                },
                common::reach_fuzz_lib::ProbeMode::Demand => CardOptions {
                    usable: false,
                    demand: Some(probe.q.clone()),
                },
            };
            let fresh =
                render_card("scenario", &content, &sc.initial, &probe.q, &opts);
            let mut shared_out = String::new();
            match shared.analyze_card(&probe.q, opts.clone()) {
                Ok(rep) => {
                    render_report(
                        &mut shared_out,
                        "scenario",
                        &probe.q,
                        opts.demand.as_deref(),
                        opts.usable,
                        &rep,
                    );
                }
                Err(e) => shared_out = format!("ERR {e}"),
            }
            assert_eq!(
                fresh, shared_out,
                "seed {seed} probe {}: shared vs fresh",
                probe.q
            );
        }
    }
}
