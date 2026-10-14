//! grep gate, Rust form (TEST-12, plan §4-W0 / §5.4): `rand::` must not
//! appear anywhere under src/core or src/engine — the counter RNG in
//! core/dice.rs is the ONLY randomness (law 5). The TS originals
//! (test/dice.test.ts:935-943, test/decks.test.ts:831-838) grep src/ for
//! `Math.random`; this is the gate skeleton — later waves extend it
//! (engine-path unwrap/expect, float in the dice path — the vector table
//! itself is that probe).

use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // a module dir that has not landed yet (engine/ is W3)
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().is_some_and(|ext| ext == "rs") {
            out.push(p);
        }
    }
}

#[test]
fn no_rand_crate_anywhere_in_core_or_engine_sources() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    for d in ["core", "engine"] {
        collect_rs_files(&src.join(d), &mut files);
    }
    let offenders: Vec<String> = files
        .iter()
        .filter_map(|f| {
            fs::read_to_string(f)
                .map(|text| text.contains("rand::"))
                .unwrap_or(false)
                .then(|| f.display().to_string())
        })
        .collect();
    assert_eq!(
        offenders,
        Vec::<String>::new(),
        "rand:: must not appear in core/engine sources: {offenders:?}"
    );
}
