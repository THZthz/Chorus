//! `continue via` — same-situation continuation (spec.md §4.2): the recipe's
//! execution runs its effects, consuming settlement, and books, and then the
//! situation adopts the target recipe instead of completing — bindings
//! released into the interior as unbound storage, the timer re-armed, and a
//! `Continued` event emitted. The arms re-check pre-materialization: a target
//! whose `when` misses the situation's interior sums — or whose `max` is
//! spent — falls through to the normal completion, and adoption spends one
//! `max` count. Instant self-continuation is bounded by the resolution
//! budget (Livelock).

mod common;

use common::{boot, compile, count_named, narration, situations, state_bytes, zone_kind_of};
use story_content::runtime::DiagKind;
use story_core::{EntityId, RecipeId};
use story_engine::Event;

fn game(src: &str) -> story_engine::Engine {
    boot(compile(src), 1)
}

/// The oldest and second-oldest live entities of a card name.
fn two_named(e: &story_engine::Engine, name: &str) -> (EntityId, EntityId) {
    let mut ids: Vec<_> = e
        .state()
        .entities
        .values()
        .filter(|ent| ent.name == name)
        .map(|ent| ent.id)
        .collect();
    ids.sort();
    (
        *ids.first().unwrap_or_else(|| panic!("no live `{name}`")),
        *ids.get(1).unwrap_or_else(|| panic!("no second live `{name}`")),
    )
}

const HAPPY: &str = r#"
game "Continue"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coal
    stackable
    aspects spark 1
    start in hall count 2

verbs
  verb ritual committable instances 1
    recipe stage
      duration 10m
      slots
        fuel: 1..1 aspect:spark
        ash: 1..1 consuming aspect:spark
      effects
        narrate "stage"
        continue via storm
    recipe storm
      priority 20
      duration 5m
      when count(interior, aspect:spark) >= 1
      effects
        narrate "storm"
"#;

/// (a) The happy path: at settlement the situation adopts the target —
/// `Continued` fires instead of `SituationCompleted`, the non-consuming
/// binding is released into the interior as unbound storage (not to the
/// table, not re-bound), the consuming slot's card burned at the step, and
/// the timer re-arms at `now + <target duration>`. The adopted recipe then
/// completes normally at its own end moment.
#[test]
fn continue_adopts_the_target_releases_storage_and_rearms_the_timer() {
    let mut e = game(HAPPY);
    let (coal1, coal2) = two_named(&e, "coal");
    common::commit(&mut e, "ritual", &[("fuel", coal1), ("ash", coal2)])
        .expect("stage opens");
    let sit = situations(&e)[0];

    let events = e.advance(10).expect("stage settles at 10m");
    assert_eq!(narration(&events), vec!["stage"]);
    // The step reported continuation, not completion.
    assert!(
        !events
            .iter()
            .any(|ev| matches!(ev, Event::SituationCompleted { .. }))
    );
    let continued: Vec<_> = events
        .iter()
        .filter_map(|ev| match ev {
            Event::Continued { situation, recipe } => Some((*situation, recipe.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(continued, vec![(sit, RecipeId::from("ritual.storm"))]);

    // The situation went on: same instance, target recipe, timer re-armed at
    // 10 + 5, the burned consuming slot gone, the fuel card storage inside.
    assert_eq!(situations(&e), vec![sit]);
    let s = &e.state().situations[&sit];
    assert_eq!(s.recipe.as_str(), "ritual.storm");
    assert_eq!(s.ends_at, 15);
    assert!(
        s.bindings.values().all(Vec::is_empty),
        "every binding was released into the interior"
    );
    assert_eq!(count_named(&e, "coal"), 1, "the consuming slot burned");
    assert_eq!(zone_kind_of(&e, coal1), "interior", "storage stays inside");

    // The adopted recipe completes at its own end moment; the interior
    // leftovers go home.
    let events = e.advance(5).expect("storm settles at 15m");
    assert_eq!(narration(&events), vec!["storm"]);
    assert!(events.iter().any(|ev| matches!(
        ev,
        Event::SituationCompleted { recipe, .. } if recipe.as_str() == "ritual.storm"
    )));
    assert!(situations(&e).is_empty());
    assert_eq!(count_named(&e, "coal"), 1);
    assert_eq!(zone_kind_of(&e, coal1), "table", "storage went home");
}

const WHEN_FAILS: &str = r#"
game "Fallthrough"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0
  ghost: 0

cards
  card coal
    stackable
    aspects spark 1
    start in hall count 2

verbs
  verb ritual committable instances 1
    recipe stage
      duration 10m
      slots
        fuel: 1..1 aspect:spark
        ash: 1..1 consuming aspect:spark
      effects
        narrate "stage"
        continue via storm
    recipe storm
      priority 20
      duration 5m
      when count(interior, aspect:ghost) >= 1
      effects
        narrate "storm"
"#;

/// (b) The target's `when` re-checks against the situation's interior sums
/// (released storage + new bindings); on a miss the situation falls through
/// to the normal completion sequence — no `Continued`, and the interior
/// leftovers go home.
#[test]
fn when_missing_the_interior_sums_falls_through_to_completion() {
    let mut e = game(WHEN_FAILS);
    let (coal1, coal2) = two_named(&e, "coal");
    common::commit(&mut e, "ritual", &[("fuel", coal1), ("ash", coal2)])
        .expect("stage opens");

    let events = e.advance(10).expect("stage settles");
    assert_eq!(narration(&events), vec!["stage"]);
    assert!(
        !events
            .iter()
            .any(|ev| matches!(ev, Event::Continued { .. })),
        "a failed adoption is not a continuation"
    );
    let completed: Vec<_> = events
        .iter()
        .filter_map(|ev| match ev {
            Event::SituationCompleted { recipe, .. } => Some(recipe.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        completed,
        vec![RecipeId::from("ritual.stage")],
        "the executing recipe completed"
    );
    assert!(situations(&e).is_empty());
    assert_eq!(count_named(&e, "coal"), 1, "only the consuming slot burned");
    assert_eq!(zone_kind_of(&e, coal1), "table", "the released storage went home");
}

const BOOKS: &str = r#"
game "PerStep"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card keeper
    stackable
    start in hall
    on complete
      rule beat
        effects
          narrate "keeper-book"

verbs
  verb ritual committable instances 1
    recipe stage
      duration 10m
      slots
        held: 1..1
      effects
        narrate "stage"
        continue via storm
      on complete
        rule own
          effects
            narrate "stage-book"
    recipe storm
      priority 20
      duration 5m
      effects
        narrate "storm"
      on complete
        rule own2
          effects
            narrate "storm-book"
"#;

/// (c) Books fire per recipe execution: each continue step walks the recipe
/// book and card books exactly like a completion's book phase — and the
/// card released into the interior by the previous step walks again as
/// unbound interior storage at the next step.
#[test]
fn books_fire_at_every_continue_step() {
    let mut e = game(BOOKS);
    common::commit_named(&mut e, "ritual", &[("held", "keeper")]).expect("stage opens");

    // Step 1: stage's own book, then the bound keeper's book.
    let events = e.advance(10).expect("stage settles");
    assert_eq!(
        narration(&events),
        vec!["stage", "stage-book", "keeper-book"],
        "recipe book first, card book second"
    );
    assert!(
        events
            .iter()
            .any(|ev| matches!(ev, Event::Continued { .. }))
    );

    // Step 2: storm's books; the keeper is now unbound interior storage and
    // still walks (the widened card-book walk).
    let events = e.advance(5).expect("storm settles");
    assert_eq!(
        narration(&events),
        vec!["storm", "storm-book", "keeper-book"],
        "the adopted recipe's books fired; the released storage card's book walked again"
    );
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::SituationCompleted { .. })));
}

const TIMER_LOOP: &str = r#"
game "Heartbeat"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb pulse committable instances any
    recipe tick
      duration 5m
      effects
        narrate "tick"
        continue via tick
"#;

/// (f, timer) A recipe continuing to itself is legal: the loop is bounded by
/// the re-armed timer — exactly one step per re-arm, nothing in between.
#[test]
fn self_continue_loops_once_per_rearmed_timer() {
    let mut e = game(TIMER_LOOP);
    common::commit_named(&mut e, "pulse", &[]).expect("opens, ends at 5");
    let sit = situations(&e)[0];

    let events = e.advance(5).expect("first tick");
    assert_eq!(narration(&events), vec!["tick"]);
    assert_eq!(e.state().situations[&sit].ends_at, 10);

    // The in-between window is silent: the timer, not the tick, bounds it.
    let events = e.advance(3).expect("no tick yet");
    assert!(narration(&events).is_empty());
    let events = e.advance(2).expect("second tick");
    assert_eq!(narration(&events), vec!["tick"]);
    let events = e.advance(5).expect("third tick");
    assert_eq!(narration(&events), vec!["tick"]);

    assert_eq!(situations(&e), vec![sit], "the same instance loops");
    let s = &e.state().situations[&sit];
    assert_eq!(s.recipe.as_str(), "pulse.tick");
    assert_eq!(s.ends_at, 20);
}

const INSTANT_LOOP: &str = r#"
game "Flash"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb strobe committable instances 1
    recipe flash
      duration 0
      effects
        narrate "flash"
        continue via flash
"#;

/// (f, budget) An instant recipe continuing to itself resolves inline at
/// every step; each step charges the budget, so the loop bottoms out in the
/// Livelock rollback — the whole commit restores bit-for-bit.
#[test]
fn instant_self_continue_over_budget_livelocks_and_rolls_back() {
    let mut e = game(INSTANT_LOOP);
    let before = state_bytes(&e);
    let err = common::commit_named(&mut e, "strobe", &[]).expect_err("livelocks");
    assert_eq!(err.kind, DiagKind::Livelock);
    assert_eq!(state_bytes(&e), before, "the whole commit rolled back");
}

const MAXED_LOOP: &str = r#"
game "Maxed"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb pulse committable instances any
    recipe tick
      max 2
      duration 5m
      effects
        narrate "tick"
        continue via tick
"#;

/// (g) `max` is consulted on the continuation walk (spec.md §4.2): a
/// max-exhausted target misses like a failed `when`, so a self-continue loop
/// stops at its cap, and each adoption spends one count — adoption
/// materializes the new recipe.
#[test]
fn max_exhausted_target_misses_and_adoption_spends_a_count() {
    let mut e = game(MAXED_LOOP);
    common::commit_named(&mut e, "pulse", &[]).expect("opens; the commit spent count 1");
    let sit = situations(&e)[0];

    // Step 1: one count is left; the walk adopts and spends it.
    let events = e.advance(5).expect("first tick");
    assert_eq!(narration(&events), vec!["tick"]);
    assert!(
        events.iter().any(|ev| matches!(ev, Event::Continued { .. })),
        "the cap had room, so the arm adopted"
    );
    assert_eq!(e.state().situations[&sit].recipe.as_str(), "pulse.tick");
    assert_eq!(
        common::recipe_runs(&e, "pulse.tick"),
        2,
        "adoption spent the second count"
    );

    // Step 2: the cap is spent; the arm misses like a failed `when` and the
    // situation completes normally.
    let events = e.advance(5).expect("second tick");
    assert_eq!(narration(&events), vec!["tick"]);
    assert!(
        !events
            .iter()
            .any(|ev| matches!(ev, Event::Continued { .. })),
        "a max-exhausted target is not adopted"
    );
    assert!(events.iter().any(|ev| matches!(
        ev,
        Event::SituationCompleted { recipe, .. } if recipe.as_str() == "pulse.tick"
    )));
    assert!(situations(&e).is_empty());
    assert_eq!(common::recipe_runs(&e, "pulse.tick"), 2, "the cap held");
}
