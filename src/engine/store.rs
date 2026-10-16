//! Append-only log + JSONL persistence (spec §7). The log is the source of
//! truth; entity/relation tables are projections built by fold.
//!
//! Byte identity (law 1): [`save_log`] writes one
//! `serde_json::to_string(record)` per row — the serde field order on
//! [`ActionRecord`] and its payload types is pinned, and serde_json's
//! string escaping matches the engine's canonical escape set exactly
//! (`\"` `\\`, `\b\t\n\f\r`, other <0x20 as lowercase `\u00xx`, DEL and
//! non-ASCII raw — see `core::jsonw`), so save → load → save reproduces the
//! same bytes.
//!
//! Deliberate shapes (neither test-pinned):
//! - the write is atomic (tmp + rename) rather than an in-place truncate;
//!   the crash-window behavior differs, the game-visible bytes do not;
//! - [`load_log`] errors carry the file name and 1-based line number, not
//!   the bare per-line parse message.

use std::fs;
use std::path::Path;

use crate::core::types::State;
use crate::engine::engine::ActionRecord;
use crate::error::{ChorusError, Result};

/// The append-only action log. Append-only: the invariant is maintained by
/// the CALLER — undo truncates `records` directly (see [`Log::truncate`]).
pub struct Log {
    pub records: Vec<ActionRecord>,
    /// The base state the constructor stores. Production code never reads
    /// it; the only readers are test fixtures. Kept public as a TEST
    /// FIXTURE, not engine state; do not build gameplay on it.
    pub initial_state: State,
}

impl Log {
    pub fn new(initial_state: State) -> Log {
        Log { records: Vec::new(), initial_state }
    }

    /// The next record id: one past the last appended record.
    #[must_use]
    pub fn next_record_id(&self) -> i64 {
        self.records.len() as i64 + 1
    }

    /// Append every record produced by an execution step, in order.
    pub fn append_all(&mut self, records: Vec<ActionRecord>) {
        self.records.extend(records);
    }

    /// The undo truncation point: drop every record past `len`; undo then
    /// re-folds the surviving prefix (see [`crate::engine::fold::fold_up_to`]).
    pub fn truncate(&mut self, len: usize) {
        self.records.truncate(len);
    }
}

/// One JSON record per line, `\n`-joined with a trailing newline.
pub fn save_log(path: &Path, records: &[ActionRecord]) -> Result<()> {
    let mut body = String::new();
    for r in records {
        let line = serde_json::to_string(r)
            .map_err(|e| ChorusError::new(format!("record {r:?} is not serializable: {e}")))?;
        body.push_str(&line);
        body.push('\n');
    }
    // atomic write (deliberate, see module doc): write a sibling
    // tmp file, then rename over the target — a reader never sees a partial
    // log. Single-threaded game, so a fixed suffix cannot collide.
    let tmp = path.with_extension("jsonl.tmp");
    fs::write(&tmp, body.as_bytes())
        .map_err(|e| ChorusError::new(format!("cannot write {}: {e}", tmp.display())))?;
    fs::rename(&tmp, path)
        .map_err(|e| ChorusError::new(format!("cannot write {}: {e}", path.display())))?;
    Ok(())
}

/// One JSON record per line. ONLY truly empty lines are dropped; every
/// other line must parse, and a failure carries the file name and 1-based
/// line number.
pub fn load_log(path: &Path) -> Result<Vec<ActionRecord>> {
    let text = fs::read_to_string(path)
        .map_err(|e| ChorusError::new(format!("cannot read {}: {e}", path.display())))?;
    let mut out = Vec::new();
    for (i, line) in text.split('\n').enumerate() {
        if line.is_empty() {
            continue; // filter(Boolean)
        }
        let record: ActionRecord = serde_json::from_str(line)
            .map_err(|e| ChorusError::new(format!("{}:{}: {}", path.display(), i + 1, e)))?;
        out.push(record);
    }
    Ok(out)
}

// --------------------------------------------------------------------------
// Tests: the JSONL byte-identity discipline (law 1). These pin the store
// contract.
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
                        max: None,
                        aspects: Vec::new(),
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
        ana.attrs.insert("container".into(), crate::core::types::AttrValue::Str("house".into()));
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
            ExecOptions { first_record_id: 1, id_gen: None },
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
            ExecOptions { first_record_id: records.len() as i64 + 1, id_gen: None },
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
        assert_eq!(first.iter().filter(|b| **b == b'\n').count(), records.len());
        let loaded = load_log(&path).unwrap();
        assert_eq!(loaded, records, "loadLog round-trips every field");
        save_log(&path, &loaded).unwrap();
        let second = fs::read(&path).unwrap();
        assert_eq!(first, second, "save(load(save(log))) is byte-stable (law 1)");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_seed_double_run_is_byte_identical() {
        // the law-1 judgment, engine-shaped: two identical
        // plays produce identical JSONL bytes
        let a = play_two_turns();
        let b = play_two_turns();
        let to_lines = |rs: &[ActionRecord]| -> Vec<String> {
            rs.iter().map(|r| serde_json::to_string(r).unwrap()).collect()
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
        log.truncate(1); // the undo truncation
        assert_eq!(log.records.len(), 1);
        assert_eq!(log.records[0], records[0]);
        assert_eq!(log.next_record_id(), 2);
    }
}
