//! Per-recipe duration (spec §6.4): the recipe-line `dur <duration>` modifier
//! overrides the verb's `duration:` FIELD for that recipe's own completions —
//! the window end, the BEGIN row's `advancedMin` sum, the multi-clause
//! stagger and the chained follow-up's window each read the SELECTED
//! recipe's length. Selection, slot binding and the chain ops are untouched.
//!
//! Conventions follow tests/engine/run_chains.rs: a fresh dictionary per
//! test, script-text rigs, settled turns (settle_step closes the windows it
//! opens), and play/fold equality asserted through the shared `snap`.

use crate::common::rig::{begin_recipes, kind_spine, settle_line};
use crate::common::snap::snap;
use chorus::content::script::compile_script;
use chorus::content::{compile_content, ContentRaw, RecipeDef, SlotDef, VerbContent};
use chorus::core::clock::parse_clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::types::{add_entity, clone_state, new_state, Entity, State};
use chorus::engine::fold::fold;
use chorus::engine::{execute_step, ExecOptions, ExecResult, Step};
use indexmap::IndexMap;

// ---------------------------------------------------------------- rigs ------

/// The rig's compile: the EFFECTIVE dictionary (script-declared verbs like
/// TAP/GLANCE register into it), the compiled content and the base state.
fn compile_rig(text: &str) -> (Dictionary, chorus::content::CompiledContent, State) {
    let compiled = compile_script(text, &core_dictionary()).unwrap();
    let dict = compiled.dict;
    (dict, compiled.content, compiled.state)
}

/// The rig: ANA at HOUSE (no aspects) and BERTHA beside her (`seasoned:1`),
/// plus the caller's verbs block.
fn rig(verbs: &str) -> String {
    format!(
        "
script: Recipe Dur Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  BERTHA is character with seasoned:1 at HOUSE
  HOUSE is place

verbs:
{verbs}
"
    )
}

/// A pure advance (the bare-clock step).
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

/// The open windows of a state as (start, end) pairs, insertion order.
fn window_spans(s: &State) -> Vec<(i64, i64)> {
    s.windows.values().map(|w| (w.start, w.end)).collect()
}

/// The COMPLETE rows' (recipe, t_game) pairs of a settled turn.
fn completions(r: &ExecResult) -> Vec<(String, String)> {
    r.records
        .iter()
        .filter(|rec| rec.kind == "complete")
        .map(|rec| (rec.recipes.join(","), rec.t_game.clone()))
        .collect()
}

// ------------------------------------------------------------ the ladder ----

/// The two-recipe ladder: a `dur 10m` recipe gated on the seasoned actor in
/// front of the verb's `duration: 1h` default.
const LADDER: &str = "  WORK:
    actors: 1 kind:character
    duration: 1h
    recipes:
      work.overtime dur 10m when actors.aspect.seasoned >= 1 -> NARRATE \"fast.\"
      default -> NARRATE \"slow.\"
";

#[test]
fn the_matched_recipe_dur_overrides_the_verb_window() {
    let (dict, c, s) = compile_rig(&rig(LADDER));
    let base = clone_state(&s);
    // BERTHA is seasoned: first match picks work.overtime — the turn
    // completes ONE tick out, not the verb's six
    let r = settle_line(&c, &dict, s, "0:0 @ BERTHA WORK", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"]);
    assert_eq!(begin_recipes(&r), [vec!["work.overtime".to_string()]]);
    let begin = &r.records[0];
    assert_eq!(begin.advanced_min, 10, "the BEGIN row carries the recipe's length");
    assert_eq!(completions(&r), [("work.overtime".to_string(), "0:1".to_string())]);
    assert_eq!(r.records[2].advanced_min, 10, "the closing wait spans the recipe's window");
    // the fold rebuilds the same short turn
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

#[test]
fn the_unmatched_ladder_recipe_keeps_the_verb_window() {
    let (dict, c, s) = compile_rig(&rig(LADDER));
    // ANA is unseasoned: the default recipe fires — the verb's 1h stands
    // (the same commit the dur recipe would have shortened to one tick)
    let r = settle_line(&c, &dict, s, "0:0 @ ANA WORK", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(begin_recipes(&r), [vec!["work.default".to_string()]]);
    assert_eq!(r.records[0].advanced_min, 60);
    assert_eq!(completions(&r), [("work.default".to_string(), "0:6".to_string())]);
}

#[test]
fn a_recipe_dur_longer_than_the_verb_default_wins() {
    // the override is not a min(): a recipe may run LONGER than its verb
    let (dict, c, s) = compile_rig(&rig("  STUDY:\n    actors: 1 kind:character\n    duration: 30m\n    recipes:\n      study.deep dur 1h -> NARRATE \"deep.\"\n"),
    );
    let r = settle_line(&c, &dict, s, "0:0 @ ANA STUDY", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(r.records[0].advanced_min, 60);
    assert_eq!(completions(&r), [("study.deep".to_string(), "0:6".to_string())]);
}

#[test]
fn a_zero_recipe_dur_completes_at_its_begin_minute() {
    // `dur 0m` mirrors the verb-level zero: the window completes exactly at
    // its begin minute (§6.6's zero-duration posture)
    let (dict, c, s) = compile_rig(&rig("  GLANCE:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      glance.quick dur 0m -> NARRATE \"seen.\"\n"),
    );
    let r = settle_line(&c, &dict, s, "0:0 @ ANA GLANCE", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(kind_spine(&r), ["action", "complete", "wait"]);
    assert_eq!(r.records[0].advanced_min, 0);
    assert_eq!(completions(&r), [("glance.quick".to_string(), "0:0".to_string())]);
    assert_eq!(r.records[2].advanced_min, 0, "the closing wait collapses to a point");
}

#[test]
fn multi_clause_windows_stagger_by_each_recipe_dur() {
    // the §6.7 stagger: clause 2 begins where clause 1's RECIPE dur ends
    let (dict, c, s) = compile_rig(&rig(
            "  DREAM:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      dream.start dur 10m -> NARRATE \"dreamt.\"\n  WORK:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      work.shift dur 20m -> NARRATE \"worked.\"\n",
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM; ANA WORK", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(begin_recipes(&r), [vec!["dream.start".to_string(), "work.shift".to_string()]]);
    // advancedMin sums the EFFECTIVE lengths; the completions land one and
    // three ticks out — 0..10, then 10..30
    assert_eq!(r.records[0].advanced_min, 30);
    assert_eq!(
        completions(&r),
        [
            ("dream.start".to_string(), "0:1".to_string()),
            ("work.shift".to_string(), "0:3".to_string()),
        ]
    );
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

// --------------------------------------------------------------- chains -----

/// A chained follow-up completes under ITS OWN recipe's dur: the parent's
/// 10m window hands off to a 30m follow-up (§6.7 — each link's length is its
/// own).
#[test]
fn a_chained_followup_uses_its_own_recipe_dur() {
    let (dict, c, s) = compile_rig(&rig(
            "  DREAM:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      dream.start dur 10m when count(actors) >= 1 -> NARRATE \"dreamt.\", RUN \"wait.rest\"\n  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      wait.rest dur 30m when count(actors) >= 1 -> NARRATE \"waited.\"\n",
        ),
    );
    let base = clone_state(&s);
    let r = settle_line(&c, &dict, s, "0:0 @ ANA DREAM", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(kind_spine(&r), ["action", "complete", "action", "wait"]);
    assert_eq!(r.records[0].advanced_min, 10, "the parent runs its own 10m");
    // the follow-up's BEGIN row rides the follow-up recipe's OWN 30m — not
    // the parent's 10m, not either verb's 1h — beginning at the completion
    // clock, still open
    let follow = &r.records[2];
    assert_eq!(follow.verb, "WAIT");
    assert_eq!(follow.recipes, ["wait.rest"]);
    assert_eq!(follow.start_min, Some(10));
    assert_eq!(follow.advanced_min, 30);
    assert_eq!(window_spans(&r.state), vec![(10, 40)]);
    // the follow-up completes three ticks after its begin
    let adv = advance(&c, &dict, r.state, "0:4", 5);
    assert_eq!(adv.rejected, None);
    assert_eq!(kind_spine(&adv), ["complete", "wait"]);
    assert_eq!(completions(&adv), [("wait.rest".to_string(), "0:4".to_string())]);
    assert!(adv.state.windows.is_empty());
    // the whole two-link chain folds identically
    let mut all = r.records.clone();
    all.extend(adv.records.iter().cloned());
    let replayed = fold(&all, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

// ----------------------------------------------------- script/hand shapes ---

#[test]
fn the_dur_compiles_through_the_script_and_the_raw_face_alike() {
    // the script face: the modifier lands on the compiled recipe, the verb
    // FIELD stays the default
    let compiled = compile_script(&rig(LADDER), &core_dictionary()).unwrap();
    let work = compiled.content.verbs.get("WORK").unwrap();
    assert_eq!(work.duration, 60);
    assert_eq!(work.recipes[0].dur, Some(10));
    assert_eq!(work.recipes[1].dur, None);
    // the code-level ContentRaw face carries the same field and the engine
    // honors it
    let raw = ContentRaw {
        verbs: [(
            "TAP".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".into(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".into()),
                    shown: None,
                }],
                duration: 30,
                recipes: vec![RecipeDef {
                    id: Some("tap.once".into()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: Some(5),
                    out: vec!["NARRATE \"tap.\"".to_string()],
                }],
            },
        )]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    };
    // the raw face skips the load's registration — the hand-built verb
    // registers into the caller's dictionary itself (model.rs's rule)
    let mut dict = core_dictionary();
    dict.register_verb("TAP").unwrap();
    let content = compile_content(&raw, &dict, &[]).unwrap();
    assert_eq!(content.verbs["TAP"].recipes[0].dur, Some(5));
    let r = settle_line(&content, &dict, new_state_zero(), "0:0 @ ANA TAP", 1).unwrap();
    assert_eq!(r.rejected, None);
    assert_eq!(r.records[0].advanced_min, 5, "the hand-built dur drives the window too");
    assert_eq!(r.state.now, 5, "mid-tick end lands minute-exact (v0.5)");
}

fn new_state_zero() -> State {
    let mut s = new_state(0);
    add_entity(
        &mut s,
        Entity {
            id: "s_ana".to_string(),
            kind: "character".to_string(),
            name: "ANA".to_string(),
            aspects: IndexMap::new(),
            attrs: IndexMap::new(),
            born_action: None,
            dead_action: None,
            stackable: false,
        },
    )
    .unwrap();
    s
}
