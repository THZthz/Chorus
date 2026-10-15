//! Nightjar explorer (4 tests). Exploratory story test — plays Murder on the Nightjar Flyer the
//! way no human would. Three passes, all deterministic:
//!
//!   A. sweep      — at every state of the win path (plus states harvested
//!                   from a deterministic sampled walk), execute the
//!                   EXHAUSTIVE verb × role-fill space and check the engine
//!                   invariants on each outcome (§6.5, §6.6, §6.7).
//!   B. walk       — a seeded random walk of hundreds of steps (with
//!                   periodic undo), asserting after every step that the log
//!                   alone reproduces the state (state = fold(log), §7).
//!   C. surface    — the typed front door: slash commands, garbage, and the
//!                   player-typed EVENT guard.
//!
//! Anything here failing means an engine bug, not a content bug: the checks
//! are invariants, not plot. The shared lib lives in
//! tests/common/explorer_lib.rs (explorer_world/candidates_for/
//! WIN_PATH/win_path_states/sampled_walk_states/SCRIPT_TEXT/clause_key).

mod common;

use common::explorer_lib::{
    candidates_for, explorer_world, sampled_walk_states, win_path_states, Depth, ExplorerWorld,
    WIN_PATH,
};
use common::snap::snap;

use chorus::content::content::CompiledContent;
use chorus::core::format::format_sentence;
use chorus::core::parse::parse_sentence;
use chorus::core::r#ref::sort_refs;
use chorus::core::types::{clone_state, State};
use chorus::engine::engine::{execute_step, ActionRecord, ExecOptions, ExecResult, Step};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::session::game::{Game, TurnKind};
use chorus::session::render::prose_lines;

// -------------------------- shared invariants ------------------------------

fn check_state_shape(s: &State, content: &CompiledContent) {
    let mut seen = std::collections::HashSet::new();
    for e in s.entities.values() {
        if e.dead_action.is_some() {
            continue;
        }
        // §9.38: a name the content declares stackable is a pool — duplicate
        // living copies (SUSPICION, the RUMOURS deck's gossip) are legal; the
        // card's own flag must agree with the declaration, or the mint lied
        let declared =
            content.stackable_names.contains(&e.name) || content.stackable_kinds.contains(&e.kind);
        assert_eq!(
            e.stackable, declared,
            "{}: stackable flag disagrees with the declaration",
            e.name
        );
        if !declared {
            assert!(
                !seen.contains(&e.name),
                "duplicate living name {}",
                e.name
            );
        }
        seen.insert(e.name.clone());
        // §9.38: the id is in the name's pool
        assert!(
            s.by_name.get(&e.name).is_some_and(|pool| pool.contains(&e.id)),
            "{} is not in byName's pool for its name",
            e.name
        );
        if let Some(chorus::core::types::AttrValue::Str(c)) = e.attrs.get("container") {
            assert!(
                s.entities.contains_key(c),
                "{} sits nowhere",
                e.name
            );
        }
    }
    for r in s.relations.values() {
        if r.dead_action.is_some() {
            continue;
        }
        assert!(
            s.entities.contains_key(&r.a) && s.entities.contains_key(&r.b),
            "dangling relation {}",
            r.type_
        );
    }
}

fn check_records(records: &[ActionRecord], world: &ExplorerWorld) {
    for rec in records {
        // losslessness: the canonical sentence re-parses and re-formats to
        // itself
        let s = parse_sentence(&rec.sentence, &world.dict)
            .unwrap_or_else(|e| panic!("record sentence does not parse: {}: {e}", rec.sentence));
        assert_eq!(
            format_sentence(&s, Some(&world.dict)).unwrap(),
            rec.sentence
        );
        // prose rendering must never error on a committed record
        prose_lines(rec, &world.content, &world.dict)
            .unwrap_or_else(|e| panic!("prose_lines threw on {}: {e}", rec.sentence));
        assert_eq!(rec.actors, sort_refs(&rec.actors));
        assert!(rec.advanced_min >= 0);
        if rec.kind == "event" {
            assert_eq!(rec.advanced_min, 0);
            assert!(rec.trigger.is_some());
            assert_eq!(rec.verb, "");
        }
    }
}

/// full spec-conformance check of one executed step against its pre-state.
/// Returns "refused" or "applied".
fn check_step(pre: &State, r: &ExecResult, step: &Step, world: &ExplorerWorld) -> &'static str {
    assert_eq!(r.rejected, None, "step was rejected: {:?}", r.rejected);
    assert!(!r.records.is_empty());
    check_records(&r.records, world);
    let last = r.records.last().unwrap();

    // §6.5 v0.5: a refused BEGIN logs the attempt alone and discards the
    // whole turn — its advance phase (if any) goes with it, nothing moves
    if last.kind == "action" && last.outcomes.iter().any(|o| o.op == "REFUSE") {
        assert_eq!(r.records.len(), 1);
        assert_eq!(last.advanced_min, 0);
        assert!(last.recipes.len() <= step.clauses.len());
        // JS identity `r.state === pre` — the refused step returns the
        // untouched input state; snap equality is the Rust observable
        assert_eq!(snap(&r.state), snap(pre));
        return "refused";
    }

    // a turn ends at its boundary: action turns end with the BEGIN row,
    // pure advances with the wait row (v0.5)
    if !step.clauses.is_empty() {
        assert_eq!(last.kind, "action");
    }

    // v0.5 clock rows: at most one wait row per turn (one advance pass), its
    // span partitions time, and only it moves `now`
    let waits: Vec<&ActionRecord> = r.records.iter().filter(|x| x.kind == "wait").collect();
    assert!(waits.len() <= 1);
    if waits.len() == 1 {
        assert_eq!(
            r.state.now,
            waits[0].start_min.unwrap_or(0) + waits[0].advanced_min
        );
    } else {
        assert_eq!(r.state.now, pre.now); // beginning at now moves nothing
    }

    // BEGIN semantics: recipes = clauses; advanced_min = the window length —
    // §6.6 v0.5 (authored ADVANCE defers to the completion, where it extends
    // the resolving pass's wait span instead of the BEGIN row)
    if last.kind == "action" {
        assert_eq!(last.recipes.len(), step.clauses.len());
        let sum_dur: i64 = step
            .clauses
            .iter()
            .map(|c| world.content.verbs.get(&c.verb).map_or(0, |v| v.duration))
            .sum();
        assert_eq!(last.advanced_min, sum_dur);
    }
    // state = fold(log): the records alone reproduce the played state (§7) —
    // open windows included
    assert_eq!(
        snap(&fold(
            &r.records,
            &clone_state(pre),
            &world.dict,
            &world.content
        )
        .unwrap()),
        snap(&r.state)
    );
    check_state_shape(&r.state, &world.content);
    "applied"
}

/// one execute_step on a private clone of `state` — the JS engine passes
/// `state` by reference and lets execute_step draft internally; the Rust
/// engine consumes its input, so the clone IS the draft hand-off.
fn exec_on(
    world: &ExplorerWorld,
    state: &State,
    step: Step,
    label: &str,
    tag: &str,
) -> ExecResult {
    execute_step(
        &world.content,
        &world.dict,
        clone_state(state),
        step,
        ExecOptions {
            first_record_id: 1,
            id_gen: None,
        },
    )
    .unwrap_or_else(|e| panic!("engine threw at {label} on [{tag}]: {e}"))
}

// ------------------- pass A: exhaustive sweep ------------------------------

#[test]
fn the_exhaustive_verb_x_fill_space_at_every_sweep_state_keeps_the_invariants() {
    let world = explorer_world();
    let (mut states, _) = win_path_states(&world).unwrap();
    states.extend(sampled_walk_states(&world).unwrap());
    let mut total = 0usize;
    let mut applied = 0usize;
    let mut refused = 0usize;
    let mut rejected = 0usize;
    let mut atomicity_probes = 0usize;
    let mut verbs_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (label, state) in &states {
        // phase 1: everything the generator can name
        let mut refusers: indexmap::IndexMap<String, chorus::core::sentence::Clause> =
            indexmap::IndexMap::new();
        let mut appliers: indexmap::IndexMap<String, chorus::core::sentence::Clause> =
            indexmap::IndexMap::new();
        for cand in candidates_for(
            &world.content,
            &world.dict,
            state,
            &world.player,
            Depth::Full,
        ) {
            total += 1;
            let verb_name = cand.tag.split(' ').next().unwrap_or("").to_string();
            if world.content.verbs.contains_key(&verb_name) {
                verbs_seen.insert(verb_name);
            }
            let r = exec_on(&world, state, cand.step.clone(), label, &cand.tag);
            if r.rejected.is_some() {
                rejected += 1;
                assert_eq!(r.records.len(), 0);
                // JS identity `r.state === state` — untouched input back
                assert_eq!(snap(&r.state), snap(state));
                continue;
            }
            let outcome = check_step(state, &r, &cand.step, &world);
            if outcome == "refused" {
                refused += 1;
                // remember one refuser and one applier per verb for phase 2
                if cand.step.clauses.len() == 1 {
                    if let Some(c) = cand.step.clauses.first() {
                        refusers.entry(c.verb.clone()).or_insert_with(|| c.clone());
                    }
                }
            } else {
                applied += 1;
                if cand.step.clauses.len() == 1 {
                    if let Some(c) = cand.step.clauses.first() {
                        appliers.entry(c.verb.clone()).or_insert_with(|| c.clone());
                    }
                }
            }
        }
        // phase 2: step-atomicity probes (§6.5, §6.6) — pair each refusing
        // clause with WAIT and with a clause that applies, both orders; the
        // REFUSE must collapse the WHOLE step: no time, no effects from the
        // clause that would have succeeded
        let wait_clauses = parse_sentence(
            &format!(
                "{} @ {} WAIT",
                chorus::core::clock::format_clock(chorus::core::clock::next_step_clock(state.now)),
                world.player
            ),
            &world.dict,
        )
        .expect("the WAIT probe sentence parses")
        .clauses;
        let Some(wait_clause) = wait_clauses.first() else {
            continue;
        };
        let any_applier = appliers.values().next().cloned();
        for refuser in refusers.values().take(4) {
            let partners: Vec<chorus::core::sentence::Clause> = vec![
                Some(wait_clause.clone()),
                appliers
                    .get(&refuser.verb)
                    .cloned()
                    .or(any_applier.clone()),
            ]
            .into_iter()
            .flatten()
            .collect();
            for partner in partners {
                for clauses in [
                    vec![refuser.clone(), partner.clone()],
                    vec![partner, refuser.clone()],
                ] {
                    total += 1;
                    atomicity_probes += 1;
                    let step = Step {
                        start_min: None,
                        clock: Some(chorus::core::clock::next_step_clock(state.now)),
                        clauses,
                        t_real: None,
                    };
                    let r = exec_on(&world, state, step.clone(), label, "atomicity probe");
                    assert_eq!(
                        r.rejected,
                        None,
                        "{label}: atomicity probe rejected: {:?}",
                        r.rejected
                    );
                    let outcome = check_step(state, &r, &step, &world);
                    if outcome == "refused" {
                        refused += 1;
                    } else {
                        applied += 1;
                    }
                }
            }
        }
        check_state_shape(state, &world.content);
    }
    // coverage floors: the sweep must actually be sweeping
    assert!(states.len() >= WIN_PATH.len() + 1 + 4);
    assert!(total > 30000, "total = {total}");
    assert!(applied > 12000, "applied = {applied}");
    assert!(refused > 2000, "refused = {refused}");
    assert!(rejected > 8000, "rejected = {rejected}");
    assert!(atomicity_probes > 100, "probes = {atomicity_probes}");
    assert_eq!(verbs_seen.len(), world.content.verbs.len());
}

// ---------------- pass B: seeded random walk with undo ---------------------

#[test]
fn hundreds_of_arbitrary_steps_and_undos_keep_state_equals_fold_of_log() {
    let world = explorer_world();
    // deterministic LCG so failures reproduce ('NIGH', the RAW state)
    let mut lcg = chorus::js::Lcg::from_raw_state(0x4e49_4748);

    let mut state = clone_state(&world.base);
    let mut records: Vec<ActionRecord> = Vec::new();
    // undo truncates to the previous turn's start (v0.5)
    let mut turn_lengths: Vec<usize> = vec![0];
    let mut id: i64 = 1;
    let mut executed = 0usize;
    let mut walk_rejects = 0usize;
    let mut endings = 0usize;
    let mut snapshots: std::collections::HashMap<usize, String> =
        std::collections::HashMap::new();
    // the walk may need to undo back to the start
    snapshots.insert(0, snap(&state));

    for i in 0..800 {
        let cands = candidates_for(
            &world.content,
            &world.dict,
            &state,
            &world.player,
            Depth::Sampled,
        );
        // mostly walk the well-formed space; keep a 20% budget for error probes
        let benign: Vec<&common::explorer_lib::Cand> = cands
            .iter()
            .filter(|c| !c.tag.contains("wrong-kind") && c.tag != "clock rewind")
            .collect();
        let pick_from: Vec<&common::explorer_lib::Cand> =
            if lcg.next_f64() < 0.2 || benign.is_empty() {
                cands.iter().collect()
            } else {
                benign
            };
        let cand = &pick_from[(lcg.next_f64() * pick_from.len() as f64).floor() as usize
            % pick_from.len()];
        let r = common::settle::settle_step(
            &world.content,
            &world.dict,
            state,
            cand.step.clone(),
            id,
            None,
        )
        .unwrap();
        // any rejection returns the untouched input state (settle rolls back
        // whole), so the assignment is exact on every branch
        let rejected = r.rejected.is_some();
        state = r.state;
        if rejected {
            walk_rejects += 1;
            continue;
        }
        turn_lengths.push(records.len());
        executed += 1;
        let n = r.records.len();
        records.extend(r.records);
        id += n as i64;
        snapshots.insert(records.len(), snap(&state));

        // the log alone must always reproduce the state (full re-fold, every
        // 10th step)
        if i % 10 == 9 {
            assert_eq!(
                snap(&fold(
                    &records,
                    &clone_state(&world.base),
                    &world.dict,
                    &world.content
                )
                .unwrap()),
                snap(&state)
            );
        }
        // §9.35 ending-awareness: a careless walk stacks SUSPICION, and
        // watch(3) — the escalation the conductor's rounds and the
        // searched-again recipe both call — ends the story when the pool
        // reaches three. Undo through the ending step (it derives from the
        // fold) so the walk keeps exercising real play and `executed` stays
        // meaningful.
        if state.ended.is_some() {
            endings += 1;
            let len = turn_lengths
                .pop()
                .expect("turn_lengths never empties (seeded [0])");
            records.truncate(len);
            state = fold(
                &records,
                &clone_state(&world.base),
                &world.dict,
                &world.content,
            )
            .unwrap();
            let expected = snapshots
                .get(&records.len())
                .unwrap_or_else(|| panic!("undo through the ending left no snapshot at length {}", records.len()));
            assert_eq!(snap(&state), *expected);
            continue;
        }
        // §7 undo: drop the last turn, re-fold, compare
        if i % 37 == 36 && records.len() > 1 {
            let len = turn_lengths
                .pop()
                .expect("turn_lengths never empties (seeded [0])");
            records.truncate(len);
            state = fold(
                &records,
                &clone_state(&world.base),
                &world.dict,
                &world.content,
            )
            .unwrap();
            let expected = snapshots
                .get(&records.len())
                .unwrap_or_else(|| panic!("undo to length {} left no snapshot", records.len()));
            assert_eq!(snap(&state), *expected);
        }
    }
    assert!(executed > 150, "executed = {executed}");
    assert!(walk_rejects > 0);
    // the escalation really fires on a careless walk
    assert!(endings > 0, "endings = {endings}");

    // the whole walked log survives a JSONL round-trip (§7)
    let dir = std::env::temp_dir().join(format!("chorus-explorer-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.jsonl");
    save_log(&path, &records).unwrap();
    let replay_ok = snap(
        &fold(
            &load_log(&path).unwrap(),
            &clone_state(&world.base),
            &world.dict,
            &world.content,
        )
        .unwrap(),
    ) == snap(&state);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(replay_ok);
}

// --------------------- pass C: the typed surface ---------------------------

#[test]
fn slash_commands_garbage_and_the_event_guard_never_throw() {
    let world = explorer_world();
    let mut game = Game::from_script(common::explorer_lib::SCRIPT_TEXT, world.dict).unwrap();
    game.handle_line("wait").unwrap();

    let cases = [
        "",
        "   ",
        "xyzzy",
        "read",
        "read the",
        "talk to",
        "the the the",
        "wait at nowhere",
        "talk to seraphine about the lost chord",
        // duplicate card
        "use on the inquest with the aether seal the aether seal",
        // rewinds the clock
        "0:0 @ MARCH WAIT",
        // events are not typed (§5.1)
        "0:0 @ EVENT body_found",
        // canonical passthrough, valid
        "MARCH WAIT",
    ];
    for line in cases {
        let kind = game
            .handle_line(line)
            .unwrap_or_else(|e| panic!("handle_line({line:?}) threw: {e}"))
            .kind;
        assert!(
            kind == TurnKind::Info || kind == TurnKind::Error || kind == TurnKind::Action,
            "unexpected kind for {line:?}"
        );
    }
    assert_eq!(game.handle_line("0:0 @ EVENT body_found").unwrap().kind, TurnKind::Error);

    assert!(!game.look().is_empty());
    assert!(!game.cards().is_empty());
    assert!(!game.journal().is_empty());
    assert!(!game.story().unwrap().is_empty());
    assert_eq!(game.handle_line("bogus").unwrap().kind, TurnKind::Error);
    assert_eq!(game.undo().unwrap().kind, TurnKind::Info);
}

#[test]
fn the_guard_a_player_typed_event_sentence_is_rejected_nothing_logged() {
    let world = explorer_world();
    let mut game = Game::from_script(common::explorer_lib::SCRIPT_TEXT, world.dict).unwrap();
    let before = game.log.records.len();
    let turn = game
        .handle_line("0:0 @ EVENT body_found -> SPAWN DREAD")
        .unwrap();
    assert_eq!(turn.kind, TurnKind::Error);
    assert_eq!(game.log.records.len(), before);
}
