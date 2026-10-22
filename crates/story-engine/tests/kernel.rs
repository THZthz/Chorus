//! Kernel behavior tests over small fixtures: completion order (T12), the
//! per-unit failure policy (T4), instant recipes (inline open+complete, the
//! recursion budget, the autonomous pulse), commit atomicity (T3), total
//! locks (T8), context-free expiry books (T9), and the chain failure
//! taxonomy (T10).

mod common;

use common::{boot, compile, count_named, narration, recipe_runs, situations, state_bytes};
use story_content::runtime::DiagKind;
use story_engine::{CommitAction, EffectRegistry, Engine, Event, StartSkipReason};

fn game(src: &str) -> Engine {
    boot(compile(src), 1)
}

mod order {
    use super::*;

    const SRC: &str = r#"
game "Order"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  stamped: 0

cards
  card alpha
    stackable
    start in hall
    on complete
      rule announce
        effects
          narrate "card-alpha"

  card beta
    stackable
    start in hall
    on complete [stamped]
      rule qualified
        effects
          narrate "card-beta-qualified"
    on complete
      rule plain
        effects
          narrate "card-beta-plain"

  card gamma
    stackable
    start in hall

verbs
  verb ritual committable instances 1
    recipe cast
      duration 0
      declares stamped
      slots
        focus: 1..1
        second: 1..1
      effects
        narrate "effects-first"
      on complete
        rule book
          effects
            narrate "recipe-book"
"#;

    /// T12: the recipe's book fires before the card books; card books are
    /// deduplicated across slots and ordered by first bind; the
    /// aspect-qualified book fires only under a matching `declares`; an
    /// instant completion keeps the whole order inside the one commit.
    #[test]
    fn completion_order_recipe_book_then_card_books_in_bind_order() {
        let mut e = game(SRC);
        let alpha = common::named(&e, "alpha");
        let beta = common::named(&e, "beta");
        let gamma = common::named(&e, "gamma");

        // alpha first, beta second: alpha's book walks before beta's.
        let events = common::commit(&mut e, "ritual", &[("focus", alpha), ("second", beta)])
            .expect("instant commit");
        assert_eq!(
            narration(&events),
            vec![
                "effects-first",
                "recipe-book",
                "card-alpha",
                "card-beta-qualified",
                "card-beta-plain",
            ],
            "recipe book first, then card books in bind order"
        );
        assert!(events
            .iter()
            .any(|ev| matches!(ev, Event::SituationCompleted { .. })));

        // Binding beta first swaps the card-book order.
        let events = common::commit(&mut e, "ritual", &[("focus", beta), ("second", alpha)])
            .expect("instant commit");
        assert_eq!(
            narration(&events),
            vec![
                "effects-first",
                "recipe-book",
                "card-beta-qualified",
                "card-beta-plain",
                "card-alpha",
            ]
        );

        // The same card in both slots walks once (deduplicated).
        let events = common::commit(&mut e, "ritual", &[("focus", alpha), ("second", alpha)])
            .expect("instant commit");
        assert_eq!(
            narration(&events),
            vec!["effects-first", "recipe-book", "card-alpha"]
        );

        // A card without a book stays silent; the rest of the order holds.
        let events = common::commit(&mut e, "ritual", &[("focus", gamma), ("second", alpha)])
            .expect("instant commit");
        assert_eq!(
            narration(&events),
            vec!["effects-first", "recipe-book", "card-alpha"]
        );
    }
}

mod interior_books {
    use super::*;

    /// The card-book walk covers ALL interior cards: the bound bindings first
    /// in bind order, then unbound interior storage in mint order. A card
    /// spawned into the interior during the effects gets its completion book
    /// walked like any bound card (kernel-api.md §8).
    #[test]
    fn unbound_interior_storage_cards_walk_their_books() {
        let src = r#"
game "StorageBooks"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card holder
    stackable
    start in hall
    on complete
      rule announce
        effects
          narrate "holder-book"
  card note
    stackable
    start in hall
    on complete
      rule ping
        effects
          narrate "note-book"

verbs
  verb ritual committable instances 1
    recipe cast
      duration 0
      slots
        h: 1..1
      effects
        spawn note
        narrate "cast"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "ritual", &[("h", "holder")])
            .expect("instant commit");
        assert_eq!(
            narration(&events),
            vec!["cast", "holder-book", "note-book"],
            "bound cards first in bind order, then unbound interior storage in mint order"
        );
    }

    /// The aspect-qualified book's second catalyst pass, with its per-stack
    /// key constraint: a `on complete [k]` book fires through the pool half
    /// when the situation's interior aspect pool contains `k` — bound cards
    /// included — AND the walking card's own declaration (element id and
    /// aspect keys) carries `k`; a recipe `declares` fires it anywhere. A
    /// card that does not itself carry `k` stays silent even when the pool
    /// holds it (a stack's candidate triggers are only those declared on its
    /// own elements).
    #[test]
    fn qualified_books_fire_on_the_interior_aspect_pool() {
        let src = r#"
game "PoolGate"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  cred: 0

cards
  card key
    stackable
    aspects cred 1
    start in hall count 2
    on complete [cred]
      rule unlock
        effects
          narrate "key-unlock"
  card blank
    stackable
    start in hall count 2
    on complete [cred]
      rule never
        effects
          narrate "blank-unlock"

verbs
  verb gate committable instances any
    recipe open
      duration 0
      slots
        k: 1..1
        d: 1..1
      effects
        narrate "opened"
"#;
        // The bound key's own `cred` aspect both fills the interior pool and
        // sits on the key card's own declaration: the key's `[cred]` book
        // fires through the pool pass; the blank card carries no `cred` on
        // its own declaration, so its `[cred]` book stays silent — the pool
        // half alone no longer fires a book on a stack that cannot declare
        // the trigger.
        let mut e = game(src);
        let key1 = common::named(&e, "key");
        let blank1 = e
            .state()
            .entities
            .values()
            .find(|c| c.name == "blank")
            .map(|c| c.id)
            .unwrap();
        let events = common::commit(&mut e, "gate", &[("k", key1), ("d", blank1)])
            .expect("instant commit");
        assert_eq!(
            narration(&events),
            vec!["opened", "key-unlock"],
            "the pool holds cred and the key card carries it; the blank card does not, so only its own silence"
        );

        // Without any cred carrier in the interior, the same books stay
        // silent.
        let mut e = game(src);
        let blank2 = common::named(&e, "blank");
        let blank3 = e
            .state()
            .entities
            .values()
            .filter(|c| c.name == "blank")
            .map(|c| c.id)
            .nth(1)
            .unwrap();
        let events = common::commit(&mut e, "gate", &[("k", blank2), ("d", blank3)])
            .expect("instant commit");
        assert_eq!(narration(&events), vec!["opened"], "the pool lacks cred");
    }

    /// The pass-1 half is conjunct-free: a recipe `declares` fires the
    /// `[k]` book on every walking card whose declaration carries `k`,
    /// regardless of the pool — and the recipe's own declaration reaches
    /// cards whose own keys do not name the catalyst.
    #[test]
    fn declared_books_fire_without_the_pool() {
        let src = r#"
game "DeclGate"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  cred: 0

cards
  card tome
    stackable
    start in hall
    on complete [cred]
      rule unlock
        effects
          narrate "tome-unlock"

verbs
  verb gate committable instances any
    recipe open
      duration 0
      declares cred
      slots
        t: 1..1
      effects
        narrate "opened"
"#;
        let mut e = game(src);
        let tome = common::named(&e, "tome");
        let events = common::commit(&mut e, "gate", &[("t", tome)]).expect("instant commit");
        assert_eq!(
            narration(&events),
            vec!["opened", "tome-unlock"],
            "the declaring recipe fires the tome's [cred] book with an empty pool"
        );
    }
}

mod failure {
    use super::*;

    const SRC: &str = r#"
game "Fragile"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

verbs
  verb failboat autonomous instances 1
    recipe snap
      duration 10m
      effects
        consume aspect:ghost 1

  verb okbot autonomous instances 1
    recipe hum
      duration 10m
      effects
        narrate "still-humming"
"#;

    /// T4: an effect failure rolls the unit back to its checkpoint, aborts the
    /// unit's situation through the release path with `ResolutionFailed`, and
    /// the advance continues — a failed resolution is normal traffic, never a
    /// hard-stuck run.
    #[test]
    fn per_unit_failure_aborts_and_the_advance_continues() {
        let mut e = game(SRC);
        assert_eq!(situations(&e).len(), 2, "the boot checkpoint opened both");

        let events = e.advance(10).expect("advance");
        assert!(events.iter().any(|ev| matches!(
            ev,
            Event::ResolutionFailed { reason, .. } if reason.contains("consume shortage")
        )));
        assert_eq!(narration(&events), vec!["still-humming"]);

        // The checkpoint at the settled moment reopens both verbs; the next
        // advance works untouched.
        assert_eq!(situations(&e).len(), 2);
        let events = e.advance(10).expect("advance");
        assert_eq!(narration(&events), vec!["still-humming"]);
        assert!(events
            .iter()
            .any(|ev| matches!(ev, Event::ResolutionFailed { .. })));
    }

    /// Checkpoint granularity is per verb: a later verb's instant-completion
    /// failure rolls back that verb's open only — an earlier verb's instance
    /// survives with its events — and the failed situation is named while it
    /// is still live, before its rollback.
    #[test]
    fn checkpoint_failure_keeps_sibling_opens() {
        let src = r#"
game "CpFail"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

cards
  card token
    stackable
    start in hall

verbs
  verb keeper autonomous instances 1
    recipe hold
      duration 10m
      effects
        narrate "held"

  verb broken autonomous instances 1
    recipe snap
      duration 0
      effects
        consume aspect:ghost 1
"#;
        let mut e = game(src);
        let events = e.advance(10).expect("advance");
        assert_eq!(
            narration(&events),
            vec!["held"],
            "the sibling's events survive the failure"
        );
        let failed: Vec<_> = events
            .iter()
            .filter_map(|ev| match ev {
                Event::ResolutionFailed { sit, reason } => Some((*sit, reason.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(failed.len(), 1, "exactly one failure reported");
        let &(failed_sit, ref reason) = failed.as_slice().first().unwrap();
        assert!(reason.contains("consume shortage"));
        let live = situations(&e);
        assert_eq!(live.len(), 1, "only the keeper's instance is live");
        assert!(
            !live.contains(&failed_sit),
            "the reported id is the failed one"
        );
        assert_eq!(
            e.state().situations[&live[0]].recipe.as_str(),
            "keeper.hold"
        );
    }
}

mod instant {
    use super::*;

    const SRC: &str = r#"
game "Instant"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1

verbs
  verb poke committable instances 1
    recipe jab
      duration 0
      effects
        spawn coin
        narrate "poked"

  verb loop committable instances any
    recipe recurse
      duration 0
      effects
        start loop

  verb pulse autonomous instances 1
    recipe tick
      duration 0
      effects
        narrate "ticked"
"#;

    /// A `duration 0` commit completes inline: the open and completion events
    /// return together and the interior leftovers go home (spec.md §4.2).
    #[test]
    fn instant_commit_opens_and_completes_together() {
        let mut e = game(SRC);
        let events = common::commit_named(&mut e, "poke", &[]).expect("commit");
        assert_eq!(narration(&events), vec!["poked"]);
        assert!(events
            .iter()
            .any(|ev| matches!(ev, Event::SituationCompleted { .. })));
        assert_eq!(count_named(&e, "coin"), 1);
        let coin = common::named(&e, "coin");
        assert_eq!(common::zone_kind_of(&e, coin), "table", "leftovers go home");
        assert_eq!(recipe_runs(&e, "poke.jab"), 1);
        // The boot checkpoint already pulsed the autonomous instant verb.
        e.advance(50).expect("advance to 50m");
        let events = e.advance(10).expect("one more checkpoint");
        assert_eq!(narration(&events), vec!["ticked"], "exactly one pulse");
        assert_eq!(recipe_runs(&e, "pulse.tick"), 3, "boot, 50m, and 60m");
    }

    /// The recursion budget: an instant self-start chain bottoms out at the
    /// native depth cap and takes the Livelock rollback path (T5) — no coin,
    /// no run counters — on an ordinary main thread, no big-stack harness.
    #[test]
    fn instant_chain_over_budget_livelocks_and_rolls_back() {
        let mut e = game(SRC);
        let before = state_bytes(&e);
        let err = common::commit_named(&mut e, "loop", &[]).expect_err("livelocks");
        assert_eq!(err.kind, DiagKind::Livelock);
        assert_eq!(state_bytes(&e), before, "the whole commit rolled back");
    }
}

mod atomicity {
    use super::*;

    const SRC: &str = r#"
game "Dice"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0
  ghost: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall count 2

verbs
  verb risky committable instances 1
    recipe gamble
      duration 0
      effects
        roll 1d6 as fate
        consume aspect:ghost 1

  verb safe committable instances 1
    recipe store
      duration 5m
      slots
        loot: 1..1 aspect:spark

  verb taker committable instances 1
    recipe steal
      duration 5m
      slots
        loot: 1..1 aspect:spark
"#;

    /// Boot warnings are traffic, not refusals: a boot instance skipped by a
    /// uniqueness collision and an undeclared host legacy key report as
    /// warnings, and play proceeds (the key folds anyway).
    #[test]
    fn boot_warns_on_skipped_mint_and_undeclared_legacy_key() {
        let src = r#"
game "BootWarn"
  seed host
  turn 1m
  home hall

zones
  hall

legacies
  favor: 0

cards
  card idol
    unique
    start in hall count 2
"#;
        let content = compile(src);
        let mut legacy = std::collections::BTreeMap::new();
        legacy.insert("favor".to_string(), 1);
        legacy.insert("mystery".to_string(), 5);
        let (e, diags) = Engine::boot(content, EffectRegistry::new(), 1, Some(legacy));
        assert!(!diags.iter().any(|d| d.is_error()), "{diags:?}");
        assert!(
            diags
                .iter()
                .any(|d| d.message.contains("boot instance of `idol` skipped")),
            "{diags:?}"
        );
        assert!(
            diags.iter().any(|d| d
                .message
                .contains("host legacy key `mystery` is not declared")),
            "{diags:?}"
        );
        assert_eq!(
            count_named(&e, "idol"),
            1,
            "the colliding boot mint skipped"
        );
        assert_eq!(
            e.legacy_out().get("mystery"),
            Some(&5),
            "the key folds anyway"
        );
    }

    /// T3 + T11: an effect failure mid-commit — after the die rolled —
    /// returns the refusal with the state bit-restored, rng counter included,
    /// and zero time spent; a retried identical commit re-rolls identical
    /// dice (T6).
    #[test]
    fn commit_effect_failure_restores_bit_for_bit() {
        let mut e = game(SRC);
        let before = state_bytes(&e);
        let now_before = e.state().now;
        let err = common::commit_named(&mut e, "risky", &[]).expect_err("consume shortage");
        assert_eq!(err.kind, DiagKind::EffectFailure);
        assert_eq!(
            state_bytes(&e),
            before,
            "rng counter restored with the state"
        );
        assert_eq!(e.state().now, now_before);

        // A twin engine retries the same failing commit, then plays on: the
        // retried dice and the follow-up are identical (bit-restored).
        let mut twin = game(SRC);
        common::commit_named(&mut twin, "risky", &[]).expect_err("same failure");
        common::commit_named(&mut twin, "safe", &[("loot", "coin")]).expect("opens");
        common::commit_named(&mut e, "safe", &[("loot", "coin")]).expect("opens");
        assert_eq!(state_bytes(&e), state_bytes(&twin));
    }

    /// T8: locks are total — a card bound by any live instance refuses every
    /// other ask, whatever situation it comes from.
    #[test]
    fn locks_are_total_across_situations() {
        let mut e = game(SRC);
        let coin = common::named(&e, "coin");
        common::commit_named(&mut e, "safe", &[("loot", "coin")]).expect("binds the coin");
        let err = common::commit_named(&mut e, "taker", &[("loot", "coin")])
            .expect_err("locked by the live instance");
        assert_eq!(err.kind, DiagKind::EntityLocked);
        // Any other bind of the card is locked too, from any asking side.
        let sit = e.state().situations.keys().copied().next().unwrap();
        let err = e.bind(sit, "loot".into(), coin).expect_err("locked");
        assert_eq!(err.kind, DiagKind::EntityLocked);
    }
}

mod expiry {
    use super::*;

    const SRC: &str = r#"
game "Decay"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card ephemeral
    lifetime 5m
    aspects spark 1
    start in hall
    on expire
      rule linger when total(bound, aspect:spark) == 0 and count(interior, aspect:spark) == 0
        effects
          narrate "expired-clean"

verbs
  verb binder committable instances 1
    recipe hold
      duration 10m
      slots
        kept: 1..1 aspect:spark
"#;

    /// T9: expiry books are context-free — `bound` and `interior` read empty —
    /// and the book replaces default destruction.
    #[test]
    fn expiry_book_is_context_free_and_replaces_destruction() {
        let mut e = game(SRC);

        let events = e.advance(5).expect("advance to the due moment");
        assert_eq!(narration(&events), vec!["expired-clean"]);
        assert_eq!(
            count_named(&e, "ephemeral"),
            1,
            "the book replaced destruction"
        );
        let card = common::named(&e, "ephemeral");
        assert_eq!(e.state().entities[&card].expires_at, None, "one-shot timer");
    }

    /// A bound card's expiry is deferred: it fires on the next resolution
    /// pass after release (spec.md §4.6).
    #[test]
    fn expiry_is_deferred_while_bound_and_fires_after_release() {
        let mut e = game(SRC);
        let card = common::named(&e, "ephemeral");
        common::commit_named(&mut e, "binder", &[("kept", "ephemeral")]).expect("bound");

        // Past due while bound: nothing fires, the timer waits.
        let events = e.advance(5).expect("to the due moment");
        assert!(narration(&events).is_empty(), "bound cards do not expire");
        assert_eq!(e.state().entities[&card].expires_at, Some(5));
        assert_eq!(count_named(&e, "ephemeral"), 1);

        // Released at 10m, past due: the same pass expires it, book first.
        let events = e.advance(5).expect("to the release moment");
        assert_eq!(narration(&events), vec!["expired-clean"]);
        assert_eq!(count_named(&e, "ephemeral"), 1);
    }
}

mod chains {
    use super::*;

    /// A `start` whose `when` never holds ends the chain silently.
    #[test]
    fn start_with_failing_when_ends_the_chain_silently() {
        let src = r#"
game "Chains"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb caller committable instances 1
    recipe go
      duration 5m
      effects
        start shy

  verb shy uncommittable instances 1
    recipe unreachable
      chain-only
      duration 5m
      when count(world, aspect:spark) >= 99
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "caller", &[]).expect("opens");
        let events = e.advance(5).expect("the completion");
        assert_eq!(
            events
                .iter()
                .filter(|ev| matches!(ev, Event::ChainEnded { verb } if verb.as_str() == "shy"))
                .count(),
            1
        );
    }

    /// T10: a recipe that matches but materializes short of a slot minimum —
    /// two slots competing for one card — yields
    /// `StartSkipped { MaterializationFailed }` with the parent untouched.
    #[test]
    fn matched_but_materialization_short_skips_the_open() {
        let src = r#"
game "Chains"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb caller committable instances 1
    recipe go
      duration 5m
      effects
        start greedy

  verb greedy uncommittable instances 1
    recipe compete
      chain-only
      duration 5m
      when count(world, aspect:spark) >= 1
      slots
        left: 1..1 aspect:spark
        right: 1..1 aspect:spark
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "caller", &[]).expect("opens");
        let events = e.advance(5).expect("the completion");
        assert!(events.iter().any(|ev| matches!(
            ev,
            Event::StartSkipped { verb, reason: StartSkipReason::MaterializationFailed }
                if verb.as_str() == "greedy"
        )));
        assert!(events
            .iter()
            .any(|ev| matches!(ev, Event::SituationCompleted { .. })));
    }

    /// Candidacy counts each slot's minimum separately against the
    /// state-qualifying candidates: a `2..2` slot with one candidate is not a
    /// candidate at all — the chain ends silently — while two candidates fill
    /// it and the recipe opens.
    #[test]
    fn per_slot_minimum_gates_engine_source_candidacy() {
        let one_coin = r#"
game "MinFill"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb caller committable instances 1
    recipe go
      duration 5m
      effects
        start pairer

  verb pairer uncommittable instances 1
    recipe pair
      chain-only
      duration 5m
      when count(world, aspect:spark) >= 1
      slots
        left: 2..2 aspect:spark
"#;
        let mut e = game(one_coin);
        common::commit_named(&mut e, "caller", &[]).expect("opens");
        let events = e.advance(5).expect("the completion");
        assert!(events.iter().any(|ev| matches!(
            ev,
            Event::ChainEnded { verb } if verb.as_str() == "pairer"
        )));
        assert!(
            !events.iter().any(
                |ev| matches!(ev, Event::StartSkipped { verb, .. } if verb.as_str() == "pairer")
            ),
            "an unfillable minimum is not a materialization skip"
        );

        let two_coins = one_coin.replace("start in hall count 1", "start in hall count 2");
        let mut e = game(&two_coins);
        common::commit_named(&mut e, "caller", &[]).expect("opens");
        let events = e.advance(5).expect("the completion");
        let live: Vec<String> = situations(&e)
            .into_iter()
            .map(|s| e.state().situations[&s].recipe.to_string())
            .collect();
        assert_eq!(
            live,
            vec!["pairer.pair".to_string()],
            "the pair opens with two candidates"
        );
        assert!(narration(&events).is_empty(), "no narration, just the open");
    }
}

/// A `start via` hand-off still checks `when` and the instance cap; the
/// pinned recipe opens instead of the selection scan's winner.
#[test]
fn start_via_pins_the_recipe() {
    let src = r#"
game "Handoff"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 5m
      effects
        start hidden_verb via alt

  verb hidden_verb uncommittable instances 2
    recipe main
      chain-only
      duration 5m
      effects
        narrate "main-path"
    recipe alt
      chain-only
      duration 5m
      effects
        narrate "alt-path"
"#;
    let mut e = game(src);
    common::commit_named(&mut e, "caller", &[]).expect("opens");
    let events = e.advance(10).expect("completion, then the hand-off");
    assert_eq!(narration(&events), vec!["alt-path"]);
    assert!(events
        .iter()
        .any(|ev| matches!(ev, Event::SituationCompleted { .. })));
}

/// The recognition face answers without mutating state or spending time
/// (kernel-api.md §1): the would-be selection, bind eligibility, and the
/// greedy-ordered slot candidates.
#[test]
fn recognition_face_previews_without_mutating() {
    let src = include_str!("../../../docs/emberwatch.sw");
    let mut e = boot(compile(src), 7);

    // The would-be selection including the guard outcome.
    let coal = common::named(&e, "coal");
    assert_eq!(
        e.preview_commit(&CommitAction {
            verb: "tend".into(),
            bindings: vec![("fuel".into(), coal)],
        })
        .expect("feed is the would-be selection")
        .0,
        "tend.feed"
    );
    assert_eq!(
        e.preview_commit(&CommitAction {
            verb: "tend".into(),
            bindings: Vec::new()
        })
        .expect("blaze wins the empty-handed scan")
        .0,
        "tend.blaze"
    );
    let d = e
        .preview_commit(&CommitAction {
            verb: "sup".into(),
            bindings: Vec::new(),
        })
        .expect_err("morsel minimum unmet");
    assert_eq!(d.kind, DiagKind::NoRecipe);
    let d = e
        .preview_commit(&CommitAction {
            verb: "omen_sight".into(),
            bindings: Vec::new(),
        })
        .expect_err("uncommittable verbs refuse");
    assert_eq!(d.kind, DiagKind::VerbAutonomous);
    assert_eq!(e.state().now, 0, "preview spends no time");

    // Slot candidates: greedy-ordered, filter-matching, unbound only.
    assert!(e
        .slot_candidates("tend.feed".into(), "kindle".into())
        .is_empty());
    assert_eq!(
        e.slot_candidates("tend.feed".into(), "fuel".into()).len(),
        4,
        "every loose coal is a candidate"
    );

    // Bind eligibility on a live situation's slots.
    common::commit_named(&mut e, "tend", &[("fuel", "coal")]).expect("feed opens");
    let after_open = state_bytes(&e);
    let bound: Vec<_> = e
        .state()
        .situations
        .values()
        .flat_map(|s| s.bindings.values().flatten().map(|b| b.entity))
        .collect();
    let sit = e
        .state()
        .situations
        .values()
        .find(|s| s.recipe.as_str() == "tend.feed")
        .map(|s| s.id)
        .expect("the feed is live");
    let other_coal = e
        .state()
        .entities
        .values()
        .find(|c| c.name == "coal" && !bound.contains(&c.id))
        .map(|c| c.id)
        .expect("a free coal");
    assert_eq!(
        e.can_bind(sit, "fuel".into(), other_coal).unwrap_err().kind,
        DiagKind::ZoneNotBindable,
        "fuel is not an open slot"
    );
    assert_eq!(
        e.can_bind(sit, "kindle".into(), common::named(&e, "tea"))
            .unwrap_err()
            .kind,
        DiagKind::SlotFilterFail,
        "tea carries no kindle"
    );
    // The recognition face mutated nothing: same bytes as before the asks.
    assert_eq!(state_bytes(&e), after_open, "recognition never mutates");
}
