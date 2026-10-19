//! Decks — seed-fixed permutation draws + the DRAW
//! op (spec §4.4): the four permutation vectors
//! (computed independently in python3), the drawFrom three-rule
//! probe re-proved through a COMPILED deck, and play-side draw sequences, INTO placement, the
//! DRAWN+SPAWN concretization, exhaustion through the engine, cursor
//! reconstruction through JSONL/fold/undo, the livelock rule, load-time deck
//! validation, DRAW inside function bodies and content-drift loud failures.
//!
//! Every test builds a FRESH Dictionary (SEEK is a per-test
//! registration). Error-message patterns are copied CHARACTER FOR CHARACTER
//! from the frozen literals.
//! Reference-identity semantics (`r.state === state`) have no Rust
//! observable: `settle_step` hands the ORIGINAL (never-mutated) state back
//! on a refusal, as a value — `snap()` equality IS the semantic content of
//! those checks (noted at each site, never weakened).

use crate::common::harness::{assert_err_msg, assert_refusal, assert_rejected};
use crate::common::rig::{compile, settle_line, tmp_log_path};
use crate::common::seed::seed_state;
use crate::common::snap::snap;
use chorus::engine::settle::{act, done, refusal};

use chorus::content::script::compile_script;
use chorus::content::{
    compile_content, permute_positions, CompiledContent, ContentRaw, DeckRaw, PrototypeDef,
    RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::parse::{parse_outcomes, parse_sentence};
use chorus::core::r#ref::Ref;
use chorus::core::sentence::Arg;
use chorus::core::types::{clone_state, AttrValue, State};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{ActionRecord, ExecResult};
use chorus::session::game::{Game, TurnKind};

// ---------------------------------------------------------------- rigs ------

/// The file-level dict registers SEEK (the
/// placement-default verb; verbs are code-registered, §10). Per-test fresh.
fn deck_dict() -> Dictionary {
    let mut d = core_dictionary();
    d.register_verb("SEEK").unwrap();
    d
}

/// The deck rig: ANA at
/// HOUSE, KITCHEN inside HOUSE, stackable gossip prototypes by default, and
/// a `default -> DRAW RUMOURS` WAIT verb.
#[derive(Default)]
struct Rig<'a> {
    seed: i64,
    entities: &'a str,
    prototypes: Option<&'a str>,
    decks: Option<&'a str>,
    triggers: Option<&'a str>,
    functions: Option<&'a str>,
    verbs: Option<&'a str>,
    draw_deck: Option<&'a str>,
}

fn rig(o: Rig) -> String {
    let prototypes = o
        .prototypes
        .unwrap_or(
            "  RUMOUR is idea stackable\n  HEARSAY is idea stackable\n  SCANDAL is idea stackable",
        )
        .to_string();
    // the draw-deck
    // default only applies when the verbs block itself is defaulted
    let draw_deck = o.draw_deck.unwrap_or("RUMOURS");
    let verbs = match o.verbs {
        Some(v) => v.to_string(),
        None => format!(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW {draw_deck}"
        ),
    };
    let section = |header: &str, body: Option<&str>| match body {
        Some(b) => format!("\n{header}:\n{b}"),
        None => String::new(),
    };
    format!(
        "
script: Deck Rig
player: ANA
start: 0:0
seed: {seed}

entities:
  ANA is character at HOUSE
  HOUSE is place
  KITCHEN is place in HOUSE
{entities}

prototypes:
{prototypes}{decks}{triggers}{functions}

verbs:
{verbs}
",
        seed = o.seed,
        entities = o.entities,
        decks = section("decks", o.decks),
        triggers = section("triggers", o.triggers),
        functions = section("functions", o.functions),
        verbs = verbs,
    )
}

/// The flagship example deck (seed 0 order:
/// RUMOUR, HEARSAY, RUMOUR, SCANDAL — pinned via permute_positions below).
const PLAN_DECK: &str = "RUMOURS spec RUMOUR, RUMOUR, HEARSAY, SCANDAL defaultcard RUMOUR reset";

/// The drawn names of a result,
/// in order (the DRAWN row's second arg).
fn drawn_names(r: &ExecResult) -> Vec<String> {
    drawn_names_of(&r.records)
}

fn drawn_names_of(records: &[ActionRecord]) -> Vec<String> {
    records
        .iter()
        .flat_map(|rec| {
            rec.outcomes.iter().filter(|o| o.op == "DRAWN").map(|o| match o.args.get(1) {
                Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => name.clone(),
                _ => "(?)".to_string(),
            })
        })
        .collect()
}

/// Play `n` WAITs, returning
/// the drawn names, the records, and the final state.
fn play_draws(
    content: &CompiledContent,
    dict: &Dictionary,
    mut state: State,
    n: usize,
) -> (Vec<String>, Vec<ActionRecord>, State) {
    let mut names: Vec<String> = Vec::new();
    let mut records: Vec<ActionRecord> = Vec::new();
    let mut id = 1i64;
    for _ in 0..n {
        let ExecResult { records: recs, state: st, rejected } =
            settle_line(content, dict, state, "0:0 @ ANA WAIT", id).unwrap();
        if let Some(rej) = rejected {
            panic!("unexpected rejection: {rej}");
        }
        names.extend(drawn_names_of(&recs));
        id += recs.len() as i64;
        records.extend(recs);
        state = st;
    }
    (names, records, state)
}

// ---------------------------------------------------------------------------
// The permutation vectors and the drawFrom three
// rules; re-proved through a compiled deck below.
// ---------------------------------------------------------------------------

#[test]
fn permutation_is_pure_in_seed_and_deck_id_with_position_vectors() {
    // The shuffle keys are pinned splitmix64
    // values, so these are not probabilistic asserts; the n=7 and
    // negative-seed rows were computed independently (python3).
    assert_eq!(permute_positions(4, 0, "RUMOURS"), vec![0, 2, 1, 3]);
    assert_eq!(permute_positions(4, 42, "RUMOURS"), vec![3, 0, 1, 2]);
    assert_eq!(permute_positions(7, 0, "SEVEN"), vec![0, 6, 3, 1, 4, 5, 2]);
    assert_eq!(permute_positions(7, -3, "NEG"), vec![4, 0, 2, 6, 3, 1, 5]);
}

/// The minimal script shape with the
/// gossip prototypes and a DRAW-verb; seed 0 keeps the pinned order
/// RUMOUR, HEARSAY, RUMOUR, SCANDAL (the permutation vectors above).
fn deck_rig(decks: &str) -> String {
    format!(
        r#"script: Probe

prototypes:
  RUMOUR is idea stackable
  HEARSAY is idea stackable
  SCANDAL is idea stackable

entities:
  ANA is character

decks:
{decks}

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> DRAW RUMOURS
"#
    )
}

#[test]
fn compiled_deck_draws_obey_the_three_rules_below_period_reset_wrap_defaultcard_forever() {
    // Through the full script compiler, plus the
    // defaultcard-forever leg. No uniques anywhere, so the manifested set is
    // empty and the serving rule is the raw position face.
    let dict = core_dictionary();
    let game = compile_script(
        &deck_rig("  RUMOURS spec RUMOUR, RUMOUR, HEARSAY, SCANDAL defaultcard RUMOUR reset\n  SOLO spec HEARSAY reset\n  GOSSIP spec RUMOUR, HEARSAY, SCANDAL defaultcard HEARSAY\n  SINGLES spec HEARSAY"),
        &dict,
    )
    .unwrap();
    let none = indexmap::IndexSet::new();
    let serve = |deck: &chorus::content::CompiledDeck, cursor: i64| {
        chorus::content::draw_from(&game.content, deck, cursor, &none).unwrap()
    };
    let rumours = game.content.decks.get("RUMOURS").unwrap();
    assert_eq!(serve(rumours, 0).0.name, "RUMOUR");
    assert_eq!(serve(rumours, 3).0.name, "SCANDAL");
    assert_eq!(serve(rumours, 4).0.name, "RUMOUR"); // reset wraps: period 4
    assert_eq!(serve(rumours, 9).0.name, "HEARSAY"); // 9 % 4 = 1
    let solo = game.content.decks.get("SOLO").unwrap();
    assert_eq!(serve(solo, 17).0.name, "HEARSAY");
    // defaultcard forever: past the spec every draw serves the default
    // (GOSSIP's seed-0 permutation is the identity [0,1,2], pinned here so
    // draw 0 below is the in-period first spec entry)
    let gossip = game.content.decks.get("GOSSIP").unwrap();
    assert_eq!(serve(gossip, 0).0.name, "RUMOUR"); // in-period
    for cursor in [3, 10, 99] {
        assert_eq!(serve(gossip, cursor).0.name, "HEARSAY");
    }
    // neither: the verbatim exhaustion error
    let singles = game.content.decks.get("SINGLES").unwrap();
    let err = chorus::content::draw_from(&game.content, singles, 1, &none).unwrap_err();
    assert_eq!(err.msg, "deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard' (§4.4)");
}

// ---------------------------------------------------------------------------
// the permutation (§4.4): pure in (seed, deck id), positions shuffled
// ---------------------------------------------------------------------------

#[test]
fn same_seed_and_deck_id_the_identical_draw_sequence_across_two_fresh_compiles() {
    let dict = deck_dict();
    let text = rig(Rig { seed: 0, decks: Some(PLAN_DECK), ..Default::default() });
    let (ca, sa) = compile(&dict, &text);
    let (cb, sb) = compile(&dict, &text);
    let (ra_names, _, _) = play_draws(&ca, &dict, sa, 6);
    let (rb_names, _, _) = play_draws(&cb, &dict, sb, 6);
    assert_eq!(ra_names, rb_names);
    assert_eq!(&ra_names[..4], ["RUMOUR", "HEARSAY", "RUMOUR", "SCANDAL"]); // seed 0 order, pinned
}

/// (the vector half lives in
/// permutation_is_pure_in_seed_and_deck_id_with_position_vectors above —
/// same scenario; the compile-side half is here)
#[test]
fn different_seed_different_order_seed_pair_0_vs_42_provably_differs() {
    let dict = deck_dict();
    let (ca, sa) =
        compile(&dict, &rig(Rig { seed: 0, decks: Some(PLAN_DECK), ..Default::default() }));
    let (cb, sb) =
        compile(&dict, &rig(Rig { seed: 42, decks: Some(PLAN_DECK), ..Default::default() }));
    let (ra, _, _) = play_draws(&ca, &dict, sa, 4);
    let (rb, _, _) = play_draws(&cb, &dict, sb, 4);
    assert_eq!(ra, ["RUMOUR", "HEARSAY", "RUMOUR", "SCANDAL"]); // seed 0
    assert_eq!(rb, ["SCANDAL", "RUMOUR", "RUMOUR", "HEARSAY"]); // seed 42
    assert_ne!(ra, rb);
}

#[test]
fn multiset_weighting_intact_a_a_b_serves_a_twice_per_period() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some("WEIGHTED spec RUMOUR, RUMOUR, SCANDAL defaultcard SCANDAL reset"),
            draw_deck: Some("WEIGHTED"),
            ..Default::default()
        }),
    );
    let (mut names, _, _) = play_draws(&c, &dict, s, 3);
    names.sort();
    assert_eq!(names, ["RUMOUR", "RUMOUR", "SCANDAL"]); // 2:1 weighting preserved
}

#[test]
fn the_permutation_lives_on_compiledcontent_not_state() {
    let dict = deck_dict();
    let (c, s) = compile(&dict, &rig(Rig { decks: Some(PLAN_DECK), ..Default::default() }));
    let order: Vec<&str> =
        c.decks.get("RUMOURS").unwrap().order.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(order, ["RUMOUR", "HEARSAY", "RUMOUR", "SCANDAL"]);
    assert_eq!(s.decks.len(), 0); // cursors only, and none yet
    assert_eq!(s.rng, 0);
    let (_, _, after_state) = play_draws(&c, &dict, s, 4);
    assert_eq!(after_state.rng, 0); // decks never touch the dice counter (§4.4)
}

// ---------------------------------------------------------------------------
// the DRAW op (§4.4): INTO placement and the SPAWN defaults
// ---------------------------------------------------------------------------

#[test]
fn draw_into_a_place_lands_the_card_there() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW RUMOURS INTO KITCHEN",
            ),
            ..Default::default()
        }),
    );
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let rumour = r.state.entities.values().find(|e| e.name == "RUMOUR").unwrap();
    assert_eq!(rumour.attrs.get("container"), Some(&AttrValue::Str("s_kitchen".into())));
}

#[test]
fn no_into_the_at_card_default_wins_over_the_actor_container() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            verbs: Some(
                "  SEEK:\n    actors: 1 kind:character\n    at: 0..1 kind:place\n    duration: 0m\n    recipes:\n      default -> DRAW RUMOURS",
            ),
            ..Default::default()
        }),
    );
    let at_card = settle_line(&c, &dict, s, "0:0 @ ANA AT KITCHEN SEEK", 1).unwrap();
    assert_eq!(at_card.rejected, None);
    let placed = at_card.state.entities.values().find(|e| e.name == "RUMOUR").unwrap();
    assert_eq!(placed.attrs.get("container"), Some(&AttrValue::Str("s_kitchen".into())));
    let placed_container = placed.attrs.get("container").cloned();
    // without AT, the mint goes to the actor's container (ANA is at HOUSE);
    // cursor 1 serves HEARSAY at seed 0 — look up whatever was drawn
    let carried = settle_line(&c, &dict, at_card.state, "0:0 @ ANA SEEK", 2).unwrap();
    assert_eq!(carried.rejected, None);
    let drawn = &drawn_names(&carried)[0];
    let second = carried.state.entities.values().find(|e| &e.name == drawn).unwrap();
    assert_eq!(second.attrs.get("container"), Some(&AttrValue::Str("s_house".into())));
    assert_ne!(second.attrs.get("container"), placed_container.as_ref());
}

#[test]
fn a_trigger_draw_without_into_mints_containerless() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            triggers: Some("rumour_hour at hour 1 -> DRAW RUMOURS"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      default -> NARRATE \"{actors} wait.\"",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap(); // 1h crosses hour 1 (slot 2)
    assert_eq!(r.rejected, None);
    let event = r.records.iter().find(|x| x.kind == "event").unwrap();
    let ops: Vec<&str> = event.outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["DRAWN", "SPAWN"]);
    let rumour = r.state.entities.values().find(|e| e.name == "RUMOUR").unwrap();
    assert_eq!(rumour.attrs.get("container"), None); // the §4.4 empty-bindings note
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn a_trigger_draw_with_an_explicit_into_lands_in_the_named_place() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            triggers: Some("rumour_hour at hour 1 -> DRAW RUMOURS INTO HOUSE"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      default -> NARRATE \"{actors} wait.\"",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let rumour = r.state.entities.values().find(|e| e.name == "RUMOUR").unwrap();
    assert_eq!(rumour.attrs.get("container"), Some(&AttrValue::Str("s_house".into())));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn an_into_that_cannot_resolve_refuses_the_window_cursor_unmoved() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW RUMOURS INTO OBJECT",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap(); // no object bound
    assert_eq!(r.rejected, None);
    assert_refusal(&r, r"INTO target of 'RUMOURS' cannot resolve");
    assert_eq!(r.state.decks.get("RUMOURS"), None); // the scratch died with the refusal
                                                    // §6.4: the window registered at begin — a window that later refuses
                                                    // still counted (the one deliberate delta; the cursor scratch died with
                                                    // it, and play/fold agree on exactly that state)
    assert_eq!(r.state.executed.get("wait.default"), Some(&1));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ---------------------------------------------------------------------------
// DRAWN+SPAWN concretization (§4.4): two rows, round-trip, guards
// ---------------------------------------------------------------------------

#[test]
fn the_record_carries_drawn_then_the_concrete_spawn_and_the_row_round_trips() {
    let dict = deck_dict();
    let (c, s) = compile(&dict, &rig(Rig { decks: Some(PLAN_DECK), ..Default::default() }));
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let rec = done(&r).unwrap();
    let ops: Vec<&str> = rec.outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["DRAWN", "SPAWN"]);
    let drawn = &rec.outcomes[0];
    let tags: Vec<&str> = drawn.args.iter().map(|a| a.tag()).collect();
    assert_eq!(tags, ["ref", "ref"]);
    let (deck_arg, name_arg) = match (&drawn.args[0], &drawn.args[1]) {
        (
            Arg::Ref { r#ref: Ref::Name { name: d, .. } },
            Arg::Ref { r#ref: Ref::Name { name: n, .. } },
        ) => (d, n),
        _ => unreachable!("tags pinned above"),
    };
    assert_eq!(deck_arg, "RUMOURS");
    assert_eq!(name_arg, "RUMOUR"); // seed 0 serves RUMOUR first
    assert!(rec.sentence.contains("DRAWN(RUMOURS, RUMOUR)"), "{}", rec.sentence);
    assert!(rec.sentence.contains("SPAWN RUMOUR"), "{}", rec.sentence);
    let reparsed = parse_sentence(&rec.sentence, &dict).unwrap();
    assert_eq!(format_sentence(&reparsed, &dict).unwrap(), rec.sentence);
}

#[test]
fn the_row_round_trips_for_an_unsafe_card_name_too() {
    // a name failing SAFE_NAME formats re-quoted through format_ref; the DRAWN
    // paren path must accept ref's quoted-string form or the recorded row
    // would be unparseable (law 2)
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            entities: "  X-Y is item stackable at HOUSE", // stackable: the living declaration shares its name with the mint
            decks: Some("PAIR spec X-Y defaultcard X-Y"),
            draw_deck: Some("PAIR"),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let sentence = &done(&r).unwrap().sentence;
    assert!(sentence.contains("DRAWN(PAIR, \"X-Y\")"), "{sentence}");
    let reparsed = parse_sentence(sentence, &dict).unwrap();
    assert_eq!(format_sentence(&reparsed, &dict).unwrap(), *sentence);
    // and the DRAWN assertion reads the same name back at fold
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(replayed.decks.get("PAIR"), Some(&1));
}

#[test]
fn the_drawn_parse_format_entry_parses_pins_its_shape_formats_bare() {
    let dict = deck_dict();
    let parsed = parse_outcomes("DRAWN(RUMOURS, HEARSAY)", &dict, None).unwrap();
    let ops: Vec<&str> = parsed.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["DRAWN"]);
    let tags: Vec<&str> = parsed[0].args.iter().map(|a| a.tag()).collect();
    assert_eq!(tags, ["ref", "ref"]);
    let src = "0:0 @ ANA WAIT -> DRAWN(RUMOURS, HEARSAY)";
    let s = parse_sentence(src, &dict).unwrap();
    assert_eq!(format_sentence(&s, &dict).unwrap(), src);
    assert_err_msg(
        parse_outcomes("DRAWN(RUMOURS)", &dict, None),
        "DRAWN takes exactly two name refs",
    );
    assert_err_msg(
        parse_outcomes("DRAWN(RUMOURS, HEARSAY, SCANDAL)", &dict, None),
        "DRAWN takes exactly two name refs",
    );
    assert_err_msg(parse_outcomes("DRAWN(1, 2)", &dict, None), "DRAWN takes exactly two name refs");
    assert_err_msg(parse_outcomes("DRAWN RUMOURS, HEARSAY", &dict, None), "requires parentheses");
}

/// Authoring DRAWN is a load error (engine-written
/// only, like ROLLED), through recipes, triggers, expanded function bodies
/// and untaken IF branches alike.
#[test]
fn authoring_drawn_is_a_load_error_engine_written_only() {
    let dict = deck_dict();
    let recipe_raw = || ContentRaw {
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
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["DRAWN(RUMOURS, RUMOUR)".to_string()],
                }],
                ambient: false,
            },
        )]
        .into_iter()
        .collect(),
        decks: Some(vec![DeckRaw {
            id: "RUMOURS".into(),
            spec: vec!["RUMOUR".into()],
            ..Default::default()
        }]),
        prototypes: Some(
            [("RUMOUR".to_string(), PrototypeDef { kind: "idea".into(), ..Default::default() })]
                .into_iter()
                .collect(),
        ),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&recipe_raw(), &dict, &[]),
        "recipe wait\\.default: DRAWN is engine-written",
    );

    let trigger_raw = ContentRaw {
        triggers: Some(vec![TriggerDef::Slot(SlotTriggerDef {
            id: "t".into(),
            slot: 3,
            once: None,
            out: vec!["DRAWN(RUMOURS, RUMOUR)".to_string()],
        })]),
        decks: Some(vec![DeckRaw {
            id: "RUMOURS".into(),
            spec: vec!["RUMOUR".into()],
            ..Default::default()
        }]),
        prototypes: Some(
            [("RUMOUR".to_string(), PrototypeDef { kind: "idea".into(), ..Default::default() })]
                .into_iter()
                .collect(),
        ),
        ..ContentRaw::default()
    };
    assert_err_msg(compile_content(&trigger_raw, &dict, &[]), "trigger t: DRAWN is engine-written");

    // inside a function body: expansion runs before the lint, so the call
    // site's compile names the recipe
    let fn_raw = ContentRaw {
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
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["helper()".to_string()],
                }],
                ambient: false,
            },
        )]
        .into_iter()
        .collect(),
        functions: Some(vec![chorus::content::FunctionDef {
            name: "helper".into(),
            params: vec![],
            body: "DRAWN(RUMOURS, RUMOUR)".into(),
        }]),
        decks: Some(vec![DeckRaw {
            id: "RUMOURS".into(),
            spec: vec!["RUMOUR".into()],
            ..Default::default()
        }]),
        prototypes: Some(
            [("RUMOUR".to_string(), PrototypeDef { kind: "idea".into(), ..Default::default() })]
                .into_iter()
                .collect(),
        ),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&fn_raw, &dict, &[]),
        "recipe wait\\.default: DRAWN is engine-written",
    );

    // and a DRAWN hidden in an untaken IF branch is still a load error
    let if_raw = ContentRaw {
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
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: vec!["IF exists(NOSUCH) -> (DRAWN(RUMOURS, RUMOUR))".to_string()],
                }],
                ambient: false,
            },
        )]
        .into_iter()
        .collect(),
        decks: Some(vec![DeckRaw {
            id: "RUMOURS".into(),
            spec: vec!["RUMOUR".into()],
            ..Default::default()
        }]),
        prototypes: Some(
            [("RUMOUR".to_string(), PrototypeDef { kind: "idea".into(), ..Default::default() })]
                .into_iter()
                .collect(),
        ),
        ..ContentRaw::default()
    };
    assert_err_msg(compile_content(&if_raw, &dict, &[]), "DRAWN is engine-written");
}

/// the WAIT-verb base raw
fn draw_base_raw(out: &[&str]) -> ContentRaw {
    ContentRaw {
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
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: None,
                    out: out.iter().map(|s| (*s).to_string()).collect(),
                }],
                ambient: false,
            },
        )]
        .into_iter()
        .collect(),
        prototypes: Some(
            [("RUMOUR".to_string(), PrototypeDef { kind: "idea".into(), ..Default::default() })]
                .into_iter()
                .collect(),
        ),
        decks: Some(vec![DeckRaw {
            id: "RUMOURS".into(),
            spec: vec!["RUMOUR".into()],
            ..Default::default()
        }]),
        ..ContentRaw::default()
    }
}

#[test]
fn draw_without_a_deck_id_with_parens_or_naming_an_unknown_deck_is_a_load_error() {
    let dict = deck_dict();
    assert_err_msg(compile_content(&draw_base_raw(&["DRAW"]), &dict, &[]), "DRAW needs a deck id");
    assert_err_msg(parse_outcomes("DRAW(RUMOURS)", &dict, None), "paren-less form");
    assert_err_msg(
        compile_content(&draw_base_raw(&["DRAW NOSUCH"]), &dict, &[]),
        "recipe wait\\.default: DRAW 'NOSUCH' is not a declared deck",
    );
    assert_err_msg(
        compile_content(&draw_base_raw(&["IF exists(HOUSE) -> (DRAW NOSUCH)"]), &dict, &[]),
        "DRAW 'NOSUCH' is not a declared deck", // recurses into branches like check_named_ref
    );
    let trigger_raw = ContentRaw {
        triggers: Some(vec![TriggerDef::Slot(SlotTriggerDef {
            id: "t".into(),
            slot: 3,
            once: None,
            out: vec!["DRAW NOSUCH".to_string()],
        })]),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&trigger_raw, &dict, &[]),
        "trigger t: DRAW 'NOSUCH' is not a declared deck",
    );
}

#[test]
fn draw_and_drawn_join_the_reserved_words_and_a_function_cannot_take_their_names() {
    let dict = deck_dict();
    assert!(dict.reserved.contains("DRAW"));
    assert!(dict.reserved.contains("DRAWN"));
    assert_err_msg(parse_sentence("0:0 @ ANA WAIT DRAW", &dict), "reserved"); // quote it instead
    assert_eq!(parse_outcomes("SPAWN \"DRAW\"", &dict, None).unwrap().len(), 1); // the quote is the escape hatch
    for name in ["DRAW", "DRAWN"] {
        let raw = ContentRaw {
            functions: Some(vec![chorus::content::FunctionDef {
                name: name.into(),
                params: vec![],
                body: "NARRATE \"x\"".into(),
            }]),
            ..ContentRaw::default()
        };
        assert_err_msg(compile_content(&raw, &dict, &[]), "collides with an outcome op");
    }
}

// ---------------------------------------------------------------------------
// exhaustion (§4.4): reset wraps, defaultcard forever, neither rejects
// ---------------------------------------------------------------------------

#[test]
fn reset_wraps_the_sequence_is_periodic_with_period_len_spec() {
    let dict = deck_dict();
    let (c, s) = compile(&dict, &rig(Rig { decks: Some(PLAN_DECK), ..Default::default() }));
    let (names, _, _) = play_draws(&c, &dict, s, 5);
    assert_eq!(names, ["RUMOUR", "HEARSAY", "RUMOUR", "SCANDAL", "RUMOUR"]); // draw 5 == draw 1
}

#[test]
fn defaultcard_forever_past_the_spec_every_draw_serves_the_default() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some("GOSSIP spec RUMOUR, HEARSAY, SCANDAL defaultcard HEARSAY"),
            draw_deck: Some("GOSSIP"),
            ..Default::default()
        }),
    );
    let (names, _, _) = play_draws(&c, &dict, s, 5);
    assert_eq!(names, ["RUMOUR", "HEARSAY", "SCANDAL", "HEARSAY", "HEARSAY"]);
}

#[test]
fn neither_reset_nor_defaultcard_the_draw_fails_nothing_logged_cursor_unmoved() {
    let dict = deck_dict();
    let (c, state) = compile(
        &dict,
        &rig(Rig {
            decks: Some("SINGLES spec HEARSAY, SCANDAL"),
            draw_deck: Some("SINGLES"),
            ..Default::default()
        }),
    );
    let mut cur = state;
    let mut id = 1i64;
    for _ in 0..2 {
        let ok = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
        assert_eq!(ok.rejected, None);
        id += ok.records.len() as i64;
        cur = ok.state;
    }
    assert_eq!(cur.decks.get("SINGLES"), Some(&2));
    let r = settle_line(&c, &dict, clone_state(&cur), "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r.rejected, None);
    assert_refusal(&r, "deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard'");
    assert_eq!(r.state.decks.get("SINGLES"), Some(&2)); // cursor unmoved (§9.2 analog)
                                                        // §6.4: the window registered at begin — the completion-time refusal
                                                        // still counted (the one delta; nothing logged but the attempt)
    assert_eq!(r.state.executed.get("wait.default"), Some(&(cur.executed["wait.default"] + 1)));
    let replayed_attempt = fold(&r.records, &cur, &dict, &c).unwrap();
    assert_eq!(snap(&replayed_attempt), snap(&r.state));
    // the fold treats the empty log as the base state: the cursor sits at 2
    let replayed = fold(&[], &cur, &dict, &c).unwrap();
    assert_eq!(replayed.decks.get("SINGLES"), Some(&2));
}

// ---------------------------------------------------------------------------
// cursor reconstruction (law 1): fold === play, decks included
// ---------------------------------------------------------------------------

#[test]
fn a_draw_inside_an_untaken_if_branch_leaves_the_cursor_untouched() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> IF exists(NOSUCH) -> (DRAW RUMOURS) ELSE -> (NARRATE \"quiet\")",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"]); // the taken branch only
    assert_eq!(r.state.decks.len(), 0);
    // §6.4: the window completed — the step counted (the one snap delta;
    // the untaken branch consumed no draw, §6.7)
    assert_eq!(r.state.executed.get("wait.default"), Some(&1));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn a_failure_after_a_successful_draw_refuses_the_window_draw_and_cursor_discarded() {
    // the cursor advance happened on the completion scratch inside DRAW; the
    // later outcome's throw refuses the WINDOW, so neither the mint nor the
    // cursor survives — play and fold agree on the empty decks map
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW RUMOURS, MOVE ANA INTO NOSUCH",
            ),
            ..Default::default()
        }),
    );
    let r = settle_line(&c, &dict, clone_state(&s), "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_refusal(&r, "MOVE: unresolvable destination");
    assert_eq!(r.state.decks.len(), 0);
    // §6.4: the window registered at begin — the completion-time refusal
    // still counted (the one delta; neither the mint nor the cursor survived)
    assert_eq!(r.state.executed.get("wait.default"), Some(&1));
    let replayed = fold(&r.records, &s, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state)); // play and fold agree on it
    assert_eq!(replayed.decks.len(), 0);
}

#[test]
fn undo_past_a_draw_rebuilds_the_cursor_from_the_fold() {
    let dict = core_dictionary();
    let mut game =
        Game::from_script(&rig(Rig { decks: Some(PLAN_DECK), ..Default::default() }), dict)
            .unwrap();
    game.handle_line("wait").unwrap();
    game.wait().unwrap();
    game.handle_line("wait").unwrap();
    game.wait().unwrap();
    assert_eq!(game.state.decks.get("RUMOURS"), Some(&2));
    // `.filter(...).at(-1)` — the last DRAWN-bearing record
    let before_undo = game
        .log
        .records
        .iter()
        .rev()
        .find(|r| r.sentence.contains("DRAWN"))
        .unwrap()
        .sentence
        .clone(); // draw #2 (seed 0: HEARSAY)
                  // drop the second draw turn (its settle + its begin) — no stale window
    game.undo().unwrap();
    game.undo().unwrap();
    assert_eq!(game.state.decks.get("RUMOURS"), Some(&1)); // folded, not remembered
    assert_eq!(game.journal().join("\n").matches("DRAWN").count(), 1);
    // drawing again replays the SAME card: the cursor was folded back to 1,
    // so draw #2 serves the identical permutation entry, byte for byte
    game.handle_line("wait").unwrap();
    game.wait().unwrap();
    assert_eq!(game.state.decks.get("RUMOURS"), Some(&2));
    let after = game
        .log
        .records
        .iter()
        .rev()
        .find(|r| r.sentence.contains("DRAWN"))
        .unwrap()
        .sentence
        .clone();
    assert_eq!(after, before_undo);
}

#[test]
fn two_identical_compiles_agree_play_to_fold_even_mid_deck() {
    let dict = deck_dict();
    let (ca, sa) =
        compile(&dict, &rig(Rig { seed: 7, decks: Some(PLAN_DECK), ..Default::default() }));
    let (cb, sb) =
        compile(&dict, &rig(Rig { seed: 7, decks: Some(PLAN_DECK), ..Default::default() }));
    let base_a = clone_state(&sa);
    let base_b = clone_state(&sb);
    let (_, records, state) = play_draws(&ca, &dict, sa, 3);
    assert_eq!(snap(&fold(&records, &base_a, &dict, &ca).unwrap()), snap(&state));
    // the same log under a second compile of the SAME content: replays identically
    assert_eq!(snap(&fold(&records, &base_b, &dict, &cb).unwrap()), snap(&state));
}

#[test]
fn jsonl_fold_equality_with_the_deck_wrapped_and_with_defaultcard_draws_past_the_period() {
    // wrapped: the PLAN deck resets — 9 draws land the cursor at 9 (two full
    // periods of 4 plus one), so the fold must rebuild a past-the-period cursor
    let mut wrapped = Game::from_script(
        &rig(Rig { decks: Some(PLAN_DECK), ..Default::default() }),
        core_dictionary(),
    )
    .unwrap();
    for _ in 0..9 {
        assert_eq!(wrapped.handle_line("wait").unwrap().kind, TurnKind::Action);
        wrapped.wait().unwrap();
    }
    assert_eq!(wrapped.state.decks.get("RUMOURS"), Some(&9));
    // exhausted+defaultcard: a period-3 deck with no reset — 5 draws land 2
    // past the end, both served by the defaultcard
    let mut defaulted = Game::from_script(
        &rig(Rig {
            decks: Some("GOSSIP spec RUMOUR, HEARSAY, SCANDAL defaultcard HEARSAY"),
            draw_deck: Some("GOSSIP"),
            ..Default::default()
        }),
        core_dictionary(),
    )
    .unwrap();
    for _ in 0..5 {
        assert_eq!(defaulted.handle_line("wait").unwrap().kind, TurnKind::Action);
        defaulted.wait().unwrap();
    }
    assert_eq!(defaulted.state.decks.get("GOSSIP"), Some(&5));
    for (tag, game) in [("wrapped", &wrapped), ("defaulted", &defaulted)] {
        let path = tmp_log_path(&format!("decks-{tag}"));
        save_log(&path, &game.log.records).unwrap();
        let replayed = fold(
            &load_log(&path).unwrap(),
            &clone_state(&game.log.initial_state),
            &game.dict,
            &game.content,
        )
        .unwrap();
        assert_eq!(snap(&replayed), snap(&game.state));
        let _ = std::fs::remove_file(&path);
    }
}

// ---------------------------------------------------------------------------
// decks compose with stackable pools (§4.2 x §4.4); the livelock rule
// ---------------------------------------------------------------------------

#[test]
fn stackable_deck_entries_pool_two_rumour_draws_a_pool_of_two_living_copies() {
    let dict = deck_dict();
    let (c, s) = compile(&dict, &rig(Rig { decks: Some(PLAN_DECK), ..Default::default() }));
    let (_, _, state) = play_draws(&c, &dict, s, 3); // seed 0: RUMOUR, HEARSAY, RUMOUR
    let pool = state.by_name.get("RUMOUR").unwrap();
    assert_eq!(pool.len(), 2);
    for id in pool {
        let e = state.entities.get(id).unwrap();
        assert_eq!(e.dead_action, None);
        assert!(e.stackable);
    }
    assert_eq!(state.by_name.get("HEARSAY").unwrap().len(), 1);
}

#[test]
fn non_stackable_livelock_a_redraw_of_a_living_card_rejects_identically_forever() {
    // the authoring rule (§10): deck entries should be STACKABLE, or be
    // consumed/renamed on the same step — a non-stackable entry whose previous
    // copy still lives makes every subsequent serve of that entry fail
    // identically (cursor never advances on rejection), pinning the deck shut
    let dict = deck_dict();
    let (c, state) = compile(
        &dict,
        &rig(Rig {
            prototypes: Some("  WHISPER is idea"),
            decks: Some("PAIR spec WHISPER, WHISPER reset"),
            draw_deck: Some("PAIR"),
            ..Default::default()
        }),
    );
    let first = settle_line(&c, &dict, state, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(first.rejected, None);
    let cur = first.state;
    assert_eq!(cur.decks.get("PAIR"), Some(&1));
    for _ in 0..3 {
        let r = settle_line(&c, &dict, clone_state(&cur), "0:0 @ ANA WAIT", 2).unwrap();
        assert_eq!(r.rejected, None);
        assert_eq!(refusal(&r).as_deref(), Some("SPAWN: name 'WHISPER' already in use")); // the documented stuck-state message
        assert_eq!(r.state.decks.get("PAIR"), Some(&1)); // stuck forever
                                                         // §6.4: every attempt registers its window (the count climbs by one,
                                                         // identically each retry) while the deck stays pinned — the stuck
                                                         // state is otherwise the same, and play/fold agree on it
        assert_eq!(r.state.executed.get("wait.default"), Some(&(cur.executed["wait.default"] + 1)));
        let replayed = fold(&r.records, &cur, &dict, &c).unwrap();
        assert_eq!(snap(&replayed), snap(&r.state));
    }
}

#[test]
fn the_documented_escape_consuming_the_drawn_copy_on_the_same_step_lets_every_serve_through() {
    // the livelock rule's other half — a NON-stackable entry is fine when the
    // same step destroys/renames the previous copy; draw #2 must succeed on a
    // freed name and the deck must keep cycling through draws 3 and 4
    let dict = deck_dict();
    let (c, state) = compile(
        &dict,
        &rig(Rig {
            prototypes: Some("  WHISPER is idea"),
            decks: Some("PAIR spec WHISPER, WHISPER reset"),
            draw_deck: Some("PAIR"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW PAIR, DESTROY WHISPER",
            ),
            ..Default::default()
        }),
    );
    let mut cur = state;
    let mut id = 1i64;
    let mut records: Vec<ActionRecord> = Vec::new();
    let base = clone_state(&cur);
    for _ in 0..4 {
        let r = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
        assert_eq!(r.rejected, None); // no livelock: the name frees every period
        id += r.records.len() as i64;
        records.extend(r.records);
        cur = r.state;
    }
    assert_eq!(cur.decks.get("PAIR"), Some(&4)); // the cursor advanced every draw
    assert_eq!(
        cur.entities.values().filter(|e| e.name == "WHISPER" && e.dead_action.is_none()).count(),
        0
    );
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&cur));
}

// ---------------------------------------------------------------------------
// load-time deck validation (§10, §4.4)
// ---------------------------------------------------------------------------

#[test]
fn an_unknown_spec_entry_is_a_load_error_naming_the_deck_and_the_entry() {
    let dict = deck_dict();
    let err = compile_script(
        &rig(Rig { decks: Some("RUMOURS spec RUMOUR, NOSUCH"), ..Default::default() }),
        &dict,
    )
    .unwrap_err();
    let re = regex::Regex::new(
        "deck RUMOURS: spec entry 'NOSUCH' is neither a declared prototype nor an entity",
    )
    .unwrap();
    assert!(re.is_match(&err.msg), "{err}");
    // declared entity lines count as resolutions too (compile_script passes them)
    let ok = compile_script(
        &rig(Rig {
            entities: "  OLD_MAP is item at HOUSE",
            decks: Some("RELICS spec OLD_MAP"),
            draw_deck: Some("RELICS"),
            ..Default::default()
        }),
        &dict,
    );
    assert!(ok.is_ok());
}

#[test]
fn an_unknown_defaultcard_is_a_load_error_naming_the_deck_and_the_entry() {
    let dict = deck_dict();
    let err = compile_script(
        &rig(Rig { decks: Some("RUMOURS spec RUMOUR defaultcard NOSUCH"), ..Default::default() }),
        &dict,
    )
    .unwrap_err();
    let re = regex::Regex::new(
        "deck RUMOURS: defaultcard 'NOSUCH' is neither a declared prototype nor an entity",
    )
    .unwrap();
    assert!(re.is_match(&err.msg), "{err}");
}

#[test]
fn an_empty_spec_is_a_load_error_at_parse_and_at_compile() {
    let dict = deck_dict();
    let err = compile_script(&rig(Rig { decks: Some("EMPTY spec"), ..Default::default() }), &dict)
        .unwrap_err();
    let re = regex::Regex::new("deck EMPTY: the spec is empty").unwrap();
    assert!(re.is_match(&err.msg), "{err}");
    let raw = ContentRaw {
        decks: Some(vec![DeckRaw { id: "EMPTY".into(), spec: vec![], ..Default::default() }]),
        ..ContentRaw::default()
    };
    assert_err_msg(compile_content(&raw, &dict, &[]), "deck EMPTY: the spec is empty");
}

#[test]
fn deck_ids_must_be_upper_snake_unreserved_and_unique() {
    let dict = deck_dict();
    let err = compile_script(
        &rig(Rig { decks: Some("gossip spec RUMOUR"), ..Default::default() }),
        &dict,
    )
    .unwrap_err();
    let re = regex::Regex::new("deck 'gossip' must be UPPER_SNAKE").unwrap();
    assert!(re.is_match(&err.msg), "{err}");

    let err =
        compile_script(&rig(Rig { decks: Some("DRAW spec RUMOUR"), ..Default::default() }), &dict)
            .unwrap_err();
    let re = regex::Regex::new("'DRAW' is reserved and cannot be a deck id").unwrap();
    assert!(re.is_match(&err.msg), "{err}");

    let err = compile_script(
        &rig(Rig {
            decks: Some("RUMOURS spec RUMOUR\nRUMOURS spec HEARSAY"),
            ..Default::default()
        }),
        &dict,
    )
    .unwrap_err();
    let re = regex::Regex::new("duplicate deck 'RUMOURS'").unwrap();
    assert!(re.is_match(&err.msg), "{err}");
}

// ---------------------------------------------------------------------------
// DRAW inside function bodies (§10 x §4.4)
// ---------------------------------------------------------------------------

#[test]
fn a_param_as_the_into_target_expands_from_a_recipe_call() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            functions: Some("drop(target) -> DRAW RUMOURS INTO target"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> drop(KITCHEN)",
            ),
            ..Default::default()
        }),
    );
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let rumour = r.state.entities.values().find(|e| e.name == "RUMOUR").unwrap();
    assert_eq!(rumour.attrs.get("container"), Some(&AttrValue::Str("s_kitchen".into())));
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["DRAWN", "SPAWN"]);
}

#[test]
fn the_same_function_from_a_trigger_explicit_into_lands_and_replays() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            functions: Some("drop(target) -> DRAW RUMOURS INTO target"),
            triggers: Some("rumour_fn at hour 1 -> drop(HOUSE)"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      default -> NARRATE \"{actors} wait.\"",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let rumour = r.state.entities.values().find(|e| e.name == "RUMOUR").unwrap();
    assert_eq!(rumour.attrs.get("container"), Some(&AttrValue::Str("s_house".into())));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn a_param_as_the_deck_id_expands_and_passes_the_deck_check_at_the_call_site() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(&format!("{PLAN_DECK}\n  QUIET spec HEARSAY")),
            functions: Some("pull(deck) -> DRAW deck"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> pull(QUIET)",
            ),
            ..Default::default()
        }),
    );
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(drawn_names(&r), ["HEARSAY"]);
    assert_eq!(r.state.decks.get("QUIET"), Some(&1));
}

#[test]
fn a_role_into_expanded_into_a_trigger_rejects_at_runtime() {
    // guards stay load-time for the DECK id only; the INTO role resolves at
    // runtime like every ref position — and in a trigger nothing binds, so
    // the draw refuses to materialize nowhere (§4.4's stricter INTO)
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            functions: Some("drop(target) -> DRAW RUMOURS INTO target"),
            triggers: Some("rumour_fn at hour 1 -> drop(OBJECT)"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      default -> NARRATE \"{actors} wait.\"",
            ),
            ..Default::default()
        }),
    );
    let before = snap(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_rejected(&r, "INTO target of 'RUMOURS' cannot resolve");
    assert_eq!(r.records.len(), 0); // §9.2: the whole step rejects
                                    // `state === before` (identity): on a rejection the input state's value
                                    // rides back untouched (identity → snap equality)
    assert_eq!(snap(&r.state), before);
}

#[test]
fn a_function_body_drawing_with_progress_after_it_still_trips_the_progress_guard() {
    // the trigger PROGRESS/REFUSE guards walk the EXPANDED list — a DRAW in
    // the body cannot hide a later PROGRESS from them
    let dict = deck_dict();
    let err = compile_script(
        &rig(Rig {
            decks: Some(PLAN_DECK),
            functions: Some("sneaky() -> DRAW RUMOURS, PROGRESS(30m, \"tick\")"),
            triggers: Some("sneaky_hour at hour 1 -> sneaky()"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      default -> NARRATE \"{actors} wait.\"",
            ),
            ..Default::default()
        }),
        &dict,
    )
    .unwrap_err();
    let re = regex::Regex::new("trigger sneaky_hour: PROGRESS belongs to verb recipes").unwrap();
    assert!(re.is_match(&err.msg), "{err}");
}

// ---------------------------------------------------------------------------
// content drift (§4.4): the DRAWN assertion fires instead of silent divergence
// ---------------------------------------------------------------------------

fn drift_raw(seed: i64, spec: &[&str]) -> ContentRaw {
    ContentRaw {
        seed: Some(seed),
        verbs: [(
            "WAIT".to_string(),
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
                    dur: None,
                    out: vec!["DRAW RUMOURS".to_string()],
                }],
                ambient: false,
            },
        )]
        .into_iter()
        .collect(),
        prototypes: Some(
            [("RUMOUR", true), ("HEARSAY", true), ("SCANDAL", true)]
                .into_iter()
                .map(|(n, st)| {
                    (
                        n.to_string(),
                        PrototypeDef { kind: "idea".into(), stackable: st, ..Default::default() },
                    )
                })
                .collect(),
        ),
        decks: Some(vec![DeckRaw {
            id: "RUMOURS".into(),
            spec: spec.iter().map(|s| (*s).to_string()).collect(),
            defaultcard: Some("RUMOUR".into()),
            reset: true,
        }]),
        ..ContentRaw::default()
    }
}

/// ANA carries container 'house' from
/// the seed world.
fn drift_world() -> State {
    seed_state()
}

#[test]
fn a_seed_change_under_an_old_log_the_assertion_names_the_deck_cursor_both_names() {
    // §9.2 asymmetry, decks edition: a seed change shifts every FUTURE draw —
    // and where the dice replay past rows from the log alone, a deck's cursor
    // row must re-derive its expectation, so the drifted PAST fails loud
    let dict = deck_dict();
    let played =
        compile_content(&drift_raw(0, &["RUMOUR", "RUMOUR", "HEARSAY", "SCANDAL"]), &dict, &[])
            .unwrap();
    let drifted =
        compile_content(&drift_raw(42, &["RUMOUR", "RUMOUR", "HEARSAY", "SCANDAL"]), &dict, &[])
            .unwrap();
    let played_names: Vec<&str> =
        played.decks.get("RUMOURS").unwrap().order.iter().map(|e| e.name.as_str()).collect();
    let drifted_names: Vec<&str> =
        drifted.decks.get("RUMOURS").unwrap().order.iter().map(|e| e.name.as_str()).collect();
    assert_ne!(played_names, drifted_names);
    let r = settle_line(&played, &dict, drift_world(), "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let err = fold(&r.records, &drift_world(), &dict, &drifted).unwrap_err();
    let re = regex::Regex::new(
        "DRAWN: deck 'RUMOURS' draw #0 recorded 'RUMOUR' but the compiled deck serves",
    )
    .unwrap();
    assert!(re.is_match(&err.msg), "{err}");
}

#[test]
fn a_changed_spec_under_the_same_seed_fires_too_a_removed_deck_says_so() {
    let dict = deck_dict();
    let played =
        compile_content(&drift_raw(0, &["RUMOUR", "RUMOUR", "HEARSAY", "SCANDAL"]), &dict, &[])
            .unwrap();
    let respecced =
        compile_content(&drift_raw(0, &["SCANDAL", "HEARSAY", "RUMOUR", "RUMOUR"]), &dict, &[])
            .unwrap();
    let r = settle_line(&played, &dict, drift_world(), "0:0 @ ANA WAIT", 1).unwrap();
    let err = fold(&r.records, &drift_world(), &dict, &respecced).unwrap_err();
    let re = regex::Regex::new("content drift between play and fold").unwrap();
    assert!(re.is_match(&err.msg), "{err}");
    // a deck that vanished from the compiled content: unreachable through
    // compile_content (the DRAW check pins the id at load), so simulate the
    // drift on the compiled table
    let mut deckless =
        compile_content(&drift_raw(0, &["RUMOUR", "RUMOUR", "HEARSAY", "SCANDAL"]), &dict, &[])
            .unwrap();
    deckless.decks.shift_remove("RUMOURS");
    let err = fold(&r.records, &drift_world(), &dict, &deckless).unwrap_err();
    let re = regex::Regex::new("deck 'RUMOURS' is not in the compiled decks").unwrap();
    assert!(re.is_match(&err.msg), "{err}");
}

#[test]
fn drift_in_the_defaultcard_alone_fires_too_past_the_period_the_assertion_still_re_derives() {
    // a spec of one, no reset: draw #0 is the spec entry, draws #1.. carry the
    // defaultcard — changing ONLY the defaultcard must trip draw #1's assertion
    let dict = deck_dict();
    let raw = |defaultcard: &str| ContentRaw {
        seed: Some(0),
        verbs: [(
            "WAIT".to_string(),
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
                    dur: None,
                    out: vec!["DRAW SMALL".to_string()],
                }],
                ambient: false,
            },
        )]
        .into_iter()
        .collect(),
        prototypes: Some(
            ["RUMOUR", "HEARSAY", "SCANDAL"]
                .into_iter()
                .map(|n| {
                    (
                        n.to_string(),
                        PrototypeDef { kind: "idea".into(), stackable: true, ..Default::default() },
                    )
                })
                .collect(),
        ),
        decks: Some(vec![DeckRaw {
            id: "SMALL".into(),
            spec: vec!["RUMOUR".into()],
            defaultcard: Some(defaultcard.to_string()),
            ..Default::default()
        }]),
        ..ContentRaw::default()
    };
    let played = compile_content(&raw("RUMOUR"), &dict, &[]).unwrap();
    let drifted = compile_content(&raw("SCANDAL"), &dict, &[]).unwrap();
    let mut records: Vec<ActionRecord> = Vec::new();
    let mut cur = drift_world();
    let mut id = 1i64;
    for _ in 0..3 {
        let r = settle_line(&played, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
        assert_eq!(r.rejected, None);
        id += r.records.len() as i64;
        records.extend(r.records);
        cur = r.state;
    }
    // the identical compile replays clean (control — a non-drifted fold never trips)
    let control = compile_content(&raw("RUMOUR"), &dict, &[]).unwrap();
    assert!(fold(&records, &drift_world(), &dict, &control).is_ok());
    // only the defaultcard changed: draw #1 (past the period) is the first to fire
    let err = fold(&records, &drift_world(), &dict, &drifted).unwrap_err();
    let re = regex::Regex::new(
        "DRAWN: deck 'SMALL' draw #1 recorded 'RUMOUR' but the compiled deck serves 'SCANDAL'",
    )
    .unwrap();
    assert!(re.is_match(&err.msg), "{err}");
}

#[test]
fn refuse_after_a_draw_the_draft_dies_the_cursor_never_moves_the_fold_skips_the_rows() {
    let dict = deck_dict();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            decks: Some(PLAN_DECK),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW RUMOURS, REFUSE(\"not this one\")",
            ),
            ..Default::default()
        }),
    );
    let before = snap(&s);
    let r = settle_line(&c, &dict, clone_state(&s), "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["DRAWN", "SPAWN", "REFUSE"]);
    assert_eq!(act(&r).unwrap().advanced_min, 0);
    // `r.state === state`: the draft was discarded — the input state's
    // value rides back untouched (identity → snap equality)
    assert_eq!(snap(&r.state), before);
    assert_eq!(s.decks.get("RUMOURS"), None);
    // the fold treats the refused record as a no-op — the cursor never moves there either
    let replayed = fold(&r.records, &s, &dict, &c).unwrap();
    assert_eq!(replayed.decks.get("RUMOURS"), None);
    assert_eq!(snap(&replayed), before);
}
