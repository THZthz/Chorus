//! Unique cards (§4.2, v0.9.6) — the mint face, the manifested set and the
//! deck's lazy filter, through the full script compiler and the engine:
//!
//! - a unique mint whose name is living is SKIPPED (CS B9's
//!   retire-the-incumbent is deliberately not copied): the concrete SPAWN
//!   row still logs, byte-identical to a minting one, and the fold replays
//!   it through the same living-check (play == fold, snap);
//! - the manifested set is PERMANENT — a minted-then-destroyed unique still
//!   never serves from a deck again (the filter reads the manifested set,
//!   not the living set);
//! - the deck filter is LAZY at draw: a manifested-unique position (and any
//!   later occurrence of the same unique) is consumed within the serving
//!   draw, `reset`'s reshuffle skips the dead positions too, and true
//!   exhaustion falls to the deck's own `defaultcard`/exhaustion faces;
//! - a LIVING-BUT-LOCKED copy counts as living (a window's lock table
//!   blocks the second mint);
//! - REPLACE's unique face: the spawn half skips, the destroy half applies.
//!
//! Seed 0 permutations pinned from the frozen vectors: QUEST [0,1,2] (the
//! identity), SHRINE [0,2,1].
//!
//! Every test builds a FRESH Dictionary. Error-message patterns are copied
//! CHARACTER FOR CHARACTER from the frozen literals.

use crate::common::rig::{compile, settle_line, tmp_log_path};
use crate::common::snap::snap;
use chorus::content::script::compile_script;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::r#ref::Ref;
use chorus::core::sentence::Arg;
use chorus::core::types::{clone_state, State};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{execute_step, ActionRecord, ExecOptions, ExecResult, Step};
use chorus::session::game::{Game, TurnKind};

// ---------------------------------------------------------------- rigs ------

/// The unique rig: ANA at HOUSE, the prototypes block as authored, and a
/// WAIT verb (duration 0m) whose default recipe draws/executes `body`.
#[derive(Default)]
struct Rig<'a> {
    seed: i64,
    entities: &'a str,
    prototypes: &'a str,
    decks: Option<&'a str>,
    verbs: Option<&'a str>,
}

fn rig(o: Rig) -> String {
    let verbs = o.verbs.unwrap_or(
        "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> NARRATE \"idle.\"",
    );
    let section = |header: &str, body: Option<&str>| match body {
        Some(b) => format!("\n{header}:\n{b}"),
        None => String::new(),
    };
    format!(
        "
script: Unique Rig
player: ANA
start: 0:0
seed: {seed}

entities:
  ANA is character at HOUSE
  HOUSE is place
{entities}

prototypes:
{prototypes}{decks}

verbs:
{verbs}
",
        seed = o.seed,
        entities = o.entities,
        prototypes = o.prototypes,
        decks = section("decks", o.decks),
        verbs = verbs,
    )
}

/// The drawn names of a result, in order (the DRAWN row's second arg).
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

/// The SPAWN rows of a result, in order (the row's first ref's name).
fn spawn_rows_of(records: &[ActionRecord]) -> Vec<String> {
    records
        .iter()
        .flat_map(|rec| {
            rec.outcomes.iter().filter(|o| o.op == "SPAWN").map(|o| match o.args.first() {
                Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => name.clone(),
                _ => "(?)".to_string(),
            })
        })
        .collect()
}

/// Play `n` WAITs, returning the records and the final state.
fn play(
    content: &chorus::content::CompiledContent,
    dict: &Dictionary,
    mut state: State,
    n: usize,
) -> (Vec<ActionRecord>, State) {
    let mut records: Vec<ActionRecord> = Vec::new();
    let mut id = 1i64;
    for _ in 0..n {
        let ExecResult { records: recs, state: st, rejected } =
            settle_line(content, dict, state, "0:0 @ ANA WAIT", id).unwrap();
        if let Some(rej) = rejected {
            panic!("unexpected rejection: {rej}");
        }
        id += recs.len() as i64;
        records.extend(recs);
        state = st;
    }
    (records, state)
}

fn living_named(state: &State, name: &str) -> usize {
    state.by_name.get(name).map(Vec::len).unwrap_or(0)
}

// ----------------------------------------------------------- the mint face --

#[test]
fn a_unique_spawn_mints_once_the_second_row_still_logs_and_the_fold_agrees() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  CHALICE is idea unique",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN CHALICE, SPAWN CHALICE",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    // ONE living copy, BOTH rows logged — the skip is a logged row that
    // mints nothing, byte-identical in shape to the minting one
    assert_eq!(living_named(&r.state, "CHALICE"), 1);
    assert_eq!(spawn_rows_of(&r.records), ["CHALICE", "CHALICE"]);
    // the minted card is not stackable (a unique prototype is never
    // stackable, §4.2)
    let chalice = r.state.entities.values().find(|e| e.name == "CHALICE").unwrap();
    assert!(!chalice.stackable);
    // manifested permanently, and play == fold through the same living-check
    assert_eq!(r.state.manifested_uniques, indexmap::IndexSet::from(["CHALICE".to_string()]));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn a_unique_drawn_twice_from_two_decks_still_yields_one_copy() {
    // the DRAW face: each deck's serve is a SPAWN — the second serve's mint
    // is skipped (RIGHT's defaultcard deals the manifested CHALICE, the mint
    // face deals nothing), the DRAWN rows still log (the cursors moved), and
    // play == fold
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  CHALICE is idea unique",
            decks: Some("LEFT spec CHALICE\nRIGHT spec CHALICE defaultcard CHALICE"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW LEFT, DRAW RIGHT",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(drawn_names_of(&r.records), ["CHALICE", "CHALICE"]);
    assert_eq!(living_named(&r.state, "CHALICE"), 1);
    assert_eq!(r.state.decks.get("LEFT"), Some(&1));
    assert_eq!(r.state.decks.get("RIGHT"), Some(&1));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn replace_onto_a_living_unique_skips_the_spawn_and_still_destroys_the_target() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            entities: "  BEE is item at HOUSE",
            prototypes: "  CHALICE is idea unique",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN CHALICE, REPLACE BEE WITH CHALICE",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    // the REPLACE's spawn half skipped (CHALICE living), its destroy half
    // applied: BEE dead, exactly one living CHALICE, four rows logged
    // (SPAWN, the REPLACE pair SPAWN+DESTROY)
    assert_eq!(spawn_rows_of(&r.records), ["CHALICE", "CHALICE"]);
    let bee = r.state.entities.values().find(|e| e.name == "BEE").unwrap();
    assert!(bee.dead_action.is_some());
    assert_eq!(living_named(&r.state, "CHALICE"), 1);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn replace_x_with_x_on_a_unique_destroys_the_incumbent_and_mints_nothing() {
    // the dying card is still in the pool at mint time (the shared
    // living-check), so the spawn skips and the destroy pops — pinned: the
    // shared check, no REPLACE special case
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  CHALICE is idea unique",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN CHALICE, REPLACE CHALICE WITH CHALICE",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(living_named(&r.state, "CHALICE"), 0);
    // the manifestation outlives the card
    assert_eq!(r.state.manifested_uniques, indexmap::IndexSet::from(["CHALICE".to_string()]));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ------------------------------------------------- the manifested set -------

#[test]
fn a_destroyed_unique_mints_again_but_never_serves_from_a_deck_again() {
    // QUEST (seed 0) is the identity permutation: U serves before B. After
    // the destroy the NAME is free (the second SPAWN mints a fresh copy),
    // but the deck entry stays dead — the filter reads the manifested set,
    // not the living set
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  U is idea unique\n  B is idea stackable",
            decks: Some("QUEST spec U, B"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      mint when NOT exists(U) -> SPAWN U\n      recycle when exists(U) -> DESTROY U, SPAWN U, DRAW QUEST",
            ),
            ..Default::default()
        }),
    );
    let mut cur = s;
    let mut id = 1i64;
    let mut records: Vec<ActionRecord> = Vec::new();
    let base = clone_state(&cur);
    // turn 1: no U living yet — the mint recipe runs
    let r1 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r1.rejected, None);
    id += r1.records.len() as i64;
    records.extend(r1.records.clone());
    cur = r1.state;
    assert_eq!(living_named(&cur, "U"), 1);
    assert_eq!(cur.decks.get("QUEST"), None);
    // turn 2: destroy it, mint it again (the freed name), draw — the deck
    // skips the manifested U and serves B
    let r2 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r2.rejected, None);
    records.extend(r2.records.clone());
    cur = r2.state;
    assert_eq!(drawn_names_of(&r2.records), ["B"]);
    assert_eq!(living_named(&cur, "U"), 1); // the fresh copy, not the deck's
    assert_eq!(cur.decks.get("QUEST"), Some(&2)); // both positions consumed
    assert_eq!(cur.manifested_uniques, indexmap::IndexSet::from(["U".to_string()]));
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&cur));
}

// ------------------------------------------------- the deck lazy filter -----

#[test]
fn the_deck_filter_skips_manifested_positions_and_the_reset_reshuffle_skips_them_too() {
    // SHRINE (seed 0) serves spec positions [0, 2, 1] = A, A, B. Draw 1
    // mints A (manifests); draw 2's A position is dead INSIDE the same draw
    // and B is served (cursor 3 — both dead positions consumed); draw 3
    // wraps: the reshuffle skips the manifested A's and serves B again.
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  A is idea unique\n  B is idea stackable",
            decks: Some("SHRINE spec A, B, A reset"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW SHRINE",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let (records, state) = play(&c, &dict, s, 5);
    assert_eq!(drawn_names_of(&records), ["A", "B", "B", "B", "B"]);
    assert_eq!(living_named(&state, "A"), 1); // exactly one copy, ever
                                              // the consumed-positions cursor: 1, then 3 (draw 2 consumed the dead A
                                              // within its serve), then +3 per wrapped draw (two dead A positions
                                              // consumed inside each reshuffle scan)
    assert_eq!(state.decks.get("SHRINE"), Some(&12));
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&state));
}

#[test]
fn true_exhaustion_by_manifested_uniques_falls_to_the_defaultcard() {
    // QUEST (identity) serves U then U; the second draw's period is
    // all-dead (the manifested unique + the later occurrence), so the deck
    // is truly exhausted on draw 2 — D serves from there on, forever.
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  U is idea unique\n  D is idea stackable",
            decks: Some("QUEST spec U, U defaultcard D reset"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW QUEST",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let (records, state) = play(&c, &dict, s, 4);
    assert_eq!(drawn_names_of(&records), ["U", "D", "D", "D"]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&state));
}

#[test]
fn truly_exhausted_by_uniques_with_neither_reset_nor_defaultcard_the_draw_fails() {
    let dict = core_dictionary();
    let (c, state) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  U is idea unique",
            decks: Some("SOLO spec U"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW SOLO",
            ),
            ..Default::default()
        }),
    );
    let mut cur = state;
    let mut id = 1i64;
    let ok = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(ok.rejected, None);
    id += ok.records.len() as i64;
    cur = ok.state;
    assert_eq!(cur.decks.get("SOLO"), Some(&1));
    let base = clone_state(&cur);
    let r = settle_line(&c, &dict, clone_state(&cur), "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r.rejected, None);
    assert!(r.records.iter().flat_map(|x| x.outcomes.iter()).any(|o| o.op == "REFUSE"
        && match o.args.first() {
            Some(Arg::Str { v }) =>
                v == "deck 'SOLO' is exhausted — no 'reset' and no 'defaultcard' (§4.4)",
            _ => false,
        }));
    assert_eq!(r.state.decks.get("SOLO"), Some(&1)); // the cursor unmoved
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ------------------------------------------------- the lock interplay -------

#[test]
fn a_unique_locked_in_a_window_counts_as_living_the_second_mint_skips() {
    // the GATHER window locks its object (the unique) for 30m; the WAIT
    // completion inside that window's lifetime skips its SPAWN — the
    // by_name pool only ever holds living ids, so a locked copy IS a living
    // copy (§4.2)
    let mut dict = core_dictionary();
    dict.register_verb("GATHER").unwrap();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  CHALICE is idea unique",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN CHALICE\n  GATHER:\n    actors: 1 kind:character\n    object: 1 kind:idea\n    duration: 30m\n    recipes:\n      default -> NARRATE \"{actors} gather {object}.\"",
            ),
            ..Default::default()
        }),
    );
    let mut cur = s;
    let mut id = 1i64;
    let mut records: Vec<ActionRecord> = Vec::new();
    let base = clone_state(&cur);
    // turn 1: mint the unique
    let r1 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r1.rejected, None);
    id += r1.records.len() as i64;
    records.extend(r1.records.clone());
    cur = r1.state;
    // turn 2: commit GATHER through the raw exec face (settle_line would
    // settle the world straight through the 30m window) — the window opens
    // and locks the CHALICE
    let step = parse_sentence("0:0 @ ANA GATHER CHALICE", &dict).unwrap();
    let r2 = execute_step(
        &c,
        &dict,
        cur,
        Step {
            start_min: None,
            clock: Some(step.clock.unwrap_or(chorus::core::clock::Clock { day: 0, tick: 0 })),
            clauses: step.clauses,
            t_real: None,
        },
        ExecOptions { first_record_id: id, id_gen: None },
    )
    .unwrap();
    assert_eq!(r2.rejected, None);
    id += r2.records.len() as i64;
    records.extend(r2.records.clone());
    cur = r2.state;
    assert_eq!(cur.windows.len(), 1);
    let w = cur.windows.values().next().unwrap();
    assert!(w.locked.iter().any(|id| cur.entities.get(id).is_some_and(|e| e.name == "CHALICE")));
    // turn 3: the second mint while the lock holds — the WAIT's outcomes
    // apply at the current clock, BEFORE the settle pass resolves the
    // GATHER window — skipped
    let r3 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r3.rejected, None);
    records.extend(r3.records.clone());
    cur = r3.state;
    assert_eq!(spawn_rows_of(&r3.records), ["CHALICE"]); // the row still logs
    assert_eq!(living_named(&cur, "CHALICE"), 1);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&cur));
}

// ----------------------- the living-check and manifested-set boundaries ----

#[test]
fn a_derive_renamed_mint_off_a_unique_prototype_escapes_the_living_check() {
    // uniqueness attaches to the RESOLVED name: `deriving name from object`
    // lands each mint on a fresh RELIC_<OBJECT> name that no living-check
    // ever sees as a duplicate — two commits mint two living cards — and
    // nothing records in the manifested set (the resolved names are not the
    // declared unique)
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            entities: "  BEE is item at HOUSE\n  FLY is item at HOUSE",
            prototypes: "  RELIC is idea unique deriving name from object",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    object: 1 kind:item\n    duration: 0m\n    recipes:\n      default -> SPAWN RELIC",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r1 = settle_line(&c, &dict, s, "0:0 @ ANA WAIT BEE", 1).unwrap();
    assert_eq!(r1.rejected, None);
    let r2 = settle_line(&c, &dict, r1.state, "0:0 @ ANA WAIT FLY", r1.records.len() as i64 + 1)
        .unwrap();
    assert_eq!(r2.rejected, None);
    // both mints landed — no living-check skip fired, the rows carry the
    // resolved names
    let mut rows = spawn_rows_of(&r1.records);
    rows.extend(spawn_rows_of(&r2.records));
    assert_eq!(rows, ["RELIC_BEE", "RELIC_FLY"]);
    assert_eq!(living_named(&r2.state, "RELIC_BEE"), 1);
    assert_eq!(living_named(&r2.state, "RELIC_FLY"), 1);
    // the escape extends to the manifested set: nothing recorded
    assert!(r2.state.manifested_uniques.is_empty());
    let mut records = r1.records.clone();
    records.extend(r2.records.clone());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
}

#[test]
fn an_initial_entity_s_deck_entry_does_not_skip_on_living_alone_the_serve_refuses_the_window() {
    // the dead test reads the MANIFESTED set (plus later occurrences of
    // DECLARED uniques) — never the living set — and an entity name is in
    // neither: the draw of a deck entry naming a LIVING initial entity
    // serves the position, the DRAW's SPAWN half then hits the §4.2 name
    // gate, and the window refuses at completion (the turn stands, the
    // cursor draft dies with the refusal). Destroyed first, the same entry
    // serves and MINTS a fresh copy: the block was living-alone, and no
    // manifestation is ever recorded for an entity name.
    let mut dict = core_dictionary();
    dict.register_verb("TOSS").unwrap();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            entities: "  U is idea at HOUSE\n  B is idea at HOUSE",
            decks: Some("QUEST spec U, B"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW QUEST\n  TOSS:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DESTROY U",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let mut cur = s;
    let mut id = 1i64;
    let mut records: Vec<ActionRecord> = Vec::new();
    // turn 1: the living initial U blocks — the serve lands on its position
    // (no skip), the DRAW's SPAWN half hits the §4.2 name gate, and the
    // window REFUSES at completion: the turn stands (the logged REFUSE row
    // carries the gate message) and the deck cursor dies with the draft
    let r1 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r1.rejected, None);
    assert!(
        r1.records.iter().flat_map(|x| x.outcomes.iter()).any(|o| o.op == "REFUSE"
            && match o.args.first() {
                Some(Arg::Str { v }) => v == "SPAWN: name 'U' already in use",
                _ => false,
            }),
        "living-alone does not kill the deck position — the serve lands and the mint refuses"
    );
    assert_eq!(r1.state.decks.get("QUEST"), None);
    records.extend(r1.records.clone());
    cur = r1.state;
    // turn 2: destroy the initial copy
    let r2 = settle_line(&c, &dict, cur, "0:0 @ ANA TOSS", id + r1.records.len() as i64).unwrap();
    assert_eq!(r2.rejected, None);
    id += (r1.records.len() + r2.records.len()) as i64;
    records.extend(r2.records.clone());
    cur = r2.state;
    assert_eq!(living_named(&cur, "U"), 0);
    // turn 3: the same entry now serves and MINTS — the position was live
    // all along (no manifested marking exists to skip it)
    let r3 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r3.rejected, None);
    records.extend(r3.records.clone());
    assert_eq!(drawn_names_of(&r3.records), ["U"]);
    assert_eq!(living_named(&r3.state, "U"), 1);
    assert_eq!(r3.state.decks.get("QUEST"), Some(&1));
    assert!(r3.state.manifested_uniques.is_empty());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r3.state));
}

#[test]
fn undo_across_a_unique_mint_returns_the_card_and_rebuilds_the_manifested_set() {
    // state = fold(log) (§7): undo truncates the log and re-folds, so the
    // minted card is taken back AND its manifested-set marking goes with it
    // — the unique name is mintable again, and the surviving log's fresh
    // fold IS the state (play == fold after the undo)
    let dict = core_dictionary();
    let mut game = Game::from_script(
        &rig(Rig {
            prototypes: "  U is idea unique\n  B is idea stackable",
            decks: Some("QUEST spec U, B"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      mint when NOT exists(U) -> SPAWN U\n      default -> DRAW QUEST",
            ),
            ..Default::default()
        }),
        dict,
    )
    .unwrap();
    // mint the unique
    assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
    game.wait().unwrap();
    assert_eq!(living_named(&game.state, "U"), 1);
    assert_eq!(game.state.manifested_uniques, indexmap::IndexSet::from(["U".to_string()]));
    // undo the advance and the mint: the card is gone, the manifestation
    // with it
    game.undo().unwrap();
    game.undo().unwrap();
    assert_eq!(living_named(&game.state, "U"), 0);
    assert!(game.state.manifested_uniques.is_empty());
    let replayed =
        fold(&game.log.records, &clone_state(&game.log.initial_state), &game.dict, &game.content)
            .unwrap();
    assert_eq!(snap(&replayed), snap(&game.state));
    // the mint returns the card — the same turn mints again
    assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
    game.wait().unwrap();
    assert_eq!(living_named(&game.state, "U"), 1);
    assert_eq!(game.state.manifested_uniques, indexmap::IndexSet::from(["U".to_string()]));
}

// ------------------------------ end-to-end: save → load → fold byte-stability

#[test]
fn a_unique_run_saves_and_refolds_identically() {
    let dict = core_dictionary();
    let mut game = Game::from_script(
        &rig(Rig {
            prototypes: "  A is idea unique\n  B is idea stackable",
            decks: Some("SHRINE spec A, B, A reset"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW SHRINE",
            ),
            ..Default::default()
        }),
        dict,
    )
    .unwrap();
    for _ in 0..4 {
        assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
        game.wait().unwrap();
    }
    assert_eq!(game.state.manifested_uniques, indexmap::IndexSet::from(["A".to_string()]));
    let path = tmp_log_path("decks-unique");
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

#[test]
fn the_unique_rig_compiles_the_second_time_against_its_own_dictionary() {
    // compile_script is pure; loading the same text twice against one
    // dictionary stays idempotent — with a unique prototype in the mix
    let dict = core_dictionary();
    let text = rig(Rig {
        prototypes: "  CHALICE is idea unique\n  B is idea stackable",
        decks: Some("QUEST spec CHALICE, B"),
        ..Default::default()
    });
    assert!(compile_script(&text, &dict).is_ok());
    assert!(compile_script(&text, &dict).is_ok());
}
