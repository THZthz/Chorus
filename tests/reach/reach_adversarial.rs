//! Hand-crafted adversarial scenarios for the static card-reachability
//! analyzer (S1–S14), **analyzer side
//! only** (the engine-side halves of these tests — settle traces,
//! the exhaustive explorer — are confirmed by the engine test-suite; this
//! file carries the reach-verdict / existence / chain / notes / usable_via
//! assertions).
//!
//! Verdict contract under test (the reach header contract): 'unobtainable' must never
//! fire wrongly; 'obtainable' must be backed by a real chain; guarded sites
//! demote to 'unproven'.

use indexmap::{IndexMap, IndexSet};

use chorus::content::content::{
    compile_content, ContentRaw, DeckRaw, ExpireTriggerDef, PrototypeDef, PrototypeDerive,
    RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::dice::roll_die;
use chorus::core::dict::core_dictionary;
use chorus::core::types::{add_entity, new_state, AspectVal, Entity, State};
use chorus::reach::{analyze_card, CardOptions, CardVerdict, Existence};

/// Rig verbs are
/// code-registered (§10). One fresh dictionary per test.
fn compile(raw: &ContentRaw) -> chorus::content::content::CompiledContent {
    let mut dict = core_dictionary();
    for name in raw.verbs.keys() {
        if !dict.is_verb(name) {
            dict.register_verb(name).expect("verb registration");
        }
    }
    compile_content(raw, &dict, &[]).expect("scenario compiles")
}

/// One scenario entity (id derived from the lowercased name).
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

/// A state holding exactly these scenario entities.
fn state_with(es: Vec<Entity>) -> State {
    let mut s = new_state(0);
    for e in es {
        add_entity(&mut s, e).expect("scenario entities never collide (unique names)");
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
    }
}

fn verb(slots: Vec<SlotDef>, duration: i64, recipes: Vec<RecipeDef>) -> VerbContent {
    VerbContent {
        slots,
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
        max: None,
        aspects: Vec::new(),
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn recipe_when(when: &str, out: &[&str]) -> RecipeDef {
    RecipeDef {
        id: None,
        when: Some(when.to_string()),
        shown: None,
        hint: None,
        max: None,
        aspects: Vec::new(),
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

fn proto(kind: &str) -> PrototypeDef {
    PrototypeDef {
        kind: kind.to_string(),
        ..PrototypeDef::default()
    }
}

fn protos(entries: &[(&str, PrototypeDef)]) -> Option<IndexMap<String, PrototypeDef>> {
    Some(
        entries
            .iter()
            .map(|(n, p)| ((*n).to_string(), p.clone()))
            .collect(),
    )
}

fn slot_trigger(id: &str, slot: i64, out: &[&str]) -> TriggerDef {
    TriggerDef::Slot(SlotTriggerDef {
        id: id.to_string(),
        slot,
        once: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    })
}

fn expire_trigger(id: &str, on_expire_of: &str, out: &[&str]) -> TriggerDef {
    TriggerDef::Expire(ExpireTriggerDef {
        id: id.to_string(),
        on_expire_of: on_expire_of.to_string(),
        out: out.iter().map(|s| (*s).to_string()).collect(),
    })
}

fn deck(id: &str, spec: &[&str], defaultcard: Option<&str>, reset: bool) -> DeckRaw {
    DeckRaw {
        id: id.to_string(),
        spec: spec.iter().map(|s| (*s).to_string()).collect(),
        defaultcard: defaultcard.map(str::to_string),
        reset,
    }
}

fn analyze(
    content: &chorus::content::content::CompiledContent,
    state: &State,
    target: &str,
) -> chorus::reach::CardReport {
    analyze_card(content, state, target, CardOptions::default()).unwrap()
}

fn analyze_usable(
    content: &chorus::content::content::CompiledContent,
    state: &State,
    target: &str,
) -> chorus::reach::CardReport {
    analyze_card(
        content,
        state,
        target,
        CardOptions {
            usable: true,
            ..CardOptions::default()
        },
    )
    .unwrap()
}

fn analyze_demand(
    content: &chorus::content::content::CompiledContent,
    state: &State,
    demand: &str,
) -> chorus::reach::CardReport {
    analyze_card(
        content,
        state,
        "",
        CardOptions {
            demand: Some(demand.to_string()),
            ..CardOptions::default()
        },
    )
    .unwrap()
}

// Keep the IndexSet import honest (used by one scenario's name set compare)
#[allow(unused)]
fn _witness(_s: &IndexSet<String>) {}

// ---------------------------------------------------------------- S1
/// (analyzer half).
/// FORGE (60m) mints KEYCARD at completion; INSPECT binds kind:item — the
/// static fixpoint must chain the mint into the slot; window geometry cannot
/// demote the claims (the analyzer is time-free).
#[test]
fn s1_parallel_windows_the_mint_chains_into_the_slot_and_stays_usable() {
    let raw = ContentRaw {
        prototypes: protos(&[("KEYCARD", proto("item"))]),
        verbs: IndexMap::from([
            (
                "FORGE".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    60,
                    vec![recipe(&["SPAWN KEYCARD"])],
                ),
            ),
            (
                "INSPECT".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("object", 1, 1, Some("kind:item")),
                    ],
                    30,
                    vec![recipe(&["NARRATE \"inspected.\""])],
                ),
            ),
            (
                "TOUCH".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("object", 1, 1, Some("kind:item")),
                    ],
                    30,
                    vec![recipe(&["NARRATE \"touched.\""])],
                ),
            ),
            (
                "WAIT".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    30,
                    vec![recipe(&[])],
                ),
            ),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[]), ent("LAMP", "item", &[])]);

    // FORGE is committable (ANA fills actors) → its SPAWN site is proven →
    // KEYCARD obtainable; INSPECT.object's query matches KEYCARD's descriptor
    let card = analyze_usable(&content, &state, "KEYCARD");
    assert_eq!(card.verdict, CardVerdict::Obtainable);
    assert_eq!(card.existence, Existence::Proven);
    assert!(card.chain.join("\n").contains("FORGE"));
    assert!(card
        .usable_via
        .iter()
        .any(|v| v.starts_with("INSPECT.object")));
    let lamp = analyze_usable(&content, &state, "LAMP");
    assert_eq!(lamp.verdict, CardVerdict::Obtainable);
}

// ---------------------------------------------------------------- S2
/// (analyzer half).
/// SET(PAPER, expires) + `on expire of PAPER -> SPAWN ASH`: the expiry
/// trigger counts as a producer once its named card is available AND
/// expirable.
#[test]
fn s2_expiry_chain_the_expiry_trigger_proves_its_mint() {
    let raw = ContentRaw {
        prototypes: protos(&[("ASH", proto("item"))]),
        verbs: IndexMap::from([
            (
                "TORCH".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    0,
                    vec![recipe(&["SET(PAPER, expires, 30m)"])],
                ),
            ),
            (
                "WAIT".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    30,
                    vec![recipe(&[])],
                ),
            ),
        ]),
        triggers: Some(vec![expire_trigger(
            "paper_to_ash",
            "PAPER",
            &["SPAWN ASH"],
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![
        ent("ANA", "character", &[]),
        ent("PAPER", "item", &[]),
    ]);

    let ash = analyze(&content, &state, "ASH");
    assert_eq!(ash.verdict, CardVerdict::Obtainable);
    assert!(ash.chain.join("\n").contains("paper_to_ash"));
    let paper = analyze(&content, &state, "PAPER");
    assert_eq!(paper.verdict, CardVerdict::Obtainable);
}

// ---------------------------------------------------------------- S3
/// (analyzer half).
/// SPAWN A; REPLACE A WITH B; REPLACE B WITH C — the fixpoint proves SEED
/// first, then the REPLACE sites in later rounds; transience does not demote.
#[test]
fn s3_replace_chain_c_obtainable_through_the_two_hop_chain() {
    let raw = ContentRaw {
        prototypes: protos(&[
            ("SEED", proto("item")),
            ("SPROUT", proto("item")),
            ("TREE", proto("item")),
        ]),
        verbs: IndexMap::from([
            (
                "PLANT".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    0,
                    vec![recipe(&["SPAWN SEED"])],
                ),
            ),
            (
                "WATER".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    0,
                    vec![recipe(&["REPLACE SEED WITH SPROUT"])],
                ),
            ),
            (
                "TEND".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    0,
                    vec![recipe(&["REPLACE SPROUT WITH TREE"])],
                ),
            ),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);

    let tree = analyze(&content, &state, "TREE");
    assert_eq!(tree.verdict, CardVerdict::Obtainable);
    let sprout = analyze(&content, &state, "SPROUT");
    assert_eq!(sprout.verdict, CardVerdict::Obtainable);
    let seed = analyze(&content, &state, "SEED");
    assert_eq!(seed.verdict, CardVerdict::Obtainable);
    assert!(seed.chain.join("\n").contains("SPAWN mints"));
}

// ---------------------------------------------------------------- S4
/// (analyzer half).
/// SUSPICION is stackable; PROBE's object slot takes TWO kind:state cards —
/// slot_support expands a stackable mint fact into name#copy instances, so
/// min:2 is satisfiable from ONE source.
#[test]
fn s4_stackable_pool_feeding_a_min2_slot() {
    let raw = ContentRaw {
        prototypes: protos(&[
            (
                "SUSPICION",
                PrototypeDef {
                    kind: "state".to_string(),
                    stackable: true,
                    ..PrototypeDef::default()
                },
            ),
            ("INSIGHT", proto("idea")),
        ]),
        verbs: IndexMap::from([
            (
                "STEW".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    0,
                    vec![recipe(&["SPAWN SUSPICION"])],
                ),
            ),
            (
                "PROBE".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("object", 2, 2, Some("kind:state")),
                    ],
                    0,
                    vec![recipe(&["SPAWN INSIGHT"])],
                ),
            ),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);

    let insight = analyze(&content, &state, "INSIGHT");
    assert_eq!(insight.verdict, CardVerdict::Obtainable);
    assert!(insight.chain.join("\n").contains("SUSPICION"));
    assert!(insight.chain.join("\n").contains("PROBE committable"));
}

// ---------------------------------------------------------------- S5
/// (analyzer half).
/// Chained derive mints must never be called structurally absent.
#[test]
fn s5_derive_of_derive_the_chained_names_are_candidate_never_unobtainable() {
    let raw = ContentRaw {
        prototypes: protos(&[
            (
                "RUMOUR",
                PrototypeDef {
                    kind: "idea".to_string(),
                    derive: Some(PrototypeDerive {
                        name_from: Some("object".to_string()),
                        aspects: None,
                    }),
                    ..PrototypeDef::default()
                },
            ),
            (
                "WHISPER",
                PrototypeDef {
                    kind: "idea".to_string(),
                    derive: Some(PrototypeDerive {
                        name_from: Some("object".to_string()),
                        aspects: None,
                    }),
                    ..PrototypeDef::default()
                },
            ),
        ]),
        verbs: IndexMap::from([
            (
                "HEAR".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("object", 1, 1, Some("kind:item")),
                    ],
                    0,
                    vec![recipe(&["SPAWN RUMOUR"])],
                ),
            ),
            (
                "RETELL".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("object", 1, 1, Some("kind:idea")),
                    ],
                    0,
                    vec![recipe(&["SPAWN WHISPER"])],
                ),
            ),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![
        ent("ANA", "character", &[]),
        ent("THE_BOOK", "item", &[("secret", 2)]),
    ]);

    let d1 = analyze(&content, &state, "RUMOUR_THE_BOOK");
    assert_eq!(d1.verdict, CardVerdict::Unproven);
    assert_eq!(d1.existence, Existence::Candidate);
    assert!(d1.notes.join(" ").contains("derived-mint"));
    let d2 = analyze(&content, &state, "WHISPER_RUMOUR_THE_BOOK");
    assert_eq!(d2.verdict, CardVerdict::Unproven);
    assert_eq!(d2.existence, Existence::Candidate);
    let bare = analyze(&content, &state, "RUMOUR");
    assert_eq!(bare.verdict, CardVerdict::Unproven);
    assert!(bare.notes.join(" ").contains("derive"));
}

// ---------------------------------------------------------------- S6
/// (analyzer half).
/// Stackable variant: full reachability. Non-stackable control: the
/// analyzer's freeze line is coarser than the truth (every pre-freeze trigger
/// draw site caps at ONE draw; decks take the MAX over sites) — DUSK stays
/// proven, DAWN/EMBER/ECHO demote to unproven, nothing is 'unobtainable'.
#[test]
fn s6_deck_reset_and_defaultcard_with_the_non_stackable_freeze_nuance() {
    let mk = |stackable: bool| {
        let p = |kind: &str| PrototypeDef {
            kind: kind.to_string(),
            stackable,
            ..PrototypeDef::default()
        };
        ContentRaw {
            prototypes: protos(&[
                ("DAWN", p("item")),
                ("DUSK", p("item")),
                ("EMBER", p("item")),
                ("ECHO", p("item")),
            ]),
            decks: Some(vec![
                deck("CYCLE", &["DAWN", "DUSK"], None, true),
                deck("FALLBACK", &["EMBER"], Some("ECHO"), false),
            ]),
            triggers: Some(vec![
                slot_trigger("draw_c1", 1, &["DRAW CYCLE"]),
                slot_trigger("draw_c2", 2, &["DRAW CYCLE"]),
                slot_trigger("draw_f1", 3, &["DRAW FALLBACK"]),
                slot_trigger("draw_f2", 4, &["DRAW FALLBACK"]),
                slot_trigger("draw_c3", 5, &["DRAW CYCLE"]),
            ]),
            verbs: IndexMap::from([(
                "WAIT".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    30,
                    vec![recipe(&[])],
                ),
            )]),
            ..ContentRaw::default()
        }
    };

    // --- the clean (stackable) variant: full reachability
    let content = compile(&mk(true));
    let state = state_with(vec![ent("ANA", "character", &[])]);
    for name in ["DAWN", "DUSK", "EMBER", "ECHO"] {
        let rep = analyze(&content, &state, name);
        assert_eq!(rep.verdict, CardVerdict::Obtainable, "{name}");
        assert!(rep.chain.join("\n").contains("deck "), "{name}");
    }

    // --- the non-stackable control: the coarser freeze line
    let plain = compile(&mk(false));
    let dusk = analyze(&plain, &state, "DUSK");
    assert_eq!(dusk.verdict, CardVerdict::Obtainable); // order[0]: one draw suffices
    let dawn = analyze(&plain, &state, "DAWN");
    assert_eq!(dawn.verdict, CardVerdict::Unproven); // needs 2 CYCLE draws
    let ember = analyze(&plain, &state, "EMBER");
    assert_eq!(ember.verdict, CardVerdict::Unproven); // its trigger is dup_risk-flagged
    let echo = analyze(&plain, &state, "ECHO");
    assert_eq!(echo.verdict, CardVerdict::Unproven); // needs 2 FALLBACK draws
}

// ---------------------------------------------------------------- S7
/// (analyzer half).
/// `SPAWN rite:RELIC` overrides the minted kind; the KIND carries the
/// stackable declaration so the mint stacks; demand 'kind:rite' is proven by
/// the overridden MINT descriptor.
#[test]
fn s7_kind_override_plus_kind_stackable_and_demand_matching_the_overridden_descriptor() {
    let raw = ContentRaw {
        prototypes: protos(&[
            ("RELIC", proto("item")),
            (
                "RITE_PROTO",
                PrototypeDef {
                    kind: "rite".to_string(),
                    stackable: true,
                    ..PrototypeDef::default()
                },
            ),
        ]),
        verbs: IndexMap::from([(
            "CONSECRATE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                0,
                vec![recipe(&["SPAWN rite:RELIC"])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[])]);

    let demand = analyze_demand(&content, &state, "kind:rite");
    assert_eq!(demand.verdict, CardVerdict::Obtainable);
    assert!(demand.chain.join("\n").contains("kind overridden"));
    let relic = analyze(&content, &state, "RELIC");
    assert_eq!(relic.verdict, CardVerdict::Obtainable);
    let rite_proto = analyze(&content, &state, "RITE_PROTO");
    assert_eq!(rite_proto.verdict, CardVerdict::Unobtainable);
}

// ---------------------------------------------------------------- S8
/// (analyzer half).
/// Three same-slot triggers sharing a non-stackable mint: the shared mint
/// dooms the whole event pass — every sibling's mints demote, in BOTH
/// declaration orders; the distinct-mint control stays obtainable.
#[test]
fn s8_same_slot_triple_triggers_with_overlapping_mint_names_demote_in_both_orders() {
    let mk = |order: &str| {
        let ta = ("ta", &["SPAWN P_TRIPLE", "SPAWN NAME_A"][..]);
        let tb = ("tb", &["SPAWN P_TRIPLE", "SPAWN NAME_B"][..]);
        let tc = ("tc", &["SPAWN P_TRIPLE", "SPAWN NAME_C"][..]);
        let triggers: Vec<TriggerDef> = match order {
            "abc" => vec![
                slot_trigger(ta.0, 5, ta.1),
                slot_trigger(tb.0, 5, tb.1),
                slot_trigger(tc.0, 5, tc.1),
            ],
            _ => vec![
                slot_trigger(tc.0, 5, tc.1),
                slot_trigger(tb.0, 5, tb.1),
                slot_trigger(ta.0, 5, ta.1),
            ],
        };
        ContentRaw {
            prototypes: protos(&[
                ("P_TRIPLE", proto("item")),
                ("NAME_A", proto("idea")),
                ("NAME_B", proto("idea")),
                ("NAME_C", proto("idea")),
            ]),
            triggers: Some(triggers),
            verbs: IndexMap::from([(
                "WAIT".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    30,
                    vec![recipe(&[])],
                ),
            )]),
            ..ContentRaw::default()
        }
    };
    let state = state_with(vec![ent("ANA", "character", &[])]);

    for order in ["abc", "cba"] {
        let content = compile(&mk(order));
        for name in ["P_TRIPLE", "NAME_A", "NAME_B", "NAME_C"] {
            let rep = analyze(&content, &state, name);
            assert_ne!(rep.verdict, CardVerdict::Obtainable, "{order}:{name}");
            assert_eq!(rep.verdict, CardVerdict::Unproven, "{order}:{name}");
            assert_eq!(rep.existence, Existence::Candidate, "{order}:{name}");
        }
    }

    // control: the same triple WITHOUT the shared mint — all three obtainable
    let distinct_raw = ContentRaw {
        prototypes: protos(&[
            ("NAME_A", proto("idea")),
            ("NAME_B", proto("idea")),
            ("NAME_C", proto("idea")),
        ]),
        triggers: Some(vec![
            slot_trigger("ta", 5, &["SPAWN NAME_A"]),
            slot_trigger("tb", 5, &["SPAWN NAME_B"]),
            slot_trigger("tc", 5, &["SPAWN NAME_C"]),
        ]),
        verbs: IndexMap::from([(
            "WAIT".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&[])],
            ),
        )]),
        ..ContentRaw::default()
    };
    let distinct = compile(&distinct_raw);
    for name in ["NAME_A", "NAME_B", "NAME_C"] {
        let rep = analyze(&distinct, &state, name);
        assert_eq!(rep.verdict, CardVerdict::Obtainable, "{name}");
    }
}

// ---------------------------------------------------------------- S9
/// (analyzer half).
/// The gate-durability rule (seed 6028): a static gate on exists(ROPE) with a
/// ROPE killer demotes the site to candidate.
#[test]
fn s9_gate_death_the_mortal_gate_demotes_to_unproven() {
    let raw = ContentRaw {
        prototypes: protos(&[("GOAL", proto("item"))]),
        verbs: IndexMap::from([(
            "RITUAL".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                0,
                vec![recipe_when("exists(ROPE)", &["SPAWN GOAL"])],
            ),
        )]),
        triggers: Some(vec![slot_trigger("rope_snaps", 2, &["DESTROY ROPE"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let state = state_with(vec![ent("ANA", "character", &[]), ent("ROPE", "item", &[])]);

    let goal = analyze(&content, &state, "GOAL");
    assert_eq!(goal.verdict, CardVerdict::Unproven);
    assert_eq!(goal.existence, Existence::Candidate);
    assert!(goal.notes.join(" ").contains("candidate source"));
}

// ---------------------------------------------------------------- S10
/// (analyzer half).
/// self_gated: the recipe's own mint fills a required slot and collides
/// post-bootstrap — GOAL stays unproven; with the slot OPTIONAL the bootstrap
/// exists and the analyzer flips to obtainable.
#[test]
fn s10_self_filler_collision_selfgated_demotes_until_the_slot_is_optional() {
    let mk = |with_min: i64| ContentRaw {
        prototypes: protos(&[("X", proto("item")), ("GOAL", proto("idea"))]),
        verbs: IndexMap::from([
            (
                "MAKER".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    0,
                    vec![recipe(&["SPAWN X"])],
                ),
            ),
            (
                "SELFMADE".to_string(),
                verb(
                    vec![
                        slot("actors", 1, 1, Some("kind:character")),
                        slot("with", with_min, 1, Some("kind:item")),
                    ],
                    0,
                    vec![recipe(&["SPAWN X", "SPAWN GOAL"])],
                ),
            ),
        ]),
        ..ContentRaw::default()
    };
    let content = compile(&mk(1));
    let state = state_with(vec![ent("ANA", "character", &[])]);

    let x = analyze(&content, &state, "X");
    assert_eq!(x.verdict, CardVerdict::Obtainable); // via MAKER
    let goal = analyze(&content, &state, "GOAL");
    assert_eq!(goal.verdict, CardVerdict::Unproven);
    assert_eq!(goal.existence, Existence::Candidate);
    assert!(goal.notes.join(" ").contains("candidate source"));

    // control (the rule's precision): with the with-slot OPTIONAL the
    // bootstrap EXISTS — the self-gated filler branch skips min:0 slots
    let optional = compile(&mk(0));
    let goal_opt = analyze(&optional, &state, "GOAL");
    assert_eq!(goal_opt.verdict, CardVerdict::Obtainable);
}

// ---------------------------------------------------------------- S13
/// (analyzer half).
/// Roll-guarded mint: the analyzer cannot evaluate roll — the mint arm is
/// runtime-gated → 'unproven' for BOTH seeds (never 'obtainable', never
/// 'unobtainable'). Seed search via roll_die exactly like the rig.
#[test]
fn s13_roll_guarded_mint_stays_unproven_for_both_seeds() {
    let mk = |seed: i64| {
        ContentRaw {
        seed: Some(seed),
        prototypes: protos(&[("PRIZE", proto("item"))]),
        verbs: IndexMap::from([(
            "GAMBLE".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                0,
                vec![recipe(&[
                    "ROLL(1, 6)",
                    "IF roll >= 4 -> (SPAWN PRIZE) ELSE -> (REFUSE(\"frozen — the house always wins\"))",
                ])],
            ),
        )]),
        ..ContentRaw::default()
    }
    };
    let low_seed = (0..).find(|&s| roll_die(s, 0, 6) < 4).unwrap();
    let high_seed = (0..).find(|&s| roll_die(s, 0, 6) >= 4).unwrap();
    assert!(roll_die(low_seed, 0, 6) < 4);
    assert!(roll_die(high_seed, 0, 6) >= 4);
    let state = state_with(vec![ent("ANA", "character", &[])]);

    for (label, seed) in [("low", low_seed), ("high", high_seed)] {
        let rep = analyze(&compile(&mk(seed)), &state, "PRIZE");
        assert_eq!(rep.verdict, CardVerdict::Unproven, "seed {label}");
        assert_eq!(rep.existence, Existence::Candidate, "seed {label}");
    }
}

// ---------------------------------------------------------------- S14
/// (analyzer half).
/// Two-stage expiry arming (seed 100625): the arming chain must be checked
/// recursively — a witness inside an expiry trigger needs THAT trigger's own
/// card expirable too.
#[test]
fn s14_two_stage_expiry_arming_the_broken_chain_stays_unproven() {
    let mk = |with_armer: bool| {
        let mut verbs = IndexMap::from([(
            "WAIT".to_string(),
            verb(
                vec![slot("actors", 1, 1, Some("kind:character"))],
                30,
                vec![recipe(&[])],
            ),
        )]);
        if with_armer {
            verbs.insert(
                "STUDY".to_string(),
                verb(
                    vec![slot("actors", 1, 1, Some("kind:character"))],
                    0,
                    vec![recipe(&["SET(CRATE, expires, 30m)"])],
                ),
            );
        }
        ContentRaw {
            prototypes: protos(&[("PRIZE", proto("idea"))]),
            triggers: Some(vec![
                expire_trigger("t_arms", "CRATE", &["SET(ANA, expires, 30m)"]),
                expire_trigger("t_grand", "ANA", &["SPAWN PRIZE"]),
            ]),
            verbs,
            ..ContentRaw::default()
        }
    };
    let state = state_with(vec![
        ent("ANA", "character", &[]),
        ent("CRATE", "item", &[]),
    ]);

    // dead chain: t_grand is a live site, so the verdict is unproven (guarded,
    // NOT structurally absent)
    let dead = compile(&mk(false));
    let prize_dead = analyze(&dead, &state, "PRIZE");
    assert_eq!(prize_dead.verdict, CardVerdict::Unproven);

    // control: a committable verb arms CRATE, and the WHOLE chain proves
    let live = compile(&mk(true));
    let prize_live = analyze(&live, &state, "PRIZE");
    assert_eq!(prize_live.verdict, CardVerdict::Obtainable);
}

// ---------------------------------------------------------------- extra
/// The zero-entity corner (Q17): a script with no entities at
/// all reaches site_reason's empty-init-names branch. The verb must not be
/// DEAD for the candidate listing to run, so the universe carries a
/// character-kind prototype (a static descriptor keeps can_ever_match true);
/// the gated site then stays a candidate whose reason names the unknown
/// committability.
#[test]
fn q17_zero_entity_scripts_keep_the_site_reason_branch_reachable() {
    let raw = ContentRaw {
        prototypes: protos(&[("PILLAR", proto("item")), ("VISITOR", proto("character"))]),
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
    let state = new_state(0);
    let rep = analyze(&content, &state, "PILLAR");
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    let candidate = rep
        .candidates
        .iter()
        .find(|c| c.via.contains("EXPLORE"))
        .expect("the gated spawn is the candidate source");
    assert!(candidate.reason.contains("verb committability unknown"));
}
