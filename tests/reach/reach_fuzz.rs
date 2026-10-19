//! Randomized differential testing for the card-reachability analyzer —
//! 100 seeds pinning DETERMINISM, not soundness.
//!
//! The soundness contract under fire (the oracle run_fuzz_seed enforces):
//!   unobtainable ⇒ never appears in any explored play   (hard — the forward
//!                  truth is an under-approximation, sound under any bounds)
//!   obtainable   ⇒ appears in some explored play        (hard; downgraded to
//!                  inconclusive when the exploration hit its state cap)
//!
//! The scenario space (gen_scenario v11) now generates completion triggers
//! (`on complete [<aspect>]`), §6.7 chain candidates in verb recipes and
//! trigger bodies (the recipe-body table is the GLOBAL build prefix —
//! recipe-body RUNs cross verbs; slot/expire bodies chain onto the global
//! duration>0 table), recipe-line `aspects`, recipe-line `dur`, and the two
//! §6.3 predicate sum scopes in the `when` candidate table, ambient verbs,
//! and `unique` prototypes (riding deck specs) — features whose
//! analyzer
//! coverage is still being established, and which legitimately surface open
//! false-`obtainable` gaps (the Wave-3 worklist, triaged by the reach-fuzz
//! campaign: `cargo run --bin reach-fuzz -- --seeds 200`). Pinning "these
//! 100 seeds have 0 failures" would pin the open gaps red right here, so
//! soundness is asserted by the CAMPAIGN, not by this suite. What each seed
//! pins instead is that `run_fuzz_seed` is a pure function of its seed: two
//! runs must agree on the failure set, the inconclusive set and the
//! exploration bookkeeping — a generator or oracle that drifts (or a
//! draw-order accident) breaks this first, before it can masquerade as a
//! verdict change. Failures still print to the test output so a red
//! campaign seed is visible from the test log.
//!
//! The v11 campaign ledger (the reshuffled scenario space surfaced latent
//! gap shapes on four seeds — every one a CHAIN-class interaction between
//! the §6.6/§6.7 event-channel follow-ups and the dead-verb / mint-collision
//! demotions, none involving `unique` — those scenarios declare none, so the
//! unique model is inert on them):
//!   - 70, 103, 170: a completion/slot trigger or chain follow-up applies a
//!     recipe whose sibling chain arm collides or whose home verb is
//!     slot-dead — the analyzer proves the mint, the play's completion dies
//!     with the whole settle (false `obtainable`);
//!   - 84: a chain-fed recipe (FORK onto a slot-dead verb) mints at play
//!     over empty event bindings, but the dead-verb demotion closes every
//!     site of that verb (false `unobtainable`).
//!
//! The ledger also carries one UNIQUE-shaped composition, triaged by hand
//! and pinned deliberately as an engine+reach split (reach_unique.rs), not
//! generated: deck `QUEST spec U, B`, a `once` slot trigger drawing QUEST,
//! and any verb minting U. The analyzer's manifested projection is load-time
//! only (in-play mints never mark manifested in its world), so its
//! once-window pins the trigger's single serve on U's live deck position
//! and judges B unobtainable/None, while at play the mint manifests U first
//! and the draw skips the dead position and serves B — a false
//! `unobtainable`, the conservative direction (U non-unique → the analyzer
//! is correct: the single serve genuinely takes position 0).
//!
//! The seed entries are the chain model's open faces, pre-dating v11.
//!
//! Bounds rationale (2026-10 audit, v7 scenarios): the default 8 steps ×
//! 500 states per run, twice per seed; complete-trigger worlds explore
//! noticeably wider than v1's (complete triggers fire on EVERY completion —
//! no once), but the base cap still bounds each run, and the deep phase
//! (+7 steps × 20 states) resolves obtainable claims before any
//! inconclusive is reported. The CLI (reach-fuzz bin) shakes thousands.
//! The suite and the bin share ONE implementation — `chorus::reach::fuzz`,
//! so bin and test cannot drift.

use crate::common::snap::snap;
use chorus::content::compile_content;
use chorus::core::dict::core_dictionary;
use chorus::core::predicate::{Pred, ValExpr};
use chorus::core::query::{CmpOp, QueryTerm};
use chorus::core::r#ref::Ref;
use chorus::core::sentence::Arg;
use chorus::engine::fold::fold;
use chorus::reach::fuzz::{drive_fuzz_seed, gen_scenario, run_fuzz_seed, FuzzOutcome};

/// One seed under the DEFAULT bounds (8 steps × 500 states), run twice: the
/// two outcomes must agree field for field — same failure set, same
/// inconclusive set, same exploration bookkeeping. Any failures print once
/// (open analyzer gaps are the campaign's ledger, not this test's verdict).
fn assert_seed_deterministic(seed: u32) {
    let run = || -> FuzzOutcome {
        run_fuzz_seed(seed, 8, 500).expect("the fuzz oracle never errors on its own scenarios")
    };
    let a = run();
    let b = run();
    if !a.failures.is_empty() {
        println!("seed {} (open gaps — campaign ledger): {}", seed, a.failures.join("; "));
    }
    assert_eq!(a.seed, b.seed, "seed {seed}: the seed echo drifted");
    assert_eq!(a.failures, b.failures, "seed {seed}: the failure set is not stable");
    assert_eq!(a.inconclusive, b.inconclusive, "seed {seed}: the inconclusive set is not stable");
    assert_eq!(a.states_seen, b.states_seen, "seed {seed}: the exploration count drifted");
    assert_eq!(a.deduped, b.deduped, "seed {seed}: the dedup count drifted");
    assert_eq!(a.probes, b.probes, "seed {seed}: the probe list drifted");
}

macro_rules! fuzz_seed {
    ($name:ident, $seed:expr) => {
        #[test]
        fn $name() {
            assert_seed_deterministic($seed);
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

/// The frozen caller discipline, replayed: drive one seed through
/// `drive_fuzz_seed` (K applied settle_steps or the attempt cap), then fold
/// the accumulated log over the pristine base — the replay must rebuild the
/// driven final state exactly (§7 law 6). The content is rebuilt from the
/// same scenario seed (`gen_scenario` is draw-for-draw deterministic under
/// ONE generator version), so the fold input matches the driver's compile
/// input byte for byte.
#[test]
fn the_driven_seed_log_folds_back_to_the_driven_state() {
    let seed = 7;
    let run = drive_fuzz_seed(seed, 25, 400).expect("the driver completes");
    assert!(run.applied > 0, "seed {seed} must apply at least one step");

    let sc = gen_scenario(seed);
    // the scenario's own load-effective dictionary — the same shape the
    // driver used (the sample may declare script verbs)
    let dict = sc.dict;
    let content = compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("the scenario compiles");
    let replayed = fold(&run.records, &run.base, &dict, &content).expect("the log replays");
    assert_eq!(snap(&replayed), snap(&run.final_state));
}

// The v4 `when` table's aspect-scoped world forms: two seeds whose scenarios
// verifiably carry one (a CountQuery/ExistsQuery whose inner query reads an
// Aspect term) — and, on the second seed, the v4 closed (strict `<`)
// aspect-count form the open-ended table lacked — pinned deterministic like
// the sweep above; the differential exercises the analyzer's world-form
// evaluation path (open and closed faces) through them.
fn scenario_has_aspect_world_form(seed: u32) -> bool {
    scenario_pred_has(seed, &|p| {
        matches!(p, Pred::ExistsQuery { query }
            if query.iter().flatten().any(|t| matches!(t, QueryTerm::Aspect { .. })))
            || matches!(p, Pred::Cmp { l, r, .. }
                if [l, r].iter().any(|v| matches!(v, ValExpr::CountQuery { query }
                    if query.iter().flatten().any(|t| matches!(t, QueryTerm::Aspect { .. })))))
    })
}

// The v9 sum scopes (§6.3): a comparison whose operand is
// `total(bound, aspect:k)` (the window's per-binding sum) or
// `total(table, aspect:k)` (the world sum) — the shapes the analyzer's
// window-model and world-sum intervals decide.
fn scenario_has_sum_scope_when(seed: u32) -> bool {
    scenario_pred_has(seed, &|p| {
        matches!(p, Pred::Cmp { l, r, .. }
        if [l, r].iter().any(|v| matches!(
            v,
            ValExpr::TotalBound { .. } | ValExpr::TotalTable { .. }
        )))
    })
}

/// The v4 closed face: a count-form comparison whose left operand is an
/// aspect-scoped pool count and whose op is closed (`<` / `<=`) — the
/// shape the floor-violation rule decides, invisible to the v3 table.
fn scenario_has_closed_aspect_count_form(seed: u32) -> bool {
    scenario_pred_has(seed, &|p| {
        matches!(p, Pred::Cmp { l: ValExpr::CountQuery { query }, op, r: ValExpr::Int { .. } }
            if matches!(op, CmpOp::Lt | CmpOp::Le)
                && query.iter().flatten().any(|t| matches!(t, QueryTerm::Aspect { .. })))
    })
}

fn scenario_pred_has(seed: u32, f: &dyn Fn(&Pred) -> bool) -> bool {
    use chorus::core::dict::core_dictionary;
    fn pred_has(p: &Pred, f: &dyn Fn(&Pred) -> bool) -> bool {
        match p {
            Pred::And(a, b) | Pred::Or(a, b) => pred_has(a, f) || pred_has(b, f),
            Pred::Not(a) => pred_has(a, f),
            Pred::Cmp { .. } | Pred::ExistsQuery { .. } => f(p),
            _ => false,
        }
    }
    let sc = gen_scenario(seed);
    let mut dict = core_dictionary();
    for v in sc.raw.verbs.keys() {
        let _ = dict.register_verb(v);
    }
    let content =
        compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("the generated scenario compiles");
    content
        .verbs
        .values()
        .any(|v| v.recipes.iter().any(|r| r.when.as_ref().is_some_and(|w| pred_has(w, f))))
}

#[test]
fn seed_9_emits_the_world_form_and_the_closed_aspect_count_form_and_stays_deterministic() {
    assert!(scenario_has_aspect_world_form(9));
    assert!(scenario_has_closed_aspect_count_form(9));
    assert_seed_deterministic(9);
}

#[test]
fn seed_22_emits_an_aspect_scoped_world_form_when_and_stays_deterministic() {
    assert!(scenario_has_aspect_world_form(22));
    assert_seed_deterministic(22);
}

// The v5 recipe-body chain table: the GLOBAL build prefix, so a recipe-body
// RUN may target another verb's recipe (§6.7 cross-verb chains). The probe
// pins a seed whose scenario verifiably carries one — and that the loaded
// scenario explores and stays differential-clean on it.
fn scenario_recipe_run_crosses_verbs(seed: u32) -> bool {
    let sc = gen_scenario(seed);
    let mut dict = core_dictionary();
    for v in sc.raw.verbs.keys() {
        let _ = dict.register_verb(v);
    }
    let content =
        compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("the generated scenario compiles");
    content.verbs.values().any(|v| {
        v.recipes.iter().any(|r| {
            r.out.iter().any(|o| {
                o.op == "RUN"
                    && o.args.iter().any(|a| match a {
                        Arg::Ref { r#ref: Ref::Name { name, .. } } => {
                            content.recipe_verbs.get(name).is_some_and(|owner| owner != &v.name)
                        }
                        _ => false,
                    })
            })
        })
    })
}

// The v10 ambient draw (§6.9): a verb drawn ambient declares no slots, a
// nonzero duration, and recipes whose REFUSE band and `dur` band stayed
// off. The probe pins a seed whose scenario verifiably carries one.
fn scenario_has_ambient_verb(seed: u32) -> bool {
    gen_scenario(seed).raw.verbs.values().any(|v| v.ambient)
}

#[test]
fn seed_2_carries_an_ambient_verb_and_stays_deterministic() {
    assert!(scenario_has_ambient_verb(2));
    assert_seed_deterministic(2);
}

#[test]
fn seed_24_carries_a_cross_verb_recipe_body_run_and_a_sum_scope_when_and_stays_deterministic() {
    // the v9 sum-scope probe rides this seed too: a `when` whose comparison
    // names `total(bound, aspect:a)` or `total(table, aspect:b)` — the
    // analyzer's window-model and world-sum interval faces, exercised by the
    // same differential sweep. (Seed re-pinned under generator v11: the
    // unique draw shifted the draw order.)
    assert!(scenario_has_sum_scope_when(24));
    assert!(scenario_recipe_run_crosses_verbs(24));
    assert_seed_deterministic(24);
}

// The v8 recipe `dur` band: a minority of recipes complete under a window of
// their own (§6.4). The probe pins a seed whose scenario verifiably carries
// one — including a `dur 0m` recipe on a duration>0 verb, the
// effective-duration face the analyzer's bounded-horizon rules must read.
fn scenario_has_recipe_dur(seed: u32) -> bool {
    let sc = gen_scenario(seed);
    let mut dict = core_dictionary();
    for v in sc.raw.verbs.keys() {
        let _ = dict.register_verb(v);
    }
    let content =
        compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("the generated scenario compiles");
    content
        .verbs
        .values()
        .any(|v| v.duration > 0 && v.recipes.iter().any(|r| r.dur.is_some_and(|d| d < v.duration)))
}

#[test]
fn seed_6_carries_a_recipe_dur_override_and_stays_deterministic() {
    // (seed re-pinned under generator v11: the unique draw shifted the draw
    // order)
    assert!(scenario_has_recipe_dur(6));
    assert_seed_deterministic(6);
}

// The v7 event-body chain band: slot/expire bodies draw RUN/FORK onto the
// global duration>0 table (§6.6's event-chain channel). The probe pins a
// seed whose scenario verifiably carries one.
fn scenario_event_body_chains(seed: u32) -> bool {
    use chorus::content::CompiledTrigger;
    let sc = gen_scenario(seed);
    let mut dict = core_dictionary();
    for v in sc.raw.verbs.keys() {
        let _ = dict.register_verb(v);
    }
    let content =
        compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("the generated scenario compiles");
    content.triggers.iter().any(|t| {
        matches!(t, CompiledTrigger::Slot { .. } | CompiledTrigger::Expire { .. })
            && t.out().iter().any(|o| o.op == "RUN" || o.op == "FORK")
    })
}

#[test]
fn seed_20_carries_an_event_body_chain_op_and_stays_deterministic() {
    // (seed re-pinned under generator v11: the unique draw shifted the draw
    // order)
    assert!(scenario_event_body_chains(20));
    assert_seed_deterministic(20);
}

// The v11 unique draw (§4.2): a prototype rolled unique — never stackable —
// riding a deck spec. The probe pins a seed whose scenario verifiably carries
// both: the mint face and the deck filter are the analyzer's unique-rule
// faces, exercised by the same differential sweep.
fn scenario_has_unique_deck_card(seed: u32) -> bool {
    let sc = gen_scenario(seed);
    let mut dict = core_dictionary();
    for v in sc.raw.verbs.keys() {
        let _ = dict.register_verb(v);
    }
    let content =
        compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("the generated scenario compiles");
    content.unique_names.iter().any(|n| content.prototypes.get(n).is_some_and(|p| p.unique))
        && content.decks.values().any(|d| d.order.iter().any(|c| content.mint_is_unique(&c.name)))
}

#[test]
fn seed_8_carries_a_unique_prototype_riding_a_deck_and_stays_deterministic() {
    assert!(scenario_has_unique_deck_card(8));
    assert_seed_deterministic(8);
}
