//! The analyzer face (story-analyze spec §2.2): in-memory state installation
//! (`Clone` + `with_state`/`state_mut`), the context-free evaluation face,
//! and the forkable settlement face — a forked call reproduces the unforked
//! semantics when every chance takes the kernel's outcome, recorded outcomes
//! replay bit-for-bit, T4 rollback stays kernel-side, and the budgets are
//! untouched.

mod common;

use std::cell::RefCell;

use common::{boot, compile, state_bytes};
use story_content::ir::{Expr, QueryTerm, Scope};
use story_core::{SlotName, VerbId};
use story_engine::{
    ChanceOutcome, ChancePoint, CommitAction, EffectRegistry, Engine, Event,
    MAX_RESOLUTION_DEPTH, MAX_RESOLUTIONS_PER_ADVANCE,
};

fn game(src: &str) -> Engine {
    boot(compile(src), 1)
}

/// The shared fixture: an instant roll recipe (commit-path chance point), a
/// timed draw recipe (advance-path chance point), a count-0 roll, an invalid
/// roll, a draw past the permutation, and a roll followed by a failing
/// consume (the T4 path).
const SRC: &str = r#"
game "Forkface"
  seed host
  turn 10m
  home bench

zones
  bench

aspects
  charge: 0

cards
  card tool
    kind gadget  stackable
    aspects charge 1
    start in bench count 2
  card prize
    kind treasure  stackable

decks
  deck bag
    entries prize, prize, prize
    wrap

legacies
  luck: 0

verbs
  verb cast committable instances 3
    recipe spin
      craftable
      duration 0
      effects
        roll 2d6 + 1 as sight
        legacy luck = sight

  verb whiff committable instances 3
    recipe zero
      craftable
      duration 0
      effects
        roll 0d6 + 5 as grace
        legacy luck = grace

  verb botch committable instances 3
    recipe misfire
      craftable
      duration 0
      effects
        roll 1d0 as bad

  verb pick committable instances 3
    recipe grab
      craftable
      duration 10m
      effects
        draw from bag count 2 into bench

  verb sweep committable instances 3
    recipe take
      craftable
      duration 10m
      effects
        draw from bag count 4 into bench

  verb doom committable instances 3
    recipe gamble
      craftable
      duration 30m
      effects
        roll 1d6 + 2 as omen
        consume aspect:charge 99

  verb look committable instances 3
    recipe glance
      craftable
      duration 0
      when legacy.luck >= 3
      effects
        narrate "you see it"
"#;

fn action(verb: &str) -> CommitAction {
    CommitAction {
        verb: VerbId::from(verb),
        bindings: Vec::<(SlotName, story_core::EntityId)>::new(),
    }
}

mod install {
    use super::*;

    #[test]
    fn with_state_swaps_only_the_state() {
        let src = compile(SRC);
        let mut e1 = game(SRC);
        e1.advance(10).unwrap();
        let fresh = Engine::boot(src.clone(), EffectRegistry::new(), 1, None).0;
        let e2 = fresh.with_state(e1.state().clone());
        assert_eq!(state_bytes(&e1), state_bytes(&e2));
        // Both engines take identical next steps from the installed state.
        let ev1 = e1.commit(action("cast")).unwrap();
        let mut e2 = e2;
        let ev2 = e2.commit(action("cast")).unwrap();
        assert_eq!(ev1, ev2);
        assert_eq!(state_bytes(&e1), state_bytes(&e2));
    }

    #[test]
    fn state_mut_writes_in_place() {
        let mut e = game(SRC);
        e.state_mut().legacy.insert("luck".to_string(), 7);
        assert_eq!(e.state().legacy["luck"], 7);
        // The engine reads the write through its ordinary face.
        let when = e
            .content()
            .recipe("look.glance")
            .and_then(|r| r.when.clone())
            .expect("fixture has a when");
        assert!(e.eval_cond(&when));
    }

    #[test]
    fn clone_is_independent() {
        let mut e1 = game(SRC);
        let e2 = e1.clone();
        e1.commit(action("cast")).unwrap();
        assert_eq!(e2.state().legacy["luck"], 0);
        assert_ne!(state_bytes(&e1), state_bytes(&e2));
    }
}

mod evaluation_face {
    use super::*;

    #[test]
    fn context_free_condition_and_scopes() {
        let mut e = game(SRC);
        let when = e
            .content()
            .recipe("look.glance")
            .and_then(|r| r.when.clone())
            .expect("fixture has a when");
        assert!(!e.eval_cond(&when));
        e.state_mut().legacy.insert("luck".to_string(), 3);
        assert!(e.eval_cond(&when));
        // Context-free scopes: world enumerates the two boot tools.
        let world = e.scope_entities(&Scope::World);
        assert_eq!(world.len(), 2);
        // Context-free term matching.
        assert!(e.matches(world[0], &[QueryTerm::Kind("gadget".to_string())]));
        // `counter` reads 0 without a recipe context.
        assert_eq!(e.eval_expr(&Expr::Counter), 0);
    }
}

mod fork {
    use super::*;

    /// A recorder: captures every chance point; `choose` decides the outcome.
    struct Recorder {
        seen: RefCell<Vec<ChancePoint>>,
        roll: Option<i64>,
        draw: Option<Vec<story_core::CardId>>,
    }

    impl Recorder {
        fn recording(roll: Option<i64>, draw: Option<Vec<story_core::CardId>>) -> Self {
            Recorder {
                seen: RefCell::new(Vec::new()),
                roll,
                draw,
            }
        }

        fn hook(&self, point: &ChancePoint) -> Option<ChanceOutcome> {
            self.seen.borrow_mut().push(point.clone());
            match point {
                ChancePoint::Roll { .. } => self.roll.map(ChanceOutcome::Roll),
                ChancePoint::Draw { .. } => self.draw.clone().map(ChanceOutcome::Draw),
            }
        }

        fn points(&self) -> Vec<ChancePoint> {
            self.seen.borrow().clone()
        }
    }

    #[test]
    fn forked_commit_without_choices_matches_plain_commit() {
        let mut plain = game(SRC);
        let mut forked = game(SRC);
        let plain_events = plain.commit(action("cast")).unwrap();
        let rec = Recorder::recording(None, None);
        let mut hook = |p: &ChancePoint| rec.hook(p);
        let forked_events = forked.commit_forked(action("cast"), &mut hook).unwrap();
        assert_eq!(plain_events, forked_events);
        assert_eq!(state_bytes(&plain), state_bytes(&forked));
        assert_eq!(rec.points().len(), 1);
        assert_eq!(
            rec.points()[0],
            ChancePoint::Roll {
                count: 2,
                sides: 6,
                bonus: 1,
                bind: "sight".to_string()
            }
        );
    }

    #[test]
    fn recorded_roll_outcomes_replay_bit_for_bit() {
        let mut first = game(SRC);
        let rec = Recorder::recording(Some(41), None);
        let mut hook = |p: &ChancePoint| rec.hook(p);
        first.commit_forked(action("cast"), &mut hook).unwrap();
        assert_eq!(first.state().legacy["luck"], 41);

        let mut second = game(SRC);
        let rec2 = Recorder::recording(Some(41), None);
        let mut hook2 = |p: &ChancePoint| rec2.hook(p);
        second.commit_forked(action("cast"), &mut hook2).unwrap();
        assert_eq!(state_bytes(&first), state_bytes(&second));
    }

    #[test]
    fn count_zero_roll_is_not_a_chance_point() {
        let mut e = game(SRC);
        let before = e.state().rng_counter;
        let rec = Recorder::recording(Some(99), None);
        let mut hook = |p: &ChancePoint| rec.hook(p);
        e.commit_forked(action("whiff"), &mut hook).unwrap();
        assert!(rec.points().is_empty());
        assert_eq!(e.state().legacy["luck"], 5);
        assert_eq!(e.state().rng_counter, before);
    }

    #[test]
    fn invalid_roll_operands_stay_an_effect_failure() {
        let mut e = game(SRC);
        let before = state_bytes(&e);
        let rec = Recorder::recording(Some(1), None);
        let mut hook = |p: &ChancePoint| rec.hook(p);
        let err = e.commit_forked(action("botch"), &mut hook).unwrap_err();
        assert_eq!(err.kind, story_content::runtime::DiagKind::EffectFailure);
        assert!(rec.points().is_empty());
        assert_eq!(state_bytes(&e), before);
    }

    #[test]
    fn forked_advance_draws_the_recorded_cards() {
        let mut e = game(SRC);
        e.commit(action("pick")).unwrap();
        let prizes: Vec<story_core::CardId> =
            ["prize", "prize"].iter().map(|n| n.to_string().into()).collect();
        let rec = Recorder::recording(None, Some(prizes));
        let mut hook = |p: &ChancePoint| rec.hook(p);
        let ev = e.advance_forked(10, &mut hook).unwrap();
        // Both draws minted prizes into the declared zone, in order.
        let prize_count = e
            .state()
            .entities
            .values()
            .filter(|ent| ent.name == "prize")
            .count();
        assert_eq!(prize_count, 2);
        assert_eq!(e.state().decks[&story_core::DeckId::from("bag")], 2);
        assert_eq!(rec.points().len(), 1);
        match &rec.points()[0] {
            ChancePoint::Draw { deck, count, into } => {
                assert_eq!(deck.as_str(), "bag");
                assert_eq!(*count, 2);
                assert!(
                    matches!(into, Some(story_content::ir::Into::Zone(z)) if z == "bench"),
                    "expected `into bench`, got {into:?}"
                );
            }
            other => panic!("expected a Draw point, got {other:?}"),
        }
        // Replay the same recorded outcome through a fresh engine.
        let mut replay = game(SRC);
        replay.commit(action("pick")).unwrap();
        let rec2 = Recorder::recording(None, Some(vec![
            story_core::CardId::from("prize"),
            story_core::CardId::from("prize"),
        ]));
        let mut hook2 = |p: &ChancePoint| rec2.hook(p);
        let ev2 = replay.advance_forked(10, &mut hook2).unwrap();
        assert_eq!(ev, ev2);
        assert_eq!(state_bytes(&e), state_bytes(&replay));
    }

    #[test]
    fn deterministic_draw_path_survives_a_none_hook() {
        let mut plain = game(SRC);
        plain.commit(action("pick")).unwrap();
        plain.advance(10).unwrap();

        let mut forked = game(SRC);
        forked.commit(action("pick")).unwrap();
        let rec = Recorder::recording(None, None);
        let mut hook = |p: &ChancePoint| rec.hook(p);
        forked.advance_forked(10, &mut hook).unwrap();
        assert_eq!(state_bytes(&plain), state_bytes(&forked));
        assert_eq!(rec.points().len(), 1);
    }

    #[test]
    fn draw_past_the_permutation_takes_the_kernel_path() {
        // The bag holds 3 entries; a draw of 4 wraps through the permutation,
        // so no chance point fires and the hook is never consulted.
        let mut e = game(SRC);
        e.commit(action("sweep")).unwrap();
        let rec = Recorder::recording(None, Some(vec![story_core::CardId::from("prize")]));
        let mut hook = |p: &ChancePoint| rec.hook(p);
        e.advance_forked(10, &mut hook).unwrap();
        assert!(rec.points().is_empty());
        assert_eq!(e.state().decks[&story_core::DeckId::from("bag")], 4);
        assert_eq!(
            e.state().entities.values().filter(|ent| ent.name == "prize").count(),
            4
        );
    }

    #[test]
    fn t4_rollback_stays_kernel_side_under_fork() {
        let mut e = game(SRC);
        e.commit(action("doom")).unwrap();
        let before_rng = e.state().rng_counter;
        let before_entities = e.state().entities.clone();
        let rec = Recorder::recording(Some(3), None);
        let mut hook = |p: &ChancePoint| rec.hook(p);
        let ev = e.advance_forked(30, &mut hook).unwrap();
        // The roll fired as a chance point, the consume failed, and the unit
        // rolled back: a ResolutionFailed event, the situation aborted, and
        // the entry state restored (rng included).
        assert_eq!(rec.points().len(), 1);
        assert!(matches!(
            &rec.points()[0],
            ChancePoint::Roll { count: 1, sides: 6, bonus: 2, .. }
        ));
        assert!(ev.iter().any(|e| matches!(e, Event::ResolutionFailed { .. })));
        assert!(e.state().situations.is_empty());
        // The unit rolled back (rng and deck cursors included) while the
        // advance itself still spent its time: everything but `now` matches
        // the entry state.
        assert_eq!(e.state().now, 30);
        assert_eq!(e.state().rng_counter, before_rng);
        assert_eq!(e.state().legacy["luck"], 0);
        assert_eq!(e.state().entities, before_entities);
    }

    #[test]
    fn budget_constants_are_unchanged() {
        assert_eq!(MAX_RESOLUTIONS_PER_ADVANCE, 4096);
        assert_eq!(MAX_RESOLUTION_DEPTH, 128);
    }
}
