//! Shared test adapters — the ONE home of the helpers every binary's tests
//! reach through `crate::crate::common::rig`:
//!
//! - `settle_line` — parse a canonical sentence + settle it through
//!   `chorus::engine::settle::settle_step` with the load-bearing
//!   `x{first_record_id}_{n}` id-generator closure (the REPLACE-sentence
//!   assertions regex-match the minted `#x\d+_1` ids, so the id sequence is
//!   part of the pinned surface — ONE copy here, never re-spelled per file);
//! - `compile` — compile a script text fresh (compile_script is pure, so the
//!   result IS the pristine pre-play state);
//! - `tmp_log_path` — the per-process temp log file;
//! - the record-projection helpers (`kinds` / `ops` / `said` / …) every
//!   outcome-level assertion reads through;
//! - the one-verb INSPECT rig shared by tests/dice/dice.rs and
//!   tests/script/conditionals.rs;
//! - `auto_settle_line` — the session Game adapter that completes a committed
//!   action's open windows before the next input.

// each integration test crate compiles this module separately and not every
// helper is used by every binary — the helpers are the shared discipline,
// not per-file dead code (the allow lives in this one place)
#![allow(dead_code)]

use chorus::content::script::compile_script;
use chorus::content::CompiledContent;
use chorus::core::clock::Clock;
use chorus::core::dict::Dictionary;
use chorus::core::parse::parse_sentence;
use chorus::core::sentence::{Arg, Outcome};
use chorus::engine::settle::settle_step;
use chorus::engine::{ActionRecord, ExecResult, Step};
use chorus::error::Result;
use chorus::session::game::{Game, Turn, TurnKind};

/// parse + settle one canonical line: the shared shape of every engine test's
/// `run` adapter. The id-generator closure `x{first_record_id}_{n}` (counter
/// incrementing once per minted id) is LOAD-BEARING — the REPLACE-sentence
/// assertions regex-match the minted `#x\d+_1` ids — so it lives here exactly
/// once. The step clock defaults to day 0 / tick 0 when the sentence
/// omits one.
pub fn settle_line(
    content: &CompiledContent,
    dict: &Dictionary,
    state: chorus::core::types::State,
    src: &str,
    first_record_id: i64,
) -> Result<ExecResult> {
    let s = parse_sentence(src, dict).unwrap();
    let mut n: i64 = 0;
    let mut id_gen = || {
        n += 1;
        format!("x{first_record_id}_{n}")
    };
    settle_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, tick: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        first_record_id,
        Some(&mut id_gen),
    )
}

/// compile a script text fresh: `(content, base)`. `compile_script` is pure,
/// so a fresh compile IS the pristine pre-play state.
pub fn compile(dict: &Dictionary, text: &str) -> (CompiledContent, chorus::core::types::State) {
    let compiled = compile_script(text, dict).unwrap();
    (compiled.content, compiled.state)
}

/// Temp log path, unique per process and tag:
/// `<tmp>/chorus-test-<tag>-<pid>/log.jsonl`.
pub fn tmp_log_path(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("chorus-test-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("log.jsonl")
}

// ------------------------ record projections -------------------------------
// The one-line lenses outcome-level assertions read through. Each shape is
// named for what it takes: `*_record` a single log row, `*_of(r, i)` record i
// of a settled turn, plain names the whole-turn aggregate.

/// The kind spine of a settled turn, in record order.
pub fn kind_spine(r: &ExecResult) -> Vec<&str> {
    r.records.iter().map(|rec| rec.kind.as_str()).collect()
}

/// The kind list of a record slice (the raw `records` vectors).
pub fn kinds(records: &[ActionRecord]) -> Vec<&str> {
    records.iter().map(|x| x.kind.as_str()).collect()
}

/// The op list of ONE record's outcomes.
pub fn ops(rec: &ActionRecord) -> Vec<&str> {
    rec.outcomes.iter().map(|o| o.op.as_str()).collect()
}

/// The op list of an outcomes slice (IF arms, fallbacks, one COMPLETE row).
pub fn ops_outcomes(outcomes: &[Outcome]) -> Vec<&str> {
    outcomes.iter().map(|o| o.op.as_str()).collect()
}

/// The op list of record `i` of a settled turn.
pub fn ops_of(r: &ExecResult, i: usize) -> Vec<&str> {
    r.records[i].outcomes.iter().map(|o| o.op.as_str()).collect()
}

/// The NARRATE-string arguments of an outcomes slice, in order
/// ("(not a string)" for a non-string first arg).
pub fn narrated(outcomes: &[Outcome]) -> Vec<String> {
    outcomes
        .iter()
        .filter(|o| o.op == "NARRATE")
        .map(|o| match o.args.first() {
            Some(Arg::Str { v }) => v.clone(),
            _ => "(not a string)".to_string(),
        })
        .collect()
}

/// The NARRATE strings of record `i` of a settled turn.
pub fn narrated_of(r: &ExecResult, i: usize) -> Vec<String> {
    narrated(&r.records[i].outcomes)
}

/// The NARRATE strings of ONE record.
pub fn said_record(rec: &ActionRecord) -> Vec<String> {
    narrated(&rec.outcomes)
}

/// The NARRATE strings of a settled turn — v0.5: NARRATE defers to the
/// completion, so gather across the COMPLETE rows.
pub fn said(r: &ExecResult) -> Vec<String> {
    r.records.iter().filter(|x| x.kind == "complete").flat_map(|x| narrated(&x.outcomes)).collect()
}

/// The BEGIN rows' recipe ids of a settled turn (one entry per action row).
pub fn begin_recipes(r: &ExecResult) -> Vec<Vec<String>> {
    r.records.iter().filter(|rec| rec.kind == "action").map(|rec| rec.recipes.clone()).collect()
}

/// The settled turn's BEGIN row's recipe ids (the `act(r)` spine row).
pub fn begin_recipe(r: &ExecResult) -> Vec<String> {
    chorus::engine::settle::act(r).map(|rec| rec.recipes.clone()).unwrap_or_default()
}

/// The execution map as entry pairs (insertion order observable).
pub fn executed_entries(s: &chorus::core::types::State) -> Vec<(String, i64)> {
    s.executed.iter().map(|(k, v)| (k.clone(), *v)).collect()
}

// ------------------------ the one-verb INSPECT rig --------------------------

/// the extra `ContentRaw` sections the INSPECT rig's spread accepts.
#[derive(Default)]
pub struct Extra {
    pub triggers: Option<Vec<chorus::content::TriggerDef>>,
    pub prototypes: Option<indexmap::IndexMap<String, chorus::content::PrototypeDef>>,
    pub functions: Option<Vec<chorus::content::FunctionDef>>,
    pub endings: Option<indexmap::IndexMap<String, chorus::content::EndingDef>>,
}

/// Throwing through — the load-error assertions feed this Result straight to
/// `assert_err_msg`. A one-verb rig: INSPECT takes actors 1..2 and an
/// optional object.
pub fn try_inspect_content(
    dict: &Dictionary,
    out: &[&str],
    extra: Extra,
) -> Result<CompiledContent> {
    chorus::content::compile_content(
        &chorus::content::ContentRaw {
            verbs: [(
                "INSPECT".to_string(),
                chorus::content::VerbContent {
                    slots: vec![
                        chorus::content::SlotDef {
                            role: "actors".into(),
                            min: 1,
                            max: Some(2),
                            query: Some("kind:character".into()),
                            shown: None,
                        },
                        chorus::content::SlotDef {
                            role: "object".into(),
                            min: 0,
                            max: Some(1),
                            query: None,
                            shown: None,
                        },
                    ],
                    duration: 0,
                    recipes: vec![chorus::content::RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        out: out.iter().map(|s| (*s).to_string()).collect(),
                    }],
                },
            )]
            .into_iter()
            .collect(),
            prototypes: extra.prototypes,
            triggers: extra.triggers,
            endings: extra.endings,
            functions: extra.functions,
            ..chorus::content::ContentRaw::default()
        },
        dict,
        &[],
    )
}

/// the unwrapping face used at play call sites.
pub fn inspect_content(dict: &Dictionary, out: &[&str], extra: Extra) -> CompiledContent {
    try_inspect_content(dict, out, extra).unwrap()
}

// ------------------------ the session adapter -------------------------------

/// The v0.5 session test adapter: settle each committed action's window right
/// away, so Game-loop tests keep their pre-parallel commit-completes-at-once
/// shape (the nightjar story suite and the session suites share it).
pub fn auto_settle_line(game: &mut Game, line: &str) -> Result<Turn> {
    let t = game.handle_line(line)?;
    let t = if t.kind == TurnKind::Action && !game.state.windows.is_empty() {
        let settle = game.wait()?;
        Turn { kind: TurnKind::Action, lines: [t.lines, settle.lines].concat() }
    } else {
        t
    };
    Ok(t)
}
