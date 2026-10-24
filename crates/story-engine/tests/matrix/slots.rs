//! Entries 2, 5, 31 — slot release: take slots, the releases override, and the mid-run bind happy path.

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


// matrix 45
/// A greedy slot tops up from the world just before its completion
/// evaluates: a card minted mid-flight is pulled and read by the
/// completion's own conditions (spec.md §4.4).
#[test]
fn matrix_45_greedy_topup_before_completion() {
    let src = r#"
game "Wait"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

cards
  card waiter_card
    unique
    start in hall

  card orb
    stackable
    aspects spark 1

verbs
  verb waiter committable instances 1
    recipe wait
      duration 10m
      slots
        snag: 0..1 greedy open aspect:spark
      effects
        if (total(interior, aspect:spark) >= 1)
          narrate "fed"
        else
          narrate "starved"

  verb maker committable instances 1
    recipe mint
      duration 0
      effects
        spawn orb count 1 into hall
        narrate "minted"
"#;
    let mut e = game(&src);
    // The waiter opens with nothing to grab (min 0); the orb is minted
    // mid-flight onto the table.
    common::commit(&mut e, "waiter", &[]).expect("opens");
    common::commit(&mut e, "maker", &[]).expect("mints");
    let orb = common::named(&e, "orb");
    let events = e.advance(10).expect("settles");
    // The pre-completion top-up pulled the orb before the effects read it.
    assert!(
        events.iter().any(|ev| matches!(
            ev,
            Event::Bound { slot, entity, .. }
                if *slot == SlotName::from("snag") && *entity == orb
        )),
        "the orb is grabbed at the settle: {events:?}"
    );
    assert!(
        narration(&events).iter().any(|t| t.contains("fed")),
        "the completion read the pulled card: {:?}",
        narration(&events)
    );
    assert_eq!(count_named(&e, "orb"), 1, "released, not destroyed");
}
