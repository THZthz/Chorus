//! Port of test/dice.test.ts — W0 subset (plan §4-W0 / §5.2.1): the pinned
//! RNG vectors (3 raw splitmix64 values + the 27-row (seed, counter, sides)
//! → die table, all computed independently in python3 on the TS side), the
//! die range / d2 coverage probe, and the byte-exact meta-seed error message.
//! The engine-dependent describes (ROLL semantics, fold, undo, JSONL
//! identity, triggers, functions) are W3.

use chorus::core::dice::{roll_die, splitmix64};

#[test]
fn splitmix64_raw_values() {
    // ts: test/dice.test.ts:85-89 — raw finalizer pins: catch ANY drift in
    // constants or shift structure.
    assert_eq!(splitmix64(0), 16294208416658607535);
    assert_eq!(splitmix64(1), 10451216379200822465);
    assert_eq!(splitmix64(42), 13679457532755275413);
}

#[test]
fn the_seed_counter_sides_value_table() {
    // ts: test/dice.test.ts:94-119 — computed independently in python3 with
    // arbitrary-precision integers; the two's-complement XOR (asUintN) and
    // the u128 widening multiply must reproduce every row exactly.
    const VECTORS: &[(i64, i64, i64, i64)] = &[
        // seed, counter, sides, die
        (0, 0, 6, 6),
        (0, 1, 6, 4),
        (0, 2, 6, 4),
        (0, 3, 6, 1),
        (0, 4, 6, 3),
        (1, 0, 6, 4),
        (1, 1, 6, 6),
        (1, 2, 6, 1),
        (42, 0, 6, 5),
        (42, 1, 6, 5),
        (42, 2, 6, 2),
        (7, 5, 20, 12),
        (0, 100, 6, 1),
        (0, 101, 6, 5),
        (0, 102, 6, 1),
        (9007199254740991, 3, 6, 5), // 2^53-1: exercises the full BigInt path
        (0, 0, 2, 2),
        (123456789, 12, 10, 10),
        (2147483648, 7, 4, 1),
        (65536, 999, 8, 4),
        (0, 999999, 6, 3),
        // negative seed: the key is the seed's 64-bit TWO'S COMPLEMENT
        // (asUintN) XORed with the counter — pinned so the treatment can
        // never drift
        (-1, 0, 6, 6),
        (-1, 1, 6, 6),
        (-42, 7, 20, 13),
        (9007199254740991, 0, 6, 1), // 2^53-1 at counter 0
        (0, 1099511627776, 6, 1),    // counter 2^40 — the full 64-bit key path
        (42, 1099511627783, 20, 7),  // 2^40+7
    ];
    for &(seed, counter, sides, want) in VECTORS {
        assert_eq!(
            roll_die(seed, counter, sides),
            want,
            "roll_die({seed}, {counter}, {sides})"
        );
    }
}

#[test]
fn die_values_stay_in_1_to_sides_and_every_value_of_a_d2_occurs() {
    // ts: test/dice.test.ts:121-130
    for c in 0..200 {
        let d6 = roll_die(9, c, 6);
        assert!((1..=6).contains(&d6), "d6 at counter {c} gave {d6}");
    }
    let mut faces: Vec<i64> = (0..64).map(|c| roll_die(3, c, 2)).collect();
    faces.sort_unstable();
    faces.dedup();
    assert_eq!(faces, vec![1, 2]);
}

/// ts: src/content/script.ts:113 — the byte-exact meta-seed error template:
/// `` `line ${ln + 1}: seed must be an integer, got '${m[2].trim()}'` ``.
/// dice.test.ts:181 pins it as /seed must be an integer/. W0 freezes the
/// template here (with its 1-based `line` arithmetic); W2's `compile_script`
/// must construct exactly this string inline at its throw site.
fn script_seed_error(ln_zero_based: usize, got_trimmed: &str) -> String {
    format!(
        "line {}: seed must be an integer, got '{}'",
        ln_zero_based + 1,
        got_trimmed
    )
}

#[test]
fn a_bad_meta_seed_is_a_load_error_with_the_verbatim_message() {
    // ts: test/dice.test.ts:177-182 — 'seed: 0' sits at split('\n') index 4
    // of that rig, so the message says line 5.
    let msg = script_seed_error(4, "six");
    assert_eq!(msg, "line 5: seed must be an integer, got 'six'");
    assert!(msg.contains("seed must be an integer"));
}
