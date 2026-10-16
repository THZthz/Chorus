//! Script-compiler acceptance — the script-compiler behaviors, exercised
//! against the Nightjar Flyer. The lenient new-core-verbs suite below
//! needs Game/lenient.
//!
//! The script text is DATA: embedded with include_str! from
//! scripts/the-nightjar-flyer.txt.

use chorus::content::content::{
    compile_content, for_each_authored_outcome, CompiledEnding, CompiledTrigger, ContentRaw,
    DeckCard, DeclaredEntity, DeriveAspect, PrototypeDerive,
};
use chorus::content::script::{compile_script, parse_script};
use chorus::core::dict::core_dictionary;
use chorus::core::r#ref::Ref;
use chorus::core::sentence::{Arg, Outcome};
use chorus::core::types::{AspectVal, Clock};
use chorus::error::Result;

#[path = "../common/mod.rs"]
mod common;
use common::harness::assert_err_msg;

/// The bundled
/// showcase, read as text. include_str! pins the path
/// `scripts/the-nightjar-flyer.txt` (the script text is data, never
/// precompiled).
const FLYER: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/the-nightjar-flyer.txt"));

fn aspects(pairs: &[(&str, i64)]) -> chorus::core::types::Aspects {
    pairs.iter().map(|(k, v)| ((*k).to_string(), AspectVal::Num(*v))).collect()
}

fn deck_of<'a>(
    content: &'a chorus::content::content::CompiledContent,
    id: &str,
) -> &'a chorus::content::content::CompiledDeck {
    content.decks.get(id).unwrap_or_else(|| panic!("deck {id} missing"))
}

fn names(deck: &chorus::content::content::CompiledDeck) -> Vec<&str> {
    deck.order.iter().map(|c: &DeckCard| c.name.as_str()).collect()
}

#[test]
fn meta_lines_set_title_player_goal_the_starting_clock_and_the_dice_seed() {
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    assert_eq!(game.meta.title.as_deref(), Some("Murder on the Nightjar Flyer"));
    assert_eq!(game.meta.player.as_deref(), Some("MARCH"));
    assert_eq!(
        game.meta.goal.as_deref(),
        Some(
            "unmask Lord Vane's killer — gather proofs aboard and put them to the inquest before Vienna"
        )
    );
    assert_eq!(game.meta.start, Some(Clock { day: 0, halfhour: 42 }));
    assert_eq!(game.meta.seed, Some(1899));
    assert_eq!(game.state.now, 42 * 30);
}

#[test]
fn entities_carry_kinds_aspects_and_containers() {
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    let march_id = &game.state.by_name["MARCH"][0];
    let march = &game.state.entities[march_id.as_str()];
    assert_eq!(march.kind, "character");
    assert_eq!(
        march.attrs.get("container"),
        Some(&chorus::core::types::AttrValue::Str("s_guard_van".into()))
    );
    let guard = game.state.entities.get("s_the_guard").unwrap();
    assert_eq!(guard.aspects, aspects(&[("duty", 2), ("unmet", 1)]));
    assert_eq!(
        guard.attrs.get("container"),
        Some(&chorus::core::types::AttrValue::Str("s_guard_van".into()))
    );
    // a character can carry a card: the claim ticket rides in the guard's pocket
    let ticket = game.state.entities.get("s_the_claim_ticket").unwrap();
    assert_eq!(
        ticket.attrs.get("container"),
        Some(&chorus::core::types::AttrValue::Str("s_the_guard".into()))
    );
}

#[test]
fn the_train_is_a_zone_not_a_card_her_cars_carry_it_as_an_attribute() {
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    assert!(!game.state.by_name.contains_key("NIGHTJAR_FLYER"));
    let zone = Some(&chorus::core::types::AttrValue::Str("NIGHTJAR_FLYER".into()));
    assert_eq!(game.state.entities["s_guard_van"].attrs.get("zone"), zone);
    assert_eq!(game.state.entities["s_firebox"].attrs.get("zone"), zone);
}

#[test]
fn places_sit_on_the_corridor_with_explicit_directions() {
    // The stored edge keeps the authored
    // direction, and queries find it from either endpoint order
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    let south = game.state.relations.get("s_firebox|s_guard_van|south").unwrap();
    assert_eq!(south.type_, "south");
    let south = game.state.relations.get("s_dining_car|s_first_cabin|south").unwrap();
    assert_eq!(south.type_, "south");
    let read = game.content.verbs.get("TRAVEL_TO").unwrap();
    let gate = &read.recipes[0];
    assert_eq!(gate.id, "travel.no_link");
    assert!(gate.when.is_some());
}

#[test]
fn verb_blocks_compile_slots_durations_and_recipes() {
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    let read = game.content.verbs.get("READ").unwrap();
    assert_eq!(read.duration, 60);
    let object_slot = read.slot_by_role.get("object").unwrap();
    assert_eq!(object_slot.min, 1);
    assert_eq!(object_slot.max, Some(1));
    assert_eq!(
        read.recipes.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        [
            "read.manifest",
            "read.french_gate",
            "read.cylinder",
            "read.telegram",
            "read.ledger",
            "read.letter",
            "read.companions",
            "read.default",
        ]
    );
    assert!(!read.recipes[1].shown); // hidden gate
    assert!(read.recipes[0].when.is_some());
    assert!(read.recipes[7].when.is_none()); // default
}

#[test]
fn triggers_and_prototypes_compile() {
    // The timetable: recurring + `once`,
    // the `at hour N` alias (hour 1 = slot 2), and the expiry trigger form
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    let got: Vec<String> = game
        .content
        .triggers
        .iter()
        .map(|t| match t {
            CompiledTrigger::Slot { id, slot, once, .. } => format!("{id}|{slot}|{once}"),
            CompiledTrigger::Expire { id, on_expire_of, .. } => {
                format!("{id}|{on_expire_of}")
            }
            // §9.48: the FLYER carries no `on complete` lines — nothing maps
            CompiledTrigger::Complete { .. } => String::new(),
        })
        .collect();
    assert_eq!(
        got,
        [
            "body_found|44|true",
            "night_porter|46|true",
            "conductor_rounds|45|false",
            "border_stop|0|true",
            "the_long_tunnel|2|true",
            "the_furnace_takes_it|4|true",
            "dawn_watch|8|false",
            "lord_tended|THE_LATE_LORD",
            "vienna_arrival|12|true",
        ]
    );
    let late_lord = game.content.prototypes.get("THE_LATE_LORD").unwrap();
    // the pinned assertion is a SUBSET match; THE_LATE_LORD carries
    // exactly {aether:1}
    assert_eq!(late_lord.kind, "item");
    assert_eq!(late_lord.aspects, Some(aspects(&[("aether", 1)])));
    let brack = game.content.prototypes.get("BRACK").unwrap();
    // subset: BRACK is `character with duty:2, nerve:2` — the pin covers
    // only kind + duty
    assert_eq!(brack.kind, "character");
    assert_eq!(brack.aspects.as_ref().and_then(|a| a.get("duty")), Some(&AspectVal::Num(2)));
}

#[test]
fn the_v0_4_sections_compile_pools_deck_endings_and_expanded_functions() {
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    assert_eq!(game.content.seed, 1899); // the header's documented, calibrated seed
                                         // pools: SUSPICION and the gossip cards are declared stackable
    for name in ["SUSPICION", "RUMOUR", "HEARSAY", "SCANDAL"] {
        assert!(game.content.stackable_names.contains(name), "{name}");
    }
    // the deck: resolved entries, the seed-fixed permutation of the 2:1:1
    // spec, and its exhaustion rule
    let deck = deck_of(&game.content, "RUMOURS");
    assert_eq!(names(deck), ["SCANDAL", "RUMOUR", "RUMOUR", "HEARSAY"]); // permutation at seed 1899
    assert_eq!(
        deck.order.iter().map(|c| c.kind.as_str()).collect::<Vec<_>>(),
        ["idea", "idea", "idea", "idea"]
    );
    assert_eq!(deck.defaultcard, Some(DeckCard { name: "RUMOUR".into(), kind: "idea".into() }));
    assert!(deck.reset);
    // the escalation ending is declared (patrol_check's ENDING resolves at load)
    let off = game.content.endings.get("OFF_THE_FLYER").unwrap();
    assert_eq!(off.title, "Off the Flyer at Speed");
    assert_eq!(off.flavour, "melancholy");
    // functions expand away at load (there is no runtime function table):
    // the compiled bodies carry the call sites' splices
    let explore = game.content.verbs.get("EXPLORE").unwrap();
    // patrol_check expanded into the cabin recipe — its ROLL is there
    let cabin = explore.recipes.iter().find(|r| r.id == "explore.cabin").unwrap();
    assert!(cabin.out.iter().any(|o| o.op == "ROLL"));
    // sus expanded with the {who} interpolation baked at the call site
    let searched = explore.recipes.iter().find(|r| r.id == "explore.searched").unwrap();
    let steward = searched.out.iter().find_map(|o| {
        if o.op == "NARRATE" {
            match o.args.first() {
                Some(Arg::Str { v }) => Some(v.clone()),
                _ => None,
            }
        } else {
            None
        }
    });
    assert_eq!(
        steward.as_deref().unwrap_or(""),
        "A steward will remember MARCH asking after things that are none of an inspector's business."
    ); // the nerve pool feeds the rude searches' dice
    let luggage = explore.recipes.iter().find(|r| r.id == "explore.luggage").unwrap();
    assert!(luggage.out.iter().any(|o| o.op == "ROLL"));
    assert!(luggage.out.iter().any(|o| o.op == "IF"));
    // the patrol trigger carries its expanded DRAW + patrol_check roll
    let rounds = game.content.triggers.iter().find(|t| t.id() == "conductor_rounds").unwrap();
    assert!(matches!(rounds, CompiledTrigger::Slot { .. }));
    assert!(rounds.out().iter().any(|o| o.op == "DRAW"));
    assert!(rounds.out().iter().any(|o| o.op == "ROLL"));
}

#[test]
fn the_showcase_compiles_the_full_script_grammar() {
    // (§4–§10 sweep)
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    // entity lines: stackable (§9.38) on the resource kind (§4.1), plus a
    // symmetric relation (§4.2) and a derive prototype (§6.4, §9.23)
    let funds = game.state.entities.get("s_funds").unwrap();
    assert_eq!(funds.kind, "resource");
    assert!(funds.stackable);
    assert!(game.content.stackable_names.contains("FUNDS"));
    let neighbor = game.state.relations.get("s_firebox|s_guard_van|neighbor").unwrap();
    assert_eq!(neighbor.type_, "neighbor");
    assert_eq!(neighbor.value, 1);
    let hunch = game.content.prototypes.get("HUNCH").unwrap();
    assert_eq!(
        hunch.derive,
        Some(PrototypeDerive {
            name_from: Some("object".into()),
            aspects: Some(
                [("hunch", DeriveAspect { from_object_aspect: "secret".into(), minus: 1 })]
                    .into_iter()
                    .map(|(k, v)| (k.to_string(), v))
                    .collect()
            ),
        })
    );
    assert_eq!(hunch.aspects, Some(aspects(&[("aether", 1)])));
    // the bare deck line: no defaultcard, no reset (§9.42)
    let dreams = deck_of(&game.content, "DREAMS");
    assert_eq!(names(dreams), ["VOID", "PORTENT", "PORTENT"]); // permutation at seed 1899
    assert!(dreams.defaultcard.is_none());
    assert!(!dreams.reset);
    // the ending table: explicit flavour, grand flavour, and the default (§9.35)
    assert_eq!(
        game.content.endings.get("QUIET_NIGHT"),
        Some(&CompiledEnding {
            title: "The Flyer Reaches Vienna".into(),
            flavour: "melancholy".into()
        })
    );
    assert_eq!(game.content.endings["CASE_CLOSED"].flavour, "grand");
    // slot-count vocabulary (§6.1): exact 2, open-ended 2.. and 0..
    let mansus_actors = game.content.verbs["MANSUS"].slot_by_role.get("actors").unwrap();
    assert_eq!((mansus_actors.min, mansus_actors.max), (2, Some(2)));
    let work_actors = game.content.verbs["WORK"].slot_by_role.get("actors").unwrap();
    assert_eq!((work_actors.min, work_actors.max), (2, None)); // None = Infinity
    let dream_with = game.content.verbs["DREAM"].slot_by_role.get("with").unwrap();
    assert_eq!((dream_with.min, dream_with.max), (0, None));
    // slot queries (§4.5): aspect level, negated aspect, and the state flag
    // (no '|' in either — one alternation group of N terms, §9.49)
    let work_at = game.content.verbs["WORK"].slot_by_role.get("at").unwrap();
    assert_eq!(work_at.query.as_ref().unwrap().len(), 1);
    assert_eq!(work_at.query.as_ref().unwrap()[0].len(), 2);
    assert_eq!(dream_with.query.as_ref().unwrap().len(), 1);
    assert_eq!(dream_with.query.as_ref().unwrap()[0].len(), 3);
    // compound durations (§9.45): 1h30m in a verb config and a PROGRESS tick
    assert_eq!(game.content.verbs["STUDY"].duration, 90);
    // the id-less recipe mints a stable id; the shown gate carries its hint (§6.2)
    let study_gate = &game.content.verbs["STUDY"].recipes[0];
    assert_eq!(study_gate.id, "study.recipe0"); // the 0-based mint, shared by compile and lint (Q1 unified)
    assert!(study_gate.shown);
    assert_eq!(study_gate.hint.as_deref(), Some("Whoever studies must read the tongue."));
    let use_on = &game.content.verbs["USE_ON"];
    let inquest_short = use_on.recipes.iter().find(|r| r.id == "inquest.short").unwrap();
    assert!(inquest_short.shown);
    let hint = inquest_short.hint.as_deref().unwrap_or("");
    assert!(hint.contains("three proofs"), "hint = {hint}");
    // outcome ops new to the showcase, where the story authors them
    let out = |verb: &str, id: &str| -> Vec<String> {
        game.content.verbs[verb]
            .recipes
            .iter()
            .find(|r| r.id == id)
            .unwrap()
            .out
            .iter()
            .map(|o| o.op.clone())
            .collect()
    };
    assert_eq!(
        out("USE_ON", "inquest.win"),
        ["PROGRESS", "PROGRESS", "NARRATE", "SPAWN", "REPLACE", "ENDING"] // the win ends the story (§9.35)
    );
    assert_eq!(out("ACQUIRE", "acquire.claim"), ["SET", "MOVE", "SET", "NARRATE"]);
    assert_eq!(out("DREAM", "dream.seal"), ["DRAW", "ADVANCE", "NARRATE"]);
    // REVEAL rides inside the luggage find's taken-branch IF (§9.39): walk arms
    let mut ops: Vec<String> = Vec::new();
    let luggage = game.content.verbs["EXPLORE"]
        .recipes
        .iter()
        .find(|r| r.id == "explore.luggage")
        .unwrap()
        .out
        .clone();
    for_each_authored_outcome(&luggage, &mut |o: &Outcome| {
        ops.push(o.op.clone());
        Ok(())
    })
    .unwrap();
    assert!(ops.contains(&"REVEAL".to_string()), "ops = {ops:?}");
    // the SPAWN-then-count escalation (§6.8)
    let searched =
        game.content.verbs["EXPLORE"].recipes.iter().find(|r| r.id == "explore.searched").unwrap();
    assert!(searched.out.iter().any(|o| o.op == "IF"));
    assert!(out("WAIT", "wait.till_dawn").contains(&"ADVANCE".to_string()));
    let meet_guard =
        game.content.verbs["TALK"].recipes.iter().find(|r| r.id == "talk.meet_guard").unwrap();
    assert!(meet_guard.out.iter().any(|o| o.op == "RELATE"));
    // triggers: RELATE/UNRELATE corridor closures and the expiry REPLACE
    let tunnel = game.content.triggers.iter().find(|t| t.id() == "the_long_tunnel").unwrap();
    assert!(tunnel.out().iter().any(|o| o.op == "UNRELATE"));
    let dawn = game.content.triggers.iter().find(|t| t.id() == "dawn_watch").unwrap();
    assert!(dawn.out().iter().any(|o| o.op == "RELATE"));
    let tended = game.content.triggers.iter().find(|t| t.id() == "lord_tended").unwrap();
    assert!(tended.out().iter().any(|o| o.op == "REPLACE"));
}

#[test]
fn templates_compile() {
    let dict = core_dictionary();
    let game = compile_script(FLYER, &dict).unwrap();
    assert_eq!(
        game.content.templates.get("READ").map(String::as_str),
        Some("{actors} read {object}.")
    );
    assert_eq!(
        game.content.templates.get("TALK").map(String::as_str),
        Some("{actors} question {to}.")
    );
    // all twelve core verbs carry a prose template
    assert_eq!(game.content.templates.len(), 12);
}

#[test]
fn expiry_triggers_parse_into_their_own_trigger_form() {
    // (§9.32)
    let dict = core_dictionary();
    let script = parse_script(
        "triggers:\n  decay_moth on expire of INFLUENCE_MOTH_G -> REPLACE INFLUENCE_MOTH_G WITH INFLUENCE_MOTH_E\n",
        &dict,
    )
    .unwrap();
    assert_eq!(
        script.content.triggers,
        Some(vec![chorus::content::content::TriggerDef::Expire(
            chorus::content::content::ExpireTriggerDef {
                id: "decay_moth".into(),
                on_expire_of: "INFLUENCE_MOTH_G".into(),
                out: vec!["REPLACE INFLUENCE_MOTH_G WITH INFLUENCE_MOTH_E".to_string()],
            }
        )])
    );
    // compiled: the expire variant with parsed outcomes; no existence check on the name
    let compiled = compile_content(&script.content, &dict, &[]).unwrap();
    assert_eq!(
        compiled.triggers,
        vec![CompiledTrigger::Expire {
            id: "decay_moth".into(),
            on_expire_of: "INFLUENCE_MOTH_G".into(),
            out: vec![Outcome {
                op: "REPLACE".into(),
                args: vec![
                    Arg::Ref { r#ref: Ref::Name { kind: None, name: "INFLUENCE_MOTH_G".into() } },
                    Arg::Ref { r#ref: Ref::Name { kind: None, name: "INFLUENCE_MOTH_E".into() } },
                ],
                arms: None,
                fallback: None,
                call: None,
            }],
        }]
    );
}

#[test]
fn bad_expiry_trigger_grammar_fails_loudly() {
    let dict = core_dictionary();
    assert_err_msg(
        parse_script("triggers:\n  decay_moth on expire -> SPAWN DREAD\n", &dict),
        r"triggers expect",
    );
    assert_err_msg(
        parse_script("triggers:\n  decay_moth expire of MOTH -> SPAWN DREAD\n", &dict),
        r"triggers expect",
    );
    assert_err_msg(
        parse_script("triggers:\n  decay_moth on expire of MOTH\n", &dict),
        r"triggers expect",
    );
}

#[test]
fn bad_scripts_fail_loudly() {
    let dict = core_dictionary();
    assert_err_msg(parse_script("entities:\n  X is dragon\n", &dict), r"unknown kind");
    assert_err_msg(
        parse_script("entities:\n  A is pals of B\n", &dict),
        r"unknown kind", // 'pals' is not a registered relation
    );
    assert_err_msg(parse_script("verbs:\n  FLY:\n    duration: 1h\n", &dict), r"not registered");
    assert_err_msg(parse_script("nonsense line\n", &dict), r"section header");
}

// ---------------------------------------------------------------------------
// compile_script(the-nightjar-flyer) — the full
// load path over the embedded showcase. Here the compile must simply succeed
// and the headline derived fields must match the pinned values (seed
// permutation, stackable sets, minted ids) — which the tests above assert
// field-by-field.
// ---------------------------------------------------------------------------

/// A direct-raw compile_content caller passes NO entities (deck entries
/// validate against prototypes only) — the deck entries here name entity
/// lines, so this must fail with `deck GOSSIP: spec entry 'RUMOUR' is neither a
/// declared prototype nor an entity` (the documented compiler behavior).
#[test]
fn direct_compile_content_without_entities_rejects_entity_deck_entries() {
    let dict = core_dictionary();
    let raw = ContentRaw {
        decks: Some(vec![chorus::content::content::DeckRaw {
            id: "GOSSIP".into(),
            spec: vec!["RUMOUR".into()],
            defaultcard: None,
            reset: false,
        }]),
        ..ContentRaw::default()
    };
    let entities: Vec<DeclaredEntity> = Vec::new();
    let err = compile_content(&raw, &dict, &entities).unwrap_err();
    assert_eq!(
        err.msg,
        "deck GOSSIP: spec entry 'RUMOUR' is neither a declared prototype nor an entity"
    );
}

/// compile_content with entities passes them to deck resolution AND builds
/// the stackable sets in one shot (the single build point).
#[test]
fn direct_compile_content_with_entities_resolves_deck_entries_and_stackable() {
    let dict = core_dictionary();
    let raw = ContentRaw {
        decks: Some(vec![chorus::content::content::DeckRaw {
            id: "GOSSIP".into(),
            spec: vec!["RUMOUR".into()],
            defaultcard: None,
            reset: false,
        }]),
        ..ContentRaw::default()
    };
    let entities =
        vec![DeclaredEntity { name: "RUMOUR".into(), kind: "idea".into(), stackable: true }];
    let compiled = compile_content(&raw, &dict, &entities).unwrap();
    assert_eq!(
        names(deck_of(&compiled, "GOSSIP")),
        ["RUMOUR"] // permute_positions(1, 0, 'GOSSIP') is the identity
    );
    assert!(compiled.stackable_names.contains("RUMOUR"));
    assert!(compiled.stackable_kinds.contains("idea"));
}

/// The frozen guard texts: the cycle/depth/undeclared
/// guards fire through the REAL compile path (content.rs build_fn_table),
/// with the exact texts pre-frozen by tests/fn_expand_direct.rs.
#[test]
fn function_guards_fire_through_the_real_compile_path_with_the_frozen_texts() -> Result<()> {
    let dict = core_dictionary();
    // cycle: a -> b -> a
    let cyclic = "functions:\n  a -> b()\n  b -> a()\n";
    let err = compile_script(cyclic, &dict).unwrap_err();
    assert_eq!(
        err.msg,
        "functions recurse: a -> b -> a — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle"
    );
    // depth: the chain f1..f9
    let mut deep = String::from("functions:\n");
    for i in 1..=9 {
        deep.push_str(&format!(
            "  f{i} -> {}\n",
            if i == 9 { "NARRATE \"bottom\"".to_string() } else { format!("f{}()", i + 1) }
        ));
    }
    let err = compile_script(&deep, &dict).unwrap_err();
    assert_eq!(err.msg, "function 'f1' sits 9 calls deep — the nesting cap is 8");
    Ok(())
}

// ---------------------------------------------------------------------------
// The lenient-input suite for the new core verbs. Per-test fresh Dictionary.
// ---------------------------------------------------------------------------

use chorus::session::lenient::{parse_player_input, LenientCtx, ParsedInput};

/// Lenient input routes the new core
/// verbs (v0.3). Registration is the gate: unregistered verbs fail the
/// compile with 'not registered'.
mod lenient_input_routes_the_new_core_verbs {
    use super::*;
    use chorus::content::content::{RecipeDef, SlotDef, VerbContent};
    use chorus::core::types::{add_entity, AttrValue, Attrs, Entity, State};
    use common::seed::seed_state;

    /// STUDY (READ's grammar, §4.3) and
    /// MANSUS (EXPLORE's grammar, §4.3) registered on a fresh dictionary.
    fn verbs_content(
        dict: &chorus::core::dict::Dictionary,
    ) -> chorus::content::content::CompiledContent {
        let verb = |slots: Vec<SlotDef>, out: &str| VerbContent {
            slots,
            duration: 60,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: vec![out.to_string()],
            }],
        };
        let slot = |role: &str, min: i64, max: Option<i64>, query: &str| SlotDef {
            role: role.to_string(),
            min,
            max,
            query: Some(query.to_string()),
            shown: None,
        };
        let raw = ContentRaw {
            verbs: [
                (
                    "STUDY",
                    verb(
                        vec![
                            slot("actors", 1, Some(2), "kind:character"),
                            slot("object", 1, Some(1), "kind:text"),
                            slot("at", 0, Some(1), "kind:place"),
                        ],
                        "NARRATE(\"{actors} study {object}.\")",
                    ),
                ),
                (
                    "MANSUS",
                    verb(
                        vec![
                            slot("actors", 1, Some(1), "kind:character"),
                            slot("object", 1, Some(1), "kind:place"),
                        ],
                        "NARRATE(\"{actors} walk the Mansus to {object}.\")",
                    ),
                ),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
            ..ContentRaw::default()
        };
        compile_content(&raw, dict, &[]).unwrap()
    }

    /// The seed world plus a text
    /// and a place to route at.
    fn ctx() -> (chorus::core::dict::Dictionary, chorus::content::content::CompiledContent, State) {
        let verbs_dict = core_dictionary();
        let verbs_content = verbs_content(&verbs_dict);
        let mut state = seed_state();
        let card = |id: &str, kind: &str, name: &str, container: &str| Entity {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects: Default::default(),
            attrs: {
                let mut a = Attrs::new();
                a.insert("container".to_string(), AttrValue::Str(container.to_string()));
                a
            },
            born_action: None,
            dead_action: None,
            stackable: false,
        };
        add_entity(&mut state, card("manifest", "text", "THE_MANIFEST", "house")).unwrap();
        add_entity(&mut state, card("white_door", "place", "THE_WHITE_DOOR", "villa")).unwrap();
        (verbs_dict, verbs_content, state)
    }

    #[test]
    fn study_plus_bare_name_routes_to_the_object_role_like_read() {
        let (dict, content, state) = ctx();
        let r = parse_player_input(
            "study the manifest",
            &LenientCtx { dict: &dict, content: &content, state: &state, player: Some("ANA") },
        );
        assert!(r.is_ok());
        let ParsedInput::Ok { step, .. } = r else { unreachable!() };
        let c0 = &step.clauses[0];
        assert_eq!(c0.verb, "STUDY");
        assert_eq!(c0.actors, vec![Ref::Name { kind: None, name: "ANA".into() }]);
        assert_eq!(c0.objects, vec![Ref::Name { kind: None, name: "THE_MANIFEST".into() }]);
    }

    #[test]
    fn mansus_plus_place_routes_to_the_object_role_like_explore() {
        let (dict, content, state) = ctx();
        let r = parse_player_input(
            "mansus the white door",
            &LenientCtx { dict: &dict, content: &content, state: &state, player: Some("ANA") },
        );
        assert!(r.is_ok());
        let ParsedInput::Ok { step, .. } = r else { unreachable!() };
        let c0 = &step.clauses[0];
        assert_eq!(c0.verb, "MANSUS");
        assert_eq!(c0.actors, vec![Ref::Name { kind: None, name: "ANA".into() }]);
        assert_eq!(c0.objects, vec![Ref::Name { kind: None, name: "THE_WHITE_DOOR".into() }]);
    }
}
