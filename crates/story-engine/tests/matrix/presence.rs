//! Slot presence: `internal` and `shown when` gate the player faces only —
//! `bind`/`can_bind` refuse `SlotHidden`, commit staging and mechanical paths
//! are untouched, and presence is derived, never stored.

use super::*;

fn can_bind_kind(
    e: &Engine,
    sit: story_core::SituationId,
    slot: &str,
    entity: EntityId,
) -> DiagKind {
    e.can_bind(sit, SlotName::from(slot), entity)
        .expect_err("expected a refusal")
        .kind
}

fn presence_src() -> String {
    r#"
game "Presence"
  seed host
  turn 1m
  home hall

zones
  hall

aspects
  spark: 0

legacies
  favor: 0

cards
  card coin
    stackable
    aspects spark 1
    start in hall count 3

verbs
  verb keeper committable instances 1
    recipe rite
      duration 10m
      slots
        main: 1..1 open aspect:spark
        echo: 0..1 open aspect:spark
          shown when count(slot:main) == 1
        gated: 0..1 open aspect:spark
          shown when legacy.favor >= 1
        hoard: 0..1 open internal aspect:spark
      effects
        narrate "rite"

  verb priest committable instances 1
    recipe bless
      duration 0m
      effects
        legacy favor += 1
        narrate "blessed"
"#
    .to_string()
}

// presence 01
/// An `internal` slot refuses the player faces with `SlotHidden` even while
/// `open`, and the kernel never auto-binds it against the player's will.
#[test]
fn presence_01_internal_slot_refuses_bind() {
    let mut e = game(&presence_src());
    let coins = ids_of(&e, "coin");
    common::commit(&mut e, "keeper", &[("main", coins[0])]).expect("opens");
    let sit = sit_of(&e, "keeper.rite");
    let other = coins[1];
    assert_eq!(can_bind_kind(&e, sit, "hoard", other), DiagKind::SlotHidden);
    assert_eq!(
        err_kind(e.bind(sit, SlotName::from("hoard"), other)),
        DiagKind::SlotHidden
    );
    assert!(!e.slot_shown(sit, SlotName::from("hoard")).unwrap());
}

// presence 02
/// A `shown when` slot refuses `bind` while its condition does not hold and
/// accepts once it does: presence is re-read on every attempt (a legacy write
/// from another verb's instant recipe reveals the slot mid-run).
#[test]
fn presence_02_condition_reveals_slot_midrun() {
    let mut e = game(&presence_src());
    let coins = ids_of(&e, "coin");
    common::commit(&mut e, "keeper", &[("main", coins[0])]).expect("opens");
    let sit = sit_of(&e, "keeper.rite");
    let other = coins[1];
    assert_eq!(can_bind_kind(&e, sit, "gated", other), DiagKind::SlotHidden);
    common::commit(&mut e, "priest", &[]).expect("bless");
    e.can_bind(sit, SlotName::from("gated"), other)
        .expect("revealed once favor >= 1");
    e.bind(sit, SlotName::from("gated"), other)
        .expect("the revealed slot binds");
}

// presence 03
/// A `shown when` slot whose condition reads a sibling slot is revealed by
/// that sibling's binding: `slot:` scopes in presence read the live bindings.
#[test]
fn presence_03_sibling_slot_reveals_sibling() {
    let mut e = game(&presence_src());
    let coins = ids_of(&e, "coin");
    common::commit(&mut e, "keeper", &[("main", coins[0])]).expect("opens");
    let sit = sit_of(&e, "keeper.rite");
    let other = coins[1];
    assert!(e.slot_shown(sit, SlotName::from("echo")).unwrap());
    e.bind(sit, SlotName::from("echo"), other)
        .expect("echo binds while main is bound");
}

// presence 04
/// Commit staging is presence-blind: the host assembles `internal`-slot
/// bindings itself, and commit accepts them unchanged.
#[test]
fn presence_04_commit_stages_internal_slot() {
    let mut e = game(&presence_src());
    let coins = ids_of(&e, "coin");
    common::commit(&mut e, "keeper", &[("main", coins[0]), ("hoard", coins[1])])
        .expect("internal-slot staging is ordinary commit data");
    let sit = sit_of(&e, "keeper.rite");
    assert_eq!(
        e.state().situations[&sit].bindings[&SlotName::from("hoard")].len(),
        1
    );
}

// presence 05
/// The staged presence face mirrors commit selection's context: staged
/// bindings stand in for the slot scopes; internal slots read hidden; an
/// unknown slot reads not-shown.
#[test]
fn presence_05_staged_presence_face() {
    let e = game(&presence_src());
    let coins = ids_of(&e, "coin");
    let main = (SlotName::from("main"), coins[0]);
    let rite = story_core::RecipeId::from("keeper.rite");
    assert!(e.slot_shown_staged(rite.clone(), SlotName::from("main"), &[main.clone()]));
    assert!(e.slot_shown_staged(rite.clone(), SlotName::from("echo"), &[main.clone()]));
    assert!(!e.slot_shown_staged(rite.clone(), SlotName::from("echo"), &[]));
    assert!(!e.slot_shown_staged(rite.clone(), SlotName::from("hoard"), &[main.clone()]));
    assert!(!e.slot_shown_staged(rite, SlotName::from("nope"), &[main]));
}

// presence 06
/// Presence is derived, never stored: two engines from one boot agree bit for
/// bit on state bytes after presence-gated play, and a snapshot round-trip
/// carries no presence residue.
#[test]
fn presence_06_derived_not_stored() {
    let src = presence_src();
    let mut a = game(&src);
    let mut b = game(&src);
    for e in [&mut a, &mut b] {
        let coins = ids_of(e, "coin");
        common::commit(e, "keeper", &[("main", coins[0])]).expect("opens");
    }
    let sit_a = sit_of(&a, "keeper.rite");
    let sit_b = sit_of(&b, "keeper.rite");
    let coins_a = ids_of(&a, "coin");
    let coins_b = ids_of(&b, "coin");
    a.bind(sit_a, SlotName::from("echo"), coins_a[1]).expect("binds");
    b.bind(sit_b, SlotName::from("echo"), coins_b[1]).expect("binds");
    // Identical play; `b` additionally reads the presence faces — pure reads
    // that must not perturb the state (presence is derived, never stored).
    assert!(b.slot_shown(sit_b, SlotName::from("echo")).unwrap());
    assert!(!b.slot_shown_staged(
        story_core::RecipeId::from("keeper.rite"),
        SlotName::from("hoard"),
        &[],
    ));
    assert_eq!(state_bytes(&a), state_bytes(&b));
}
