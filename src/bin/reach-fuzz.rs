//! Deep runs for the reach fuzz (the test suite keeps seeds few for CI; this
//! CLI shakes thousands).
//!
//!   cargo run --bin reach-fuzz -- --seeds 500
//!   cargo run --bin reach-fuzz -- --seeds 1000 --steps 9 --states 800 --from 300
//!
//! Frozen surface: flags default
//! 200/7/400/1; FAIL lines AND the soft inconclusive lines go to stderr;
//! progress + the summary go to stdout; exit code 1 iff there is a failure;
//! failing seeds print as a space-separated list. `run_fuzz_seed` is a pure
//! function of its seed (the shared implementation lives in
//! tests/common/reach_fuzz_lib.rs, mounted here — single source of truth
//! with the test suite).

// The fuzz machinery is the shared TEST lib; bins cannot see tests/, so the
// shared files are mounted by path. `reach_fuzz_lib`'s
// `use super::settle::settle_step` resolves against this crate root, where
// `settle` is mounted first.
#[path = "../../tests/common/settle.rs"]
mod settle;
#[path = "../../tests/common/reach_fuzz_lib.rs"]
mod reach_fuzz_lib;

/// `Number(str)` (ECMA-262) for the flag values — trim DSL whitespace,
/// '' → 0, decimal/hex/±Infinity literals, anything else NaN. (A NaN feeds
/// straight into the loop condition, which then never runs —
/// replicated below by keeping the loop in f64.)
fn parse_canonical_number(s: &str) -> f64 {
    let t: String = {
        let mut chars: Vec<char> = s.chars().collect();
        while chars.first().is_some_and(|c| chorus::core::text::is_dsl_ws_char(*c)) {
            chars.remove(0);
        }
        while chars.last().is_some_and(|c| chorus::core::text::is_dsl_ws_char(*c)) {
            chars.pop();
        }
        chars.into_iter().collect()
    };
    if t.is_empty() {
        return 0.0;
    }
    if t == "Infinity" || t == "+Infinity" {
        return f64::INFINITY;
    }
    if t == "-Infinity" {
        return f64::NEG_INFINITY;
    }
    let hex = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("+0x"))
        .or_else(|| t.strip_prefix("-0x"));
    if let Some(h) = hex {
        let sign = if t.starts_with('-') { -1.0 } else { 1.0 };
        return sign * u64::from_str_radix(h, 16).map(|v| v as f64).unwrap_or(f64::NAN);
    }
    // decimal: optional sign, digits with optional fraction and exponent —
    // anything else is NaN (str::parse would also accept "inf"/"nan", which
    // `Number` rejects)
    let mut body = t.as_str();
    if let Some(rest) = body.strip_prefix(['+', '-']) {
        body = rest;
    }
    let is_decimal = !body.is_empty()
        && body.chars().all(|c| c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-');
    if is_decimal {
        t.parse::<f64>().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

/// The `Number → String` display (ECMA-262) for a flag-derived number — integers print
/// bare, NaN prints
/// "NaN" (the summary line embeds these verbatim).
fn display_number(v: f64) -> String {
    if v.is_nan() {
        "NaN".to_string()
    } else if v.is_infinite() {
        if v > 0.0 { "Infinity".to_string() } else { "-Infinity".to_string() }
    } else if v.fract() == 0.0 && v.abs() < 9.007_199_254_740_992e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn main() -> std::process::ExitCode {
    // the flag readers, defaults 200/7/400/1
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str, dflt: f64| -> f64 {
        args.iter()
            .position(|a| a == name)
            .map(|i| args.get(i + 1).map(|v| parse_canonical_number(v)).unwrap_or(f64::NAN))
            .unwrap_or(dflt)
    };
    let seeds = flag("--seeds", 200.0);
    let steps = flag("--steps", 7.0);
    let states = flag("--states", 400.0);
    let from = flag("--from", 1.0);

    let mut failures = 0.0f64;
    let mut inconclusive = 0.0f64;
    let mut worst = 0.0f64;
    let mut dup_pushes = 0.0f64;
    // the bad-seed list stays in f64 so the join prints fractional
    // `--from`-derived seeds exactly like the canonical number display
    let mut bad_seeds: Vec<f64> = Vec::new();
    let t0 = std::time::Instant::now();
    // the seed loop runs from .. from+seeds in f64: NaN seeds
    // never enter the loop (NaN < NaN is false)
    let mut seed = from;
    while seed < from + seeds {
        // `run_fuzz_seed` — ToInt32(seed) feeds the LCG; realistic
        // seeds are small positive integers, where the cast is exact.
        // REGISTERED DIVERGENCE (pathological argv
        // only): `steps as usize`/`states as usize` TRUNCATE a fractional
        // flag, while untruncated f64 loop bounds stay alive —
        // `--steps 2.5` runs steps 0,1,2
        // (three layers) and the state cap admits
        // 3 states for `--states 2.5`. Integer flags, the defaults and NaN
        // (`parse_canonical_number` = `Number`, registered at its definition) are exact;
        // only a fractional flag digs one level shallower here.
        let out = reach_fuzz_lib::run_fuzz_seed(seed as i32 as u32, steps as usize, states as usize)
            .unwrap_or_else(|e| {
                eprintln!("Error: {e}");
                std::process::exit(1);
            });
        worst = worst.max(out.states_seen as f64);
        dup_pushes += out.deduped as f64;
        for f in &out.failures {
            eprintln!("FAIL {f}"); // console.error
            failures += 1.0;
        }
        if !out.failures.is_empty() {
            bad_seeds.push(seed);
        }
        for i in &out.inconclusive {
            eprintln!("soft {i}"); // console.warn — soft lines ride stderr too
            inconclusive += 1.0;
        }
        // the progress check is a NUMBER equality (`done % 50 == 0`): a
        // fractional
        // seed run never hits it, so the modulo stays in f64 (an integer cast
        // would fire on 50.5)
        let done = seed - from + 1.0;
        if done % 50.0 == 0.0 {
            println!(
                "… {}/{} seeds, {} failures",
                display_number(done),
                display_number(seeds),
                display_number(failures)
            );
        }
        seed += 1.0;
    }
    // the summary + failing-seed lines
    let elapsed = t0.elapsed().as_secs_f64();
    println!(
        "\n{} seeds × ≤{} steps × ≤{} states: {} failure(s), {} inconclusive, busiest world {} states, {} duplicate push(es) deduped, {:.1}s",
        display_number(seeds),
        display_number(steps),
        display_number(states),
        display_number(failures),
        display_number(inconclusive),
        display_number(worst),
        display_number(dup_pushes),
        elapsed
    );
    if !bad_seeds.is_empty() {
        let joined: Vec<String> = bad_seeds.iter().map(|s| display_number(*s)).collect();
        println!("failing seeds: {}", joined.join(" "));
    }
    std::process::ExitCode::from(u8::from(failures > 0.0))
}
