//! Commit atomicity (T3, T11): bit-for-bit restore, boot warnings, and total locks (T8).

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
