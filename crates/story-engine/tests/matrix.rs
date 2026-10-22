//! M2 behavioral matrix (kernel-api.md §10): every mechanism entry carries at
//! least one positive and one refusal path, tagged `// matrix <NN>` so coverage
//! is greppable. The testable invariants T1-T14 (§9) close the file: each
//! carries an executable assertion here or a forwarding comment naming the
//! test that owns it.

mod common;

use common::{aspect, boot, compile, count_named, narration, recipe_runs, situations, state_bytes};
use story_content::code;
use story_content::runtime::DiagKind;
use story_core::{EntityId, SituationId, SlotName};
use story_engine::{Engine, Event, StartSkipReason};

fn game(src: &str) -> Engine {
    boot(compile(src), 1)
}

/// All live ids of one card name, mint order.
fn ids_of(engine: &Engine, name: &str) -> Vec<EntityId> {
    engine
        .state()
        .entities
        .values()
        .filter(|e| e.name == name)
        .map(|e| e.id)
        .collect()
}

fn zone_of(engine: &Engine, e: EntityId) -> story_core::ZoneId {
    engine.state().entities[&e].zone
}

/// The live situation running `recipe` (exactly one expected).
fn sit_of(engine: &Engine, recipe: &str) -> SituationId {
    engine
        .state()
        .situations
        .values()
        .find(|s| s.recipe.as_str() == recipe)
        .map(|s| s.id)
        .unwrap_or_else(|| panic!("no live situation of `{recipe}`"))
}

fn err_kind(r: Result<Vec<Event>, story_content::RuntimeDiagnostic>) -> DiagKind {
    r.expect_err("expected a refusal").kind
}

fn completed_count(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Event::SituationCompleted { .. }))
        .count()
}

fn chain_ended(events: &[Event], verb: &str) -> bool {
    events
        .iter()
        .any(|e| matches!(e, Event::ChainEnded { verb: v } if v.as_str() == verb))
}

fn start_skipped(events: &[Event], verb: &str, reason: &StartSkipReason) -> bool {
    events.iter().any(|e| matches!(e, Event::StartSkipped { verb: v, reason: r } if v.as_str() == verb && r == reason))
}

/// Compile refusals, as stable diagnostic numbers.
fn compile_error_codes(src: &str) -> Vec<u32> {
    story_lang::compile_source("matrix", src)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.number)
        .collect()
}

// ---------------------------------------------------------------------------
// Entries 1, 3, 11, 12 — the mint pipeline: aliasing, group uniqueness,
// unique-mint skip, card carriers.
// ---------------------------------------------------------------------------

mod minting {
    use super::*;

    // matrix 01
    /// Every mint of an aliasing name uses the aliased card's body: kind and
    /// aspects ride on the mint (spec.md §4.5).
    #[test]
    fn matrix_01_mint_aliasing_pos() {
        let src = r#"
game "Alias"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  worth: 0

cards
  card gem
    kind stone  stackable
    aspects worth 5

  card fake
    mint gem

verbs
  verb forge committable instances 1
    recipe strike
      duration 0
      effects
        spawn fake
        narrate "struck"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "forge", &[]).expect("mints");
        assert_eq!(narration(&events), vec!["struck"]);
        assert_eq!(count_named(&e, "fake"), 1, "the alias mints under its name");
        let fake = common::named(&e, "fake");
        assert_eq!(aspect(&e, fake, "worth"), 5, "the aliased body's aspects");
        assert_eq!(
            e.state().entities[&fake].kind.as_deref(),
            Some("stone"),
            "the aliased body's kind"
        );
    }

    // matrix 01
    /// An alias naming an unknown card is refused at compile time.
    #[test]
    fn matrix_01_mint_aliasing_neg() {
        let src = r#"
game "Alias"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card fake
    mint nosuch
"#;
        assert!(compile_error_codes(src).contains(&code::E244));
    }

    // matrix 01
    /// Boot instances materialize from the card's own declared body, never
    /// the aliased one (spec.md §4.5, language.md §2.4). Pins the spec
    /// reading; see the findings list in the M2 report.
    #[test]
    fn matrix_01_mint_aliasing_boot_uses_own_body() {
        let src = r#"
game "Alias"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  worth: 0

cards
  card gem
    kind stone  stackable
    aspects worth 5

  card replica
    mint gem
    aspects worth 1
    start in hall
"#;
        let e = game(src);
        assert_eq!(count_named(&e, "replica"), 1);
        let replica = common::named(&e, "replica");
        assert_eq!(
            aspect(&e, replica, "worth"),
            1,
            "boot reads the card's own declaration"
        );
        assert_eq!(
            e.state().entities[&replica].kind,
            None,
            "boot reads the card's own declaration"
        );
    }

    // matrix 03
    /// Group exclusivity: while any member of the group lives no other member
    /// mints; once the blocker is gone the mint succeeds.
    #[test]
    fn matrix_03_group_uniqueness_pos() {
        let src = r#"
game "Runes"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  runemark: 0

cards
  card rune_a
    group runes
    aspects runemark 1
    start in hall

  card rune_b
    group runes

verbs
  verb caller committable instances 1
    recipe summon_b
      duration 0
      effects
        spawn rune_b

  verb breaker committable instances 1
    recipe smash
      duration 0
      effects
        destroy world name:rune_a
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "caller", &[]).expect("runs");
        assert!(
            count_named(&e, "rune_b") == 0,
            "the group member is skipped while rune_a lives"
        );
        common::commit_named(&mut e, "breaker", &[]).expect("smashes rune_a");
        common::commit_named(&mut e, "caller", &[]).expect("runs again");
        assert_eq!(count_named(&e, "rune_b"), 1, "mints once the group is free");
    }

    // matrix 03
    /// A bound member still blocks the group (a bound card counts as live),
    /// and disjoint groups are independent.
    #[test]
    fn matrix_03_group_uniqueness_neg() {
        let src = r#"
game "Runes"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card rune_a
    group runes
    aspects spark 1
    start in hall

  card rune_b
    group runes

  card lone
    group others

verbs
  verb binder committable instances 1
    recipe hold
      duration 10m
      slots
        kept: 1..1 aspect:spark

  verb caller committable instances 1
    recipe summon
      duration 0
      effects
        spawn rune_b
        spawn lone
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "binder", &[("kept", "rune_a")]).expect("binds rune_a");
        let events = common::commit_named(&mut e, "caller", &[]).expect("runs");
        assert_eq!(count_named(&e, "rune_b"), 0, "the bound member blocks");
        assert!(
            events
                .iter()
                .any(|ev| matches!(ev, Event::SpawnSkipped { name } if name == "rune_b")),
            "the skip is reported"
        );
        assert_eq!(count_named(&e, "lone"), 1, "a disjoint group mints freely");
    }

    // matrix 11
    /// A `unique` mint onto a live copy is skipped: the event fires, no
    /// residue is left, and no entity id is burned by the skip.
    #[test]
    fn matrix_11_unique_mint_skip_pos() {
        let src = r#"
game "Relics"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  mark: 0

cards
  card relic
    unique
    aspects mark 1

verbs
  verb forge committable instances 1
    recipe strike
      duration 0
      effects
        spawn relic count 2
"#;
        let mut e = game(src);
        let next_before = e.state().next_entity;
        let events = common::commit_named(&mut e, "forge", &[]).expect("runs");
        let skips: Vec<_> = events
            .iter()
            .filter_map(|ev| match ev {
                Event::SpawnSkipped { name } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(skips, vec!["relic".to_string()], "one skip reported");
        assert_eq!(count_named(&e, "relic"), 1);
        assert_eq!(
            e.state().next_entity,
            next_before + 1,
            "the skipped mint burns no id"
        );
    }

    // matrix 11
    /// The default single-copy policy hard-fails instead: `NameInUse` rolls
    /// the resolution back bit-for-bit (a failed mint never burns a deck
    /// position — the rollback restores the cursors).
    #[test]
    fn matrix_11_unique_mint_skip_neg() {
        let src = r#"
game "Mugs"
  seed host
  turn 1m
  home hall

zones
  hall

decks
  deck pouch
    entries coin
    wrap

cards
  card mug
    start in hall

  card coin
    stackable

verbs
  verb potter committable instances 1
    recipe work
      duration 0
      effects
        draw from pouch
        spawn mug
"#;
        let mut e = game(src);
        let before = state_bytes(&e);
        let err = common::commit_named(&mut e, "potter", &[]).expect_err("NameInUse");
        assert_eq!(err.kind, DiagKind::NameInUse);
        assert_eq!(state_bytes(&e), before, "rolled back bit-for-bit");
        assert_eq!(count_named(&e, "coin"), 0, "the drawn card never landed");
    }

    // matrix 12
    /// `into <card>` parks the mint in the carrier's `Of(entity)` zone;
    /// destroying the host evicts occupants home and removes the zone.
    #[test]
    fn matrix_12_into_card_pos() {
        let src = r#"
game "Mule"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  mark: 0

cards
  card mule
    unique
    start in hall

  card hallmark
    stackable
    start in hall

  card coin
    stackable
    aspects mark 1

verbs
  verb loader committable instances 1
    recipe load
      duration 0
      effects
        spawn coin into mule

  verb slayer committable instances 1
    recipe kill
      duration 0
      effects
        destroy mule
"#;
        let mut e = game(src);
        let home = zone_of(&e, common::named(&e, "hallmark"));
        common::commit_named(&mut e, "loader", &[]).expect("loads");
        let coin = common::named(&e, "coin");
        let mule = common::named(&e, "mule");
        assert_eq!(common::zone_kind_of(&e, coin), "carrier");
        assert_eq!(
            e.state().zones[&zone_of(&e, coin)].kind,
            story_core::ZoneKind::Of(mule),
            "the carrier zone belongs to the mule"
        );
        common::commit_named(&mut e, "slayer", &[]).expect("kills the mule");
        assert_eq!(zone_of(&e, coin), home, "occupants evict home");
        assert_eq!(common::zone_kind_of(&e, coin), "table");
    }

    // matrix 12
    /// `into <card>` naming a declared but dead card is an effect failure
    /// rolling the unit back.
    #[test]
    fn matrix_12_into_card_neg() {
        let src = r#"
game "Mule"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card ghost_mule
    stackable

  card coin
    stackable

verbs
  verb loader committable instances 1
    recipe load
      duration 0
      effects
        spawn coin into ghost_mule
"#;
        let mut e = game(src);
        let before = state_bytes(&e);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "loader", &[])),
            DiagKind::EffectFailure
        );
        assert_eq!(state_bytes(&e), before, "the failed spawn rolled back");
    }
}

// ---------------------------------------------------------------------------
// Entries 2, 5, 31 — slot release: take slots, the releases override, and the
// mid-run bind happy path.
// ---------------------------------------------------------------------------

mod slots {
    use super::*;

    fn take_src() -> String {
        r#"
game "Take"
  seed host
  turn 1m
  home hall

zones
  hall
  shore

aspects
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in shore count 1

  card hallmark
    stackable
    start in hall

verbs
  verb keeper committable instances 1
    recipe hold
      duration 10m
      slots
        kept: 1..1 take aspect:spark
      effects
        narrate "holding"

  verb mover committable instances 1
    recipe haul
      duration 10m
      slots
        kept: 1..1 take aspect:spark
      effects
        move aspect:spark into hall
        narrate "hauling"
"#
        .to_string()
    }

    // matrix 02
    /// A `take` slot returns its card to the zone each was bound from at
    /// release (spec.md §4.2 step 4).
    #[test]
    fn matrix_02_take_slots_pos() {
        let src = take_src();
        let mut e = game(&src);
        let coin = common::named(&e, "coin");
        let shore = zone_of(&e, coin);
        common::commit(&mut e, "keeper", &[("kept", coin)]).expect("opens");
        assert_eq!(
            zone_of(&e, coin),
            e.state()
                .situations
                .values()
                .next()
                .expect("the situation is live")
                .interior
        );
        e.advance(10).expect("completes");
        assert_eq!(zone_of(&e, coin), shore, "returned to the bound-from zone");
    }

    // matrix 02
    /// A take-slot card moved out of the interior mid-run is not yanked back:
    /// the return applies only to cards still in the interior at release.
    #[test]
    fn matrix_02_take_slots_neg() {
        let src = take_src();
        let mut e = game(&src);
        let coin = common::named(&e, "coin");
        let shore = zone_of(&e, coin);
        let hall = zone_of(&e, common::named(&e, "hallmark"));
        common::commit(&mut e, "mover", &[("kept", coin)]).expect("opens");
        e.advance(10).expect("completes");
        assert_ne!(
            zone_of(&e, coin),
            shore,
            "the card was hauled out of the interior before release"
        );
        assert_eq!(zone_of(&e, coin), hall, "it stays where the move left it");
    }

    // matrix 05
    /// The verb's `releases` zone takes the interior survivors, overriding
    /// home; a verb without one drops them home.
    #[test]
    fn matrix_05_releases_override_pos() {
        let src = r#"
game "Den"
  seed host
  turn 1m
  home hall

zones
  hall
  den

cards
  card hallmark
    stackable
    start in hall

  card denmark
    stackable
    start in den

  card coin
    stackable

verbs
  verb digger committable instances 1
    releases den
    recipe dig
      duration 5m
      effects
        spawn coin

  verb napper committable instances 1
    recipe rest
      duration 5m
      effects
        spawn coin
"#;
        let mut e = game(src);
        let hall = zone_of(&e, common::named(&e, "hallmark"));
        let den = zone_of(&e, common::named(&e, "denmark"));
        common::commit_named(&mut e, "digger", &[]).expect("digs");
        e.advance(5).expect("completes");
        let coins = ids_of(&e, "coin");
        assert_eq!(coins.len(), 1);
        assert_eq!(zone_of(&e, coins[0]), den, "releases overrides home");
        common::commit_named(&mut e, "napper", &[]).expect("naps");
        e.advance(5).expect("completes");
        let coins = ids_of(&e, "coin");
        assert_eq!(coins.len(), 2);
        let newcomer = coins
            .into_iter()
            .find(|c| zone_of(&e, *c) != den)
            .expect("one coin sits outside the releases zone");
        assert_eq!(zone_of(&e, newcomer), hall, "the default is home");
    }

    // matrix 05
    /// Only interior leftovers move to the releases zone: a card moved out of
    /// the interior mid-run stays where it was put.
    #[test]
    fn matrix_05_releases_override_neg() {
        let src = r#"
game "Den"
  seed host
  turn 1m
  home hall

zones
  hall
  den

aspects
  coinmark: 0

cards
  card hallmark
    stackable
    start in hall

  card denmark
    stackable
    start in den

  card coin
    stackable
    aspects coinmark 1

verbs
  verb digger committable instances 1
    releases den
    recipe dig
      duration 5m
      effects
        spawn coin
        move aspect:coinmark into hall
"#;
        let mut e = game(src);
        let hall = zone_of(&e, common::named(&e, "hallmark"));
        let den = zone_of(&e, common::named(&e, "denmark"));
        common::commit_named(&mut e, "digger", &[]).expect("digs");
        e.advance(5).expect("completes");
        let coin = common::named(&e, "coin");
        assert_eq!(
            zone_of(&e, coin),
            hall,
            "the card left the interior mid-run; releases never touches it"
        );
        assert_ne!(zone_of(&e, coin), den);
    }

    fn bind_src() -> &'static str {
        r#"
game "Heist"
  seed host
  turn 1m
  home hall

zones
  hall
  shore

aspects
  spark: 0
  stamp: 0

cards
  card coin_a
    stackable
    aspects spark 1
    start in hall

  card coin_b
    stackable
    aspects spark 1
    start in shore

  card coin_c
    stackable
    aspects spark 1
    start in shore

  card rock
    stackable
    start in hall

verbs
  verb heist committable instances 1
    recipe crack
      duration 10m
      slots
        loot: 1..1 consuming aspect:spark
        extra: 0..1 open take aspect:spark
      effects
        set slot:extra aspect:stamp += 1
        narrate "stamped {extra}"
"#
    }

    // matrix 31
    /// The mid-run bind happy path: the `Bound` event, the interior move, the
    /// completion effects seeing the newcomer, and the take release back to
    /// `bound_from` (spec.md §4.4).
    #[test]
    fn matrix_31_bind_happy_path_pos() {
        let mut e = game(bind_src());
        let shore = zone_of(&e, common::named(&e, "coin_b"));
        common::commit_named(&mut e, "heist", &[("loot", "coin_a")]).expect("opens");
        let sit = sit_of(&e, "heist.crack");
        e.can_bind(sit, SlotName::from("extra"), common::named(&e, "coin_b"))
            .expect("the open slot takes coin_b");
        let events = e
            .bind(sit, SlotName::from("extra"), common::named(&e, "coin_b"))
            .expect("binds");
        assert_eq!(
            events,
            vec![Event::Bound {
                sit,
                slot: SlotName::from("extra"),
                entity: common::named(&e, "coin_b"),
            }]
        );
        let interior = e.state().situations[&sit].interior;
        assert_eq!(zone_of(&e, common::named(&e, "coin_b")), interior);

        let events = e.advance(10).expect("completes");
        assert_eq!(narration(&events), vec!["stamped coin_b"]);
        assert_eq!(count_named(&e, "coin_a"), 0, "the consuming slot burned");
        let b = common::named(&e, "coin_b");
        assert_eq!(aspect(&e, b, "stamp"), 1, "the effect wrote the newcomer");
        assert_eq!(zone_of(&e, b), shore, "the take slot returned it to shore");
    }

    // matrix 31
    /// Bind refusals: a full open slot is `SlotOverMax`, a bound card is
    /// `EntityLocked`, a slot not marked `open` is `ZoneNotBindable`, a card
    /// failing the slot filter is `SlotFilterFail`, and unknown situations
    /// or slots are `UnresolvableRef`.
    #[test]
    fn matrix_31_bind_happy_path_neg() {
        let mut e = game(bind_src());
        common::commit_named(&mut e, "heist", &[("loot", "coin_a")]).expect("opens");
        let sit = sit_of(&e, "heist.crack");
        let extra = SlotName::from("extra");
        e.bind(sit, extra.clone(), common::named(&e, "coin_b"))
            .expect("first bind");
        assert_eq!(
            e.bind(sit, extra.clone(), common::named(&e, "coin_c"))
                .expect_err("slot full")
                .kind,
            DiagKind::SlotOverMax
        );
        assert_eq!(
            e.bind(sit, extra.clone(), common::named(&e, "coin_a"))
                .expect_err("loot is bound")
                .kind,
            DiagKind::EntityLocked
        );
        // The consuming `loot` slot is not `open`, so mid-run bind never
        // addresses it — the open check precedes the full check.
        assert_eq!(
            e.bind(sit, SlotName::from("loot"), common::named(&e, "coin_c"))
                .expect_err("loot is not open")
                .kind,
            DiagKind::ZoneNotBindable
        );
        // `rock` fails `extra`'s `take aspect:spark` filter while the slot
        // still has room.
        assert_eq!(
            e.bind(sit, extra.clone(), common::named(&e, "rock"))
                .expect_err("rock carries no spark")
                .kind,
            DiagKind::SlotFilterFail
        );
        assert_eq!(
            e.bind(sit, SlotName::from("nope"), common::named(&e, "coin_c"))
                .expect_err("no such slot")
                .kind,
            DiagKind::UnresolvableRef
        );
        assert_eq!(
            e.bind(SituationId(9999), extra, common::named(&e, "coin_c"))
                .expect_err("no such situation")
                .kind,
            DiagKind::UnresolvableRef
        );
    }
}

// ---------------------------------------------------------------------------
// Entries 4, 15, 16, 30 — chains: via/count, fan-out, guards, and candidacy.
// ---------------------------------------------------------------------------

mod starts {
    use super::*;

    // matrix 04
    /// `start via <recipe> count <n>` pins the recipe and fans out n opens.
    #[test]
    fn matrix_04_start_via_count_pos() {
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
      duration 0
      effects
        start picked via second count 3

  verb picked uncommittable instances any
    recipe first
      chain-only
      duration 10m
      effects
        narrate "first-path"
    recipe second
      chain-only
      duration 10m
      effects
        narrate "second-path"
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "caller", &[]).expect("opens");
        assert_eq!(recipe_runs(&e, "picked.second"), 3, "count fanned out");
        assert_eq!(situations(&e).len(), 3);
        assert_eq!(
            e.state()
                .situations
                .values()
                .filter(|s| s.recipe.as_str() == "picked.second")
                .count(),
            3,
            "via pinned the recipe"
        );
    }

    // matrix 04
    /// A `via` hand-off still checks `when`: a false one ends the chain
    /// silently and nothing opens.
    #[test]
    fn matrix_04_start_via_count_neg() {
        let src = r#"
game "Handoff"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start picked via second

  verb picked uncommittable instances any
    recipe second
      chain-only
      duration 10m
      when count(world, aspect:ghost) >= 99
      effects
        narrate "second-path"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "caller", &[]).expect("opens");
        assert!(chain_ended(&events, "picked"));
        assert_eq!(situations(&e).len(), 0, "nothing opened");
    }

    // matrix 15
    /// Fan-out across one list opens every branch, and multiple advances
    /// settle the instances in due order.
    #[test]
    fn matrix_15_multi_advance_fanout_pos() {
        let src = r#"
game "Fan"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start alpha
        start beta
        start gamma

  verb alpha uncommittable instances any
    recipe a
      chain-only
      duration 10m
      effects
        narrate "alpha-done"

  verb beta uncommittable instances any
    recipe b
      chain-only
      duration 30m
      effects
        narrate "beta-done"

  verb gamma uncommittable instances any
    recipe g
      chain-only
      duration 20m
      effects
        narrate "gamma-done"
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "caller", &[]).expect("opens");
        assert_eq!(situations(&e).len(), 3);
        assert_eq!(narration(&e.advance(10).expect("t=10")), vec!["alpha-done"]);
        assert_eq!(narration(&e.advance(10).expect("t=20")), vec!["gamma-done"]);
        assert_eq!(narration(&e.advance(10).expect("t=30")), vec!["beta-done"]);
    }

    // matrix 15
    /// Fan-out is best-effort: a branch skipped at the instance cap never
    /// rolls back its siblings in the same list.
    #[test]
    fn matrix_15_multi_advance_fanout_neg() {
        let src = r#"
game "Fan"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start capped
        start capped
        start free

  verb capped uncommittable instances 1
    recipe c
      chain-only
      duration 5m

  verb free uncommittable instances any
    recipe f
      chain-only
      duration 5m
      effects
        narrate "free-open"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "caller", &[]).expect("opens");
        assert!(start_skipped(
            &events,
            "capped",
            &StartSkipReason::InstanceCap
        ));
        let live: Vec<String> = situations(&e)
            .into_iter()
            .map(|s| e.state().situations[&s].recipe.to_string())
            .collect();
        assert_eq!(
            live,
            vec!["capped.c".to_string(), "free.f".to_string()],
            "the second capped branch skipped, the free branch opened"
        );
    }

    // matrix 16
    /// The winner's guard refuses the whole commit with its prose, costing
    /// zero state and zero time (the T3 face for refusals).
    #[test]
    fn matrix_16_guard_refused_pos() {
        let src = r#"
game "Vault"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  sealed: 0

cards
  card beacon
    unique
    aspects sealed 0
    start in hall

verbs
  verb sealer committable instances 1
    recipe seal
      duration 0
      effects
        set beacon.sealed = 1

  verb vault committable instances 1
    recipe open_it
      duration 5m
      guard "The vault is sealed." when beacon.sealed >= 1
      effects
        narrate "opened"
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "sealer", &[]).expect("seals");
        let before = state_bytes(&e);
        let now = e.state().now;
        let refused = common::commit_named(&mut e, "vault", &[]).expect_err("guarded");
        assert_eq!(refused.kind, DiagKind::GuardRefused);
        assert!(
            refused.message.contains("The vault is sealed."),
            "the guard's prose rides the refusal: {refused}"
        );
        assert_eq!(state_bytes(&e), before, "zero state change");
        assert_eq!(e.state().now, now, "zero time spent");
    }

    // matrix 16
    /// A false guard opens normally, and a guard hit during a `start` ends
    /// the chain silently instead of refusing.
    #[test]
    fn matrix_16_guard_refused_neg() {
        let src = r#"
game "Vault"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  sealed: 0

cards
  card beacon
    unique
    aspects sealed 0
    start in hall

verbs
  verb sealer committable instances 1
    recipe seal
      duration 0
      effects
        set beacon.sealed = 1

  verb vault committable instances 1
    recipe open_it
      duration 5m
      guard "The vault is sealed." when beacon.sealed >= 1
      effects
        narrate "opened"

  verb starter committable instances 1
    recipe go
      duration 0
      effects
        start vault
"#;
        let mut e = game(src);
        // Guard false: the commit proceeds.
        common::commit_named(&mut e, "vault", &[]).expect("opens");
        e.advance(5).expect("completes");
        // Guard true on the start path: the chain ends silently.
        common::commit_named(&mut e, "sealer", &[]).expect("seals");
        let events = common::commit_named(&mut e, "starter", &[]).expect("runs");
        assert!(chain_ended(&events, "vault"));
        assert_eq!(situations(&e).len(), 0);
    }

    // matrix 30
    /// Autonomous candidacy counts state-qualifying greedy candidates per
    /// slot: a satisfiable minimum opens at the checkpoint and again at the
    /// next one after release (spec.md §4.3).
    #[test]
    fn matrix_30_candidacy_pos() {
        let src = r#"
game "Watch"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card orb
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb watcher autonomous instances 1
    recipe stare
      duration 20m
      slots
        orb: 1..1 aspect:spark
      effects
        narrate "watching"
"#;
        let mut e = game(src);
        assert_eq!(
            e.state()
                .situations
                .values()
                .filter(|s| s.recipe.as_str() == "watcher.stare")
                .count(),
            1,
            "the boot checkpoint opened the stare"
        );
        let events = e.advance(20).expect("completes and repolls");
        assert_eq!(narration(&events), vec!["watching"]);
        assert!(
            events.iter().any(|ev| matches!(ev, Event::Bound { .. })),
            "the reopen at the settled moment re-bound the orb"
        );
        assert_eq!(recipe_runs(&e, "watcher.stare"), 2);
        assert_eq!(situations(&e).len(), 1);
    }

    // matrix 30
    /// Post-fix candidacy: a per-slot minimum above the state-countable
    /// candidates makes the recipe NOT a candidate — the start chain ends
    /// (`ChainEnded`, never `StartSkipped`), and an autonomous poll misses
    /// silently (review guidance for M2, entry 30).
    #[test]
    fn matrix_30_candidacy_neg_unmet_minimum() {
        let src = r#"
game "Watch"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card orb
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb watcher autonomous instances 1
    recipe stare
      duration 20m
      when count(world, aspect:spark) >= 1
      slots
        orb: 2..2 aspect:spark

  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start watcher
"#;
        let mut e = game(src);
        // Autonomous: the poll misses silently — nothing opens, no event.
        let events = e.advance(20).expect("advance");
        assert_eq!(situations(&e).len(), 0, "the unmet minimum never opens");
        assert!(
            !events.iter().any(|ev| matches!(
                ev,
                Event::StartSkipped { verb, .. } | Event::ChainEnded { verb }
                    if verb.as_str() == "watcher"
            )),
            "a poll miss is silent: {events:?}"
        );
        // Start: the chain ends, and it is not a materialization skip.
        let events = common::commit_named(&mut e, "caller", &[]).expect("opens");
        assert!(chain_ended(&events, "watcher"));
        assert!(!start_skipped(
            &events,
            "watcher",
            &StartSkipReason::MaterializationFailed
        ));
    }

    // matrix 30
    /// `StartSkipped { MaterializationFailed }` needs slots that each pass
    /// candidacy yet exhaust the shared pool at materialization: a greedy
    /// slot pulling to its max starves the later slot's minimum.
    #[test]
    fn matrix_30_candidacy_neg_materialization_short() {
        let src = r#"
game "Watch"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card orb
    stackable
    aspects spark 1
    start in hall count 2

verbs
  verb caller committable instances 1
    recipe go
      duration 0
      effects
        start watcher

  verb watcher uncommittable instances 1
    recipe stare
      chain-only
      duration 20m
      when count(world, aspect:spark) >= 2
      slots
        grab: 2..2 greedy aspect:spark
        keep: 1..1 aspect:spark
      effects
        narrate "staring"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "caller", &[]).expect("opens");
        assert!(start_skipped(
            &events,
            "watcher",
            &StartSkipReason::MaterializationFailed
        ));
        assert!(
            completed_count(&events) >= 1,
            "the parent resolution completed untouched"
        );
        assert_eq!(
            e.state()
                .situations
                .values()
                .filter(|s| s.recipe.as_str() == "watcher.stare")
                .count(),
            0,
            "the short materialization never opened"
        );
    }

    // matrix 30
    /// Locked cards are not state-qualifying candidates: an orb bound by a
    /// live instance fails the minimum and the poll misses.
    #[test]
    fn matrix_30_candidacy_neg_locked_candidates() {
        let src = r#"
game "Watch"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card orb
    stackable
    aspects spark 1
    start in hall count 1

verbs
  verb binder committable instances 1
    recipe hold
      duration 20m
      slots
        kept: 1..1 aspect:spark

  verb watcher autonomous instances 1
    recipe stare
      duration 20m
      when clock >= 10
      slots
        orb: 1..1 aspect:spark
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "binder", &[("kept", "orb")]).expect("locks the orb");
        let events = e.advance(10).expect("to the poll");
        assert_eq!(
            e.state()
                .situations
                .values()
                .filter(|s| s.recipe.as_str() == "watcher.stare")
                .count(),
            0,
            "the locked orb is not a candidate"
        );
        assert!(narration(&events).is_empty(), "a poll miss is silent");
    }
}

// ---------------------------------------------------------------------------
// Entries 21, 22, 23 — instant recipes: inline commit resolution, the
// recursion budgets, the autonomous pulse.
// ---------------------------------------------------------------------------

mod instant {
    use super::*;

    // matrix 21
    /// A `duration 0` commit completes inline: bind, effect, and completion
    /// events return together, greedy pulls applied, consuming burned, zero
    /// time spent, nothing left due.
    #[test]
    fn matrix_21_instant_commit_pos() {
        let src = r#"
game "Jab"
  seed host
  turn 1m
  home hall

zones
  hall
  shore

aspects
  spark: 0
  kindle: 0

cards
  card coal
    stackable
    aspects spark 1
    start in hall

  card wood
    stackable
    aspects kindle 1
    start in shore count 2

  card hallmark
    stackable
    start in hall

verbs
  verb poke committable instances 1
    recipe jab
      duration 0
      slots
        fuel: 1..1 consuming aspect:spark
        extra: 0..2 greedy open aspect:kindle
      effects
        narrate "jabbed"
"#;
        let mut e = game(src);
        let home = zone_of(&e, common::named(&e, "hallmark"));
        let events = common::commit_named(&mut e, "poke", &[("fuel", "coal")]).expect("commits");
        let bounds = events
            .iter()
            .filter(|ev| matches!(ev, Event::Bound { .. }))
            .count();
        assert_eq!(bounds, 3, "the explicit coal plus two greedy pulls");
        assert!(narration(&events) == vec!["jabbed"]);
        assert_eq!(completed_count(&events), 1, "completion in the same return");
        assert_eq!(e.state().now, 0, "an instant recipe spends no time");
        assert_eq!(count_named(&e, "coal"), 0, "the consuming slot burned");
        assert_eq!(count_named(&e, "wood"), 2);
        for w in ids_of(&e, "wood") {
            assert_eq!(zone_of(&e, w), home, "released survivors go home");
        }
        let events = e.advance(1).expect("quiet");
        assert!(
            events.is_empty(),
            "an instant recipe never enters the queue"
        );
    }

    // matrix 21
    /// A failing instant commit refuses with the state bit-identical and the
    /// recipe's run counter unbumped.
    #[test]
    fn matrix_21_instant_commit_neg() {
        let src = r#"
game "Jab"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

verbs
  verb poke committable instances 1
    recipe snap
      duration 0
      effects
        consume aspect:ghost 1
"#;
        let mut e = game(src);
        let before = state_bytes(&e);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "poke", &[])),
            DiagKind::EffectFailure
        );
        assert_eq!(state_bytes(&e), before);
        assert_eq!(recipe_runs(&e, "poke.snap"), 0, "the bump rolled back");
        assert_eq!(situations(&e).len(), 0);
    }

    // matrix 22
    /// A bounded instant chain resolves fully inline: three links, three
    /// completions, one commit return.
    #[test]
    fn matrix_22_recursion_budget_pos() {
        let src = r#"
game "Chain"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb a committable instances any
    recipe go
      duration 0
      effects
        start b

  verb b uncommittable instances any
    recipe go
      chain-only
      duration 0
      effects
        start c

  verb c uncommittable instances any
    recipe go
      chain-only
      duration 0
      effects
        narrate "c-done"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "a", &[]).expect("resolves inline");
        assert_eq!(completed_count(&events), 3, "the whole chain, one return");
        assert_eq!(recipe_runs(&e, "a.go"), 1);
        assert_eq!(recipe_runs(&e, "b.go"), 1);
        assert_eq!(recipe_runs(&e, "c.go"), 1);
    }

    // matrix 22
    /// The recursion budget: an exponentially forking instant self-chain
    /// breaches the per-call resolution budget and takes the Livelock
    /// rollback path — the whole commit rolls back bit-for-bit, on an
    /// ordinary main thread. (The depth-cap twin lives in
    /// kernel::instant::instant_chain_over_budget_livelocks_and_rolls_back.)
    #[test]
    fn matrix_22_recursion_budget_neg() {
        let src = r#"
game "Fork"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb loop committable instances any
    recipe recurse
      duration 0
      effects
        start loop count 2
"#;
        let mut e = game(src);
        let before = state_bytes(&e);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "loop", &[])),
            DiagKind::Livelock
        );
        assert_eq!(state_bytes(&e), before, "the whole commit rolled back");
    }

    // matrix 23
    /// An all-instant autonomous verb pulses exactly once per checkpoint and
    /// never occupies the due queue.
    #[test]
    fn matrix_23_autonomous_pulse_pos() {
        let src = r#"
game "Pulse"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb pulse autonomous instances 1
    recipe tick
      duration 0
      effects
        narrate "ticked"
"#;
        let mut e = game(src);
        assert_eq!(recipe_runs(&e, "pulse.tick"), 1, "the boot checkpoint");
        assert_eq!(situations(&e).len(), 0, "an instant pulse leaves no queue");
        let events = e.advance(1).expect("one checkpoint");
        assert_eq!(narration(&events), vec!["ticked"]);
        assert_eq!(recipe_runs(&e, "pulse.tick"), 2);
        let events = e.advance(5).expect("one more checkpoint");
        assert_eq!(narration(&events), vec!["ticked"]);
        assert_eq!(recipe_runs(&e, "pulse.tick"), 3, "one pulse per checkpoint");
        assert_eq!(situations(&e).len(), 0);
    }

    // matrix 23
    /// The post-fix checkpoint policy: a failing instant pulse rolls back
    /// that verb's open only — sibling verbs' events survive, and the
    /// `ResolutionFailed` event names the situation that the failing pulse
    /// really materialized (its `Bound` is in the same advance) before the
    /// rollback freed it.
    #[test]
    fn matrix_23_autonomous_pulse_neg() {
        let src = r#"
game "Pulse"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0
  spark: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall

verbs
  verb keeper autonomous instances 1
    recipe hold
      duration 10m
      effects
        narrate "held"

  verb keeper2 autonomous instances 1
    recipe hold2
      duration 10m
      effects
        narrate "held2"

  verb broken autonomous instances 1
    recipe snap
      duration 0
      slots
        s: 1..1 aspect:spark
      effects
        consume aspect:ghost 1
"#;
        let mut e = game(src);
        let events = e.advance(10).expect("the first settle");
        assert_eq!(
            narration(&events),
            vec!["held", "held2"],
            "the siblings' completions survived the failing pulse"
        );
        let failed: Vec<SituationId> = events
            .iter()
            .filter_map(|ev| match ev {
                Event::ResolutionFailed { sit, .. } => Some(*sit),
                _ => None,
            })
            .collect();
        assert_eq!(failed.len(), 1, "exactly one failing pulse");
        let failed = failed[0];
        assert!(
            events.iter().any(|ev| matches!(
                ev,
                Event::Bound { sit, .. } if *sit == failed
            )),
            "the failing pulse materialized and bound before it failed"
        );
        assert!(
            !e.state().situations.contains_key(&failed),
            "the failing open rolled back"
        );
        let live: Vec<String> = situations(&e)
            .into_iter()
            .map(|s| e.state().situations[&s].recipe.to_string())
            .collect();
        assert_eq!(
            live,
            vec!["keeper.hold".to_string(), "keeper2.hold2".to_string()],
            "both keepers reopened at the settled moment"
        );
    }
}

// ---------------------------------------------------------------------------
// Entries 6, 10, 17 — decay and decks: on-expire books, fallback exhaustion,
// wrap cycling.
// ---------------------------------------------------------------------------

mod decay {
    use super::*;

    fn expire_src(fail_book: bool) -> String {
        let book = if fail_book {
            "    on expire\n      rule flutter\n        effects\n          consume aspect:ghost 1\n          set subject.spark += 1\n"
        } else {
            "    on expire\n      rule flutter\n        effects\n          set subject.spark += 1\n          narrate \"fluttered\"\n"
        };
        format!(
            r#"
game "Moth"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0
  ghost: 0

cards
  card moth
    lifetime 5m
    aspects spark 0
    start in hall
{book}
verbs
  verb watcher committable instances 1
    recipe stare
      duration 30m
      effects
        narrate "staring"
"#
        )
    }

    // matrix 06
    /// An `on expire` book replaces default destruction, addresses the dying
    /// card as `subject`, and spends its one-shot timer.
    #[test]
    fn matrix_06_on_expire_pos() {
        let src = expire_src(false);
        let mut e = game(&src);
        let events = e.advance(5).expect("to the due moment");
        assert_eq!(narration(&events), vec!["fluttered"]);
        let moth = common::named(&e, "moth");
        assert_eq!(count_named(&e, "moth"), 1, "the book replaced destruction");
        assert_eq!(aspect(&e, moth, "spark"), 1, "subject writes landed");
        assert_eq!(e.state().entities[&moth].expires_at, None, "one-shot");
        let events = e.advance(5).expect("quiet");
        assert!(events.is_empty(), "the timer does not re-arm");
    }

    // matrix 06
    /// A failing expiry book rolls the unit back silently but the timer is
    /// spent: the card survives the cycle with `expires_at` cleared and
    /// never re-fires (the reviewer's implemented judgment call 5a).
    #[test]
    fn matrix_06_on_expire_neg_failing_book_spends_the_timer() {
        let src = expire_src(true);
        let mut e = game(&src);
        let moth = common::named(&e, "moth");
        let events = e.advance(5).expect("to the due moment");
        assert!(
            events.is_empty(),
            "an expiry unit carries no situation, so its failure is silent: {events:?}"
        );
        assert_eq!(count_named(&e, "moth"), 1, "rolled back, still alive");
        assert_eq!(aspect(&e, moth, "spark"), 0, "the writes rolled back");
        assert_eq!(
            e.state().entities[&moth].expires_at,
            None,
            "the timer is spent regardless"
        );
        let events = e.advance(5).expect("quiet");
        assert!(events.is_empty(), "no re-fire: expiry is one-shot");
        assert_eq!(count_named(&e, "moth"), 1);
    }

    // matrix 10
    /// A `fallback` deck serves the relief card for every draw past the
    /// permutation, however many.
    #[test]
    fn matrix_10_fallback_pos() {
        let src = r#"
game "Deals"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card aa
    stackable

  card bb
    stackable

  card cc
    stackable

decks
  deck small
    entries aa, bb
    fallback cc

verbs
  verb dreamer committable instances 1
    recipe sleep
      duration 0
      effects
        draw from small
        draw from small
        draw from small
        draw from small
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "dreamer", &[]).expect("deals");
        assert_eq!(count_named(&e, "aa"), 1);
        assert_eq!(count_named(&e, "bb"), 1);
        assert_eq!(count_named(&e, "cc"), 2, "the fallback served twice");
        assert_eq!(
            *e.state().decks.values().next().expect("the deck exists"),
            4,
            "cursor"
        );
    }

    // matrix 10
    /// A deck declaring neither relief path raises the exhaustion failure on
    /// the first draw past its entries; boot already warned about it. The
    /// unit rolls back — cursors included — and the commit refuses.
    #[test]
    fn matrix_10_fallback_neg() {
        let src = r#"
game "Deals"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card aa
    stackable

  card bb
    stackable

decks
  deck bare
    entries aa, bb

verbs
  verb dreamer committable instances 1
    recipe sleep
      duration 0
      effects
        draw from bare
        draw from bare
        draw from bare
"#;
        let content = compile(src);
        let (mut e, diags) = Engine::boot(content, story_engine::EffectRegistry::new(), 1, None);
        assert!(
            diags.iter().any(|d| d.kind == DiagKind::DeckExhausted),
            "boot warns about a deck with no relief path: {diags:?}"
        );
        let before = state_bytes(&e);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "dreamer", &[])),
            DiagKind::DeckExhausted
        );
        assert_eq!(state_bytes(&e), before, "the deck cursor rolled back");
        assert_eq!(count_named(&e, "aa"), 0);
        assert_eq!(count_named(&e, "bb"), 0);
    }

    // matrix 17
    /// A `wrap` deck cycles its permutation: the deal sequence repeats with
    /// the permutation's period.
    #[test]
    fn matrix_17_wrap_cycling_pos() {
        let src = r#"
game "Tides"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card ebb
    stackable

  card flood
    stackable

decks
  deck tides
    entries ebb, flood
    wrap

verbs
  verb dreamer committable instances 1
    recipe sleep
      duration 0
      effects
        draw from tides
        draw from tides
        draw from tides
        draw from tides
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "dreamer", &[]).expect("deals");
        assert_eq!(count_named(&e, "ebb"), 2, "the permutation cycled");
        assert_eq!(count_named(&e, "flood"), 2);
        assert_eq!(
            *e.state().decks.values().next().expect("the deck exists"),
            4
        );
    }

    // matrix 17
    /// A declared `fallback` outranks `wrap`: the relief card serves forever
    /// and the permutation never cycles.
    #[test]
    fn matrix_17_wrap_cycling_neg() {
        let src = r#"
game "Tides"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card ebb
    stackable

  card flood
    stackable

  card calm
    stackable

decks
  deck tides
    entries ebb, flood
    fallback calm
    wrap

verbs
  verb dreamer committable instances 1
    recipe sleep
      duration 0
      effects
        draw from tides
        draw from tides
        draw from tides
        draw from tides
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "dreamer", &[]).expect("deals");
        assert_eq!(count_named(&e, "ebb"), 1);
        assert_eq!(count_named(&e, "flood"), 1);
        assert_eq!(count_named(&e, "calm"), 2, "the fallback outranks wrap");
    }
}

// ---------------------------------------------------------------------------
// Entries 7, 8, 9, 13, 24, 28, 29 — targets and queries.
// ---------------------------------------------------------------------------

mod targeting {
    use super::*;

    // matrix 07
    /// `destroy` walks its target oldest-first under a count; `swap`
    /// atomizes destroy+spawn with the successor inheriting the dying card's
    /// zone; swapping oneself retires and re-mints. The swap faces pin the
    /// spec's per-name uniqueness reading (spec.md §4.5: swap is destroy +
    /// spawn, uniqueness excluding the dying card); see the findings list in
    /// the M2 report.
    #[test]
    fn matrix_07_destroy_swap_pos() {
        let src = r#"
game "Workshop"
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
    start in hall count 3

  card chalice
    unique
    start in hall

  card goblet
    unique

  card relic
    unique
    start in hall

verbs
  verb smith committable instances 1
    recipe work
      duration 0
      effects
        destroy aspect:spark count 2
        swap name:chalice count 1 to goblet
        swap name:relic count 1 to relic
"#;
        let mut e = game(src);
        let coins = ids_of(&e, "coin");
        let chalice = common::named(&e, "chalice");
        let chalice_zone = zone_of(&e, chalice);
        let relic = common::named(&e, "relic");
        let relic_zone = zone_of(&e, relic);
        common::commit_named(&mut e, "smith", &[]).expect("works");
        let survivors = ids_of(&e, "coin");
        assert_eq!(
            survivors,
            vec![coins[2]],
            "the count walked the oldest first"
        );
        assert_eq!(count_named(&e, "chalice"), 0);
        let goblet = common::named(&e, "goblet");
        assert_eq!(zone_of(&e, goblet), chalice_zone, "the successor inherits");
        assert_eq!(
            count_named(&e, "relic"),
            1,
            "the self-swap re-minted one relic"
        );
        let relic2 = common::named(&e, "relic");
        assert_ne!(relic2, relic, "a new entity, not the old one");
        assert_eq!(zone_of(&e, relic2), relic_zone);
    }

    // matrix 07
    /// A swap onto a live single-copy card is the hard `NameInUse` failure
    /// and rolls the unit back.
    #[test]
    fn matrix_07_destroy_swap_neg() {
        let src = r#"
game "Workshop"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card chalice
    unique
    start in hall

  card mug
    start in hall

verbs
  verb smith committable instances 1
    recipe work
      duration 0
      effects
        swap name:chalice count 1 to mug
"#;
        let mut e = game(src);
        let before = state_bytes(&e);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "smith", &[])),
            DiagKind::NameInUse
        );
        assert_eq!(state_bytes(&e), before, "nothing swapped");
        assert_eq!(count_named(&e, "chalice"), 1);
    }

    // matrix 08
    /// `anywhere` spans every live card including situation interiors, which
    /// `world` (public cards only) does not see: a bound card sits in the
    /// interior while its instance runs.
    #[test]
    fn matrix_08_anywhere_scope_pos() {
        let src = r#"
game "Fog"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  mark: 0

cards
  card coin
    stackable
    aspects mark 1
    start in hall count 2

verbs
  verb hoarder committable instances 1
    recipe stash
      duration 10m
      slots
        loot: 1..1 aspect:mark

  verb seeker_any committable instances 1
    recipe go
      duration 0
      when count(anywhere, aspect:mark) >= 2
      effects
        narrate "anywhere-sees"

  verb seeker_world committable instances 1
    recipe go
      duration 0
      when count(world, aspect:mark) >= 2
      effects
        narrate "world-sees"
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "hoarder", &[("loot", "coin")]).expect("stashes");
        // Mid-run: two marks anywhere (one bound inside the interior), but
        // only one public.
        let events = common::commit_named(&mut e, "seeker_any", &[]).expect("opens");
        assert_eq!(narration(&events), vec!["anywhere-sees"]);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "seeker_world", &[])),
            DiagKind::NoRecipe,
            "the bound coin is not public"
        );
        e.advance(10)
            .expect("the hoard completes; the coin unlocks home");
        let events = common::commit_named(&mut e, "seeker_world", &[]).expect("now public");
        assert_eq!(narration(&events), vec!["world-sees"]);
    }

    // matrix 08
    /// Dead cards leave `anywhere` immediately: destruction drops them from
    /// every scope.
    #[test]
    fn matrix_08_anywhere_scope_neg() {
        let src = r#"
game "Fog"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  mark: 0

cards
  card coin
    stackable
    aspects mark 1
    start in hall count 2

verbs
  verb counter committable instances 1
    recipe tally
      duration 0
      when count(anywhere, aspect:mark) == 2
      effects
        destroy aspect:mark count 1

  verb recount committable instances 1
    recipe tally
      duration 0
      when count(anywhere, aspect:mark) == 2
      effects
        narrate "still-two"
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "counter", &[]).expect("tallies and destroys one");
        assert_eq!(
            err_kind(common::commit_named(&mut e, "recount", &[])),
            DiagKind::NoRecipe,
            "one coin is dead; anywhere counts 1"
        );
    }

    // matrix 09
    /// A zone scope includes its nested subtree: `zone:tower` counts cards
    /// declared in `cabin in tower`.
    #[test]
    fn matrix_09_zone_nesting_pos() {
        let src = r#"
game "Keep"
  seed host
  turn 1m
  home hall

zones
  hall
  tower
  cabin in tower
  shore

aspects
  mark: 0

cards
  card towercoin
    unique
    aspects mark 1
    start in tower

  card hutcoin
    unique
    aspects mark 1
    start in cabin

verbs
  verb counter committable instances 1
    recipe scan_tower
      duration 0
      when count(zone:tower, aspect:mark) == 2
      effects
        narrate "two-under-tower"

  verb counter2 committable instances 1
    recipe scan_cabin
      duration 0
      when count(zone:cabin, aspect:mark) == 1
      effects
        narrate "one-in-cabin"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "counter", &[]).expect("opens");
        assert_eq!(narration(&events), vec!["two-under-tower"]);
        let events = common::commit_named(&mut e, "counter2", &[]).expect("opens");
        assert_eq!(narration(&events), vec!["one-in-cabin"]);
    }

    // matrix 09
    /// The subtree does not leak sideways: siblings of the subtree are
    /// excluded, and a card moved out of the subtree leaves the scope.
    #[test]
    fn matrix_09_zone_nesting_neg() {
        let src = r#"
game "Keep"
  seed host
  turn 1m
  home hall

zones
  hall
  tower
  cabin in tower
  shore

aspects
  mark: 0

cards
  card towercoin
    unique
    aspects mark 1
    start in tower

  card hutcoin
    unique
    aspects mark 1
    start in cabin

  card shorecoin
    unique
    aspects mark 1
    start in shore

verbs
  verb counter committable instances 1
    recipe scan_all
      duration 0
      when count(zone:tower, aspect:mark) == 3
      effects
        narrate "three"

  verb mover committable instances 1
    recipe haul
      duration 0
      effects
        move hutcoin into shore

  verb counter2 committable instances 1
    recipe scan_one
      duration 0
      when count(zone:tower, aspect:mark) == 1
      effects
        narrate "one-left"
"#;
        let mut e = game(src);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "counter", &[])),
            DiagKind::NoRecipe,
            "the shore sibling is not under the tower"
        );
        common::commit_named(&mut e, "mover", &[]).expect("hauls the hut coin out");
        let events = common::commit_named(&mut e, "counter2", &[]).expect("opens");
        assert_eq!(narration(&events), vec!["one-left"]);
    }

    // matrix 13
    /// Negated query terms exclude their matches: `-kind:` and `-aspect:`
    /// narrow a scope.
    #[test]
    fn matrix_13_negated_terms_pos() {
        let src = r#"
game "Sort"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  mark: 0

cards
  card coin
    kind matter  stackable
    aspects mark 1
    start in hall

  card gem
    kind stone  stackable
    aspects mark 1
    start in hall

verbs
  verb sorter committable instances 1
    recipe sort
      duration 0
      when count(world, kind:matter -kind:stone) == 1 and count(world, -kind:matter) == 1 and count(world, aspect:mark -name:gem) == 1
      effects
        narrate "sorted"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "sorter", &[]).expect("opens");
        assert_eq!(narration(&events), vec!["sorted"]);
    }

    // matrix 13
    /// Negation does not bypass declaration checks: an unknown kind inside a
    /// negated term is the same compile refusal as a plain one.
    #[test]
    fn matrix_13_negated_terms_neg() {
        let base = r#"
game "Sort"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  mark: 0

cards
  card coin
    kind matter  stackable
    aspects mark 1
    start in hall

verbs
  verb sorter committable instances 1
    recipe sort
      duration 0
      when count(world, %TERM%) == 1
      effects
        narrate "sorted"
"#;
        assert!(compile_error_codes(&base.replace("%TERM%", "kind:potion")).contains(&code::E203));
        assert!(compile_error_codes(&base.replace("%TERM%", "-kind:potion")).contains(&code::E203));
    }

    // matrix 24
    /// The slot target writes per bound card; a scoped query walks its scope
    /// oldest-first under a count.
    #[test]
    fn matrix_24_shared_targets_pos() {
        let src = r#"
game "Targets"
  seed host
  turn 1m
  home hall

zones
  hall
  vault

aspects
  spark: 0
  stamp: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall count 3

  card vaultcoin
    stackable
    aspects spark 1
    start in vault

verbs
  verb worker committable instances 1
    recipe work
      duration 0
      slots
        s: 1..2 aspect:spark
      effects
        set slot:s aspect:stamp += 1
        destroy zone:vault aspect:spark count 1
        destroy world aspect:spark count 2
"#;
        let mut e = game(src);
        let coins = ids_of(&e, "coin");
        let vault = ids_of(&e, "vaultcoin");
        common::commit(&mut e, "worker", &[("s", coins[2])]).expect("works");
        assert_eq!(aspect(&e, coins[2], "stamp"), 1, "the slot write");
        assert_eq!(aspect(&e, coins[0], "stamp"), 0, "per bound card only");
        assert_eq!(
            ids_of(&e, "vaultcoin"),
            Vec::<EntityId>::new(),
            "vault emptied"
        );
        assert_eq!(
            ids_of(&e, "coin"),
            vec![coins[2]],
            "the world query killed the two oldest hall coins"
        );
        assert_eq!(vault.len(), 1, "checked before destruction");
    }

    // matrix 24
    /// A bare query resolves interior-oldest-first, then world-oldest-first.
    #[test]
    fn matrix_24_shared_targets_bare_query_pos() {
        let src = r#"
game "Targets"
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
    start in hall count 2

verbs
  verb eater committable instances 1
    recipe munch
      duration 10m
      effects
        spawn coin into interior
        consume aspect:spark 1
        consume aspect:spark 1
"#;
        let mut e = game(src);
        let world_coins = ids_of(&e, "coin");
        common::commit_named(&mut e, "eater", &[]).expect("opens");
        e.advance(10).expect("completes");
        let survivors = ids_of(&e, "coin");
        assert_eq!(
            survivors,
            vec![world_coins[1]],
            "the interior mint died first, then the oldest world coin"
        );
        assert_eq!(e.state().now, 10);
    }

    // matrix 24
    /// A consume short of its count is an effect failure rolling the unit
    /// back — a destroy's best-effort walk never fails, a consume does.
    #[test]
    fn matrix_24_shared_targets_neg() {
        let src = r#"
game "Targets"
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
    start in hall count 3

verbs
  verb worker committable instances 1
    recipe work
      duration 0
      effects
        destroy aspect:spark count 2
        consume aspect:ghost 1
"#;
        let mut e = game(src);
        let before = state_bytes(&e);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "worker", &[])),
            DiagKind::EffectFailure
        );
        assert_eq!(
            state_bytes(&e),
            before,
            "the destroy rolled back with the unit"
        );
        assert_eq!(count_named(&e, "coin"), 3);
    }

    // matrix 28
    /// The `bound` scope counts per binding: a card bound in two slots reads
    /// twice in totals and counts.
    #[test]
    fn matrix_28_bound_scope_pos() {
        let src = r#"
game "Twins"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0
  charge: 0
  bits: 0

cards
  card beacon
    unique
    start in hall

  card coin
    stackable
    aspects spark 1
    start in hall

verbs
  verb binder committable instances 1
    recipe twin
      duration 0
      slots
        a: 1..1 aspect:spark
        b: 1..1 aspect:spark
      effects
        set beacon.charge += total(bound, aspect:spark)
        set beacon.bits += count(bound, aspect:spark)
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "binder", &[("a", "coin"), ("b", "coin")])
            .expect("binds the coin twice");
        let beacon = common::named(&e, "beacon");
        assert_eq!(aspect(&e, beacon, "charge"), 2, "total counted it twice");
        assert_eq!(aspect(&e, beacon, "bits"), 2, "count counted it twice");
    }

    // matrix 28
    /// `bound` reads empty wherever no situation runs: commit selection sees
    /// no bindings, so a `when` on `bound` never blocks a commit.
    #[test]
    fn matrix_28_bound_scope_neg() {
        let src = r#"
game "Twins"
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
    start in hall count 2

verbs
  verb binder committable instances 1
    recipe twin
      duration 0
      when total(bound, aspect:spark) == 0
      slots
        a: 1..1 aspect:spark
      effects
        narrate "opened"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "binder", &[("a", "coin")]).expect("opens");
        assert_eq!(narration(&events), vec!["opened"]);
    }

    // matrix 29
    /// `kind:` terms match a card's kind tag, alone or conjoined with aspect
    /// terms.
    #[test]
    fn matrix_29_kind_terms_pos() {
        let src = r#"
game "Kinds"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  mark: 0

cards
  card coin
    kind matter  stackable
    aspects mark 1
    start in hall

  card gem
    kind stone  stackable
    aspects mark 1
    start in hall

verbs
  verb sorter committable instances 1
    recipe sort
      duration 0
      when count(world, kind:stone) == 1 and count(world, kind:stone aspect:mark>=1) == 1 and count(world, kind:matter) == 1
      effects
        narrate "kinds-ok"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "sorter", &[]).expect("opens");
        assert_eq!(narration(&events), vec!["kinds-ok"]);
    }

    // matrix 29
    /// A `kind:` term naming an undeclared kind is refused at compile time.
    #[test]
    fn matrix_29_kind_terms_neg() {
        let src = r#"
game "Kinds"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card coin
    kind matter  stackable
    start in hall

verbs
  verb sorter committable instances 1
    recipe sort
      duration 0
      when count(world, kind:potion) == 1
      effects
        narrate "no"
"#;
        assert!(compile_error_codes(src).contains(&code::E203));
    }
}

// ---------------------------------------------------------------------------
// Entries 14, 25 — trigger books: signal/on-event and card-carried books.
// ---------------------------------------------------------------------------

mod books {
    use super::*;

    fn signal_src(book: &str) -> String {
        format!(
            r#"
game "Alarm"
  seed host
  turn 1m
  home hall

zones
  hall

legacies
  favor: 0

cards
  card bell
    stackable

verbs
  verb crier committable instances 1
    recipe cry
      duration 0
      effects
        legacy favor += 50
        signal alarm
        narrate "post"

  verb crier2 committable instances 1
    recipe cry
      duration 0
      effects
        signal alarm
        narrate "post"

{book}
"#
        )
    }

    const TWO_RULE_BOOK: &str = r#"
on event alarm
  rule whisper priority 1 once when legacy.favor >= 10
    effects
      narrate "whisper"
  rule shout priority 10
    effects
      narrate "shout"
"#;

    // matrix 14
    /// `signal` ignites the named event book under the selection law: the
    /// first rule whose `when` holds fires, `once` spends it for the run,
    /// and the signaling list continues after the book.
    #[test]
    fn matrix_14_signal_books_pos() {
        let src = signal_src(TWO_RULE_BOOK);
        let mut e = game(&src);
        let events = common::commit_named(&mut e, "crier", &[]).expect("cries");
        assert_eq!(narration(&events), vec!["whisper", "post"]);
        // The once rule is spent; the scan falls through to the next rule.
        let events = common::commit_named(&mut e, "crier2", &[]).expect("cries");
        assert_eq!(narration(&events), vec!["shout", "post"]);
        let spent = e.state().fired_once.iter().cloned().collect::<Vec<_>>();
        assert_eq!(spent.len(), 1, "the once rule is marked spent");
    }

    // matrix 14
    /// A book whose every rule misses its `when` stays silent: the signal is
    /// not a failure and the signaling list runs on.
    #[test]
    fn matrix_14_signal_books_neg() {
        let book = r#"
on event alarm
  rule whisper priority 1 when legacy.favor >= 99
    effects
      narrate "whisper"
"#;
        let src = signal_src(book);
        let mut e = game(&src);
        let events = common::commit_named(&mut e, "crier", &[]).expect("cries");
        assert_eq!(narration(&events), vec!["post"], "the book stayed silent");
    }

    // matrix 25
    /// Card-carried completion books: unqualified books fire at every
    /// completion, aspect-qualified ones under a matching `declares`, after
    /// the recipe's own book, in bind order — including inside an inline
    /// instant completion.
    #[test]
    fn matrix_25_card_books_pos() {
        let src = r#"
game "Rites"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  stamped: 0

cards
  card bard
    stackable
    start in hall
    on complete
      rule song
        effects
          narrate "bard-song"

  card omened
    stackable
    aspects stamped 1
    start in hall
    on complete [stamped]
      rule vision
        effects
          narrate "omened-vision"

verbs
  verb rite_a committable instances 1
    recipe full
      duration 0
      declares stamped
      slots
        f: 1..1
        s: 1..1
      on complete
        rule own
          effects
            narrate "rite-book"

  verb rite_c committable instances 1
    recipe pull
      duration 0
      declares stamped
      slots
        f: 0..1 greedy aspect:stamped
      on complete
        rule own
          effects
            narrate "rite-c-book"

  verb starter committable instances 1
    recipe go
      duration 0
      effects
        start rite_c
"#;
        let mut e = game(src);
        // The whole order lands in one return.
        let events = common::commit_named(&mut e, "rite_a", &[("f", "bard"), ("s", "omened")])
            .expect("completes");
        assert_eq!(
            narration(&events),
            vec!["rite-book", "bard-song", "omened-vision"],
            "recipe book first, card books in bind order, qualified under declares"
        );
        // The inline face: a started instant recipe runs card books inline.
        let events = common::commit_named(&mut e, "starter", &[]).expect("resolves inline");
        assert_eq!(
            narration(&events),
            vec!["rite-c-book", "omened-vision"],
            "the greedy pull bound omened; its book fired inside the inline completion"
        );
        assert_eq!(completed_count(&events), 2, "starter.go and rite_c.pull");
    }

    // matrix 25
    /// An aspect-qualified book stays silent when the recipe does not declare
    /// the aspect and the situation's interior pool does not contain it; the
    /// completion itself is unaffected.
    #[test]
    fn matrix_25_card_books_neg() {
        let src = r#"
game "Rites"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  stamped: 0

cards
  card omened
    stackable
    start in hall
    on complete [stamped]
      rule vision
        effects
          narrate "omened-vision"

verbs
  verb rite_b committable instances 1
    recipe bare
      duration 0
      slots
        f: 1..1
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "rite_b", &[("f", "omened")]).expect("completes");
        assert!(
            narration(&events).is_empty(),
            "the qualified book never fires without `declares` or an interior pool holding the aspect"
        );
        assert_eq!(completed_count(&events), 1);
    }

    // guidance 5b
    /// Review guidance for M2, item 5b: a `once` rule's spent mark lives in
    /// the state, so a resolution-unit rollback un-spends it — the rule fires
    /// again at the next completion.
    #[test]
    fn once_rule_unspends_when_the_unit_rolls_back() {
        let src = r#"
game "Rites"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0

cards
  card bard
    stackable
    start in hall
    on complete
      rule song once
        effects
          narrate "song"

  card jinx
    stackable
    start in hall
    on complete
      rule hex
        effects
          consume aspect:ghost 1

  card muse
    stackable
    start in hall

verbs
  verb rite committable instances 1
    recipe full
      duration 5m
      slots
        f: 1..1
        s: 1..1
"#;
        let mut e = game(src);
        // bard's once book fires, then jinx's book fails the unit: the whole
        // completion rolls back, the spent mark included.
        common::commit_named(&mut e, "rite", &[("f", "bard"), ("s", "jinx")]).expect("opens");
        let events = e.advance(5).expect("the unit fails; the advance continues");
        assert!(events
            .iter()
            .any(|ev| matches!(ev, Event::ResolutionFailed { .. })));
        assert!(situations(&e).is_empty(), "the situation aborted");
        for name in ["bard", "jinx", "muse"] {
            assert_eq!(count_named(&e, name), 1, "{name} released, not consumed");
        }
        assert!(
            e.state().fired_once.is_empty(),
            "the once rule un-spent with the rollback"
        );
        // The next completion walks the once rule again.
        common::commit_named(&mut e, "rite", &[("f", "bard"), ("s", "muse")]).expect("opens");
        let events = e.advance(5).expect("completes");
        assert_eq!(narration(&events), vec!["song"]);
        assert_eq!(e.state().fired_once.len(), 1, "spent for real this time");
    }
}

// ---------------------------------------------------------------------------
// Entries 18, 19, 20 — compile-step machinery: effectset expansion, include
// splicing, meta pass-through.
// ---------------------------------------------------------------------------

mod compile_steps {
    use super::*;

    // matrix 18
    /// An `effectset` call expands at its call site: parameters splice into
    /// the expanded effects, and macros may call macros.
    #[test]
    fn matrix_18_effectset_pos() {
        let src = r#"
game "Stoke"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  heat: 0
  coal: 0

cards
  card coal
    stackable
    aspects coal 1
    start in hall count 4

  card beacon
    unique
    start in hall

effectset stoke(n)
  effects
    consume aspect:coal n
    set beacon.heat += n

effectset double_stoke(n)
  effects
    stoke(n)
    stoke(n)

verbs
  verb stoker committable instances 1
    recipe work
      duration 0
      effects
        stoke(2)
        double_stoke(1)
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "stoker", &[]).expect("works");
        let beacon = common::named(&e, "beacon");
        assert_eq!(
            aspect(&e, beacon, "heat"),
            4,
            "2 + 1 + 1 through two macros"
        );
        assert_eq!(count_named(&e, "coal"), 0, "2 + 1 + 1 coals burned");
    }

    // matrix 18
    /// An integer-expression argument splices into positions that parse full
    /// expressions (set), and must expand whole everywhere — the language
    /// reference markets `stoke(1 + total(...))` as legal (language.md §6).
    /// Pins the spec reading; see the findings list in the M2 report.
    #[test]
    fn matrix_18_effectset_expr_argument() {
        let src = r#"
game "Stoke"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  heat: 0
  coal: 0

cards
  card coal
    stackable
    aspects coal 1
    start in hall count 4

  card beacon
    unique
    start in hall

effectset stoke(n)
  effects
    consume aspect:coal n
    set beacon.heat += n

verbs
  verb stoker committable instances 1
    recipe work
      duration 0
      effects
        stoke(1 + 1)
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "stoker", &[]).expect("works");
        assert_eq!(
            count_named(&e, "coal"),
            2,
            "the full expression is the count"
        );
        let beacon = common::named(&e, "beacon");
        assert_eq!(aspect(&e, beacon, "heat"), 2);
    }

    // matrix 18
    /// Clause termination: a count expression ends at a clause keyword on
    /// the same line (`spawn ... count 1 + 1 into hall`) and at end of line
    /// (`consume aspect:coal 2 - 1`), and the next line still parses as its
    /// own effect.
    #[test]
    fn matrix_18_expr_clause_termination() {
        let src = r#"
game "Kiln"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  coal: 0

cards
  card coal
    stackable
    aspects coal 1
    start in hall count 4

verbs
  verb stoker committable instances 1
    recipe work
      duration 0
      effects
        spawn coal count 1 + 1 into hall
        consume aspect:coal 2 - 1
        start helper

  verb helper uncommittable instances any
    recipe side
      chain-only
      duration 10m
      effects
        narrate "helper"
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "stoker", &[]).expect("works");
        assert_eq!(
            count_named(&e, "coal"),
            5,
            "4 + (1 + 1) spawned, (2 - 1) consumed"
        );
        assert_eq!(recipe_runs(&e, "helper.side"), 1, "the next line started");
    }

    // matrix 18
    /// Unknown macros and wrong arities are refused at compile time.
    #[test]
    fn matrix_18_effectset_neg() {
        let base = r#"
game "Stoke"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  heat: 0

cards
  card coal
    stackable
    start in hall count 4

  card beacon
    unique
    start in hall

effectset stoke(n)
  effects
    consume aspect:coal n
    set beacon.heat += n

verbs
  verb stoker committable instances 1
    recipe work
      duration 0
      effects
        %CALL%
"#;
        assert!(
            compile_error_codes(&base.replace("%CALL%", "stoke_wrong(2)")).contains(&code::E233)
        );
        assert!(compile_error_codes(&base.replace("%CALL%", "stoke(1, 2)")).contains(&code::E234));
    }

    // matrix 19
    /// An include splices its declarations into the one namespace: the
    /// root's verbs read and bind what the included file declares.
    #[test]
    fn matrix_19_include_splicing_pos() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/matrix_inc_root.sw");
        let out = story_lang::compile_file(&path);
        let errors: Vec<String> = out
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .map(|d| d.to_string())
            .collect();
        assert!(errors.is_empty(), "clean splice: {errors:?}");
        let mut e = boot(
            std::sync::Arc::new(out.content.expect("no errors means content")),
            1,
        );
        assert_eq!(count_named(&e, "gem"), 2, "the included boot face applied");
        let events = common::commit_named(&mut e, "use_included", &[("gem", "gem")])
            .expect("the spliced card binds");
        assert_eq!(narration(&events), vec!["charged 3"]);
    }

    // matrix 19
    /// A duplicate id across two included files is [E232] at the second
    /// declaration site, and the diagnostic names both sites: the primary
    /// location and the `first declared at` note.
    #[test]
    fn matrix_19_include_splicing_neg() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/matrix_inc_dup.sw");
        let out = story_lang::compile_file(&path);
        let d = out
            .diagnostics
            .iter()
            .find(|d| d.number == code::E232 && d.is_error())
            .expect("E232 expected")
            .clone();
        assert!(
            d.file.ends_with("matrix_inc_dup_b.sw") && d.line == 3,
            "the primary location is the second site: {d}"
        );
        assert!(
            d.notes
                .iter()
                .any(|n| n.contains("matrix_inc_dup_a.sw") && n.contains(":3")),
            "the note names the first site: {d}"
        );
    }

    // matrix 20
    /// Meta annotations ride the compiled content and never touch the
    /// kernel: the engine boots and plays meta-carrying content normally.
    #[test]
    fn matrix_20_meta_passthrough_pos() {
        let src = r#"
game "Meta"
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
    start in hall count 2
    meta ui_icon "coin"

verbs
  verb poker committable instances 1
    meta ui_hint "poke it"
    recipe jab
      duration 0
      slots
        s: 1..1 aspect:spark
      effects
        narrate "poked {s}"
"#;
        let content = compile(src);
        assert_eq!(
            content.cards[&story_core::CardId::from("coin")]
                .meta
                .get("ui_icon")
                .map(String::as_str),
            Some("coin"),
            "the kernel stores what it never reads"
        );
        assert_eq!(
            content.verbs["poker"]
                .meta
                .get("ui_hint")
                .map(String::as_str),
            Some("poke it")
        );
        let mut e = boot(content, 1);
        let events = common::commit_named(&mut e, "poker", &[("s", "coin")]).expect("plays");
        assert_eq!(narration(&events), vec!["poked coin"]);
    }

    // matrix 20
    /// A repeated meta key is a compile refusal, and meta changes no
    /// behavior: two contents differing only in meta reach identical states.
    #[test]
    fn matrix_20_meta_passthrough_neg() {
        let plain = r#"
game "Meta"
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
    start in hall count 2

verbs
  verb poker committable instances 1
    recipe jab
      duration 0
      slots
        s: 1..1 aspect:spark
      effects
        narrate "poked"
"#;
        let dup = plain.replace(
            "    start in hall count 2",
            "    start in hall count 2\n    meta ui_icon \"coin\"\n    meta ui_icon \"coin\"",
        );
        assert!(compile_error_codes(&dup).contains(&code::E236));

        let meta = plain.replace(
            "    start in hall count 2",
            "    start in hall count 2\n    meta ui_icon \"coin\"",
        );
        let mut a = boot(compile(plain), 1);
        let mut b = boot(compile(&meta), 1);
        common::commit_named(&mut a, "poker", &[("s", "coin")]).expect("plays");
        common::commit_named(&mut b, "poker", &[("s", "coin")]).expect("plays");
        assert_eq!(
            state_bytes(&a),
            state_bytes(&b),
            "meta never reaches the state"
        );
    }
}

// ---------------------------------------------------------------------------
// Entry 26 — spawn handles.
// ---------------------------------------------------------------------------

mod handles {
    use super::*;

    // matrix 26
    /// A handle pool lives for its effect list: multi-mints order the pool
    /// oldest-first, targets walk it in that order, branches share it, and
    /// prose tokens render it.
    #[test]
    fn matrix_26_handles_pos() {
        let src = r#"
game "Pool"
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
  verb minter committable instances 1
    recipe work
      duration 0
      effects
        if count(anywhere, aspect:spark) >= 0
          spawn coin count 3 as pile
        narrate "pool {pile}"
        destroy pile count 2
        narrate "left {pile}"
"#;
        let mut e = game(src);
        let next_before = e.state().next_entity;
        let events = common::commit_named(&mut e, "minter", &[]).expect("mints");
        assert_eq!(
            narration(&events),
            vec!["pool coin, coin, coin", "left coin"],
            "the pool renders oldest-first and shrinks as its cards die"
        );
        let minted: Vec<EntityId> = (next_before..next_before + 3).map(EntityId).collect();
        assert_eq!(count_named(&e, "coin"), 1, "two of the pool destroyed");
        assert_eq!(
            ids_of(&e, "coin"),
            vec![minted[2]],
            "the count walked the pool oldest-first"
        );
    }

    // matrix 26
    /// A handle reference with no enclosing `spawn ... as` in the same effect
    /// list is refused at compile time — the pool cannot outlive its list.
    /// A handle-shaped prose token is refused as [E205] (language.md §9: E238
    /// covers handle targets; orphan tokens are the token checker's E205).
    #[test]
    fn matrix_26_handles_neg() {
        let orphan_target = r#"
game "Pool"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card coin
    stackable

verbs
  verb minter committable instances 1
    recipe work
      duration 0
      effects
        destroy pile
"#;
        assert!(compile_error_codes(orphan_target).contains(&code::E238));
        let orphan_token = orphan_target.replace("destroy pile", "narrate \"{pile}\"");
        assert!(compile_error_codes(&orphan_token).contains(&code::E205));
    }
}

// ---------------------------------------------------------------------------
// Entry 27 — dynamic rolls and the roll binding's scoping.
// ---------------------------------------------------------------------------

mod rolls {
    use super::*;

    // matrix 27
    /// Roll operands are full expressions evaluated at effect time; the dice
    /// advance the RNG counter once each; count 0 applies the bonus only and
    /// leaves the counter unmoved.
    #[test]
    fn matrix_27_dynamic_rolls_pos() {
        let src = r#"
game "Storm"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  dice: 0
  charge: 0

legacies
  favor: 3

cards
  card die
    stackable
    aspects dice 1
    start in hall count 2

  card beacon
    unique
    start in hall

verbs
  verb roller committable instances 1
    recipe cast
      duration 0
      effects
        roll total(world, aspect:dice)d(2 + 4) + legacy.favor as storm
        set beacon.charge += storm
        narrate "storm={storm}"
        roll 0d6 + 5 as grace
        narrate "grace={grace}"
"#;
        let mut e = game(src);
        let rng_before = e.state().rng_counter;
        let events = common::commit_named(&mut e, "roller", &[]).expect("rolls");
        assert_eq!(
            e.state().rng_counter - rng_before,
            2,
            "two dice, two counter ticks"
        );
        let storm_text = narration(&events)[0].clone();
        let storm: i64 = storm_text
            .strip_prefix("storm=")
            .expect("storm narration")
            .parse()
            .expect("integer roll");
        assert!((5..=15).contains(&storm), "3 bonus + 2d6: {storm}");
        let beacon = common::named(&e, "beacon");
        assert_eq!(aspect(&e, beacon, "charge"), storm, "the binding flowed");
        assert_eq!(narration(&events)[1], "grace=5", "count 0: bonus only");
        assert_eq!(
            e.state().rng_counter - rng_before,
            2,
            "count 0 left the counter unmoved"
        );
    }

    // matrix 27
    /// The F4 face: a roll binding IS visible to a predicate called from an
    /// `if` condition — the inlined body reads the roll through a comparison
    /// and the taken branch follows the dice (language.md §4.3). Query
    /// positions and slot filters stay E244
    /// (`matrix_27_roll_scope_compile_refusal`).
    #[test]
    fn matrix_27_roll_in_if_predicate_pos() {
        let src = r#"
game "Oracle"
  seed host
  turn 1m
  home hall

zones
  hall

predicates
  predicate fate_at_least(n) = fate >= n

cards
  card beacon
    unique
    start in hall

verbs
  verb seer committable instances 1
    recipe gaze
      duration 0
      effects
        roll 1d6 as fate
        narrate "fate={fate}"
        if fate_at_least(4)
          narrate "high"
        else
          narrate "low"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "seer", &[]).expect("rolls");
        let fate: i64 = narration(&events)[0]
            .strip_prefix("fate=")
            .expect("fate narration")
            .parse()
            .expect("integer roll");
        assert!((1..=6).contains(&fate), "one d6: {fate}");
        assert_eq!(
            narration(&events)[1],
            if fate >= 4 { "high" } else { "low" },
            "the branch follows the roll through the predicate body"
        );
    }

    // matrix 27
    /// Invalid operands are effect failures rolling the unit back: sides < 1
    /// and count < 0 both refuse, leaving the RNG counter untouched.
    #[test]
    fn matrix_27_dynamic_rolls_neg() {
        let bad_sides = r#"
game "Storm"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb roller committable instances 1
    recipe jinx
      duration 0
      effects
        roll 0d0 as jinx
"#;
        let mut e = game(bad_sides);
        let before = state_bytes(&e);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "roller", &[])),
            DiagKind::EffectFailure
        );
        assert_eq!(state_bytes(&e), before);

        let bad_count = bad_sides.replace("roll 0d0 as jinx", "roll (0 - 1)d6 as jinx");
        let mut e = game(&bad_count);
        let before = state_bytes(&e);
        assert_eq!(
            err_kind(common::commit_named(&mut e, "roller", &[])),
            DiagKind::EffectFailure
        );
        assert_eq!(state_bytes(&e), before);
    }

    // matrix 27
    /// The roll binding is scoped to its effect list: slot filters, recipe
    /// `when`s, and book rules are compiled without any roll environment, so
    /// a roll reference there is an unknown-name compile refusal — candidacy
    /// can never see a roll (review guidance for M2, item 7).
    #[test]
    fn matrix_27_roll_scope_compile_refusal() {
        let filter = r#"
game "Scope"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

predicates
  predicate fate_touched(p) = p.spark >= fate

cards
  card coin
    stackable
    aspects spark 1
    start in hall

verbs
  verb grabber committable instances 1
    recipe take
      duration 0
      slots
        s: 1..1 fate_touched
      effects
        roll 1d6 as fate
        narrate "took"
"#;
        assert!(
            compile_error_codes(filter).contains(&code::E244),
            "a slot filter can never see a roll binding"
        );
        let when = r#"
game "Scope"
  seed host
  turn 1m
  home hall

zones
  hall

cards
  card coin
    stackable
    start in hall

verbs
  verb grabber committable instances 1
    recipe take
      duration 0
      when fate >= 1
      effects
        roll 1d6 as fate
        narrate "took"
"#;
        assert!(
            compile_error_codes(when).contains(&code::E244),
            "a selection `when` can never see a roll binding"
        );
        // The target-query face pins the same scoping per the spec reading
        // (language.md §4.3 grants the binding to if conditions and prose
        // tokens, not query terms); see the findings list in the M2 report.
        let target_query = r#"
game "Scope"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

predicates
  predicate fate_touched(p) = p.spark >= fate

cards
  card coin
    stackable
    aspects spark 1
    start in hall

verbs
  verb grabber committable instances 1
    recipe take
      duration 0
      effects
        roll 1d6 as fate
        destroy world fate_touched count 1
"#;
        assert!(
            compile_error_codes(target_query).contains(&code::E244),
            "a target query term can never see a roll binding"
        );
    }
}

// ---------------------------------------------------------------------------
// Entries 35-44 — the relations primitive (spec.md §4.9).
// ---------------------------------------------------------------------------

mod relations_matrix {
    use super::*;

    /// One relation fixture family: two actors, one item, two coins. Kinds
    /// `carries` (directed exclusive), `married` (symmetric exclusive),
    /// `knows` (directed), `north_of` (directed). No boot edges — every row
    /// in the state comes from the spliced effects.
    fn relations_game(effects: &str) -> Engine {
        let src = format!(
            r#"
game "RelMat"
  seed host
  turn 1m
  home hall

zones
  hall
  vault

aspects
  worn: 0
  spark: 1

relations
  carries exclusive
  married symmetric exclusive
  knows
  north_of

cards
  card alice
    unique
    aspects spark 1
    start in hall

  card bob
    unique
    start in hall

  card sword
    unique
    start in hall

  card coin
    stackable
    start in hall count 2

verbs
  verb act committable instances any
    recipe go
      duration 0
      effects
{effects}
"#,
            effects = effects
        );
        boot(compile(&src), 1)
    }

    // matrix 35
    /// `relate`/`unrelate` basic: the written edge reads through `rel` and
    /// the `related:` scope, multi-target endpoints apply per pair in
    /// resolved order, and each logical edge speaks once.
    #[test]
    fn matrix_35_relate_unrelate_pos() {
        let mut e = relations_game(
            "        relate alice to sword as carries\n        relate world name:coin to alice as knows",
        );
        let alice = common::named(&e, "alice");
        let sword = common::named(&e, "sword");
        let coins: Vec<EntityId> = e
            .state()
            .entities
            .values()
            .filter(|x| x.name == "coin")
            .map(|x| x.id)
            .collect();
        let events = common::commit_named(&mut e, "act", &[]).expect("commits");
        assert_eq!(
            events
                .iter()
                .filter(|ev| matches!(ev, Event::Related { .. }))
                .count(),
            3,
            "one per written logical edge"
        );
        let rel = &e.state().relations;
        assert!(rel
            .get(&alice)
            .and_then(|k| k.get("carries"))
            .is_some_and(|o| o.contains(&sword)));
        // The knows edges point at alice from each coin in resolved order.
        assert_eq!(
            coins
                .iter()
                .filter(|c| rel
                    .get(c)
                    .and_then(|k| k.get("knows"))
                    .is_some_and(|o| o.contains(&alice)))
                .count(),
            2
        );

        let mut e2 = relations_game(
            "        relate alice to sword as carries\n        unrelate alice from sword as carries",
        );
        let events = common::commit_named(&mut e2, "act", &[]).expect("commits");
        assert_eq!(
            events
                .iter()
                .filter(|ev| matches!(ev, Event::Unrelated { .. }))
                .count(),
            1
        );
        assert!(e2.state().relations.is_empty());
    }

    // matrix 36
    /// Symmetric two-way read over single canonical storage: one row under
    /// the min endpoint, `rel` holds both ways, enumeration matches from
    /// either end.
    #[test]
    fn matrix_36_symmetric_two_way_read() {
        let mut e = relations_game("        relate bob to alice as married");
        let alice = common::named(&e, "alice");
        let bob = common::named(&e, "bob");
        common::commit_named(&mut e, "act", &[]).expect("commits");
        let rel = &e.state().relations;
        assert_eq!(rel.len(), 1, "one canonical row");
        let (subject, object) = if alice < bob {
            (alice, bob)
        } else {
            (bob, alice)
        };
        assert!(rel
            .get(&subject)
            .and_then(|k| k.get("married"))
            .is_some_and(|o| o.contains(&object)));
        assert!(story_core::edge_exists(rel, alice, "married", bob, true));
        assert!(story_core::edge_exists(rel, bob, "married", alice, true));
    }

    // matrix 37
    /// Exclusive steal with events: a directed kind steals per object (the
    /// prior holder is evicted before the new edge speaks); a symmetric kind
    /// gives each endpoint valence 1.
    #[test]
    fn matrix_37_exclusive_steal() {
        let mut e = relations_game(
            "        relate bob to sword as carries\n        relate alice to sword as carries",
        );
        let bob = common::named(&e, "bob");
        let alice = common::named(&e, "alice");
        let sword = common::named(&e, "sword");
        let events = common::commit_named(&mut e, "act", &[]).expect("commits");
        let spoken: Vec<String> = events
            .iter()
            .filter_map(|ev| match ev {
                Event::Related { from, to, .. } => Some(format!("+{from}->{to}")),
                Event::Unrelated { from, to, .. } => Some(format!("-{from}->{to}")),
                _ => None,
            })
            .collect();
        assert_eq!(
            spoken,
            vec![
                format!("+{bob}->{sword}"),
                format!("-{bob}->{sword}"),
                format!("+{alice}->{sword}")
            ],
            "the steal speaks between the two writes"
        );
        assert_eq!(e.state().relations.len(), 1);

        // Symmetric valence: alice-bob then alice-sword leaves one row.
        let mut e2 = relations_game(
            "        relate alice to bob as married\n        relate alice to sword as married",
        );
        let sword = common::named(&e2, "sword");
        let _ = common::commit_named(&mut e2, "act", &[]).expect("commits");
        assert_eq!(e2.state().relations.len(), 1);
        let alice2 = common::named(&e2, "alice");
        assert!(story_core::edge_exists(
            &e2.state().relations,
            alice2,
            "married",
            sword,
            true
        ));
    }

    // matrix 38
    /// `rel()` faces: positive, negative, and dead-endpoint reads; inside a
    /// slot-filter predicate (candidate as subject) and an expiry book.
    #[test]
    fn matrix_38_rel_operand_faces() {
        let src = r#"
game "Faces"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  worn: 0

relations
  carries exclusive

predicates
  predicate alice_carries(p) = rel(carries, alice, p)

cards
  card alice
    unique
    start in hall

  card bob
    unique
    start in hall

  card sword
    unique
    lifetime 1m
    start in hall
    on expire
      rule gasp when rel(carries, alice, subject)
        effects
          set alice.worn += 1

verbs
  verb act committable instances any
    recipe link
      duration 0
      effects
        relate alice to sword as carries

  verb probe committable instances any
    recipe look
      duration 0
      when rel(carries, alice, sword)
      effects
        narrate "edge"

  verb picker committable instances any
    recipe pick
      duration 0
      slots
        held: 1..1 alice_carries
      effects
        narrate "picked {held}"
"#
        .to_string();
        let mut e = boot(compile(&src), 1);
        assert!(
            common::commit_named(&mut e, "probe", &[]).is_err(),
            "no edge: no eligible recipe"
        );
        common::commit_named(&mut e, "act", &[]).expect("relates");
        let events = common::commit_named(&mut e, "probe", &[]).expect("the edge gates in");
        assert_eq!(narration(&events), vec!["edge"]);
        let events =
            common::commit_named(&mut e, "picker", &[("held", "sword")]).expect("filter matches");
        assert_eq!(narration(&events), vec!["picked sword"]);
        // The expiry book reads the edge through `subject` before its
        // (replaced) destruction.
        let events = e.advance(1).expect("expires");
        let alice = common::named(&e, "alice");
        assert_eq!(common::aspect(&e, alice, "worn"), 1);
        assert!(narration(&events).is_empty());
    }

    // matrix 39
    /// The `related:` scope in count/exists/total and as an effect target
    /// (per-card `set` walk), enumerating over cards living in a situation
    /// interior and a carrier zone.
    #[test]
    fn matrix_39_related_scope() {
        let src = r#"
game "Scope"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  worn: 0
  spark: 1

relations
  carries exclusive

cards
  card alice
    unique
    start in hall

  card sword
    unique
    start in hall

  card coin
    stackable
    start in hall count 2

verbs
  verb act committable instances any
    recipe link
      duration 0
      effects
        relate alice to sword as carries
        relate alice to world name:coin as carries
        set related:carries of alice aspect:worn += 1

  verb holder committable instances 1
    recipe hold
      duration 10m
      slots
        held: 1..1 aspect:spark
      effects
        narrate "holding"
"#
        .to_string();
        let mut e = boot(compile(&src), 1);
        common::commit_named(&mut e, "act", &[]).expect("links");
        let alice = common::named(&e, "alice");
        // The walk stamped every related card, wherever it lives.
        assert_eq!(common::aspect(&e, alice, "worn"), 0);
        let sword = common::named(&e, "sword");
        assert_eq!(common::aspect(&e, sword, "worn"), 1);
        let coins: Vec<EntityId> = e
            .state()
            .entities
            .values()
            .filter(|x| x.name == "coin")
            .map(|x| x.id)
            .collect();
        assert!(coins.iter().all(|c| common::aspect(&e, *c, "worn") == 1));
        // Relocation does not disturb enumeration: the sword is bound into a
        // situation interior; count still reads 3 related cards.
        common::commit_named(&mut e, "holder", &[("held", "sword")]).expect("binds");
        let rel = &e.state().relations;
        assert_eq!(
            rel.get(&alice)
                .and_then(|k| k.get("carries"))
                .map(|o| o.len()),
            Some(3)
        );
    }

    // matrix 40
    /// Boot `relates` lines: edges present at t=0 before any action,
    /// multi-count objects resolve to every live copy, and a missing object
    /// warns on the boot stream while play proceeds.
    #[test]
    fn matrix_40_boot_relates() {
        let src = r#"
game "BootRel"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  north_of
  adjacent symmetric

cards
  card gate
    unique
    start in hall
    relates north_of post, lantern
    relates adjacent post

  card post
    unique
    start in hall

  card lantern
    stackable
    start in hall count 2

  card ghost_post
    unique
    start in hall
    relates north_of nothing_here

  card nothing_here
    unique

verbs
  verb act committable instances any
    recipe go
      duration 0
      effects
        narrate "warm"
"#
        .to_string();
        let content = compile(&src);
        let (e, diags) =
            story_engine::Engine::boot(content, story_engine::EffectRegistry::new(), 1, None);
        assert!(
            diags
                .iter()
                .any(|d| !d.is_error() && d.kind.word() == "UnresolvableRef"),
            "the missing object warns"
        );
        assert!(!diags.iter().any(|d| d.is_error()));
        let gate = common::named(&e, "gate");
        let post = common::named(&e, "post");
        let lanterns: Vec<EntityId> = e
            .state()
            .entities
            .values()
            .filter(|x| x.name == "lantern")
            .map(|x| x.id)
            .collect();
        assert_eq!(lanterns.len(), 2);
        let north = e
            .state()
            .relations
            .get(&gate)
            .and_then(|k| k.get("north_of"))
            .cloned()
            .expect("boot edges");
        assert!(north.contains(&post) && lanterns.iter().all(|l| north.contains(l)));
        // ghost_post warned and skipped; it holds no rows at all.
        let ghost = common::named(&e, "ghost_post");
        assert!(!e.state().relations.contains_key(&ghost));
        let _ = story_core::edge_exists(&e.state().relations, post, "adjacent", gate, true);
    }

    // matrix 41
    /// Snapshot round-trip and undo: relations ride the state bytes; the
    /// host-held pre-action snapshot restores them bit-for-bit.
    #[test]
    fn matrix_41_snapshot_and_undo() {
        let mut e = relations_game(
            "        relate bob to sword as carries\n        relate alice to sword as carries",
        );
        let snap = e.snapshot();
        let snap_relations = relations_of(&snap);
        common::commit_named(&mut e, "act", &[]).expect("steals");
        assert_eq!(e.state().relations.len(), 1);
        e.restore(&snap).expect("restores");
        assert_eq!(&e.state().relations, &snap_relations);
        let again = e.snapshot();
        assert_eq!(snap.state, again.state);
    }

    fn relations_of(snap: &story_core::Snapshot) -> story_core::Relations {
        serde_json::from_slice::<story_core::State>(&snap.state)
            .expect("snapshot state deserializes")
            .relations
    }

    // matrix 42
    /// Move independence: relations survive `move` into a zone and a card
    /// carrier, and die on destroy — silently.
    #[test]
    fn matrix_42_move_independence() {
        let mut e = relations_game(
            "        relate alice to sword as carries\n        move sword into vault\n        move sword into alice",
        );
        let alice = common::named(&e, "alice");
        let sword = common::named(&e, "sword");
        common::commit_named(&mut e, "act", &[]).expect("commits");
        assert_eq!(common::zone_kind_of(&e, sword), "carrier");
        assert!(e
            .state()
            .relations
            .get(&alice)
            .and_then(|k| k.get("carries"))
            .is_some_and(|o| o.contains(&sword)));

        let mut e2 =
            relations_game("        relate alice to sword as carries\n        destroy sword");
        let events = common::commit_named(&mut e2, "act", &[]).expect("destroys");
        let alice2 = common::named(&e2, "alice");
        assert!(!e2.state().relations.contains_key(&alice2));
        assert!(!events
            .iter()
            .any(|ev| matches!(ev, Event::Unrelated { .. })));
    }

    // matrix 43
    /// `first(...)` endpoints: oldest-copy resolution, a `slot:` scope
    /// endpoint, and the empty read (false / empty, never a failure).
    #[test]
    fn matrix_43_first_endpoints() {
        let mut e = relations_game(
            "        relate alice to world name:coin as knows\n        if rel(knows, alice, first(world name:coin))\n          narrate \"oldest\"\n        if rel(knows, alice, first(zone:vault))\n          narrate \"never\"\n        if count(related:knows of first(zone:vault)) == 0\n          narrate \"empty\"",
        );
        let events = common::commit_named(&mut e, "act", &[]).expect("commits");
        assert_eq!(narration(&events), vec!["oldest", "empty"]);
        let coins: Vec<EntityId> = e
            .state()
            .entities
            .values()
            .filter(|x| x.name == "coin")
            .map(|x| x.id)
            .collect();
        let alice = common::named(&e, "alice");
        assert_eq!(
            e.state()
                .relations
                .get(&alice)
                .and_then(|k| k.get("knows"))
                .cloned()
                .unwrap_or_default(),
            coins.into_iter().collect::<std::collections::BTreeSet<_>>()
        );

        // The slot: endpoint through a card-ref.
        let src = r#"
game "SlotRef"
  seed host
  turn 1m
  home hall

zones
  hall

relations
  carries exclusive

cards
  card alice
    unique
    start in hall

  card sword
    unique
    start in hall

verbs
  verb act committable instances 1
    recipe go
      duration 0
      slots
        hero: 1..1 name:alice
      effects
        relate alice to sword as carries
        narrate "[{related:carries of first(slot:hero)}]"
"#
        .to_string();
        let mut e2 = boot(compile(&src), 1);
        let events =
            common::commit_named(&mut e2, "act", &[("hero", "alice")]).expect("binds and relates");
        assert_eq!(narration(&events), vec!["[sword]"]);
    }

    // matrix 44
    /// `unrelate a from b` with no kind severs every logical edge between
    /// the pair, either direction, once per edge.
    #[test]
    fn matrix_44_unrelate_severs_all() {
        let mut e = relations_game(
            "        relate alice to bob as knows\n        relate alice to bob as carries\n        relate bob to alice as married\n        relate bob to sword as knows\n        unrelate alice from bob",
        );
        let events = common::commit_named(&mut e, "act", &[]).expect("commits");
        assert_eq!(
            events
                .iter()
                .filter(|ev| matches!(ev, Event::Unrelated { .. }))
                .count(),
            3,
            "knows a->b, carries a->b, married (canonical) — each once"
        );
        let alice = common::named(&e, "alice");
        let bob = common::named(&e, "bob");
        let sword = common::named(&e, "sword");
        // The unrelated third party survives.
        assert!(e
            .state()
            .relations
            .get(&bob)
            .and_then(|k| k.get("knows"))
            .is_some_and(|o| o.contains(&sword)));
        assert!(!e.state().relations.contains_key(&alice));
        let _ = (alice, bob);
    }
}

// ---------------------------------------------------------------------------
// Invariants T1-T14 (kernel-api.md §9). Forwarding comments name the test
// that owns each invariant; the rest carry executable assertions here.
// ---------------------------------------------------------------------------

mod invariants {
    use super::*;

    // t1 — totality: absent declared keys, dead unique cards, empty scopes,
    // unbound slot scopes, and runtime zero division all read 0. (The
    // unbound-roll clause is authoring-unreachable: a roll reference outside
    // a binding list is the E244 refusal pinned by matrix_27_roll_scope_.)
    #[test]
    fn t1_totality_reads_are_total() {
        let src = r#"
game "Total"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  ghost: 0
  charge: 0

cards
  card beacon
    unique
    start in hall

  card slain
    unique
    start in hall

  card coin
    stackable
    start in hall

verbs
  verb prober committable instances 1
    recipe probe
      duration 0
      slots
        never: 0..1 aspect:ghost
      effects
        destroy slain
        narrate "a{slain.ghost}"
        narrate "b{beacon.ghost}"
        if count(interior, aspect:ghost) == 0
          narrate "c0"
        if count(slot:never, aspect:ghost) == 0
          narrate "d0"
        set beacon.charge += 5 / count(world, aspect:ghost)
        narrate "f{beacon.charge}"
        if alive(slain) == 0
          narrate "g0"
"#;
        let mut e = game(src);
        let events = common::commit_named(&mut e, "prober", &[]).expect("probes");
        assert_eq!(
            narration(&events),
            vec!["a0", "b0", "c0", "d0", "f0", "g0"],
            "every total read answered 0"
        );
        assert_eq!(aspect(&e, common::named(&e, "beacon"), "charge"), 0);
    }

    // t2 — constant zero divisor refused at compile time [E242]. Compile-only
    // face; a constant zero divisor cannot reach the kernel. Owned by
    // story-lang: features::compile_time_invariant_t2_zero_divisor_refused,
    // diagnostics::e242_constant_zero_divisor, and
    // diagnostics::e242_constant_zero_divisor_folded. The runtime face of
    // division totality (x / 0 == 0) is asserted by t1 above.

    // t3 — commit atomicity: any refusal leaves the state bit-identical and
    // spends zero time. Owned by kernel::atomicity::commit_effect_failure_
    // restores_bit_for_bit and emberwatch::determinism_same_seed_same_ops_
    // identical_state; the GuardRefused face is asserted by
    // matrix_16_guard_refused_pos; DeckExhausted by matrix_10_fallback_neg;
    // NameInUse by matrix_07_destroy_swap_neg and matrix_11_unique_mint_skip_neg.

    // t4 — per-unit atomicity on the advance path: the unit checkpoint
    // includes the RNG counter and deck cursors.
    #[test]
    fn t4_advance_rollback_restores_rng_and_deck_cursors() {
        let src = r#"
game "Cursed"
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

decks
  deck pouch
    entries coin
    wrap

verbs
  verb risky committable instances 1
    recipe gamble
      duration 10m
      effects
        draw from pouch
        roll 1d6 as fate
        consume aspect:ghost 1
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "risky", &[]).expect("opens");
        let (cursor, rng) = (
            *e.state().decks.values().next().expect("the deck exists"),
            e.state().rng_counter,
        );
        assert_eq!((cursor, rng), (0, 0));
        let events = e
            .advance(10)
            .expect("the unit fails; the advance continues");
        assert!(events
            .iter()
            .any(|ev| matches!(ev, Event::ResolutionFailed { .. })));
        assert_eq!(
            *e.state().decks.values().next().expect("the deck exists"),
            cursor,
            "deck cursor"
        );
        assert_eq!(e.state().rng_counter, rng, "rng counter");
        assert_eq!(count_named(&e, "coin"), 0, "the drawn card rolled back");
        assert_eq!(situations(&e).len(), 0, "the situation aborted");
        assert_eq!(e.state().now, 10, "the loop moved on to the target");
    }

    // t5 — livelock: either budget breach rolls the whole call back to its
    // entry checkpoint. The commit-path face is owned by
    // kernel::instant::instant_chain_over_budget_livelocks_and_rolls_back
    // (depth cap) and matrix_22_recursion_budget_neg (unit cap); this is the
    // advance-path face.
    #[test]
    fn t5_livelock_rolls_back_the_whole_advance() {
        let src = r#"
game "Swarm"
  seed host
  turn 1m
  home hall

zones
  hall

verbs
  verb caller committable instances 1
    recipe go
      duration 10m
      effects
        start swarm

  verb swarm uncommittable instances any
    recipe split
      chain-only
      duration 0
      effects
        start swarm count 2
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "caller", &[]).expect("opens");
        let before = state_bytes(&e);
        let now = e.state().now;
        assert_eq!(err_kind(e.advance(10)), DiagKind::Livelock);
        assert_eq!(state_bytes(&e), before, "the whole advance rolled back");
        assert_eq!(e.state().now, now);
        assert_eq!(situations(&e).len(), 1, "the caller's instance survived");
    }

    // t6 — determinism. Owned by
    // emberwatch::determinism_same_seed_same_ops_identical_state (same
    // content + seed + ops across advance splits, and the bit-restored retry
    // after a refusal).

    // t7 — endings discard the queue, refuse every entry point, and undo
    // resumes. Owned by emberwatch::ending_closes_the_run_until_restored and
    // emberwatch::scripted_session_reaches_survived.

    // t8 — locks are total. Owned by
    // kernel::atomicity::locks_are_total_across_situations; the mid-run bind
    // face (EntityLocked through `bind`) is asserted by
    // matrix_31_bind_happy_path_neg.

    // t9 — context-free scopes: expiry books and event books read `bound`,
    // `interior`, and `slot:` as empty. The expiry face is owned by
    // kernel::expiry::*; this is the event-book face, fired from inside a
    // live situation with locked bindings.
    #[test]
    fn t9_event_books_are_context_free() {
        let src = r#"
game "Probe"
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
    start in hall

verbs
  verb holder committable instances 1
    recipe hold
      duration 10m
      slots
        held: 1..1 aspect:spark
      effects
        signal probe

on event probe
  rule check when total(bound, aspect:spark) == 0 and count(interior, aspect:spark) == 0 and count(slot:held, aspect:spark) == 0
    effects
      narrate "context-free"
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "holder", &[("held", "coin")]).expect("binds");
        let events = e.advance(10).expect("completes and signals");
        assert_eq!(
            narration(&events),
            vec!["context-free"],
            "the book read empty scopes while the situation's bindings were locked"
        );
    }

    // t10 — start candidacy. Owned by matrix_30_* (post-fix semantics) and
    // kernel::chains::{per_slot_minimum_gates_engine_source_candidacy,
    // matched_but_materialization_short_skips_the_open}.

    // t11 — roll validation. Owned by matrix_27_dynamic_rolls_pos (count 0
    // leaves the RNG counter unmoved) and matrix_27_dynamic_rolls_neg
    // (invalid operands roll the unit back).

    // t12 — card books. Owned by
    // kernel::order::completion_order_recipe_book_then_card_books_in_bind_
    // order (order, dedup, declares) and matrix_25_* (declares and the
    // inline-completion face).

    // t13 — handle scope. Owned by matrix_26_handles_pos (pool behavior
    // within its list) and matrix_26_handles_neg ([E238] is the static face
    // of "empty outside its list").

    // t14 — per-binding writes: a slot-target write lands once per bound
    // card, and the same card in two slots accumulates both writes.
    #[test]
    fn t14_per_binding_writes() {
        let src = r#"
game "Stamp"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0
  stamp: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall

  card other
    stackable
    aspects spark 1
    start in hall

verbs
  verb stamper committable instances 1
    recipe mark
      duration 0
      slots
        a: 1..1 aspect:spark
        b: 1..1 aspect:spark
      effects
        set slot:a aspect:stamp += 1
        set slot:b aspect:stamp += 1
"#;
        let mut e = game(src);
        common::commit_named(&mut e, "stamper", &[("a", "coin"), ("b", "coin")]).expect("stamps");
        let coin = common::named(&e, "coin");
        assert_eq!(aspect(&e, coin, "stamp"), 2, "one write per binding");
        assert_eq!(
            aspect(&e, common::named(&e, "other"), "stamp"),
            0,
            "the other card is untouched"
        );
    }
}
