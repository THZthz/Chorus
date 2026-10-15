//! Scaling test for the reachability analyzer over Cultist-Simulator-shaped
//! synthetic content — ts: test/reach-scale.test.ts, ported test-for-test at
//! the CI size (the full 1,566×2,084 world is the reach-scale bin's job).
//! The world facts (census + fixpoint) are cached per (content, initial), so:
//!   - a warm analyze_card call amortizes the cold first call
//!   - cached and independently-rebuilt worlds produce IDENTICAL reports
//!   - the path-export invariants hold at scale
//!   - a full-name census completes and stays sound
//!
//! PLUS the §4-W5 acceptance (PR1-03): the same-parameter world built on the
//! TS side (goldens/reach-scale-census.stdout.txt, dumped by
//! tools/golden-dump.ts) must match field-for-field on the NON-timing NameStat
//! columns (name/verdict/existence/roots/nodes/depth); `ms` is timing and
//! only a magnitude reference, so it ships in neither side's comparison.

mod common;

use common::reach_scale_lib::{build_cs_scale, census, summarize, NameStat};

use chorus::content::content::compile_content;
use chorus::core::dict::core_dictionary;
use chorus::reach::{analyze_card, render_report, Analyzer, CardOptions, CardVerdict};

use common::reach_fuzz_lib;

// CI-sized slice of CS scale (the full world is `cargo run --bin reach-scale`)
const PROTOS: usize = 400;
const RECIPES: usize = 400;

/// ts: reach-scale.test.ts:23-29
#[test]
fn the_synthesized_world_has_the_expected_shape() {
    let world = build_cs_scale(PROTOS, RECIPES);
    assert_eq!(world.stats.protos, PROTOS);
    assert_eq!(world.stats.recipes, RECIPES);
    assert_eq!(world.content.verbs.len(), RECIPES + 1); // + WAIT
    assert_eq!(world.stats.decks, 67);
    assert!(world.names.len() > PROTOS);
}

/// ts: reach-scale.test.ts:31-45 — a fresh world pair pays the census+fixpoint
/// exactly once; every later call is a target lookup + path walk. ONE
/// Analyzer across cold + warm mirrors the TS module-level worldCache (the
/// one-shot `analyze_card` would re-pay the facts on every call). (The TS
/// asserts `cold > 15ms` / `warm < 50ms` as CI guardrails; the cold floor is
/// kept, the warm ceiling is scaled to the unoptimized Rust build — PR1-03
/// keeps ms a magnitude reference only, and the STRUCTURAL pin is
/// `warm × 3 < cold`.)
#[test]
fn cold_vs_warm_the_second_analysis_on_the_same_world_amortizes_the_fixpoint() {
    let fresh = build_cs_scale(PROTOS, RECIPES);
    let probe = fresh.names[fresh.names.len() >> 1].clone();
    let analyzer = Analyzer::new(&fresh.content, &fresh.initial);
    let cold_start = std::time::Instant::now();
    analyzer
        .analyze_card(&probe, CardOptions::default())
        .expect("the probe analyzes");
    let cold = cold_start.elapsed().as_secs_f64() * 1000.0;
    let warm_start = std::time::Instant::now();
    for i in 0..20 {
        analyzer
            .analyze_card(&fresh.names[i % 100], CardOptions::default())
            .expect("warm probes analyze");
    }
    let warm = warm_start.elapsed().as_secs_f64() * 1000.0 / 20.0;
    assert!(cold > 15.0, "the cold call must actually pay for the world facts (got {cold}ms)");
    assert!(
        warm * 3.0 < cold,
        "warm ({warm:.2}ms) must amortize the cold fixpoint ({cold:.1}ms)"
    );
    // ts: reach-scale.test.ts:44 — `expect(warm).toBeLessThan(50)` is a V8/JIT
    // CI guardrail; the unoptimized debug build (plan §2.4) runs the same
    // lookup + path walk an order of magnitude slower (~100ms measured), so
    // the absolute ceiling rides the PR1-03 magnitude-reference rule with
    // ×10 profile headroom. The STRUCTURAL amortization pin stays above.
    assert!(
        warm < 500.0,
        "a warm call is a lookup + path walk, not a re-analysis (got {warm}ms)"
    );
}

/// ts: reach-scale.test.ts:47-56 — two independently built worlds produce
/// byte-identical reports (cache purity). The TS compares JSON.stringify of
/// the whole report; the rendered CLI text is the stricter byte surface here
/// (same traversal, printable).
#[test]
fn two_independently_built_worlds_produce_byte_identical_reports_cache_purity() {
    let a = build_cs_scale(PROTOS, RECIPES);
    let b = build_cs_scale(PROTOS, RECIPES);
    // One Analyzer per world across the whole sample — the TS calls hit the
    // module worldCache per (content, initial), so each world's facts are
    // built once, not once per name.
    let analyzer_a = Analyzer::new(&a.content, &a.initial);
    let analyzer_b = Analyzer::new(&b.content, &b.initial);
    let mut sample: Vec<String> =
        a.names.iter().enumerate().filter(|(i, _)| i % 25 == 0).map(|(_, n)| n.clone()).collect();
    sample.extend(a.names[a.names.len() - 5..].iter().cloned());
    let mut checked = 0usize;
    for name in &sample {
        let mut ra = String::new();
        let mut rb = String::new();
        let rep_a = analyzer_a
            .analyze_card(name, CardOptions::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let rep_b = analyzer_b
            .analyze_card(name, CardOptions::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        render_report(&mut ra, "scenario", name, None, false, &rep_a);
        render_report(&mut rb, "scenario", name, None, false, &rep_b);
        assert_eq!(ra, rb, "{name}");
        checked += 1;
    }
    assert!(checked >= 20, "the sample must be non-trivial");
}

/// ts: reach-scale.test.ts:58-70 — the full-name census sweep, PLUS the
/// §4-W5 acceptance: the TS-dumped census (same 400×400 world) matches
/// field-for-field on every non-timing NameStat column.
#[test]
fn census_full_name_sweep_stays_sound_at_scale() {
    let world = build_cs_scale(PROTOS, RECIPES);
    let stats = census(&world, &world.names);
    let line = summarize(&stats);
    assert!(line.contains(&format!("{} names", stats.len())), "{line}");
    let by_verdict = |v: &str| stats.iter().filter(|st| st.verdict == v).count();
    // CS-shaped content with clean chains: most cards are judgeable, and the
    // analyzer never leaves the soundness rails
    assert!(
        by_verdict("obtainable") as f64 > stats.len() as f64 * 0.3,
        "obtainable share too low: {}",
        line
    );
    for st in &stats {
        if st.verdict == "obtainable" {
            assert!(st.roots > 0, "{}: obtainable ⇒ a proven path", st.name);
        }
        if st.verdict == "unobtainable" {
            assert_eq!(st.roots, 0, "{}: unobtainable ⇒ no path", st.name);
        }
    }
    // PR1-03 cross-language A/B: the TS census (tools/golden-dump.ts, same
    // parameters) must agree row for row, column for column (no ms)
    let ts = include_str!("goldens/reach-scale-census.stdout.txt");
    let mut ts_rows: Vec<Vec<&str>> = ts
        .split('\n')
        .filter(|l| !l.is_empty())
        .map(|l| l.split('\t').collect())
        .collect();
    let mut rust_rows: Vec<Vec<String>> = stats
        .iter()
        .map(|st: &NameStat| {
            vec![
                st.name.clone(),
                st.verdict.clone(),
                st.existence.clone(),
                st.roots.to_string(),
                st.nodes.to_string(),
                st.depth.to_string(),
            ]
        })
        .collect();
    assert_eq!(
        ts_rows.len(),
        rust_rows.len(),
        "the TS and Rust censuses cover the same names"
    );
    ts_rows.sort_by(|a, b| chorus::js::cmp_utf16(a[0], b[0]));
    rust_rows.sort_by(|a, b| chorus::js::cmp_utf16(&a[0], &b[0]));
    for (ts_row, rust_row) in ts_rows.iter().zip(&rust_rows) {
        assert_eq!(ts_row, &rust_row.as_slice(), "row {} diverges", rust_row[0]);
    }
}

/// ts: reach-scale.test.ts:72-82 — path-export invariants over a broad sample
/// (fuzz-style, at scale): obtainable/unusable ⇒ paths ≥ 1, roots mint the
/// probed name. One shared Analyzer mirrors the TS module-cache shape.
#[test]
fn path_export_invariants_over_a_broad_sample_fuzz_style_at_scale() {
    let world = build_cs_scale(PROTOS, RECIPES);
    let analyzer = Analyzer::new(&world.content, &world.initial);
    let mut checked = 0usize;
    for name in world.names.iter().enumerate().filter(|(i, _)| i % 7 == 0).map(|(_, n)| n) {
        let rep = analyzer
            .analyze_card(name, CardOptions::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        if rep.verdict != CardVerdict::Obtainable && rep.verdict != CardVerdict::Unusable {
            continue;
        }
        assert!(!rep.paths.is_empty(), "{name}");
        for root in &rep.paths {
            assert_eq!(root.mints.as_str(), name.as_str(), "{name}");
        }
        checked += 1;
    }
    assert!(checked > 30, "the sample must exercise obtainable names");
}

/// ts: reach-scale.test.ts:84-88 — demand semantics survive the scale (any
/// obtainable card matches).
#[test]
fn demand_semantics_survive_the_scale_any_obtainable_card_matches() {
    let world = build_cs_scale(PROTOS, RECIPES);
    let rep = analyze_card(
        &world.content,
        &world.initial,
        "",
        CardOptions {
            usable: false,
            demand: Some("aspect:lore>=2".to_string()),
        },
    )
    .expect("the demand query parses");
    assert_eq!(rep.verdict, CardVerdict::Obtainable);
    assert!(!rep.paths.is_empty());
}

/// the genScenario worlds' demand probes also hold at the fuzz seed the TS
/// file pins nowhere explicitly — keep one seed as a cross-lib smoke so both
/// libs stay mounted in one binary (guards the #[path] / mod wiring).
#[test]
fn the_scenario_lib_and_scale_lib_share_the_world_wiring() {
    let sc = reach_fuzz_lib::gen_scenario(1);
    let dict = core_dictionary();
    let content = compile_content(&sc.raw, &dict, &sc.entity_kinds).expect("scenario compiles");
    let rep = analyze_card(&content, &sc.initial, "GHOST", CardOptions::default())
        .expect("probe analyzes");
    assert_eq!(rep.verdict, CardVerdict::Unobtainable);
}
