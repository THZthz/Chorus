//! Full-scale census CLI for the reachability analyzer over CS-shaped
//! content (the scaling sibling of reach-fuzz; CI keeps tests/reach_scale.rs
//! small, this shakes the whole 1,566 × 2,084 world).
//!
//!   cargo run --bin reach-scale                      # full CS scale
//!   cargo run --bin reach-scale -- --recipes 400 --cores 4
//!
//! Topology: worker THREADS over a channel, each rebuilding the full world;
//! the A/B interface is preserved
//! exactly: `--worker i k P R` analyzes names with `idx % k == i` of a freshly
//! rebuilt (deterministic) world and dumps the NameStat JSON array
//! (name/ms/verdict/existence/roots/nodes/depth, in this key order) to
//! `<tmpdir>/reach-scale-<i>.json`, then exits 0.
//!
//! Memory discipline (16 GB box): the default worker
//! count is min(available_parallelism, 4), NOT one per CPU — each
//! worker rebuilds the full CS world, and a per-core default would hold that
//! world × every core. `--cores` overrides.

// The scale lib is the shared TEST lib; bins cannot see tests/, so the shared
// file is mounted by path. Single source of truth with the test suite.
#[path = "../../tests/common/reach_scale_lib.rs"]
mod reach_scale_lib;

use std::sync::mpsc;

use reach_scale_lib::{build_cs_scale, census, summarize};

/// Flag numbers parse with `Number` (ECMA-262) semantics — the flags accept plain
/// integers (the CLI's own documented interface).
fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn as_num(v: Option<String>, dflt: f64) -> f64 {
    // `Number()` semantics: parse to f64 or NaN; the default applies
    // only when the flag is absent
    match v {
        Some(s) => s.trim().parse::<f64>().unwrap_or(f64::NAN),
        None => dflt,
    }
}

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // worker mode: analyze names idx % k == i of a freshly rebuilt
    // (deterministic) world, dump per-name stats as JSON, exit
    if let Some(w) = args.iter().position(|a| a == "--worker") {
        let i = as_num(args.get(w + 1).cloned(), f64::NAN);
        let k = as_num(args.get(w + 2).cloned(), f64::NAN);
        let n_protos = as_num(args.get(w + 3).cloned(), f64::NAN) as usize;
        let n_recipes = as_num(args.get(w + 4).cloned(), f64::NAN) as usize;
        let world = build_cs_scale(n_protos, n_recipes);
        // the name filter `idx % k == i` runs on NUMBERS (f64 semantics):
        // k = 0 makes the remainder NaN (never equal), a fractional i never
        // equals an integer remainder; Rust's f64 `%` has exactly those
        // semantics (0.0 % 0.0 = NaN, NaN == i is false), where the usize
        // `%` would panic.
        let mine: Vec<String> = world
            .names
            .iter()
            .enumerate()
            .filter(|(idx, _)| (*idx as f64) % k == i)
            .map(|(_, n)| n.clone())
            .collect();
        let stats = census(&world, &mine);
        // the shard file is `reach-scale-<i>.json` — `<i>` prints like a
        // Number display: bare digits for integers, "NaN" for NaN
        let path = std::env::temp_dir().join(format!("reach-scale-{i}.json"));
        // serializing plain stats cannot fail; rule 11 keeps the
        // panic explicit and the grep gate clean
        let json = serde_json::to_string(&stats)
            .unwrap_or_else(|e| panic!("cannot serialize NameStat: {e}"));
        std::fs::write(&path, json).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
        return std::process::ExitCode::from(0);
    }

    let n_protos = as_num(flag(&args, "--protos"), 1566.0) as usize;
    let n_recipes = as_num(flag(&args, "--recipes"), 2084.0) as usize;
    // the `--cores` default carries the same `Number` semantics too — the default is
    // the CONSERVATIVE cap (see the module header); `--cores` overrides.
    let default_cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .min(4);
    let cores = as_num(flag(&args, "--cores"), default_cores as f64) as usize;

    let t0 = std::time::Instant::now();
    let world = build_cs_scale(n_protos, n_recipes);
    let compile_ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!(
        "content: {} prototypes, {} verbs × 1 recipe (~{} ops), {} decks, {} triggers",
        world.stats.protos, world.stats.recipes, world.stats.ops, world.stats.decks, world.stats.triggers
    );
    println!("build+compile: {:.0} ms", compile_ms);
    println!("census over {} names on {} workers…", world.names.len(), cores);

    // the parent shards across worker threads over a channel; every worker
    // rebuilds the full world, matching the child-process topology's output.
    let wall_start = std::time::Instant::now();
    let (tx, rx) = mpsc::channel::<(usize, Vec<reach_scale_lib::NameStat>)>();
    for i in 0..cores {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let world = build_cs_scale(n_protos, n_recipes);
            let mine: Vec<String> = world
                .names
                .iter()
                .enumerate()
                .filter(|(idx, _)| idx % cores == i)
                .map(|(_, n)| n.clone())
                .collect();
            let stats = census(&world, &mine);
            let _ = tx.send((i, stats));
        });
    }
    drop(tx);
    // shards flatten in SHARD (index) order, so the summary's verdict histogram
    // (insertion-ordered) sees the names in the same sequence
    let mut shards: Vec<Option<Vec<reach_scale_lib::NameStat>>> = (0..cores).map(|_| None).collect();
    for _ in 0..cores {
        match rx.recv() {
            Ok((i, stats)) => shards[i] = Some(stats),
            Err(e) => {
                // a failed worker aborts the whole run
                eprintln!("Error: a worker failed: {e}");
                return std::process::ExitCode::from(1);
            }
        }
    }
    let wall = wall_start.elapsed().as_secs_f64();
    let stats: Vec<reach_scale_lib::NameStat> = shards
        .into_iter()
        .flatten()
        .flatten()
        .collect();
    println!("\ncensus wall: {:.1} s on {} cores", wall, cores);
    println!("{}", summarize(&stats));
    std::process::ExitCode::from(0)
}
