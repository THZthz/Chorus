//! `RUN`/`FORK` in slot and expire trigger outcomes (§6.6's event-chain
//! channel): the load set (the chain ops legal, REFUSE/PROGRESS/ADVANCE
//! still banned), the crossing-minute begin, the EMPTY-bindings discipline
//! (role-reading `when`s never match, world forms do; the follow-up locks
//! nothing and its completion's role ops silently no-op), the busy skip
//! across trigger firings of one crossing, the hoisted-snapshot and
//! livelock-budget guards, the record ordering, and the ENDING interplay.
//!
//! Conventions follow tests/engine/run_chains.rs: a fresh dictionary per
//! test, error patterns copied CHARACTER FOR CHARACTER from the frozen
//! literals (regex-escaped), and play/fold equality asserted through the
//! shared `snap`.

use crate::common::harness::assert_err_msg;
use crate::common::rig::{compile, kind_spine, tmp_log_path};
use crate::common::snap::snap;
use chorus::content::script::compile_script;
use chorus::core::clock::parse_clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::types::{clone_state, AttrValue, State};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{execute_step, ExecOptions, ExecResult, Step};
use chorus::session::game::Game;

// ---------------------------------------------------------------- rigs ------

fn rig(verbs: &str, triggers: Option<&str>) -> String {
    let triggers = match triggers {
        Some(t) => format!("\ntriggers:\n{t}"),
        None => String::new(),
    };
    format!(
        "
script: Trigger Chain Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place
{triggers}

verbs:
{verbs}
"
    )
}

/// A pure advance (the bare-clock step) with the run_chains id-generator
/// shape.
fn advance(
    content: &chorus::content::CompiledContent,
    dict: &Dictionary,
    state: State,
    clock: &str,
    first_record_id: i64,
) -> ExecResult {
    let mut n: i64 = 0;
    let mut id_gen = || {
        n += 1;
        format!("x{first_record_id}_{n}")
    };
    execute_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(parse_clock(clock).unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id, id_gen: Some(&mut id_gen) },
    )
    .unwrap()
}

/// The WAIT verb (1h, one recipe per test) and a zero-duration DREAM.
fn two_verb_body(wait_recipes: &str) -> String {
    format!(
        "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n{wait_recipes}\n  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> NARRATE \"dream.\""
    )
}

// ------------------------------------------------------------ load checks ---

/// T-1: the load set — slot and expire bodies chain (`RUN` and `FORK`
/// compile, candidates resolving through the global table); an unknown id is
/// the shared load error; REFUSE / PROGRESS / ADVANCE keep their existing
/// event bans.
#[test]
fn the_trigger_chain_load_checks() {
    let dict = core_dictionary();
    // slot + expire bodies with RUN and FORK compile
    compile(
        &dict,
        &rig(
            &two_verb_body("      rest -> NARRATE \"waited\""),
            Some("  tick at hour 1 -> RUN \"rest\", FORK \"rest\"\n  boom on expire of THING -> FORK \"rest\""),
        ),
    );
    // unknown id — the shared global-table message, slot face ...
    assert_err_msg(
        compile_script(
            &rig(&two_verb_body("      rest -> NARRATE \"w\""), Some("  tick at hour 1 -> RUN \"nope\"")),
            &dict,
        ),
        "trigger tick: RUN 'nope' does not name a compiled recipe — candidates resolve against the global recipe table \\(§6\\.7\\)",
    );
    // ... expire face, FORK shape
    assert_err_msg(
        compile_script(
            &rig(
                &two_verb_body("      rest -> NARRATE \"w\""),
                Some("  boom on expire of THING -> FORK \"nope\""),
            ),
            &dict,
        ),
        "trigger boom: FORK 'nope' does not name a compiled recipe — candidates resolve against the global recipe table \\(§6\\.7\\)",
    );
    // REFUSE / PROGRESS / ADVANCE keep their bans in both event kinds
    assert_err_msg(
        compile_script(
            &rig(
                &two_verb_body("      rest -> NARRATE \"w\""),
                Some("  tick at hour 1 -> REFUSE(\"no\")"),
            ),
            &dict,
        ),
        "trigger tick: REFUSE belongs to verb recipes — events have no step to refuse",
    );
    assert_err_msg(
        compile_script(
            &rig(
                &two_verb_body("      rest -> NARRATE \"w\""),
                Some("  boom on expire of THING -> PROGRESS(30m, \"half\")"),
            ),
            &dict,
        ),
        "trigger boom: PROGRESS belongs to verb recipes, not events",
    );
    assert_err_msg(
        compile_script(
            &rig(
                &two_verb_body("      rest -> NARRATE \"w\""),
                Some("  tick at hour 1 -> ADVANCE(1h)"),
            ),
            &dict,
        ),
        "trigger tick: ADVANCE belongs to verb recipes — events own no advance pass to stretch",
    );
}

// ---------------------------------------------------------------- the play --

/// T-2: the crossing-minute begin — a slot trigger RUNs a 1h recipe at the
/// hour-1 crossing: the follow-up BEGINS at the crossing minute and
/// completes on the next advance; one longer advance shows the whole
/// `event, action, complete, wait` spine; the turn folds identically from
/// the log and from JSONL.
#[test]
fn a_slot_trigger_run_begins_at_the_crossing_minute() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            &two_verb_body("      rest -> NARRATE \"waited\""),
            Some("  tick at hour 1 -> RUN \"rest\""),
        ),
    );
    let base = clone_state(&s);
    // stage 1: the hour-1 crossing fires the trigger; the follow-up begins
    // AT the crossing minute and is still processing at the wait
    let r1 = advance(&c, &dict, s, "0:6", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(kind_spine(&r1), ["event", "action", "wait"]);
    assert_eq!(r1.records[1].start_min, Some(60), "the follow-up begins at the crossing minute");
    assert_eq!(r1.records[1].verb, "WAIT");
    assert_eq!(r1.records[1].actors.len(), 0, "event bindings are empty");
    assert_eq!(r1.state.windows.len(), 1);
    // stage 2: the window completes on the next advance
    let r2 = advance(&c, &dict, r1.state, "0:12", 100);
    assert_eq!(r2.rejected, None);
    assert_eq!(kind_spine(&r2), ["complete", "wait"]);
    assert_eq!(r2.state.executed.get("rest"), Some(&1));
    // the one-advance variant: the whole spine at once
    let (c2, s2) = compile(
        &dict,
        &rig(
            &two_verb_body("      rest -> NARRATE \"waited\""),
            Some("  tick at hour 1 -> RUN \"rest\""),
        ),
    );
    let r3 = advance(&c2, &dict, s2, "0:12", 1);
    assert_eq!(r3.rejected, None);
    assert_eq!(kind_spine(&r3), ["event", "action", "complete", "wait"]);
    // fold from the log, and from JSONL: identical state both ways
    let mut records = r1.records.clone();
    records.extend(r2.records.iter().cloned());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
    let path = tmp_log_path("trigger-chain-jsonl");
    save_log(&path, &records).unwrap();
    let loaded = load_log(&path).unwrap();
    assert_eq!(loaded, records);
    let refolded = fold(&loaded, &base, &dict, &c).unwrap();
    assert_eq!(snap(&refolded), snap(&r2.state));
}

/// T-3: the decay-chain hand-off — an authored `on expire` trigger
/// REPLACEs the expiring card and RUNs a cleanup recipe; the built-in
/// destroy path (no authored trigger) still cannot chain (it authors no
/// chain op and carries no channel); fold equality.
#[test]
fn an_expire_trigger_hands_off_to_a_cleanup_recipe() {
    let dict = core_dictionary();
    let text = r#"
script: Trigger Chain Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place
  LEAF is resource

prototypes:
  ASH is resource

triggers:
  decay on expire of LEAF -> REPLACE LEAF WITH ASH, RUN "cleanup"

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      cleanup -> NARRATE "swept"
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      arm when count(actors) >= 1 -> SET(LEAF, expires, 30m), NARRATE "armed"
"#;
    let (c, s) = compile(&dict, text);
    let base = clone_state(&s);
    // arm the decay, then cross the expiry minute
    let r1 = crate::common::rig::settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r1.rejected, None);
    let r2 = advance(&c, &dict, r1.state, "0:9", 100);
    assert_eq!(r2.rejected, None);
    // the crossing at 0:3 fires the authored expiry: REPLACE (its SPAWN +
    // DESTROY pair) then the cleanup BEGIN; the cleanup (1h) completes at 0:9
    assert_eq!(kind_spine(&r2), ["event", "action", "complete", "wait"]);
    assert_eq!(r2.records[0].trigger.as_deref(), Some("decay"));
    let ops: Vec<&str> = r2.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["SPAWN", "DESTROY"], "the REPLACE pair rode the EVENT row");
    assert_eq!(r2.records[1].verb, "WAIT");
    assert_eq!(r2.records[1].recipes, vec!["cleanup".to_string()]);
    assert_eq!(r2.state.executed.get("cleanup"), Some(&1));
    // the minted ASH lives, LEAF is gone
    assert!(r2.state.entities.values().any(|e| e.name == "ASH" && e.dead_action.is_none()));
    assert!(!r2.state.entities.values().any(|e| e.name == "LEAF" && e.dead_action.is_none()));
    // the built-in destroy path: an expiring card with NO authored trigger
    // cannot chain (engine-written DESTROY only)
    let armed = r#"
script: Trigger Chain Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place
  STONE is resource

triggers:
  decay on expire of LEAF -> REPLACE LEAF WITH ASH, RUN "cleanup"

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      cleanup -> NARRATE "swept"
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      arm when count(actors) >= 1 -> SET(STONE, expires, 30m), NARRATE "armed"
"#;
    let (c4, s4) = compile(&dict, armed);
    let base4 = clone_state(&s4);
    let a1 = crate::common::rig::settle_line(&c4, &dict, s4, "0:0 @ ANA DREAM", 500).unwrap();
    assert_eq!(a1.rejected, None);
    let a2 = advance(&c4, &dict, a1.state, "1:0", 600);
    assert_eq!(a2.rejected, None);
    assert_eq!(kind_spine(&a2), ["event", "wait"], "the built-in destroy authors no BEGIN");
    assert_eq!(a2.records[0].trigger.as_deref(), Some("expire:STONE"));
    let ops: Vec<&str> = a2.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["DESTROY"]);
    let mut whole = a1.records.clone();
    whole.extend(a2.records.iter().cloned());
    let replayed = fold(&whole, &base4, &dict, &c4).unwrap();
    assert_eq!(snap(&replayed), snap(&a2.state));
    let _ = base;
}

/// T-4: the empty-bindings discipline — a role-reading candidate `when:`
/// never matches from a trigger (the projection carries nothing), a
/// world-form `when:` matches; the opened window has empty refs and an
/// empty locked list, and its completion applies role ops over unbound
/// roles as silent no-ops.
#[test]
fn trigger_run_candidates_read_world_forms_only() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      roled when count(actors) >= 1 -> SET(ACTORS, silver, +1), NARRATE \"roled\"\n      world when exists(name:HOUSE) AND count(name:ANA) >= 1 -> SET(ACTORS, silver, +1), NARRATE \"worlded\"\n  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> NARRATE \"dream.\"",
            Some("  tick at hour 1 -> RUN \"roled\", \"world\""),
        ),
    );
    let base = clone_state(&s);
    let r1 = advance(&c, &dict, s, "0:6", 1);
    assert_eq!(r1.rejected, None);
    // the role-reading candidate missed; the world-form one pinned
    assert_eq!(kind_spine(&r1), ["event", "action", "wait"]);
    assert_eq!(r1.records[1].recipes, vec!["world".to_string()]);
    // the window locked nothing: empty refs, empty locked list
    assert_eq!(r1.state.windows.len(), 1);
    let w = r1.state.windows.values().next().unwrap();
    assert!(w.locked.is_empty(), "the follow-up locks nothing");
    for role in w.refs.values() {
        assert!(role.is_empty(), "no role carries a ref");
    }
    // the completion's role ops run over unbound roles: silent no-ops
    let r2 = advance(&c, &dict, r1.state, "0:12", 100);
    assert_eq!(r2.rejected, None);
    assert_eq!(kind_spine(&r2), ["complete", "wait"]);
    let ops: Vec<&str> = r2.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["NARRATE"], "the SET over the unbound actors role logged no row");
    assert!(
        !r2.state.entities.values().any(|e| e.attrs.get("silver").is_some()),
        "the SET over the unbound actors role expanded to no targets"
    );
    let mut records = r1.records.clone();
    records.extend(r2.records.iter().cloned());
    let replayed = fold(&records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
}

/// T-4b: the gate reads the EMPTY bindings — a candidate whose `when` is
/// dead at the §6.1 selection range (`NOT (count(actors) >= 1)` over a 1..1
/// slot) holds over the trigger's count-0 bindings and PINS: the same face
/// the dead-chain lint stands down for (§6.6's event bindings are empty).
#[test]
fn a_trigger_chain_candidate_selection_dead_at_the_slots_pins_over_empty_bindings() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            &two_verb_body("      night when NOT (count(actors) >= 1) -> NARRATE \"night\"\n      day when count(actors) >= 1 -> NARRATE \"day\""),
            Some("  tick once at hour 1 -> RUN \"night\", \"day\""),
        ),
    );
    let base = clone_state(&s);
    let r1 = advance(&c, &dict, s, "0:6", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(kind_spine(&r1), ["event", "action", "wait"]);
    assert_eq!(
        r1.records[1].recipes,
        vec!["night".to_string()],
        "the NOT-count candidate pins over the empty trigger bindings"
    );
    let replayed = fold(&r1.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r1.state));
}

/// T-5: the busy skip — a trigger RUN onto a verb holding an open window
/// skips silently while the free branch opens; two trigger firings in one
/// crossing chaining the same verb: the second skips (immediate
/// registration).
#[test]
fn trigger_runs_respect_the_station_rule_across_firings() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 2h\n    recipes:\n      w -> NARRATE \"w\"\n  STUDY:\n    actors: 1 kind:character\n    duration: 2h\n    recipes:\n      study.base -> NARRATE \"base\"\n      study.s -> NARRATE \"s\"\n  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> NARRATE \"dream.\"",
            Some("  tick at hour 1 -> RUN \"study.s\", \"w\"\n  echo at hour 1 -> RUN \"study.s\""),
        ),
    );
    // the DREAM window (2h) is open across the crossing: the WAIT branch of
    // the first trigger skips; the STUDY branch opens; the second firing
    // finds STUDY busy and skips
    // two clauses hold BOTH stations open across the hour-1 crossing: the
    // STUDY branch skips (the clause-2 window), the WAIT branch opens
    let r =
        crate::common::rig::settle_line(&c, &dict, s, "0:0 @ ANA DREAM ; ANA STUDY", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(r.state.executed.get("study.s"), None, "the busy STUDY branch skipped");
    assert_eq!(r.state.executed.get("w"), Some(&1), "the free WAIT branch opened");
    assert_eq!(r.state.windows.len(), 1, "DREAM and STUDY closed at the settle; W chains on");
    // contrast: with no station busy both branches open
    let (c2, s2) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      w -> NARRATE \"w\"\n  STUDY:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      study.s -> NARRATE \"s\"\n  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> NARRATE \"dream.\"",
            Some("  tick at hour 1 -> RUN \"study.s\", \"w\""),
        ),
    );
    let base2 = clone_state(&s2);
    let r2 = advance(&c2, &dict, s2, "0:6", 300);
    assert_eq!(r2.rejected, None);
    // RUN is the first-match walk: with no station busy the first candidate
    // pins and the list never reaches the second
    assert_eq!(kind_spine(&r2), ["event", "action", "wait"]);
    assert_eq!(r2.state.executed.get("study.s"), Some(&1));
    assert_eq!(r2.state.executed.get("w"), None);
    let replayed = fold(&r2.records, &base2, &dict, &c2).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
}

/// T-6: the guards — an expire trigger feeding a zero-duration completion
/// cycle spins inside one pass and trips the 1024 budget LOUD (the step is
/// rejected, nothing logs, the original state rides back); the re-armed
/// expiry variant resolves at the NEXT advance (the hoisted snapshot), with
/// bounded records per pass.
#[test]
fn the_expire_fed_completion_cycle_trips_the_budget_and_the_rearm_resolves_next_pass() {
    let dict = core_dictionary();
    // variant 1: the expire trigger RUNs a 0m recipe that re-SETs the
    // expiry (same-minute re-arm) and chains a mutual 0m cycle — the
    // completion cascade spins in-pass and trips the budget
    let text = r#"
script: Trigger Chain Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place
  LEAF is resource

triggers:
  boom on expire of LEAF -> SET(LEAF, expires, 30m), RUN "dream.ping"

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      rest -> NARRATE "rest"
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      arm when count(actors) >= 1 -> SET(LEAF, expires, 30m), NARRATE "armed"
      dream.ping -> RUN "dream.pong"
      dream.pong -> RUN "dream.ping"
"#;
    let (c, s) = compile(&dict, text);
    let r1 = crate::common::rig::settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r1.rejected, None, "the arming turn lands (the cycle is not due yet)");
    let armed = clone_state(&r1.state);
    let r2 = advance(&c, &dict, r1.state, "1:0", 100);
    // (day 1: the cycle has crossed hour 1 of day 0 dozens of times over)
    let rejected = r2
        .rejected
        .clone()
        .unwrap_or_else(|| panic!("expected the livelock rejection, got {:?}", r2.rejected));
    assert!(
        rejected.contains("livelock guard")
            && rejected.contains("1024")
            && rejected.contains("§6.7"),
        "{rejected}"
    );
    assert!(r2.records.is_empty(), "the rejected pass logs nothing");
    assert_eq!(snap(&r2.state), snap(&armed), "the pre-advance state rides back");
    // variant 2: the trigger DESTROYs the card and mints a fresh 30m copy
    // (the authored hand-off), RUNning a cleanup recipe. The mint's expiry
    // lands in-pass but the hoisted snapshot keeps it to the NEXT advance:
    // pass 1 carries exactly the crossing's records.
    let text2 = text
        .replace(
            "boom on expire of LEAF -> SET(LEAF, expires, 30m), RUN \"dream.ping\"",
            "boom on expire of LEAF -> DESTROY LEAF, SPAWN resource:LEAF expires 30m, RUN \"rest\"",
        )
        .replace("duration: 1h", "duration: 0m");
    let (c2, s2) = compile(&dict, &text2);
    let b1 = crate::common::rig::settle_line(&c2, &dict, s2, "0:0 @ ANA DREAM", 500).unwrap();
    assert_eq!(b1.rejected, None);
    let b2 = advance(&c2, &dict, b1.state, "0:6", 600);
    assert_eq!(b2.rejected, None);
    // crossing 30: the expiry event (destroy + mint) and the 0m cleanup
    // (begin + complete in-pass); the mint's expiry (60) is not in this
    // pass's snapshot
    assert_eq!(kind_spine(&b2), ["event", "action", "complete", "wait"]);
    assert_eq!(b2.records.len(), 4, "bounded records: the fresh expiry waits for the next pass");
    // the next advance hoists the fresh copy: the hand-off fires again at
    // the crossing its expires names — once, because the snapshot entry is
    // ID-keyed and the trigger destroyed that very copy (the fresh mint
    // waits for the pass after)
    let b3 = advance(&c2, &dict, b2.state, "0:7", 700);
    assert_eq!(b3.rejected, None);
    assert_eq!(kind_spine(&b3), ["event", "action", "complete", "wait"]);
    assert_eq!(b3.records.len(), 4, "still bounded: one hand-off per pass");
}

/// T-7: the budget counts trigger-fed completions — a slot trigger opening a
/// 1024-completion chain in one pass lands exactly ON the budget and
/// succeeds; one more recipe rejects the pass.
#[test]
fn the_budget_counts_trigger_fed_completions_exactly() {
    let dict = core_dictionary();
    let chain = |n: usize| {
        let mut recipes = String::new();
        for i in 0..n {
            let nxt = format!("      c{i} -> NARRATE \"c{i}\"{}\n", {
                if i + 1 < n {
                    format!(", RUN \"c{}\"", i + 1)
                } else {
                    String::new()
                }
            });
            recipes.push_str(&nxt);
        }
        rig(
            &format!(
                "  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n{recipes}  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      rest -> NARRATE \"rest\""
            ),
            Some("  tick at hour 1 -> RUN \"c0\""),
        )
    };
    // 1024 completions: exactly the budget — the pass succeeds (one
    // crossing of hour 1: the advance stays inside day 0)
    let (c, s) = compile(&dict, &chain(1024));
    let r = advance(&c, &dict, s, "0:12", 1);
    assert_eq!(r.rejected, None, "{:?}", r.rejected);
    assert_eq!(r.state.executed.get("c1023"), Some(&1));
    // 1025 completions: one past the budget — the pass rejects loud
    let (c2, s2) = compile(&dict, &chain(1025));
    let r2 = advance(&c2, &dict, s2, "0:12", 1);
    let rejected = r2
        .rejected
        .clone()
        .unwrap_or_else(|| panic!("expected the livelock rejection, got {:?}", r2.rejected));
    assert!(rejected.contains("livelock guard") && rejected.contains("1024"), "{rejected}");
    assert!(r2.records.is_empty());
}

/// T-8: the record ordering — a follow-up BEGIN sits after its own EVENT row
/// and before the next crossing's EVENT row, ids strictly increase, and the
/// fold's wait-span partition holds (the fold equality below).
#[test]
fn the_followup_begin_orders_after_its_event_and_before_the_next_crossing() {
    let dict = core_dictionary();
    let (c, s) = compile(
        &dict,
        &rig(
            &two_verb_body("      rest -> NARRATE \"waited\""),
            Some("  tick at hour 1 -> RUN \"rest\"\n  noon at hour 2 -> NARRATE \"noon\""),
        ),
    );
    let base = clone_state(&s);
    let r = advance(&c, &dict, s, "0:12", 1);
    assert_eq!(r.rejected, None);
    // crossing 1: event + BEGIN (1h window completes at the next minute)…
    // crossing 2: the completion precedes the slot-2 event (§6.6), then the
    // noon event, then the wait
    assert_eq!(kind_spine(&r), ["event", "action", "complete", "event", "wait"]);
    let ids: Vec<i64> = r.records.iter().map(|rec| rec.id).collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(ids, sorted, "ids strictly increase along the record order");
    assert_eq!(r.records[0].kind, "event");
    assert_eq!(r.records[1].kind, "action", "the BEGIN follows its own EVENT row");
    assert_eq!(r.records[3].kind, "event", "the next crossing's EVENT row comes last");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state), "the wait spans partition (the fold asserts)");
}

/// T-9: the ENDING interplay — the trigger's own ENDING closes the pass at
/// the crossing minute with the follow-up window left open; undo past the
/// turn reopens the game (session surface).
#[test]
fn a_trigger_ending_closes_the_pass_with_the_followup_open() {
    let dict = core_dictionary();
    let text = r#"
script: Trigger Ending Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place

endings:
  DOOM: "The clock stopped."

triggers:
  tick at hour 1 -> RUN "rest", ENDING DOOM

verbs:
  WAIT:
    actors: 1 kind:character
    duration: 1h
    recipes:
      rest -> NARRATE "rest"
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> NARRATE "dream."
"#;
    let (c, s) = compile(&dict, text);
    let base = clone_state(&s);
    let r = advance(&c, &dict, s, "0:6", 1);
    assert_eq!(r.rejected, None);
    // the pass closed at the crossing minute: event, action, wait — and the
    // follow-up window is still open (its 1h never elapsed)
    assert_eq!(kind_spine(&r), ["event", "action", "wait"]);
    assert!(r.state.ended.is_some());
    assert_eq!(r.state.windows.len(), 1);
    assert_eq!(r.records[2].advanced_min, 60, "the wait span ends at the crossing");
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    // undo past the ending reopens the game (the session truncates and
    // re-folds — §4.4's undo rule through the fold)
    let mut game = Game::from_script(text, core_dictionary()).unwrap();
    game.handle_line("wait").unwrap();
    game.wait().unwrap();
    assert!(game.state.ended.is_some());
    game.undo().unwrap();
    game.undo().unwrap();
    assert!(game.state.ended.is_none(), "undo reopens the game");
    assert_eq!(game.state.windows.len(), 0);
    let _ = base;
}

// ------------------------------------------------ the silent-skip ruling ----

/// The §6.6 silent-skip ruling, second-op face: `RUN "rest", FORK "rest"` in
/// one slot-trigger body pins the RUN; the FORK skips SILENTLY (an event has
/// no step to refuse — no REFUSE marker lands in the EVENT row). The row
/// replays as ordinary history: play and fold agree on `fired`, the
/// follow-up window and the clock, and undoing a later turn re-folds the
/// row to the post-crossing state exactly.
#[test]
fn a_second_chain_op_in_an_event_body_skips_silently() {
    let dict = core_dictionary();
    let text = rig(
        &two_verb_body("      rest -> NARRATE \"waited\"\n      sigh -> NARRATE \"sighed\""),
        Some("  tick once at hour 1 -> RUN \"rest\", FORK \"rest\""),
    );
    let (c, s) = compile(&dict, &text);
    let base = clone_state(&s);
    // play: the crossing pins the RUN, the FORK skips silently
    let r1 = advance(&c, &dict, s, "0:6", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(kind_spine(&r1), ["event", "action", "wait"]);
    let ops: Vec<&str> = r1.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, Vec::<&str>::new(), "the EVENT row carries no REFUSE marker");
    assert!(r1.state.fired.contains("tick"), "the trigger fired");
    assert_eq!(r1.state.windows.len(), 1, "the first op's window stands");
    // fold: the row replays as ordinary history — fired, window, clock
    let replayed = fold(&r1.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r1.state));
    // undo: a later turn's undo re-folds the EVENT row to the post-crossing
    // state (with the marker the fold dropped the row whole: fired={} and no
    // window)
    let mut game = Game::from_script(&text, core_dictionary()).unwrap();
    game.wait().unwrap();
    let after_crossing = clone_state(&game.state);
    assert_eq!(snap(&after_crossing), snap(&r1.state), "the session lands where play did");
    game.wait().unwrap();
    assert_eq!(game.state.windows.len(), 0, "the follow-up completed on the second wait");
    game.undo().unwrap();
    assert_eq!(snap(&game.state), snap(&after_crossing), "undo re-folds the row exactly");
    assert!(game.state.fired.contains("tick"), "fired survives the re-fold");
}

/// The §6.6 silent-skip ruling, inline-gate face: a pinned branch whose
/// recipe's top-level REFUSE vetoes the gate is skipped SILENTLY in an event
/// body — no marker, no window for that branch — while the free branch still
/// opens and the body CONTINUES (a SET after the vetoed RUN applies).
#[test]
fn an_inline_gate_veto_skips_its_event_branch_silently() {
    let dict = core_dictionary();
    // the FORK face: the vetoed branch skips, the innocent branch opens —
    // the two candidates live on DIFFERENT verbs (one branch per station)
    let text = rig(
        "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      gated -> REFUSE(\"the gate says no\")\n  STUDY:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      innocent -> NARRATE \"opened\"\n  DREAM:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> NARRATE \"dream.\"",
        Some("  tick once at hour 1 -> FORK \"gated\", \"innocent\""),
    );
    let (c, s) = compile(&dict, &text);
    let base = clone_state(&s);
    let r1 = advance(&c, &dict, s, "0:6", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(kind_spine(&r1), ["event", "action", "wait"]);
    let ops: Vec<&str> = r1.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, Vec::<&str>::new(), "the EVENT row carries no REFUSE marker");
    assert_eq!(r1.records[1].recipes, vec!["innocent".to_string()], "only the free branch opened");
    let replayed = fold(&r1.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r1.state));
    // undo through the re-fold
    let mut game = Game::from_script(&text, core_dictionary()).unwrap();
    game.wait().unwrap();
    let after_crossing = clone_state(&game.state);
    assert_eq!(snap(&after_crossing), snap(&r1.state));
    game.wait().unwrap();
    game.undo().unwrap();
    assert_eq!(snap(&game.state), snap(&after_crossing));
    // the RUN face: the vetoed pin opens nothing, the body continues — the
    // SET after it applies in play AND fold
    let text2 = rig(
        &two_verb_body("      gated -> REFUSE(\"the gate says no\")"),
        Some("  tick once at hour 1 -> RUN \"gated\", SET(ANA, silver, +1)"),
    );
    let (c2, s2) = compile(&dict, &text2);
    let base2 = clone_state(&s2);
    let r2 = advance(&c2, &dict, s2, "0:6", 1);
    assert_eq!(r2.rejected, None);
    assert_eq!(kind_spine(&r2), ["event", "wait"], "the vetoed pin opened no window");
    let ops: Vec<&str> = r2.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["SET"], "the body continued after the silent veto");
    let ana = r2.state.entities.values().find(|e| e.name == "ANA").unwrap();
    assert_eq!(ana.attrs.get("silver"), Some(&AttrValue::Num(1)), "the SET applied");
    let replayed2 = fold(&r2.records, &base2, &dict, &c2).unwrap();
    assert_eq!(snap(&replayed2), snap(&r2.state));
    let mut game2 = Game::from_script(&text2, core_dictionary()).unwrap();
    game2.wait().unwrap();
    let after2 = clone_state(&game2.state);
    assert_eq!(snap(&after2), snap(&r2.state));
    game2.wait().unwrap();
    game2.undo().unwrap();
    assert_eq!(snap(&game2.state), snap(&after2), "undo re-folds the SET row exactly");
}

/// The §6.6 silent-skip ruling, the composition face: `RUN "a", RUN "b",
/// SET(...)` in an event body — the first op pins, the second skips
/// silently, the SET applies (no REFUSE marker poisons the EVENT row, so
/// play and fold agree on fired, the window and the attr, at play, at fold
/// and at undo).
#[test]
fn an_event_body_continues_after_a_skipped_second_chain_op() {
    let dict = core_dictionary();
    let text = rig(
        &two_verb_body("      rest -> NARRATE \"waited\"\n      sigh -> NARRATE \"sighed\""),
        Some("  tick once at hour 1 -> RUN \"rest\", RUN \"sigh\", SET(ANA, silver, +1)"),
    );
    let (c, s) = compile(&dict, &text);
    let base = clone_state(&s);
    // play: first op pins, second skipped, SET applies
    let r1 = advance(&c, &dict, s, "0:6", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(kind_spine(&r1), ["event", "action", "wait"]);
    let ops: Vec<&str> = r1.records[0].outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["SET"], "the second op skipped silently, the SET applies");
    assert_eq!(r1.records[1].recipes, vec!["rest".to_string()], "the first op pinned");
    assert!(r1.state.fired.contains("tick"));
    assert_eq!(r1.state.windows.len(), 1);
    // fold: identical
    let replayed = fold(&r1.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r1.state));
    // undo: the re-fold lands the fired entry, the window and the attr
    let mut game = Game::from_script(&text, core_dictionary()).unwrap();
    game.wait().unwrap();
    let after_crossing = clone_state(&game.state);
    assert_eq!(snap(&after_crossing), snap(&r1.state));
    game.wait().unwrap();
    game.undo().unwrap();
    assert_eq!(snap(&game.state), snap(&after_crossing));
    assert!(game.state.fired.contains("tick"));
    assert_eq!(game.state.windows.len(), 1, "the window re-registers through the BEGIN row");
}
