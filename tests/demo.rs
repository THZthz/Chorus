//! Port of examples/demo.ts — the end-to-end fold-verify smoke (plan §4-W6
//! T6.2, UI-13: "state = fold(log)" is the replay law users can watch). The
//! TS example prints and exits; the port's console output becomes assertions
//! against the W4 goldens (tools/golden-dump.ts demoFamily — copied verbatim
//! from examples/demo.ts, custom `x{id}_{++n}` idGen included), so the demo
//! byte surface is cross-language frozen twice over.

mod common;

use common::seed::{seed_content, seed_state};

use chorus::content::content::compile_content;
use chorus::core::clock::clock_from_abs_min;
use chorus::core::dict::core_dictionary;
use chorus::core::parse::parse_sentence;
use chorus::engine::engine::{execute_step, ExecOptions, Step};
use chorus::engine::fold::fold;
use chorus::engine::store::save_log;
use chorus::session::render::{journal, prose};

const STEPS: [&str; 6] = [
    "0:28 @ ANA AT HOUSE READ THE_FORGETTING", // 14:00
    "0:30 @ ANA, BERTHA READ FAIRY_TALES",     // 15:00 — ends 16:00 → dusk_falls fires
    "0:32 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT",
    "0:34 @ BERTHA READ MAGICK_BOOK", // refused — nobody who knows is present
    "0:34 @ ANA TRAVEL_TO THE_STUDY; ANA AT THE_STUDY READ MAGICK_BOOK",
    "0:40 @ ANA WAIT",
];

/// the whole of examples/demo.ts, as one test: play the six canonical steps
/// through raw executeStep (NOT Game — the example predates the session
/// layer), print journal + prose, then fold the log over the seed state and
/// verify the replay.
#[test]
fn demo_plays_the_six_canonical_steps_and_fold_verifies() -> chorus::js::Result<()> {
    let dict = core_dictionary();
    let content = compile_content(&seed_content(), &dict, &[])?;

    let mut records = Vec::new();
    let mut state = seed_state();
    let mut id: i64 = 1;
    let mut n: usize = 0;
    for src in STEPS {
        let s = chorus::core::parse::parse_sentence(src, &dict)?;
        // ts: demo.ts:33 — `idGen: () => `x${id}_${++n}``: id is read at CALL
        // time (constant within one step), n is the closure's own counter
        // shared across every step
        let id_now = id;
        let mut id_gen = || {
            n += 1;
            format!("x{id_now}_{n}")
        };
        let r = execute_step(
            &content,
            &dict,
            state,
            Step {
                start_min: None,
                clock: Some(s.clock.expect("demo sentences carry clocks")),
                clauses: s.clauses,
                t_real: None,
            },
            ExecOptions {
                first_record_id: id,
                id_gen: Some(&mut id_gen),
            },
        )?;
        if let Some(rejected) = r.rejected {
            panic!("demo step rejected: {rejected}");
        }
        state = r.state;
        id += r.records.len() as i64;
        records.extend(r.records);
    }

    // ts: demo.ts:41-42 — the canonical journal, byte-identical to the W4
    // golden (the `x…` mint spellings included)
    let journal_text = journal(&records);
    assert_eq!(
        journal_text + "\n",
        include_str!("goldens/demo.journal.txt"),
        "demo journal diverged from the TS golden"
    );

    // ts: demo.ts:44-47 — the same log as prose, byte-identical to the W4
    // golden (prose never shows ids: names and narrations only)
    let mut prose_text = String::new();
    for rec in &records {
        prose_text.push_str("· ");
        prose_text.push_str(&prose(rec, &content, &dict)?);
        prose_text.push('\n');
    }
    assert_eq!(
        prose_text, include_str!("goldens/demo.prose.txt"),
        "demo prose diverged from the TS golden"
    );

    // ts: demo.ts:49-53 — state = fold(log): the replay law (UI-13). The
    // example prints match/MISMATCH and the folded clock; the port asserts.
    let replayed = fold(&records, &seed_state(), &dict, &content)?;
    assert_eq!(
        replayed.entities.len(),
        state.entities.len(),
        "fold(log) must repopulate exactly the played entity set"
    );
    let clock = clock_from_abs_min(replayed.now);
    assert_eq!((clock.day, clock.halfhour), (0, 40), "the folded clock is the last step's 0:40");

    // the JSONL bytes of this very log are the demo.jsonl golden (law 1) —
    // written to a scratch file and compared byte for byte
    let dir = std::env::temp_dir().join(format!("chorus-demo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let log_path = dir.join("demo.jsonl");
    save_log(&log_path, &records)?;
    let bytes = std::fs::read_to_string(&log_path).expect("scratch log");
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(
        bytes,
        include_str!("goldens/demo.jsonl"),
        "demo JSONL diverged from the TS golden"
    );

    Ok(())
}

/// the demo's determinism face (dice.test.ts:917-933's law-1 check at demo
/// scale): the same six steps twice produce byte-identical logs, and the
/// `x…` mint spelling is the demo.ts idGen's — a different idGen would print
/// differently but fold identically.
#[test]
fn demo_two_runs_are_byte_identical_and_the_x_mint_spelling_is_the_demo_idgen() -> chorus::js::Result<()> {
    let dict = core_dictionary();
    let content = compile_content(&seed_content(), &dict, &[])?;
    let play = || -> chorus::js::Result<Vec<String>> {
        let mut sentences = Vec::new();
        let mut state = seed_state();
        let mut id: i64 = 1;
        let mut n: usize = 0;
        for src in STEPS {
            let s = parse_sentence(src, &dict)?;
            let id_now = id;
            let mut id_gen = || {
                n += 1;
                format!("x{id_now}_{n}")
            };
            let r = execute_step(
                &content,
                &dict,
                state,
                Step {
                    start_min: None,
                    clock: Some(s.clock.expect("demo sentences carry clocks")),
                    clauses: s.clauses,
                    t_real: None,
                },
                ExecOptions { first_record_id: id, id_gen: Some(&mut id_gen) },
            )?;
            state = r.state;
            id += r.records.len() as i64;
            sentences.extend(r.records.iter().map(|rec| rec.sentence.clone()));
        }
        Ok(sentences)
    };
    let a = play()?;
    let b = play()?;
    assert_eq!(a, b);
    // the demo.ts idGen's fingerprint (demo.ts:33) — golden cross-check
    assert!(
        a.iter().any(|s| s.contains("AS #x3_1")),
        "the CLUE mint carries the demo.ts idGen spelling"
    );
    Ok(())
}
