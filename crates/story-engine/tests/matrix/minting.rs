//! Entries 1, 3, 11, 12 — the mint pipeline: aliasing, group uniqueness, unique-mint skip, card carriers.

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
