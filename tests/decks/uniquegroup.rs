//! Uniquegroup (§4.2, v0.10.1) — the GROUP face of uniqueness, through the
//! full script compiler and the engine:
//!
//! - a member mint SKIPS while ANY member of its group has a LIVING copy
//!   (the v0.9.6 living-check generalized): the concrete SPAWN row still
//!   logs byte-identically, and the fold replays it through the same
//!   group living-check (play == fold, snap);
//! - the manifested GROUP set is PERMANENT and name-independent — one
//!   member's mint kills EVERY member's deck entries forever (the filter
//!   reads the manifested groups, not the living set); a member's name may
//!   mint again when no member lives, its deck entries stay dead;
//! - a later occurrence of any member name is a dead position in every
//!   execution (the earliest serve-or-skip manifests the group first);
//! - `reset`'s reshuffle skips the dead member positions too, and true
//!   exhaustion falls to `defaultcard`/the §9.2 refusal;
//! - a LIVING-BUT-LOCKED member counts as living (a window's lock table
//!   blocks a sibling's mint);
//! - REPLACE's group face: the spawn half skips while any member lives,
//!   the destroy half applies (`REPLACE X WITH X` destroys and mints
//!   nothing, the group manifests);
//! - an unrelated `unique` name and an untouched second group are
//!   unaffected.
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
use chorus::core::types::{clone_state, State};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{execute_step, ActionRecord, ExecOptions, ExecResult, Step};
use chorus::session::game::{Game, TurnKind};

// ---------------------------------------------------------------- rigs ------

/// The uniquegroup rig: ANA at HOUSE, the prototypes block as authored, and
/// a WAIT verb (duration 0m) whose default recipe draws/executes `body`.
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
script: Uniquegroup Rig
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

/// The SPAWN rows of a result, in order (the row's first ref's name).
fn spawn_rows_of(records: &[ActionRecord]) -> Vec<String> {
    records
        .iter()
        .flat_map(|rec| {
            rec.outcomes.iter().filter(|o| o.op == "SPAWN").map(|o| match o.args.first() {
                Some(chorus::core::sentence::Arg::Ref {
                    r#ref: chorus::core::r#ref::Ref::Name { name, .. },
                }) => name.clone(),
                _ => "(?)".to_string(),
            })
        })
        .collect()
}

/// The DRAWN names of a result, in order (the DRAWN row's second arg).
fn drawn_names_of(records: &[ActionRecord]) -> Vec<String> {
    records
        .iter()
        .flat_map(|rec| {
            rec.outcomes.iter().filter(|o| o.op == "DRAWN").map(|o| match o.args.get(1) {
                Some(chorus::core::sentence::Arg::Ref {
                    r#ref: chorus::core::r#ref::Ref::Name { name, .. },
                }) => name.clone(),
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
fn a_member_mint_skips_while_a_sibling_lives_and_mints_once_it_dies() {
    // T-G1: GA mints; GB's mint SKIPS while GA lives (the row still logs);
    // GA destroyed; GB mints — no member lives any more
    let mut dict = core_dictionary();
    dict.register_verb("TOSS").unwrap();
    dict.register_verb("MINT2").unwrap();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1\n  GB is idea uniquegroup G1",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN GA, SPAWN GB\n  TOSS:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DESTROY GA\n  MINT2:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN GB",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    // ONE member living, BOTH rows logged — the skip is a logged row that
    // mints nothing, byte-identical in shape to the minting one
    assert_eq!(spawn_rows_of(&r.records), ["GA", "GB"]);
    assert_eq!(living_named(&r.state, "GA"), 1);
    assert_eq!(living_named(&r.state, "GB"), 0);
    // the minted member is not stackable (a group member is never
    // stackable, §4.2) and the GROUP recorded — not the name
    let ga = r.state.entities.values().find(|e| e.name == "GA").unwrap();
    assert!(!ga.stackable);
    assert_eq!(r.state.manifested_groups, indexmap::IndexSet::from(["G1".to_string()]));
    assert!(r.state.manifested_uniques.is_empty());
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    // destroy GA; the sibling's mint lands — the living-check is the
    // GROUP's, not the name's
    let r2 = settle_line(&c, &dict, clone_state(&r.state), "0:0 @ ANA TOSS", 2).unwrap();
    assert_eq!(r2.rejected, None);
    let r3 = settle_line(&c, &dict, clone_state(&r2.state), "0:0 @ ANA MINT2", 3).unwrap();
    assert_eq!(r3.rejected, None);
    assert_eq!(living_named(&r3.state, "GA"), 0);
    assert_eq!(living_named(&r3.state, "GB"), 1);
    let mut records = r.records.clone();
    records.extend(r2.records.clone());
    records.extend(r3.records.clone());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r3.state));
}

#[test]
fn a_same_name_repeat_mint_skips_like_a_unique_the_own_name_is_a_member_name() {
    // the member's OWN name is a member name, so a second mint of GA skips
    // under the same group living-check — one shared check, no name face
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN GA, SPAWN GA",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(living_named(&r.state, "GA"), 1);
    assert_eq!(spawn_rows_of(&r.records), ["GA", "GA"]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ------------------------------------------------- the manifested group -----

#[test]
fn the_group_manifests_permanently_and_every_members_deck_position_dies() {
    // T-G2: QUEST (seed 0) is the identity — order [GA, B, GB]. GA's mint
    // manifests G1; BOTH members' positions are dead from then on (B
    // serves, cursor 3); destroying GA and re-minting GA (the freed name)
    // changes nothing — the filter reads the manifested GROUP, not the
    // living set, so the next draw refuses (no reset, no defaultcard)
    let mut dict = core_dictionary();
    dict.register_verb("TOSS").unwrap();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1\n  GB is idea uniquegroup G1\n  B is idea stackable",
            decks: Some("QUEST spec GA, B, GB"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      mint when NOT exists(GA) -> SPAWN GA\n      draw -> DRAW QUEST\n  TOSS:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DESTROY GA",
            ),
            ..Default::default()
        }),
    );
    let mut cur = s;
    let mut id = 1i64;
    let mut records: Vec<ActionRecord> = Vec::new();
    let base = clone_state(&cur);
    // turn 1: mint GA — the group manifests
    let r1 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r1.rejected, None);
    id += r1.records.len() as i64;
    records.extend(r1.records.clone());
    cur = r1.state;
    assert_eq!(cur.manifested_groups, indexmap::IndexSet::from(["G1".to_string()]));
    // turn 2: the draw — both member positions cannot serve: GA's is dead
    // NOW (the group manifested), B serves at position 1, and the cursor
    // lands past it (GB's dead position is consumed on the NEXT draw)
    let r2 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(drawn_names_of(&r2.records), ["B"]);
    assert_eq!(r2.state.decks.get("QUEST"), Some(&2));
    id += r2.records.len() as i64;
    records.extend(r2.records.clone());
    cur = r2.state;
    // turn 3: destroy GA; turn 4: re-mint it (the name freed)
    let r3 = settle_line(&c, &dict, cur, "0:0 @ ANA TOSS", id).unwrap(); // TOSS
    assert_eq!(r3.rejected, None);
    id += r3.records.len() as i64;
    records.extend(r3.records.clone());
    cur = r3.state;
    assert_eq!(living_named(&cur, "GA"), 0);
    let r4 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap(); // mint
    assert_eq!(r4.rejected, None);
    id += r4.records.len() as i64;
    records.extend(r4.records.clone());
    cur = r4.state;
    assert_eq!(living_named(&cur, "GA"), 1);
    // turn 5: the draw STILL never serves a member — the manifestation
    // outlives the card, so the deck is truly exhausted (§9.2: nothing
    // logged, cursor unmoved)
    let r5 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r5.rejected, None);
    assert!(r5.records.iter().flat_map(|x| x.outcomes.iter()).any(|o| o.op == "REFUSE"
        && match o.args.first() {
            Some(chorus::core::sentence::Arg::Str { v }) =>
                v == "deck 'QUEST' is exhausted — no 'reset' and no 'defaultcard' (§4.4)",
            _ => false,
        }));
    assert_eq!(r5.state.decks.get("QUEST"), Some(&2)); // the cursor unmoved
    records.extend(r5.records.clone());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r5.state));
}

#[test]
fn a_later_member_occurrence_is_dead_in_every_execution_the_first_serve_manifests() {
    // the later-occurrence disjunct lifted to members: SHRINE (seed 0)
    // serves [GA, B, GA] — the second GA position is dead BEFORE any draw
    // (the first GA's serve — or skip — always manifests the group first),
    // so the period is exactly two live serves
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1\n  B is idea stackable",
            decks: Some("SHRINE spec GA, B, GA"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW SHRINE",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let (records, state) = play(&c, &dict, s, 3);
    assert_eq!(drawn_names_of(&records), ["GA", "B"]);
    assert_eq!(living_named(&state, "GA"), 1); // exactly one member, ever
    assert_eq!(state.decks.get("SHRINE"), Some(&3)); // the dead position consumed
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&state));
}

#[test]
fn the_reset_reshuffle_skips_member_positions_and_true_exhaustion_falls_to_the_defaultcard() {
    // T-G5: SHRINE (seed 0) serves spec positions [0, 2, 1] = GA, GA, B.
    // Draw 1 mints GA (manifests the group); draw 2's GA position is dead
    // INSIDE the same draw (manifested group AND a later occurrence) and B
    // is served (cursor 3); draw 3 wraps: the reshuffle skips the dead
    // member positions and serves B again — forever after (the deck's
    // defaultcard face never fires: B is a live entry in every period)
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1\n  B is idea stackable\n  D is idea stackable",
            decks: Some("SHRINE spec GA, B, GA reset defaultcard D"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW SHRINE",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let (records, state) = play(&c, &dict, s, 5);
    assert_eq!(drawn_names_of(&records), ["GA", "B", "B", "B", "B"]);
    assert_eq!(living_named(&state, "GA"), 1); // exactly one member, ever
                                               // the consumed-positions cursor: 1, then 3 (draw 2 consumed the dead GA
                                               // within its serve), then +3 per wrapped draw (two dead GA positions
                                               // consumed inside each reshuffle scan)
    assert_eq!(state.decks.get("SHRINE"), Some(&12));
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&state));
}

#[test]
fn a_deck_of_members_only_exhausts_to_the_defaultcard_face() {
    // every position a member of ONE manifested group: the second draw's
    // period is all-dead (the manifested group kills every member's entry),
    // so the deck is truly exhausted on draw 2 — D serves from there on
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1\n  GB is idea uniquegroup G1\n  D is idea stackable",
            decks: Some("QUEST spec GA, GB defaultcard D reset"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DRAW QUEST",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let (records, state) = play(&c, &dict, s, 4);
    assert_eq!(drawn_names_of(&records), ["GA", "D", "D", "D"]);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&state));
}

// ------------------------------------------------------ the boundaries -----

#[test]
fn an_unrelated_unique_and_a_third_group_are_unaffected() {
    // T-G3: U's manifestation and G2's untouched bookkeeping ride beside
    // G1's — the deck serves the OTHER group's member and the unrelated
    // stackable, never G1's members after G1 manifests
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes:
                "  U is idea unique\n  GA is idea uniquegroup G1\n  GB is idea uniquegroup G1\n  H is idea uniquegroup G2\n  B is idea stackable",
            decks: Some("QUEST spec GA, H, B"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN U, SPAWN GA, DRAW QUEST",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    // U manifests its name; G1 manifests its group; the G2 member's deck
    // position is untouched by G1's manifestation and SERVES (its serve is
    // a member mint, so G2 manifests too — a mint is a mint whatever the
    // site); B's position stays live behind the dead GA
    assert_eq!(r.state.manifested_uniques, indexmap::IndexSet::from(["U".to_string()]));
    assert_eq!(
        r.state.manifested_groups,
        indexmap::IndexSet::from(["G1".to_string(), "G2".to_string()])
    );
    assert_eq!(drawn_names_of(&r.records), ["H"]);
    assert_eq!(r.state.decks.get("QUEST"), Some(&2)); // dead GA consumed within the draw
                                                      // the second draw serves B — G1's manifestation never touched it
    let r2 = settle_line(&c, &dict, clone_state(&r.state), "0:0 @ ANA WAIT", 2).unwrap();
    assert_eq!(r2.rejected, None);
    assert_eq!(drawn_names_of(&r2.records), ["B"]);
    assert_eq!(r2.state.decks.get("QUEST"), Some(&3));
    let mut records = r.records.clone();
    records.extend(r2.records.clone());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
}

#[test]
fn a_locked_member_blocks_a_siblings_mint() {
    // the lock interplay: the GATHER window locks its object (the member GA)
    // for 30m; the WAIT completion inside that window's lifetime skips its
    // SPAWN GB — a locked member IS a living member (the by_name pool only
    // ever holds living ids, §4.2)
    let mut dict = core_dictionary();
    dict.register_verb("GATHER").unwrap();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1\n  GB is idea uniquegroup G1",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      sibling when exists(GA) -> SPAWN GB\n      default -> SPAWN GA\n  GATHER:\n    actors: 1 kind:character\n    object: 1 kind:idea\n    duration: 30m\n    recipes:\n      default -> NARRATE \"{actors} gather {object}.\"",
            ),
            ..Default::default()
        }),
    );
    let mut cur = s;
    let mut id = 1i64;
    let mut records: Vec<ActionRecord> = Vec::new();
    let base = clone_state(&cur);
    // turn 1: mint GA
    let r1 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r1.rejected, None);
    id += r1.records.len() as i64;
    records.extend(r1.records.clone());
    cur = r1.state;
    // turn 2: commit GATHER through the raw exec face — the window opens
    // and locks GA
    let step = parse_sentence("0:0 @ ANA GATHER GA", &dict).unwrap();
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
    assert!(w.locked.iter().any(|id| cur.entities.get(id).is_some_and(|e| e.name == "GA")));
    // turn 3: the sibling's mint while the lock holds — skipped (the row
    // still logs), exactly one member living
    let r3 = settle_line(&c, &dict, cur, "0:0 @ ANA WAIT", id).unwrap();
    assert_eq!(r3.rejected, None);
    records.extend(r3.records.clone());
    cur = r3.state;
    assert_eq!(spawn_rows_of(&r3.records), ["GB"]); // the row still logs
    assert_eq!(living_named(&cur, "GB"), 0);
    assert_eq!(living_named(&cur, "GA"), 1);
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&cur));
}

// ------------------------------------------------------- the REPLACE face ---

#[test]
fn replace_onto_a_living_group_skips_the_spawn_and_still_destroys_the_target() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            entities: "  BEE is item at HOUSE",
            prototypes: "  GA is idea uniquegroup G1\n  GB is idea uniquegroup G1",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN GA, REPLACE BEE WITH GB",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    // the REPLACE's spawn half skipped (the group living through GA), its
    // destroy half applied: BEE dead, exactly one living member (GA)
    assert_eq!(spawn_rows_of(&r.records), ["GA", "GB"]);
    let bee = r.state.entities.values().find(|e| e.name == "BEE").unwrap();
    assert!(bee.dead_action.is_some());
    assert_eq!(living_named(&r.state, "GA"), 1);
    assert_eq!(living_named(&r.state, "GB"), 0);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn replace_x_with_x_on_a_member_destroys_the_incumbent_and_mints_nothing() {
    // the dying member is still living at mint time (its name is a member
    // name in the shared group living-check), so the spawn skips and the
    // destroy pops — pinned: the shared check, no REPLACE special case
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1",
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN GA, REPLACE GA WITH GA",
            ),
            ..Default::default()
        }),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WAIT", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(living_named(&r.state, "GA"), 0);
    // the group's manifestation outlives the card
    assert_eq!(r.state.manifested_groups, indexmap::IndexSet::from(["G1".to_string()]));
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// ------------------------------------------------------- undo and persistence

#[test]
fn undo_across_a_member_mint_returns_the_card_and_rebuilds_the_manifested_groups() {
    // state = fold(log) (§7): undo truncates the log and re-folds, so the
    // minted member is taken back AND its manifested-group marking goes
    // with it — the group is mintable again, and the surviving log's fresh
    // fold IS the state (play == fold after the undo)
    let dict = core_dictionary();
    let mut game = Game::from_script(
        &rig(Rig {
            prototypes: "  GA is idea uniquegroup G1\n  B is idea stackable",
            decks: Some("QUEST spec GA, B"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      mint when NOT exists(GA) -> SPAWN GA\n      default -> DRAW QUEST",
            ),
            ..Default::default()
        }),
        dict,
    )
    .unwrap();
    // mint the member
    assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
    game.wait().unwrap();
    assert_eq!(living_named(&game.state, "GA"), 1);
    assert_eq!(game.state.manifested_groups, indexmap::IndexSet::from(["G1".to_string()]));
    // undo the advance and the mint: the card is gone, the manifestation
    // with it
    game.undo().unwrap();
    game.undo().unwrap();
    assert_eq!(living_named(&game.state, "GA"), 0);
    assert!(game.state.manifested_groups.is_empty());
    let replayed =
        fold(&game.log.records, &clone_state(&game.log.initial_state), &game.dict, &game.content)
            .unwrap();
    assert_eq!(snap(&replayed), snap(&game.state));
    // the mint returns the card — the same turn mints again
    assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
    game.wait().unwrap();
    assert_eq!(living_named(&game.state, "GA"), 1);
    assert_eq!(game.state.manifested_groups, indexmap::IndexSet::from(["G1".to_string()]));
}

#[test]
fn the_fold_rebuilds_both_manifested_sets_and_jsonl_round_trips() {
    // T-G4: a run minting BOTH faces (a unique name and a group member)
    // saves, reloads and re-folds byte-identically — both sets rebuild
    // through the one mint path
    let dict = core_dictionary();
    let mut game = Game::from_script(
        &rig(Rig {
            prototypes:
                "  U is idea unique\n  GA is idea uniquegroup G1\n  GB is idea uniquegroup G1\n  B is idea stackable",
            decks: Some("QUEST spec GA, B, GB reset defaultcard B"),
            verbs: Some(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> SPAWN U, SPAWN GA, DRAW QUEST",
            ),
            ..Default::default()
        }),
        dict,
    )
    .unwrap();
    assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
    game.wait().unwrap();
    assert_eq!(game.state.manifested_uniques, indexmap::IndexSet::from(["U".to_string()]));
    assert_eq!(game.state.manifested_groups, indexmap::IndexSet::from(["G1".to_string()]));
    let path = tmp_log_path("decks-uniquegroup");
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
fn the_uniquegroup_rig_compiles_the_second_time_against_its_own_dictionary() {
    // compile_script is pure; loading the same text twice against one
    // dictionary stays idempotent — with a member group in the mix
    let dict = core_dictionary();
    let text = rig(Rig {
        prototypes:
            "  GA is idea uniquegroup G1\n  GB is idea uniquegroup G1\n  B is idea stackable",
        decks: Some("QUEST spec GA, GB, B"),
        ..Default::default()
    });
    assert!(compile_script(&text, &dict).is_ok());
    assert!(compile_script(&text, &dict).is_ok());
}
