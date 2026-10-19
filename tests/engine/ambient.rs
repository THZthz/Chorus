//! Ambient verbs (spec §6.9): the `ambient` verb block line makes the
//! advance pass open the verb's windows itself — the §6.9 sweep at the two
//! deterministic check points (after every completion, at every crossed
//! tick), the commit refusal, the Time-loop twin (a self-chaining RUN cycle
//! the sweep bootstraps and the world-form gate steers), the zero-duration
//! budget trip, and play == fold == undo throughout.
//!
//! Conventions follow tests/engine/recipe_dur.rs: a fresh dictionary per
//! test, script-text rigs, and play/fold equality asserted through the
//! shared `snap`.

use crate::common::snap::snap;
use chorus::content::script::compile_script;
use chorus::content::CompiledContent;
use chorus::core::clock::parse_clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::types::{clone_state, State};
use chorus::engine::{execute_step, ExecOptions, ExecResult, Step};

// ---------------------------------------------------------------- rigs ------

/// The rig's compile (the effective dictionary carries the script verbs).
fn compile_rig(text: &str) -> (Dictionary, CompiledContent, State) {
    let compiled = compile_script(text, &core_dictionary()).unwrap();
    (compiled.dict, compiled.content, compiled.state)
}

/// The Time-loop twin (§6.9's flagship): the ambient TIME verb's spring
/// chain (a dur 10m → b dur 0 → c dur 0 → a …) gated on the world-form
/// `total(table, aspect:season) >= 1`; the when-less winter pulse takes
/// over when the season card dies. The plain WAIT verb exists for the
/// player action that kills the season.
fn twin_rig() -> String {
    "
script: Time Loop Twin
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  SEASON is idea with season:1 at HOUSE
  HOUSE is place

prototypes:
  TICK is idea stackable

verbs:
  TIME:
    ambient
    duration: 1h
    recipes:
      time.a dur 10m when total(table, aspect:season) >= 1 -> SPAWN TICK, RUN \"time.b\"
      time.b dur 0m when total(table, aspect:season) >= 1 -> RUN \"time.c\"
      time.c dur 0m when total(table, aspect:season) >= 1 -> RUN \"time.a\"
      time.w dur 10m -> NARRATE \"winter.\"
  WAITVERB:
    actors: 1 kind:character
    object: 1 kind:idea
    duration: 0m
    recipes:
      default -> DESTROY object
"
    .to_string()
}

/// A pure advance (the bare-clock step) — the pass the §6.9 sweep rides.
fn advance(content: &CompiledContent, dict: &Dictionary, state: State, clock: &str) -> ExecResult {
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
        ExecOptions::default(),
    )
    .unwrap()
}

/// One canonical step through the plain executor (begin only — no settle).
fn plain_line(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    src: &str,
    first_record_id: i64,
) -> ExecResult {
    let s = parse_sentence(src, dict).unwrap();
    execute_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(parse_clock("0:0").unwrap())),
            clauses: s.clauses,
            t_real: None,
        },
        ExecOptions { first_record_id, id_gen: None },
    )
    .unwrap()
}

/// The COMPLETE rows' recipe ids of a turn, in order.
fn completions(r: &ExecResult) -> Vec<String> {
    r.records.iter().filter(|rec| rec.kind == "complete").map(|rec| rec.recipes.join(",")).collect()
}

/// The BEGIN rows' (verb, recipe, trigger-marker) triples, in log order.
fn begins(r: &ExecResult) -> Vec<(String, String, Option<String>)> {
    r.records
        .iter()
        .filter(|rec| rec.kind == "action")
        .map(|rec| {
            (
                rec.verb.clone(),
                rec.recipes.first().cloned().unwrap_or_default(),
                rec.trigger.clone(),
            )
        })
        .collect()
}

/// The living copies of a name.
fn living_named(state: &State, name: &str) -> usize {
    state.entities.values().filter(|e| e.dead_action.is_none() && e.name == name).count()
}

// ------------------------------------------------------------- the sweep ----

#[test]
fn the_sweep_opens_the_first_window_at_the_passs_first_boundary() {
    let (dict, c, base) = compile_rig(&twin_rig());
    let r = advance(&c, &dict, clone_state(&base), "0:1");
    assert_eq!(r.rejected, None);
    // the first crossed tick (minute 10) carries the sweep's BEGIN — the
    // season gate holds, so the walk selects time.a, and the window is the
    // RECIPE's dur (10 minutes), not the verb's 1h default
    assert_eq!(
        begins(&r),
        [("TIME".to_string(), "time.a".to_string(), Some("ambient".to_string()))]
    );
    let begin = r.records.iter().find(|rec| rec.kind == "action").unwrap();
    assert_eq!(begin.advanced_min, 10, "the window length is the recipe's dur");
    assert!(begin.actors.is_empty(), "the actor-less clause");
    assert!(begin.objects.is_empty());
    // no completion yet — the pass target (minute 10) is the window's BEGIN
    assert!(completions(&r).is_empty());
    assert_eq!(r.records.last().unwrap().kind, "wait", "the wait row closes the pass");
    // the fold replays the sweep's BEGIN row through the ONE registration
    // path: state == fold(log)
    let replayed = fold_all(&r.records, &base, &dict, &c);
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn a_pass_with_no_crossing_and_no_completion_does_no_ambient_work() {
    // the §6.9 checks sit at the deterministic points only: a 5-minute wait
    // crosses no tick and completes nothing, so no window opens
    let (dict, c, base) = compile_rig(&twin_rig());
    let r = execute_step(
        &c,
        &dict,
        clone_state(&base),
        Step { start_min: Some(5), clock: None, clauses: vec![], t_real: None },
        ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(r.records.len(), 1, "only the wait row");
    assert!(begins(&r).is_empty());
}

#[test]
fn a_player_commit_of_an_ambient_verb_refuses() {
    let (dict, c, base) = compile_rig(&twin_rig());
    // the canonical actor-less clause is exactly the shape the sweep logs —
    // typed, it refuses; nothing logs
    let r = plain_line(&c, &dict, clone_state(&base), "0:0 @ TIME", 1);
    assert_eq!(
        r.rejected.as_deref(),
        Some("TIME runs on its own — it opens its windows without a commit (§6.9)")
    );
    assert!(r.records.is_empty());
    // a bound form refuses identically (the check sits before slot
    // validation — an ambient verb declares no slots to mis-quote)
    let r = plain_line(&c, &dict, clone_state(&base), "0:0 @ ANA TIME", 1);
    assert!(r.rejected.as_deref().unwrap().starts_with("TIME runs on its own"));
    assert!(r.records.is_empty());
}

// ------------------------------------------------------- the time loop ------

#[test]
fn the_time_loop_twin_pulses_at_its_window_length() {
    let (dict, c, base) = compile_rig(&twin_rig());
    // one bounded wait: 30 minutes. The sweep bootstraps time.a at minute
    // 10; the RUN chain (b → c → a) carries the loop from there — two full
    // spring pulses inside the one pass, one TICK each
    let r = advance(&c, &dict, clone_state(&base), "0:3");
    assert_eq!(r.rejected, None);
    assert_eq!(
        completions(&r),
        [
            "time.a".to_string(),
            "time.b".to_string(),
            "time.c".to_string(),
            "time.a".to_string(),
            "time.b".to_string(),
            "time.c".to_string(),
        ],
        "each pulse is a→b→c; the link armed at 30 completes past this pass"
    );
    assert_eq!(living_named(&r.state, "TICK"), 2, "one TICK per spring pulse");
    // play == fold across the whole pulsing pass
    let replayed = fold_all(&r.records, &base, &dict, &c);
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn the_loop_is_byte_identical_on_a_double_run() {
    let (dict, c, base) = compile_rig(&twin_rig());
    let run = || advance(&c, &dict, clone_state(&base), "0:3");
    let a = run();
    let b = run();
    let to_lines = |r: &ExecResult| -> Vec<String> {
        r.records.iter().map(|rec| serde_json::to_string(rec).unwrap()).collect()
    };
    assert_eq!(to_lines(&a), to_lines(&b), "byte-identical double-run");
    assert_eq!(snap(&a.state), snap(&b.state));
}

#[test]
fn the_world_form_gate_flips_which_branch_runs() {
    let (dict, c, base) = compile_rig(&twin_rig());
    // spring: the sweep arms the first link (no completion inside 10m)
    let spring = advance(&c, &dict, clone_state(&base), "0:1");
    assert_eq!(spring.records.iter().find(|rec| rec.kind == "action").unwrap().recipes, ["time.a"]);
    // the player kills the season (a 0-duration window; the destroy lands
    // when the next pass closes it)
    let kill = plain_line(&c, &dict, spring.state, "0:1 @ ANA WAITVERB SEASON", 2);
    assert_eq!(kill.rejected, None);
    // winter: the in-flight spring link completes (its TICK was armed under
    // spring), the chain's pin gate refuses time.b — the chain ends — and
    // the SWEEP re-arms the station with the winter pulse
    let winter = advance(&c, &dict, kill.state, "0:3");
    assert_eq!(winter.rejected, None);
    assert_eq!(
        completions(&winter),
        ["waitverb.default".to_string(), "time.a".to_string(), "time.w".to_string()],
        "the in-flight link completes; the dead pin ends the chain; winter re-arms"
    );
    assert_eq!(
        winter
            .state
            .windows
            .values()
            .map(|w| (w.recipe.clone(), w.start, w.end))
            .collect::<Vec<_>>(),
        [("time.w".to_string(), 30, 40)],
        "the sweep's re-arm stays open past the pass"
    );
    // play == fold across the flip
    let all: Vec<_> =
        spring.records.iter().chain(kill.records.iter()).chain(winter.records.iter()).collect();
    let replayed = fold_owned(all, &base, &dict, &c);
    assert_eq!(snap(&replayed), snap(&winter.state));
}

// ------------------------------------------------------------- the guard ----

/// The zero-duration self-chain rig: the sweep opens spin.self at the first
/// crossing; from there the recipe opens, completes and re-opens inside one
/// pass.
fn spin_rig() -> String {
    "
script: Spin
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place

verbs:
  SPIN:
    ambient
    duration: 0m
    recipes:
      spin.self -> NARRATE \"spin.\"
"
    .to_string()
}

#[test]
fn a_zero_duration_self_chain_trips_the_livelock_budget_loudly() {
    let (dict, c, base) = compile_rig(&spin_rig());
    let r = advance(&c, &dict, clone_state(&base), "0:1");
    // the §6.7 budget is the one guard: the step REJECTS loud, nothing logs
    let msg = r.rejected.expect("the zero-duration self-chain trips the budget");
    assert!(msg.contains("livelock guard"), "{msg}");
    assert!(msg.contains("1024"), "{msg}");
    assert!(r.records.is_empty(), "nothing logged — the throw precedes the wait row");
}

// --------------------------------------------------------- station rules ----

#[test]
fn a_busy_station_skips_and_the_window_rearms_after_its_completion() {
    // one ambient verb, one when-less 10m recipe: the sweep opens at minute
    // 10, each completion re-opens at its own check — the natural cadence,
    // one pulse per window length
    let (dict, c, base) = compile_rig(
        "
script: Rearm
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place

verbs:
  DRIP:
    ambient
    duration: 1h
    recipes:
      drip.drop dur 10m -> NARRATE \"drip.\"
",
    );
    let r = advance(&c, &dict, clone_state(&base), "0:3");
    assert_eq!(r.rejected, None);
    assert_eq!(completions(&r), ["drip.drop".to_string(), "drip.drop".to_string()]);
    // three windows opened — minute 10 (the crossing), 20 and 30 (the
    // re-arms); the station was busy in between, so each open waited for
    // its own completion's check
    let opens = begins(&r);
    assert_eq!(opens.len(), 3, "{opens:?}");
    assert!(opens.iter().all(|(_, _, t)| t.as_deref() == Some("ambient")));
    assert_eq!(
        r.state.windows.values().map(|w| (w.start, w.end)).collect::<Vec<_>>(),
        [(30, 40)],
        "the re-armed window stays open past the pass"
    );
}

#[test]
fn a_max_one_recipe_pulses_once_then_the_station_idles_silently() {
    // the §6.4 budget rides selection: once the count reaches `max 1` the
    // recipe is simply non-matching, so the sweep's walk selects nothing —
    // the completion's check and every later crossing idle the verb
    // silently (no error, no window)
    let (dict, c, base) = compile_rig(
        "
script: Once
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place

verbs:
  CHIME:
    ambient
    duration: 1h
    recipes:
      chime.hit max 1 dur 10m -> NARRATE \"chime.\"
",
    );
    let r = advance(&c, &dict, clone_state(&base), "0:5");
    assert_eq!(r.rejected, None);
    // one pulse at the first crossing; the checks at 20 (the completion's)
    // and at the crossings 30, 40 and 50 all find the recipe spent
    assert_eq!(
        begins(&r),
        [("CHIME".to_string(), "chime.hit".to_string(), Some("ambient".to_string()))]
    );
    assert_eq!(completions(&r), ["chime.hit".to_string()]);
    assert_eq!(r.state.executed.get("chime.hit"), Some(&1), "the count pins at 1");
    assert!(r.state.windows.is_empty(), "the exhausted station holds no window");
    // play == fold across the idle checks
    let replayed = fold_all(&r.records, &base, &dict, &c);
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn the_sweep_never_runs_after_the_ending() {
    // an ambient verb whose pulse ends the story: the pass closes at the
    // ending minute and no further sweep fires
    let (dict, c, base) = compile_rig(
        "
script: Doomed
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place

endings:
  DAWN: \"The night ends\"

verbs:
  FATE:
    ambient
    duration: 1h
    recipes:
      fate.strike dur 10m -> ENDING DAWN
      fate.after dur 10m -> NARRATE \"nothing.\"
",
    );
    let r = advance(&c, &dict, clone_state(&base), "0:2");
    assert_eq!(r.rejected, None);
    assert!(r.state.ended.is_some(), "the pulse ended the story");
    assert_eq!(completions(&r), ["fate.strike".to_string()], "the pass closes at the ending");
    assert_eq!(begins(&r).len(), 1, "no re-open after the ending");
    // the fold of the whole turn reproduces the ended world exactly
    let replayed = fold_all(&r.records, &base, &dict, &c);
    assert_eq!(snap(&replayed), snap(&r.state));
    assert!(replayed.ended.is_some());
}

// -------------------------------------------------------------- plumbing ----

/// `fold` with the arguments in the order the tests read them.
fn fold_all(
    records: &[chorus::engine::ActionRecord],
    base: &State,
    dict: &Dictionary,
    c: &CompiledContent,
) -> State {
    chorus::engine::fold::fold(records, base, dict, c).unwrap()
}

/// The three-turn chain of the seasons test, folded as one log.
fn fold_owned(
    records: Vec<&chorus::engine::ActionRecord>,
    base: &State,
    dict: &Dictionary,
    c: &CompiledContent,
) -> State {
    let owned: Vec<chorus::engine::ActionRecord> = records.into_iter().cloned().collect();
    chorus::engine::fold::fold(&owned, base, dict, c).unwrap()
}
