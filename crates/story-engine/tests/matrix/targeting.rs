//! Entries 7, 8, 9, 13, 24, 28, 29 — targets and queries.

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
