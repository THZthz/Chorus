//! Append-only log + JSONL persistence — ts: src/engine/store.ts (spec §7).
//! The log is the source of truth; entity/relation tables are projections
//! built by fold.
//!
//! Byte identity (law 1): [`save_log`] writes one
//! `serde_json::to_string(record)` per row — the serde field order on
//! [`ActionRecord`] and its payload types is pinned to the TS object-literal
//! order, and serde_json's string escaping matches `JSON.stringify` exactly
//! (`\"` `\\`, `\b\t\n\f\r`, other <0x20 as lowercase `\u00xx`, DEL and
//! non-ASCII raw — Node-verified in js.rs), so the bytes are TS-identical.
//!
//! Documented divergences (ENG-20, plan §2.2 — both sanctioned, neither
//! test-pinned):
//! - the write is atomic (tmp + rename) where TS `writeFileSync` truncates
//!   in place; the crash-window behavior differs, the game-visible bytes do
//!   not;
//! - [`load_log`] errors carry the file name and 1-based line number where
//!   TS surfaced the bare `JSON.parse` message.

use std::fs;
use std::path::Path;

use crate::core::types::State;
use crate::engine::engine::ActionRecord;
use crate::js::{ChorusError, Result};

/// ts: src/engine/store.ts:7-22 (`Log`). Append-only: the invariant is
/// maintained by the CALLER (play.ts:164 truncates `records` directly for
/// undo — the Rust mirror is [`Log::truncate`], ENG-02).
pub struct Log {
    pub records: Vec<ActionRecord>,
    /// ts: store.ts:9 — the constructor stores the base state. Production
    /// code never reads it (ENG-02 verified: the only readers are the
    /// decks/dice test fixtures, 2 files / 4 call sites). Kept public as a
    /// TEST FIXTURE, not engine state; do not build gameplay on it.
    pub initial_state: State,
}

impl Log {
    pub fn new(initial_state: State) -> Log {
        Log {
            records: Vec::new(),
            initial_state,
        }
    }

    /// ts: store.ts:11-13
    #[must_use]
    pub fn next_record_id(&self) -> i64 {
        self.records.len() as i64 + 1
    }

    /// ts: store.ts:15-17
    pub fn append_all(&mut self, records: Vec<ActionRecord>) {
        self.records.extend(records);
    }

    /// The undo truncation point — ts: play.ts:164
    /// `this.log.records.length = length`. (`Log.journal()` is NOT ported:
    /// dead code per plan §1.2 / UI-05.)
    pub fn truncate(&mut self, len: usize) {
        self.records.truncate(len);
    }
}

/// ts: src/engine/store.ts:24-27 — one `JSON.stringify(record)` per line,
/// `\n`-joined with a trailing newline.
pub fn save_log(path: &Path, records: &[ActionRecord]) -> Result<()> {
    let mut body = String::new();
    for r in records {
        let line = serde_json::to_string(r)
            .map_err(|e| ChorusError::new(format!("record {r:?} is not serializable: {e}")))?;
        body.push_str(&line);
        body.push('\n');
    }
    // atomic write (documented divergence, see module doc): write a sibling
    // tmp file, then rename over the target — a reader never sees a partial
    // log. Single-threaded game, so a fixed suffix cannot collide.
    let tmp = path.with_extension("jsonl.tmp");
    fs::write(&tmp, body.as_bytes())
        .map_err(|e| ChorusError::new(format!("cannot write {}: {e}", tmp.display())))?;
    fs::rename(&tmp, path)
        .map_err(|e| ChorusError::new(format!("cannot write {}: {e}", path.display())))?;
    Ok(())
}

/// ts: src/engine/store.ts:29-32 — `readFileSync(path, 'utf8')
/// .split('\n').filter(Boolean).map(JSON.parse)`. ONLY empty lines are
/// dropped (a whitespace-only line is truthy in JS and would be fed to
/// JSON.parse, which then throws — the Rust error carries the line number,
/// the documented divergence).
pub fn load_log(path: &Path) -> Result<Vec<ActionRecord>> {
    let text = fs::read_to_string(path)
        .map_err(|e| ChorusError::new(format!("cannot read {}: {e}", path.display())))?;
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        if line.is_empty() {
            continue; // filter(Boolean)
        }
        let record: ActionRecord = serde_json::from_str(line).map_err(|e| {
            ChorusError::new(format!("{}:{}: {}", path.display(), i + 1, e))
        })?;
        out.push(record);
    }
    Ok(out)
}

// --------------------------------------------------------------------------
// W3 tests: the JSONL byte-identity discipline (law 1 / ENG-20). The full
// dice.test.ts:917-933 port lands in T3.7; these pin the store contract.
// --------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::content::{compile_content, ContentRaw, RecipeDef, SlotDef, VerbContent};
    use crate::core::clock::Clock;
    use crate::core::dict::{core_dictionary, Dictionary};
    use crate::core::parse::parse_sentence;
    use crate::core::types::new_state;
    use crate::engine::engine::{execute_step, ExecOptions, Step};

    fn pace_rig() -> (Dictionary, crate::content::content::CompiledContent) {
        let mut dict = core_dictionary();
        if !dict.is_verb("PACE") {
            dict.register_verb("PACE").unwrap();
        }
        let raw = ContentRaw {
            verbs: [(
                "PACE".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    }],
                    duration: 30,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: None,
                        shown: None,
                        hint: None,
                        out: vec!["NARRATE \"d.\"".to_string()],
                    }],
                },
            )]
            .into_iter()
            .collect(),
            ..ContentRaw::default()
        };
        let content = compile_content(&raw, &dict, &[]).unwrap();
        (dict, content)
    }

    fn seed() -> State {
        let mut s = new_state(0);
        let mut ana = crate::core::types::Entity {
            id: "ana".into(),
            kind: "character".into(),
            name: "ANA".into(),
            aspects: crate::core::types::Aspects::new(),
            attrs: crate::core::types::Attrs::new(),
            born_action: None,
            dead_action: None,
            stackable: false,
        };
        ana.attrs
            .insert("container".into(), crate::core::types::AttrValue::Str("house".into()));
        crate::core::types::add_entity(&mut s, ana).unwrap();
        s
    }

    fn play_two_turns() -> Vec<ActionRecord> {
        let (dict, content) = pace_rig();
        let mut records = Vec::new();
        let r = execute_step(
            &content,
            &dict,
            seed(),
            Step {
                start_min: None,
                clock: Some(Clock { day: 0, halfhour: 0 }),
                clauses: parse_sentence("0:0 @ ANA PACE", &dict).unwrap().clauses,
                t_real: None,
            },
            ExecOptions {
                first_record_id: 1,
                id_gen: None,
            },
        )
        .unwrap();
        records.extend(r.records);
        let r2 = execute_step(
            &content,
            &dict,
            r.state,
            Step {
                start_min: None,
                clock: Some(Clock { day: 0, halfhour: 1 }),
                clauses: vec![],
                t_real: None,
            },
            ExecOptions {
                first_record_id: records.len() as i64 + 1,
                id_gen: None,
            },
        )
        .unwrap();
        records.extend(r2.records);
        records
    }

    #[test]
    fn save_then_load_resaves_byte_identical() {
        let records = play_two_turns();
        let dir = std::env::temp_dir().join(format!("chorus-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.jsonl");
        let _ = fs::remove_file(&path);
        save_log(&path, &records).unwrap();
        let first = fs::read(&path).unwrap();
        // structural facts while we have it: one JSON object per row, \n-terminated
        assert!(first.ends_with(b"\n"));
        assert_eq!(
            first.iter().filter(|b| **b == b'\n').count(),
            records.len()
        );
        let loaded = load_log(&path).unwrap();
        assert_eq!(loaded, records, "loadLog round-trips every field");
        save_log(&path, &loaded).unwrap();
        let second = fs::read(&path).unwrap();
        assert_eq!(first, second, "save(load(save(log))) is byte-stable (law 1)");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_seed_double_run_is_byte_identical() {
        // the dice.test.ts:917-933 judgment, engine-shaped: two identical
        // plays produce identical JSONL bytes
        let a = play_two_turns();
        let b = play_two_turns();
        let to_lines = |rs: &[ActionRecord]| -> Vec<String> {
            rs.iter()
                .map(|r| serde_json::to_string(r).unwrap())
                .collect()
        };
        assert_eq!(to_lines(&a), to_lines(&b));
    }

    #[test]
    fn load_log_reports_the_offending_line() {
        let dir = std::env::temp_dir().join(format!("chorus-store-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.jsonl");
        let good = serde_json::to_string(&play_two_turns()[0]).unwrap();
        fs::write(&path, format!("{good}\nnot json at all\n")).unwrap();
        let err = load_log(&path).unwrap_err();
        assert!(err.msg.contains("bad.jsonl:2:"), "{err}");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn log_mints_ids_and_truncates_for_undo() {
        let mut log = Log::new(new_state(0));
        assert_eq!(log.next_record_id(), 1);
        let records = play_two_turns();
        log.append_all(records.clone());
        assert_eq!(log.next_record_id(), records.len() as i64 + 1);
        log.truncate(1); // ts: play.ts:164 — the undo truncation
        assert_eq!(log.records.len(), 1);
        assert_eq!(log.records[0], records[0]);
        assert_eq!(log.next_record_id(), 2);
    }
}
