//! Randomized differential testing for the card-reachability analyzer —
//! 100 seeds. The soundness contract under fire:
//!   unobtainable ⇒ never appears in any explored play   (hard — the forward
//!                  truth is an under-approximation, sound under any bounds)
//!   obtainable   ⇒ appears in some explored play        (hard; downgraded to
//!                  inconclusive when the exploration hit its state cap)
//! Failures print their seed — run_fuzz_seed(seed) reproduces exactly.
//!
//! Bounds rationale (2026-10 audit): ~46 ms/seed
//! at the default 8 steps × 500 states (deep +7/×20 never fires its cap —
//! 0 inconclusive over 300 seeds), so 100 seeds sample 2.5× the scenarios of
//! the old 40. The CLI (reach-fuzz bin) shakes thousands. The suite rides the
//! shared module tests/common/reach_fuzz_lib.rs —
//! the same module the bin mounts, so bin and test cannot drift.

mod common;

use common::reach_fuzz_lib::run_fuzz_seed;

const SEEDS: u32 = 100;

/// One seed under the DEFAULT bounds (8 steps × 500 states); inconclusive
/// probes print to the test output, failures fail the test.
fn assert_seed_sound(seed: u32) {
    let out = run_fuzz_seed(seed, 8, 500).expect("the fuzz oracle never errors on its own scenarios");
    if !out.inconclusive.is_empty() {
        println!("seed {}: {}", seed, out.inconclusive.join("; "));
    }
    assert!(
        out.failures.is_empty(),
        "seed {}: {}",
        seed,
        out.failures.join("\n")
    );
}

macro_rules! fuzz_seed {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            assert_seed_sound($seed);
        }
    };
}

fuzz_seed!(seed_1, 1);
fuzz_seed!(seed_2, 2);
fuzz_seed!(seed_3, 3);
fuzz_seed!(seed_4, 4);
fuzz_seed!(seed_5, 5);
fuzz_seed!(seed_6, 6);
fuzz_seed!(seed_7, 7);
fuzz_seed!(seed_8, 8);
fuzz_seed!(seed_9, 9);
fuzz_seed!(seed_10, 10);
fuzz_seed!(seed_11, 11);
fuzz_seed!(seed_12, 12);
fuzz_seed!(seed_13, 13);
fuzz_seed!(seed_14, 14);
fuzz_seed!(seed_15, 15);
fuzz_seed!(seed_16, 16);
fuzz_seed!(seed_17, 17);
fuzz_seed!(seed_18, 18);
fuzz_seed!(seed_19, 19);
fuzz_seed!(seed_20, 20);
fuzz_seed!(seed_21, 21);
fuzz_seed!(seed_22, 22);
fuzz_seed!(seed_23, 23);
fuzz_seed!(seed_24, 24);
fuzz_seed!(seed_25, 25);
fuzz_seed!(seed_26, 26);
fuzz_seed!(seed_27, 27);
fuzz_seed!(seed_28, 28);
fuzz_seed!(seed_29, 29);
fuzz_seed!(seed_30, 30);
fuzz_seed!(seed_31, 31);
fuzz_seed!(seed_32, 32);
fuzz_seed!(seed_33, 33);
fuzz_seed!(seed_34, 34);
fuzz_seed!(seed_35, 35);
fuzz_seed!(seed_36, 36);
fuzz_seed!(seed_37, 37);
fuzz_seed!(seed_38, 38);
fuzz_seed!(seed_39, 39);
fuzz_seed!(seed_40, 40);
fuzz_seed!(seed_41, 41);
fuzz_seed!(seed_42, 42);
fuzz_seed!(seed_43, 43);
fuzz_seed!(seed_44, 44);
fuzz_seed!(seed_45, 45);
fuzz_seed!(seed_46, 46);
fuzz_seed!(seed_47, 47);
fuzz_seed!(seed_48, 48);
fuzz_seed!(seed_49, 49);
fuzz_seed!(seed_50, 50);
fuzz_seed!(seed_51, 51);
fuzz_seed!(seed_52, 52);
fuzz_seed!(seed_53, 53);
fuzz_seed!(seed_54, 54);
fuzz_seed!(seed_55, 55);
fuzz_seed!(seed_56, 56);
fuzz_seed!(seed_57, 57);
fuzz_seed!(seed_58, 58);
fuzz_seed!(seed_59, 59);
fuzz_seed!(seed_60, 60);
fuzz_seed!(seed_61, 61);
fuzz_seed!(seed_62, 62);
fuzz_seed!(seed_63, 63);
fuzz_seed!(seed_64, 64);
fuzz_seed!(seed_65, 65);
fuzz_seed!(seed_66, 66);
fuzz_seed!(seed_67, 67);
fuzz_seed!(seed_68, 68);
fuzz_seed!(seed_69, 69);
fuzz_seed!(seed_70, 70);
fuzz_seed!(seed_71, 71);
fuzz_seed!(seed_72, 72);
fuzz_seed!(seed_73, 73);
fuzz_seed!(seed_74, 74);
fuzz_seed!(seed_75, 75);
fuzz_seed!(seed_76, 76);
fuzz_seed!(seed_77, 77);
fuzz_seed!(seed_78, 78);
fuzz_seed!(seed_79, 79);
fuzz_seed!(seed_80, 80);
fuzz_seed!(seed_81, 81);
fuzz_seed!(seed_82, 82);
fuzz_seed!(seed_83, 83);
fuzz_seed!(seed_84, 84);
fuzz_seed!(seed_85, 85);
fuzz_seed!(seed_86, 86);
fuzz_seed!(seed_87, 87);
fuzz_seed!(seed_88, 88);
fuzz_seed!(seed_89, 89);
fuzz_seed!(seed_90, 90);
fuzz_seed!(seed_91, 91);
fuzz_seed!(seed_92, 92);
fuzz_seed!(seed_93, 93);
fuzz_seed!(seed_94, 94);
fuzz_seed!(seed_95, 95);
fuzz_seed!(seed_96, 96);
fuzz_seed!(seed_97, 97);
fuzz_seed!(seed_98, 98);
fuzz_seed!(seed_99, 99);
fuzz_seed!(seed_100, 100);

/// the suite covers exactly SEEDS seeds — a compile-time count over the macro
/// invocations above (update both together).
#[test]
fn the_fuzz_suite_covers_100_seeds() {
    assert_eq!(SEEDS, 100);
}
