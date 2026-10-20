//! The two bounded-time faces — `--by <minute>` (the time window) and
//! `--from-state <log>` (the midgame replay). Everything is pinned BYTE
//! EXACT where the face renders: the slot trigger's two states around M,
//! the horizon taking the tighter of M and the freeze, the by-bounded
//! duration>0 recipe, the replayed fidelity note, and the CLI's error
//! faces. The default face's byte identity is owned by the whole existing
//! suite (808 pins) plus the eight bundled-script text faces.

use std::process::Command;

use indexmap::IndexMap;

use chorus::content::script::compile_script;
use chorus::content::{
    compile_content, ContentRaw, PrototypeDef, RecipeDef, SlotDef, SlotTriggerDef, TriggerDef,
    VerbContent,
};
use chorus::core::clock::Clock;
use chorus::core::dict::core_dictionary;
use chorus::core::parse::parse_sentence;
use chorus::core::types::{add_entity, new_state, Entity, State};
use chorus::engine::store::save_log;
use chorus::engine::{execute_step, ExecOptions, Step};
use chorus::reach::export::graph_dot;
use chorus::reach::fixpoint::CardBlocker;
use chorus::reach::midgame::replay_state;
use chorus::reach::render::{render_report, render_report_by};
use chorus::reach::{Analyzer, CardOptions, CardVerdict, Existence};

// ---------------------------------------------------------------------------
// fixtures
// ---------------------------------------------------------------------------

fn ana() -> Entity {
    let mut e = Entity {
        id: "s_ana".into(),
        kind: "character".into(),
        name: "ANA".into(),
        aspects: IndexMap::new(),
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
        desc: None,
    };
    e.attrs.insert("container".into(), chorus::core::types::AttrValue::Str("ROOM".into()));
    e
}

fn state_with(es: Vec<Entity>) -> State {
    let mut s = new_state(0);
    for e in es {
        add_entity(&mut s, e).expect("fixture entities never collide");
    }
    s
}

fn compile(raw: &ContentRaw) -> chorus::content::CompiledContent {
    let dict = core_dictionary();
    compile_content(raw, &dict, &[]).expect("fixture content compiles")
}

fn proto(kind: &str) -> PrototypeDef {
    PrototypeDef { kind: kind.to_string(), ..PrototypeDef::default() }
}

fn stackable_proto(kind: &str) -> PrototypeDef {
    PrototypeDef { kind: kind.to_string(), stackable: true, ..PrototypeDef::default() }
}

fn slot_trigger(id: &str, tick: i64, once: bool, out: &[&str]) -> TriggerDef {
    TriggerDef::Slot(SlotTriggerDef {
        id: id.to_string(),
        slot: tick,
        once: Some(once),
        out: out.iter().map(|s| (*s).to_string()).collect(),
    })
}

fn actor_slot() -> SlotDef {
    SlotDef {
        role: "actors".into(),
        min: 1,
        max: Some(1),
        query: Some("kind:character".into()),
        shown: None,
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
        dur: None,
        out: out.iter().map(|s| (*s).to_string()).collect(),
    }
}

/// The clean by-window fixture: one ONCE slot trigger at tick 6
/// (minute 60) spawning THE_GEM. `once` keeps the risky-freeze model out —
/// a recurring minter would set the freeze at its own crossing.
fn gem_raw() -> ContentRaw {
    ContentRaw {
        prototypes: Some(IndexMap::from([("THE_GEM".to_string(), proto("item"))])),
        triggers: Some(vec![slot_trigger("t_gem", 6, true, &["SPAWN THE_GEM"])]),
        ..ContentRaw::default()
    }
}

// ---------------------------------------------------------------------------
// --by: the slot trigger's two states around M, byte-exact
// ---------------------------------------------------------------------------

#[test]
fn by_window_slot_trigger_two_states_around_the_bound_are_byte_exact() {
    let content = compile(&gem_raw());
    let state = state_with(vec![ana()]);

    // the analyzer's dictionary: the fixtures declare no vocabulary, so
    // the core seed IS the load-effective one (the --demand grammar's
    // kind: terms read it)
    let dict = core_dictionary();

    // the DEFAULT face byte-identical through both renderers (the
    // delegation wrapper renders exactly the frozen shape)
    let rep = Analyzer::new(&content, &state, &dict)
        .analyze_card("THE_GEM", CardOptions::default())
        .unwrap();
    let mut a = String::new();
    render_report(&mut a, "script.txt", "THE_GEM", None, false, &rep);
    let mut b = String::new();
    render_report_by(&mut b, "script.txt", "THE_GEM", None, false, None, &rep);
    assert_eq!(a, b);
    assert_eq!(
        a,
        "script.txt · card THE_GEM\n  \
         verdict: OBTAINABLE  (existence: proven)\n  path 1/1:\n    \
         · SPAWN 'THE_GEM' — via trigger t_gem\n        \
         · trigger t_gem fires on slot 6 (once: true — still fires its first crossing) — the clock is unbounded forward\n\n"
    );

    // M BEFORE the first crossing: the trigger has not fired, the site
    // falls to candidate with the honest timing reason
    let before = Analyzer::new(&content, &state, &dict)
        .by(30)
        .analyze_card("THE_GEM", CardOptions::default())
        .unwrap();
    assert_eq!(before.verdict, CardVerdict::Unproven);
    assert_eq!(before.existence, Existence::Candidate);
    let mut s = String::new();
    render_report_by(&mut s, "script.txt", "THE_GEM", None, false, Some(30), &before);
    assert_eq!(
        s,
        "script.txt · card THE_GEM  [by minute 30]\n  \
         verdict: UNPROVEN  (existence: candidate)\n  \
         candidate source: trigger t_gem — guarded (trigger t_gem's first crossing at minute 60 lies beyond the by 30 window)\n  \
         note: candidate source: trigger t_gem — guarded (trigger t_gem's first crossing at minute 60 lies beyond the by 30 window)\n\n"
    );

    // the blockers face names the same rung (judgment and reason one pass)
    let blockers = Analyzer::new(&content, &state, &dict).by(30).card_blockers("THE_GEM");
    assert_eq!(blockers.len(), 1);
    assert_eq!(blockers[0].blocker, CardBlocker::TriggerBeyondBy { minute: 60, by: 30 });

    // AT M: a crossing AT the boundary minute counts as by M
    let at = Analyzer::new(&content, &state, &dict)
        .by(60)
        .analyze_card("THE_GEM", CardOptions::default())
        .unwrap();
    assert_eq!(at.verdict, CardVerdict::Obtainable);
    let mut s = String::new();
    render_report_by(&mut s, "script.txt", "THE_GEM", None, false, Some(60), &at);
    assert_eq!(
        s,
        "script.txt · card THE_GEM  [by minute 60]\n  \
         verdict: OBTAINABLE  (existence: proven)\n  path 1/1:\n    \
         · SPAWN 'THE_GEM' — via trigger t_gem\n        \
         · trigger t_gem fires on slot 6 (once: true — still fires its first crossing) — first crossing at minute 60, inside the by 60 window\n\n"
    );

    // a WIDE M keeps the default verdict, by-annotated
    let wide = Analyzer::new(&content, &state, &dict)
        .by(2000)
        .analyze_card("THE_GEM", CardOptions::default())
        .unwrap();
    assert_eq!(wide.verdict, CardVerdict::Obtainable);
    assert!(wide.candidates.is_empty());
    assert!(wide.paths[0].supports[0]
        .ends_with("— first crossing at minute 60, inside the by 2000 window"));
}

// ---------------------------------------------------------------------------
// --by × freeze: the horizon takes the tighter bound
// ---------------------------------------------------------------------------

/// Two recurring triggers sharing one non-stackable mint: the later
/// crossing is risky, the earlier stands on the freeze boundary (the clean
/// exemption) — freeze_dist = 30, t_b demoted, THE_GEM proven via t_a.
fn frozen_pair_raw() -> ContentRaw {
    ContentRaw {
        prototypes: Some(IndexMap::from([("THE_GEM".to_string(), proto("item"))])),
        triggers: Some(vec![
            slot_trigger("t_a", 3, false, &["SPAWN THE_GEM"]),
            slot_trigger("t_b", 12, false, &["SPAWN THE_GEM"]),
        ]),
        ..ContentRaw::default()
    }
}

#[test]
fn by_and_freeze_take_the_tighter_horizon() {
    let content = compile(&frozen_pair_raw());
    let state = state_with(vec![ana()]);

    // the analyzer's dictionary: the fixtures declare no vocabulary, so
    // the core seed IS the load-effective one (the --demand grammar's
    // kind: terms read it)
    let dict = core_dictionary();

    // default: the frozen face's settled shape — proven via the clean
    // boundary trigger, the later crossing DupRisk
    let rep = Analyzer::new(&content, &state, &dict)
        .analyze_card("THE_GEM", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(rep.paths[0].supports[0].ends_with("— the clock is unbounded forward"));

    // M PAST the freeze: the freeze binds (min semantics) — the verdict is
    // the frozen shape, and the support line names the crossing minute
    let wide = Analyzer::new(&content, &state, &dict)
        .by(5000)
        .analyze_card("THE_GEM", CardOptions::default())
        .unwrap();
    assert_eq!(wide.verdict, CardVerdict::Obtainable);
    assert_eq!(
        wide.paths[0].supports[0],
        "trigger t_a fires on slot 3 (every crossing) — first crossing at minute 30, inside the by 5000 window"
    );
    let blockers = Analyzer::new(&content, &state, &dict).by(5000).card_blockers("THE_GEM");
    assert!(blockers.iter().all(|b| b.blocker == CardBlocker::DupRisk), "{blockers:?}");

    // M BEFORE the freeze: M binds — even the boundary trigger is beyond
    // the window, and the card falls to candidate
    let tight = Analyzer::new(&content, &state, &dict)
        .by(20)
        .analyze_card("THE_GEM", CardOptions::default())
        .unwrap();
    assert_eq!(tight.verdict, CardVerdict::Unproven);
    assert_eq!(tight.existence, Existence::Candidate);
    assert!(
        tight.candidates.iter().any(|c| c
            .reason
            .contains("t_a's first crossing at minute 30 lies beyond the by 20 window")),
        "{tight:?}"
    );
}

// ---------------------------------------------------------------------------
// the tie face: two lists crossing on the same tick share no clean pool
// ---------------------------------------------------------------------------

/// Two recurring triggers on the SAME tick minting the same non-stackable
/// name from different lists (the bridge fixture's freeze-pair shape): a
/// tie is no proof. Play fires the crossing's triggers in declaration order
/// on the living world — the second SPAWN refuses the shared step and the
/// whole advance rolls back (§9.2, the fired-set with it) — so neither
/// list's first application creates the pool.
fn frozen_tie_raw() -> ContentRaw {
    ContentRaw {
        prototypes: Some(IndexMap::from([("THE_GEM".to_string(), proto("item"))])),
        triggers: Some(vec![
            slot_trigger("t_tie_a", 24, false, &["SPAWN THE_GEM"]),
            slot_trigger("t_tie_b", 24, false, &["SPAWN THE_GEM"]),
        ]),
        ..ContentRaw::default()
    }
}

#[test]
fn a_first_crossing_tie_demotes_every_list_the_shared_step_never_lands() {
    let content = compile(&frozen_tie_raw());
    let state = state_with(vec![ana()]);

    // the analyzer's dictionary: the fixtures declare no vocabulary, so
    // the core seed IS the load-effective one
    let dict = core_dictionary();

    // the analyzer's face: the card falls to candidate — the census's
    // same-slot pair rule already reads the tie as risky, and the timing
    // guard's own tie face (a landing equal to the crossing demotes) is
    // pinned against the ambient sweep in reach_adversarial.rs
    let rep = Analyzer::new(&content, &state, &dict)
        .analyze_card("THE_GEM", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert_eq!(rep.existence, Existence::Candidate);

    // the engine's face: the tie crossing rejects the shared step in
    // declaration order — the mint never lands and the clock never passes it
    let r1 = execute_step(
        &content,
        &dict,
        state,
        Step {
            start_min: None,
            clock: Some(chorus::core::clock::parse_clock("4:0").unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id: 1, id_gen: None },
    )
    .unwrap();
    assert!(
        r1.rejected.as_deref().is_some_and(|m| m.contains("already in use")),
        "the tie crossing refuses the shared step: {:?}",
        r1.rejected
    );
    assert!(r1.state.by_name.get("THE_GEM").is_none_or(|pool| pool.is_empty()));
}

// ---------------------------------------------------------------------------
// --by: duration>0 recipes cannot prove inside a finite horizon
// ---------------------------------------------------------------------------

#[test]
fn by_bounds_duration_recipes_only_duration0_proves() {
    let mk = |duration: i64| ContentRaw {
        prototypes: Some(IndexMap::from([("THE_RING".to_string(), proto("item"))])),
        verbs: indexmap::IndexMap::from([(
            "WORK".to_string(),
            VerbContent {
                slots: vec![actor_slot()],
                duration,
                recipes: vec![recipe(&["SPAWN THE_RING"])],
                ambient: false,
            },
        )]),
        ..ContentRaw::default()
    };

    // unfrozen + unbounded: the duration-30 recipe proves as ever
    let content = compile(&mk(30));
    let state = state_with(vec![ana()]);

    // the analyzer's dictionary: the fixtures declare no vocabulary, so
    // the core seed IS the load-effective one (the --demand grammar's
    // kind: terms read it)
    let dict = core_dictionary();
    let rep = Analyzer::new(&content, &state, &dict)
        .analyze_card("THE_RING", CardOptions::default())
        .unwrap();
    assert_eq!(rep.verdict, CardVerdict::Obtainable);

    // the SAME world under any --by: the horizon is finite, the recipe
    // demotes with its own blocker spelling
    let by = Analyzer::new(&content, &state, &dict)
        .by(9999)
        .analyze_card("THE_RING", CardOptions::default())
        .unwrap();
    assert_eq!(by.verdict, CardVerdict::Unproven);
    let blockers = Analyzer::new(&content, &state, &dict).by(9999).card_blockers("THE_RING");
    assert_eq!(blockers[0].blocker, CardBlocker::BeyondByDuration { duration: 30, by: 9999 });
    assert_eq!(
        blockers[0].blocker.as_str(),
        "the by 9999 window bounds every commit — a duration-30 recipe completes past the analysis horizon, and only duration-0 recipes prove"
    );
    // the candidate listing carries the honest timing reason
    assert!(
        by.candidates
            .iter()
            .any(|c| c.reason.contains("the verb's 30m window completes past the by 9999 horizon")),
        "{by:?}"
    );

    // duration-0 recipes prove at any horizon (commits land at commit time)
    let instant = compile(&mk(0));
    let fast = Analyzer::new(&instant, &state, &dict)
        .by(1)
        .analyze_card("THE_RING", CardOptions::default())
        .unwrap();
    assert_eq!(fast.verdict, CardVerdict::Obtainable);
}

// ---------------------------------------------------------------------------
// --by: the recipe's OWN `dur` overrides the verb's duration (§6.4)
// ---------------------------------------------------------------------------

/// WORK with the verb default 1h and one recipe whose own window is the
/// caller's (the §6.4 override).
fn mk_dur(recipe_dur: Option<i64>) -> ContentRaw {
    ContentRaw {
        prototypes: Some(IndexMap::from([("THE_RING".to_string(), proto("item"))])),
        verbs: indexmap::IndexMap::from([(
            "WORK".to_string(),
            VerbContent {
                slots: vec![actor_slot()],
                duration: 60,
                recipes: vec![RecipeDef {
                    id: Some("work.make".into()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    dur: recipe_dur,
                    out: vec!["SPAWN THE_RING".to_string()],
                }],
                ambient: false,
            },
        )]),
        ..ContentRaw::default()
    }
}

#[test]
fn by_reads_the_recipe_dur_override_not_the_verb_default() {
    let dict = core_dictionary();
    let content = compile(&mk_dur(Some(10)));
    let state = state_with(vec![ana()]);

    // the dur-10m recipe is UNPROVABLE under a by 5m deadline — its own
    // window completes past the horizon even though the verb reads 1h...
    let by = Analyzer::new(&content, &state, &dict)
        .by(5)
        .analyze_card("THE_RING", CardOptions::default())
        .unwrap();
    assert_eq!(by.verdict, CardVerdict::Unproven);
    let blockers = Analyzer::new(&content, &state, &dict).by(5).card_blockers("THE_RING");
    assert_eq!(blockers[0].blocker, CardBlocker::BeyondByDuration { duration: 10, by: 5 });
    // ...and the candidate listing names the RECIPE's window
    assert!(
        by.candidates
            .iter()
            .any(|c| c.reason.contains("the recipe's 10m window completes past the by 5 horizon")),
        "{by:?}"
    );

    // a dur 0m recipe on the SAME duration-1h verb proves under any horizon:
    // its own window lands at its begin minute
    let instant = compile(&mk_dur(Some(0)));
    let fast = Analyzer::new(&instant, &state, &dict)
        .by(5)
        .analyze_card("THE_RING", CardOptions::default())
        .unwrap();
    assert_eq!(fast.verdict, CardVerdict::Obtainable);

    // no override — the verb's own 1h still rules the horizon check
    let plain = compile(&mk_dur(None));
    let slow = Analyzer::new(&plain, &state, &dict)
        .by(9999)
        .analyze_card("THE_RING", CardOptions::default())
        .unwrap();
    assert_eq!(slow.verdict, CardVerdict::Unproven);
    let blockers = Analyzer::new(&plain, &state, &dict).by(9999).card_blockers("THE_RING");
    assert_eq!(blockers[0].blocker, CardBlocker::BeyondByDuration { duration: 60, by: 9999 });
}

// ---------------------------------------------------------------------------
// --from-state: the replayed world
// ---------------------------------------------------------------------------

/// The midgame fixture: a once trigger (tick 6 = minute 60) that spawns
/// THE_GEM and draws BOX (advancing the cursor past order[0]), and a
/// recurring trigger at tick 18 (minute 180) that keeps drawing. The
/// deck cards are STACKABLE so the two draws stay collision-free (the
/// recurring draw stays unbounded — the clean future source).
const MIDGAME_SCRIPT: &str = "\
script: Timing Fixture
player: ANA
goal: exercise the bounded-time faces
seed: 3
start: 0:00

entities:
  ROOM is place
  ANA is character at ROOM

prototypes:
  THE_GEM is item
  A_CARD is item stackable
  B_CARD is item stackable

decks:
  BOX spec A_CARD, B_CARD

triggers:
  t_gem once at tick 6 -> SPAWN THE_GEM, DRAW BOX
  t_draw at tick 18 -> DRAW BOX
";

/// One empty-clause step = the wait row (the only clock mover): advance the
/// fold to `tick`, firing whatever the pass finds there.
fn wait_to(
    content: &chorus::content::CompiledContent,
    from: State,
    tick: i64,
    first_record_id: i64,
) -> chorus::engine::ExecResult {
    let dict = core_dictionary();
    execute_step(
        content,
        &dict,
        from,
        Step {
            start_min: None,
            clock: Some(Clock { day: 0, tick }),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions { first_record_id, id_gen: None },
    )
    .expect("the step executes")
}

#[test]
fn midgame_replay_models_fired_once_triggers_and_deck_cursors() {
    let dict = core_dictionary();
    let compiled = compile_script(MIDGAME_SCRIPT, &dict).unwrap();
    let (content, initial) = (&compiled.content, &compiled.state);
    let first = content.decks.get("BOX").unwrap().order[0].name.clone();
    let second = content.decks.get("BOX").unwrap().order[1].name.clone();

    // the script face: both deck positions provable (the recurring draw is
    // unbounded), THE_GEM via the unspent trigger
    for name in [&first, &second, "THE_GEM"] {
        let rep = Analyzer::new(content, initial, &dict)
            .analyze_card(name, CardOptions::default())
            .unwrap();
        assert_eq!(rep.verdict, CardVerdict::Obtainable, "{name}");
    }

    // play one wait step to 0:6 (minute 60): t_gem fires (spawn + draw),
    // t_draw (minute 180) does not
    let r = wait_to(content, new_state(0), 6, 1);
    assert!(r.state.fired.contains("t_gem"), "the once trigger fired");
    let path = crate::common::rig::tmp_log_path("reach-timing");
    save_log(&path, &r.records).unwrap();

    // the engine's own fold rebuilds the midgame state from the JSONL log
    let mid = replay_state(&path, initial, &dict, content).unwrap();
    assert_eq!(mid.now, 60);
    assert!(mid.fired.contains("t_gem"));
    assert_eq!(mid.decks.get("BOX"), Some(&1));
    assert!(mid.entities.values().any(|e| e.name == "THE_GEM" && e.dead_action.is_none()));

    let analyzer = Analyzer::new(content, &mid, &dict).replayed();

    // THE_GEM: the past mint rides the replayed entities as an initial fact
    let g = analyzer.analyze_card("THE_GEM", CardOptions::default()).unwrap();
    assert_eq!(g.verdict, CardVerdict::Obtainable);
    assert!(g.chain[0].contains("THE_GEM is declared in the initial entities"), "{:?}", g.chain);
    assert!(g.paths[0].via.contains("initial entity THE_GEM"), "{:?}", g.paths[0].via);
    // the fidelity note rides on the report (name-free ⇒ global), byte-exact
    assert_eq!(
        g.notes[0],
        "midgame analysis: the initial world is replayed from a log — modeled: the replayed entities and clock (first crossings re-anchor to minute 60), 1 already-fired once trigger(s) (they never fire again), 1 non-reset deck cursor(s) (draws serve from the cursor forward); read but not modeled: open windows (the lock table), the executed-recipe map, the rng counter"
    );

    // order[0]: the draw CONSUMED the position and the drawn copy is alive
    // in the replay — obtainable via that entity, the honest midgame answer
    let head = analyzer.analyze_card(&first, CardOptions::default()).unwrap();
    assert_eq!(head.verdict, CardVerdict::Obtainable);
    assert!(
        head.chain[0].contains(&format!("{first} is declared in the initial entities")),
        "{head:?}"
    );
    let blockers = analyzer.card_blockers(&first);
    assert!(
        blockers
            .iter()
            .any(|b| b.blocker == CardBlocker::AlreadyFired { trigger: "t_gem".to_string() }),
        "{blockers:?}"
    );

    // order[1]: the recurring draw still serves it, from the cursor forward
    let tail = analyzer.analyze_card(&second, CardOptions::default()).unwrap();
    assert_eq!(tail.verdict, CardVerdict::Obtainable);
    assert!(tail.chain[0].contains(&format!("deck BOX serves '{second}'")), "{:?}", tail.chain);

    // --by composes with the replay and anchors to the MIDGAME clock:
    // t_draw (tick 18) next crosses at minute 180 (120 past now=60)
    let bounded = Analyzer::new(content, &mid, &dict)
        .replayed()
        .by(100)
        .analyze_card(&second, CardOptions::default())
        .unwrap();
    assert_eq!(bounded.verdict, CardVerdict::Unproven);
    assert!(
        bounded.candidates.iter().any(|c| c
            .reason
            .contains("t_draw's first crossing at minute 180 lies beyond the by 100 window")),
        "{bounded:?}"
    );
    // ... and AT 180 it proves, the support line naming the minute
    let inside = Analyzer::new(content, &mid, &dict)
        .replayed()
        .by(180)
        .analyze_card(&second, CardOptions::default())
        .unwrap();
    assert_eq!(inside.verdict, CardVerdict::Obtainable);
    assert!(inside.paths[0].supports[0]
        .contains("— first crossing at minute 180, inside the by 180 window"));
}

#[test]
fn the_deck_cursor_bounds_the_remaining_permutation_non_reset_never_returns() {
    // the cursor-only fixture: one recurring draw (unbounded), no once
    // trigger — the cursor is the only thing bounding the permutation
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            ("A_CARD".to_string(), stackable_proto("item")),
            ("B_CARD".to_string(), stackable_proto("item")),
        ])),
        decks: Some(vec![chorus::content::DeckRaw {
            id: "BOX".to_string(),
            spec: vec!["A_CARD".to_string(), "B_CARD".to_string()],
            defaultcard: None,
            reset: false,
        }]),
        triggers: Some(vec![slot_trigger("t_draw", 18, false, &["DRAW BOX"])]),
        ..ContentRaw::default()
    };
    let content = compile(&raw);
    let dict = core_dictionary();
    let order = content.decks.get("BOX").unwrap().order.clone();
    let (first, second) = (order[0].name.clone(), order[1].name.clone());

    // the script face: both positions provable (the draw is unbounded)
    for name in [&first, &second] {
        let rep = Analyzer::new(&content, &state_with(vec![ana()]), &dict)
            .analyze_card(name, CardOptions::default())
            .unwrap();
        assert_eq!(rep.verdict, CardVerdict::Obtainable, "{name}");
    }

    // the hand-built midgame: cursor past order[0], whose copy is dead —
    // the permutation does not reset, so order[0] is structurally gone
    let mut dead = Entity {
        id: "s_drawn".into(),
        kind: "item".into(),
        name: first.clone(),
        aspects: IndexMap::new(),
        attrs: IndexMap::new(),
        born_action: Some(1),
        dead_action: Some(2),
        stackable: true,
        desc: None,
    };
    dead.attrs.insert("container".into(), chorus::core::types::AttrValue::Str("ROOM".into()));
    let mut mid = state_with(vec![ana(), dead.clone()]);
    mid.decks.insert("BOX".to_string(), 1);
    let analyzer = Analyzer::new(&content, &mid, &dict).replayed();

    let gone = analyzer.analyze_card(&first, CardOptions::default()).unwrap();
    assert_eq!(gone.verdict, CardVerdict::Unobtainable, "{gone:?}");
    let still = analyzer.analyze_card(&second, CardOptions::default()).unwrap();
    assert_eq!(still.verdict, CardVerdict::Obtainable);
    assert!(still.chain[0].contains(&format!("deck BOX serves '{second}'")), "{:?}", still.chain);

    // the --graph face shares the SAME serving verdict (one predicate, one
    // home): the consumed order[0] emits NO draw edge while order[1] does —
    // the graph can never contradict the card face over the same replay
    let dot = graph_dot(&content, &mid, &dict);
    let draw_edge =
        |name: &str| format!("\"trigger t_draw\" -> \"{name}\" [label=\"DRAW deck BOX\"]");
    assert!(!dot.contains(&draw_edge(&first)), "consumed position must emit no edge: {dot}");
    assert!(dot.contains(&draw_edge(&second)), "{dot}");
    // the control graph (cursor 0) carries BOTH edges
    let fresh_dot = graph_dot(&content, &state_with(vec![ana()]), &dict);
    assert!(fresh_dot.contains(&draw_edge(&first)));
    assert!(fresh_dot.contains(&draw_edge(&second)));

    // the control: the SAME dead copy with NO cursor re-mints — the
    // position is still ahead of the permutation, so the draw still serves
    let fresh = state_with(vec![ana(), dead]);
    let analyzer = Analyzer::new(&content, &fresh, &dict).replayed();
    let alive = analyzer.analyze_card(&first, CardOptions::default()).unwrap();
    assert_eq!(alive.verdict, CardVerdict::Obtainable);
}

#[test]
fn a_fired_once_trigger_whose_past_mint_died_leaves_only_a_candidate() {
    // hand-built midgame: t_gem spent, THE_GEM spawned and later destroyed —
    // the spent site cannot re-mint, so the card is candidate at best
    let content = compile(&gem_raw());
    let mut gem = Entity {
        id: "s_the_gem".into(),
        kind: "item".into(),
        name: "THE_GEM".into(),
        aspects: IndexMap::new(),
        attrs: IndexMap::new(),
        born_action: Some(1),
        dead_action: Some(2),
        stackable: false,
        desc: None,
    };
    gem.attrs.insert("container".into(), chorus::core::types::AttrValue::Str("ROOM".into()));
    let mut mid = state_with(vec![ana(), gem]);
    mid.now = 90;
    mid.fired.insert("t_gem".to_string());
    let dict = core_dictionary();

    let analyzer = Analyzer::new(&content, &mid, &dict).replayed();
    let rep = analyzer.analyze_card("THE_GEM", CardOptions::default()).unwrap();
    assert_eq!(rep.verdict, CardVerdict::Unproven);
    assert_eq!(rep.existence, Existence::Candidate);
    assert!(
        rep.candidates.iter().any(|c| c
            .reason
            .contains("trigger t_gem already fired in the replayed log — it never fires again")),
        "{rep:?}"
    );
    let blockers = analyzer.card_blockers("THE_GEM");
    assert_eq!(blockers[0].blocker, CardBlocker::AlreadyFired { trigger: "t_gem".to_string() });
}

// ---------------------------------------------------------------------------
// the CLI faces: composition, the fidelity head, the error faces
// ---------------------------------------------------------------------------

struct Cli {
    out: String,
    err: String,
    code: Option<i32>,
}

fn run(args: &[&str]) -> Cli {
    let out = Command::new(env!("CARGO_BIN_EXE_reach"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the bin spawns");
    Cli {
        out: String::from_utf8_lossy(&out.stdout).to_string(),
        err: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code(),
    }
}

/// Write `text` to a per-process temp file, return its path.
fn temp_file(tag: &str, name: &str, text: &str) -> String {
    let dir = std::env::temp_dir().join(format!("chorus-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p.to_string_lossy().to_string()
}

#[test]
fn the_cli_faces_compose_and_the_error_faces_hold() {
    let dict = core_dictionary();
    let compiled = compile_script(MIDGAME_SCRIPT, &dict).unwrap();
    let second = compiled.content.decks.get("BOX").unwrap().order[1].name.clone();
    let r = wait_to(&compiled.content, new_state(0), 6, 1);
    let script = temp_file("reach-timing-cli", "mid.txt", MIDGAME_SCRIPT);
    let log = temp_file("reach-timing-cli", "mid.jsonl", "");
    save_log(std::path::Path::new(&log), &r.records).unwrap();

    // the card face over the replay: the fidelity note rides, the past mint
    // proves via the replayed entity
    let c = run(&[&script, "card", "THE_GEM", "--from-state", &log]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    assert!(c.out.contains(" · card THE_GEM\n  verdict: OBTAINABLE"), "{}", c.out);
    assert!(
        c.out.contains("  note: midgame analysis: the initial world is replayed from a log —"),
        "{}",
        c.out
    );
    // without the flag the same command is the script face (no note)
    let plain = run(&[&script, "card", "THE_GEM"]);
    assert_eq!(plain.code, Some(0));
    assert!(!plain.out.contains("midgame analysis"), "{}", plain.out);

    // --by composes: one head annotation, the midgame-anchored horizon
    let c = run(&[&script, "card", &second, "--from-state", &log, "--by", "100"]);
    assert_eq!(c.code, Some(0), "{}{}", c.out, c.err);
    assert!(c.out.contains(&format!(" · card {second}  [by minute 100]")), "{}", c.out);
    assert!(c.out.contains("lies beyond the by 100 window"), "{}", c.out);
    assert!(c.out.contains("note: midgame analysis:"), "{}", c.out);

    // the argv errors: a missing --by value and a non-integer — exit 2
    // naming the flag over the short usage
    for bad in [vec![script.as_str(), "--by"], vec![script.as_str(), "--by", "soon"]] {
        let c = run(&bad);
        assert_eq!(c.code, Some(2), "{bad:?} {}{}", c.out, c.err);
        assert!(c.err.starts_with("--by needs"), "{}", c.err);
        assert!(c.err.contains("usage: reach <script.txt> <COMMAND>"), "{}", c.err);
    }
    let c = run(&[&script, "--from-state"]);
    assert_eq!(c.code, Some(2), "{}", c.err);
    assert!(c.err.starts_with("--from-state needs"), "{}", c.err);

    // a missing log file is the analyze error (exit 1, Error: line)
    let c = run(&[&script, "card", "THE_GEM", "--from-state", "no/such/log.jsonl"]);
    assert_eq!(c.code, Some(1), "{}{}", c.out, c.err);
    assert!(c.err.starts_with("Error: cannot read"), "{}", c.err);

    // `--by` survived as a MODIFIER flag, but with no command word it never
    // reaches analysis: the missing-command error of the new grammar — exit 2
    let c = run(&[&script, "--by", "30"]);
    assert_eq!(c.code, Some(2));
    assert!(
        c.err.starts_with(
            "reach needs a command — expected one of: card, all, demand, verb, world, graph"
        ),
        "{}",
        c.err
    );
}

#[test]
fn a_log_that_ends_the_game_is_the_analyze_error() {
    let dict = core_dictionary();
    // WORK is a core verb — the CLI's fresh dictionary knows it, so the
    // subprocess compiles the same script
    let script_text = "\
script: Ended Fixture
player: ANA
goal: end the story
seed: 3
start: 0:00

entities:
  ROOM is place
  ANA is character at ROOM

verbs:
  WORK:
    actors: 1 kind:character
    duration: 0m
    recipes:
      -> ENDING DOOM

endings:
  DOOM: \"The story ends\"
";
    let compiled = compile_script(script_text, &dict).unwrap();
    // the commit lands its window; the NEXT advance completes it (the
    // duration-0 window closes at the pass) and the ENDING applies
    let step = parse_sentence("0:0 @ ANA WORK", &dict).unwrap();
    let begin = execute_step(
        &compiled.content,
        &dict,
        compiled.state.clone(),
        Step {
            start_min: None,
            clock: Some(Clock { day: 0, tick: 0 }),
            clauses: step.clauses,
            t_real: None,
        },
        ExecOptions { first_record_id: 1, id_gen: None },
    )
    .unwrap();
    let r = wait_to(&compiled.content, begin.state, 3, begin.records.len() as i64 + 1);
    assert!(r.state.ended.is_some(), "the story ended");
    let script = temp_file("reach-timing-end", "end.txt", script_text);
    let log = temp_file("reach-timing-end", "end.jsonl", "");
    save_log(std::path::Path::new(&log), &r.records).unwrap();

    // the midgame fold rebuilds the ending — the CLI refuses with the
    // analyze error
    let c = run(&[&script, "card", "THE_GEM", "--from-state", &log]);
    assert_eq!(c.code, Some(1), "{}{}", c.out, c.err);
    assert!(c.err.starts_with("Error: the log ends the game ('The story ends')"), "{}", c.err);
}
