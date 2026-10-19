//! The log row (spec §7): the serde-pinned wire shape of `ActionRecord`,
//! the step input, the exec plumbing types, and the four make*Record
//! builders (BEGIN / COMPLETE / WAIT / EVENT) — plus the checked-arithmetic
//! glue every row and every counter shares. The canonical JSON form of the
//! aspects/attrs pair values lives in `core::jsonw`.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::core::clock::{clock_from_abs_min, format_clock, Clock, MIN_PER_TICK};
use crate::core::dict::Dictionary;
use crate::core::format::format_sentence;
use crate::core::r#ref::{sort_refs, Ref};
use crate::core::roles::Role;
use crate::core::sentence::{Clause, Outcome, Sentence, SentenceKind};
use crate::core::types::{OpenWindow, State};
use crate::error::Result;

use super::validate::{show, Prepared};

/// The step input: a quantized `clock` and/or a minute-precise `start_min`,
/// plus the parsed clauses. `Clone`/`Debug` support callers that copy steps
/// into scratch drafts (e.g. the fuzz campaign's candidate generators); steps are
/// freely copied, never shared — no aliasing is observable.
#[derive(Debug, Clone)]
pub struct Step {
    /// begin at this absolute minute (v0.5, minute-precise — wins over clock)
    pub start_min: Option<i64>,
    /// quantized begin clock — the input syntax; the default begin is now
    pub clock: Option<Clock>,
    pub clauses: Vec<Clause>,
    pub t_real: Option<String>,
}

/// The log row. The serde field order is pinned: every make*Record
/// construction site builds the same literal shape, and the `Option`s
/// WITHOUT `skip_serializing_if` serialize as explicit `null` fields.
///
/// `kind` is deliberately a raw String, not a closed enum: replay switches
/// on it and ANY unknown value falls through to the action branch. The
/// open-world row vocabulary is this engine's choice — rows minted by a
/// newer engine still replay in an older binary instead of failing to
/// deserialize — and the String round-trips the JSONL losslessly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub id: i64,
    pub kind: String,
    pub trigger: Option<String>,
    #[serde(rename = "tGame")]
    pub t_game: String, // display clock "day:tick" (floored — display only)
    #[serde(rename = "tReal")]
    pub t_real: Option<String>,
    #[serde(rename = "advancedMin")]
    pub advanced_min: i64, // BEGIN: window length; wait: the span; else 0
    /// v0.5 — exact minutes: BEGIN = begin minute, wait = span start. The
    /// quantized tGame cannot carry mid-tick minutes, and fold must land
    /// them exactly (expires anchoring, §4.4)
    #[serde(rename = "startMin")]
    pub start_min: Option<i64>,
    /// v0.5 — COMPLETE rows only: the window key they close
    pub window: Option<String>,
    pub actors: Vec<Ref>, // sorted union across clauses
    pub verb: String,     // first clause's verb ('' for events/waits; the verb of a COMPLETE row)
    pub objects: Vec<Ref>,
    pub instruments: Vec<Ref>,
    pub at: Option<Ref>,
    pub about: Option<Ref>,
    pub to: Option<Ref>,
    pub outcomes: Vec<Outcome>, // concrete: ids minted, sugar resolved (§9.2)
    pub recipes: Vec<String>,   // BEGIN: fired recipe ids, clause order; COMPLETE: the one recipe
    pub sentence: String,       // canonical, lossless
}

/// One executed step: the records minted, the resulting state (unchanged
/// when rejected or refused), and the rejection reason if any.
pub struct ExecResult {
    pub records: Vec<ActionRecord>,
    pub state: State,
    pub rejected: Option<String>,
}

/// Execution options: the injected `id_gen` closure rides as
/// `&mut dyn FnMut`; `None` selects the deterministic default minted inside
/// execute_step (§9.1). [`Default`] is a fresh log's options — ids mint
/// from 1 (an empty log's `next_record_id()`) with no injected gen.
pub struct ExecOptions<'a> {
    pub first_record_id: i64,
    pub id_gen: Option<&'a mut dyn FnMut() -> String>,
}

impl Default for ExecOptions<'_> {
    fn default() -> Self {
        ExecOptions { first_record_id: 1, id_gen: None }
    }
}

/// A rejection result: no records, the input state untouched.
pub(super) fn rejected(reason: String, state: State) -> ExecResult {
    ExecResult { records: Vec::new(), state, rejected: Some(reason) }
}

/// The all-inert row shell the four make*Record builders customize: only
/// `kind`, the id and the display clock are common to every row kind; the
/// serde field order is the struct's, so the shell keeps the wire shape.
fn base_record(kind: &str, id: i64, clock: Clock) -> ActionRecord {
    ActionRecord {
        id,
        kind: kind.to_string(),
        trigger: None,
        t_game: format_clock(clock),
        t_real: None,
        advanced_min: 0,
        start_min: None,
        window: None,
        actors: Vec::new(),
        verb: String::new(),
        objects: Vec::new(),
        instruments: Vec::new(),
        at: None,
        about: None,
        to: None,
        outcomes: Vec::new(),
        recipes: Vec::new(),
        sentence: String::new(),
    }
}

/// Build the BEGIN row: merged deduped actors, the first clause's role refs,
/// durations summed into `advanced_min`.
pub(super) fn make_begin_record(
    id: i64,
    start_min: i64,
    step: &Step,
    prepared: &[Prepared],
    outcomes: Vec<Outcome>,
    dict: &Dictionary,
) -> Result<ActionRecord> {
    let advanced_min = prepared.iter().fold(0i64, |s, p| s.saturating_add(p.verb.duration));
    let first = prepared.first();
    let mut seen: HashSet<String> = HashSet::new();
    let actors: Vec<Ref> = sort_refs(
        &prepared.iter().flat_map(|p| p.clause.actors.iter().cloned()).collect::<Vec<Ref>>(),
    )
    .into_iter()
    .filter(|r| {
        // dedupe on `#id` / name keys
        let k = show(r);
        if seen.contains(&k) {
            return false;
        }
        seen.insert(k);
        true
    })
    .collect();
    let verb = first.map(|p| p.clause.verb.clone()).unwrap_or_default();
    let objects = first.map(|p| p.clause.objects.clone()).unwrap_or_default();
    let instruments = first.map(|p| p.clause.with.clone()).unwrap_or_default();
    let at = first.and_then(|p| p.clause.at.clone());
    let about = first.and_then(|p| p.clause.about.clone());
    let to = first.and_then(|p| p.clause.to.clone());
    let recipes: Vec<String> = prepared.iter().map(|p| p.recipe.id.clone()).collect();
    let mut record = base_record("action", id, clock_from_abs_min(start_min));
    record.t_real = step.t_real.clone();
    record.advanced_min = advanced_min; // the window length — NOT a clock advance (v0.5)
    record.start_min = Some(start_min); // exact begin minute (fold lands it exactly)
    record.actors = actors;
    record.verb = verb;
    record.objects = objects;
    record.instruments = instruments;
    record.at = at;
    record.about = about;
    record.to = to;
    record.outcomes = outcomes;
    record.recipes = recipes;
    let mut s = Sentence::bare(SentenceKind::Action, Some(clock_from_abs_min(start_min)));
    s.clauses = step.clauses.clone();
    s.outcomes = record.outcomes.clone();
    record.sentence = format_sentence(&s, dict)?;
    Ok(record)
}

/// Build the COMPLETE row for one closed window.
pub(super) fn make_complete_record(
    id: i64,
    w: &OpenWindow,
    outcomes: Vec<Outcome>,
    dict: &Dictionary,
) -> Result<ActionRecord> {
    // the completion clock — display only; mechanics anchor via the window
    let clock = clock_from_abs_min(w.end);
    let refs_of = |role: &Role| w.refs.get(role).cloned().unwrap_or_default();
    let mut rec = base_record("complete", id, clock);
    rec.trigger = Some(w.verb.clone());
    rec.window = Some(w.key.clone());
    rec.actors = refs_of(&Role::Actors);
    rec.verb = w.verb.clone();
    rec.objects = refs_of(&Role::Object);
    rec.instruments = refs_of(&Role::With);
    rec.at = w.refs.get(&Role::At).and_then(|v| v.first().cloned());
    rec.about = w.refs.get(&Role::About).and_then(|v| v.first().cloned());
    rec.to = w.refs.get(&Role::To).and_then(|v| v.first().cloned());
    rec.outcomes = outcomes;
    rec.recipes = vec![w.recipe.clone()];
    // advanced_min stays 0: complete rows never move the clock — waits do
    let mut s = Sentence::bare(SentenceKind::Complete, Some(clock));
    s.trigger = Some(w.verb.clone());
    s.outcomes = rec.outcomes.clone();
    rec.sentence = format_sentence(&s, dict)?;
    Ok(rec)
}

/// Build the WAIT row spanning exactly `[from_min, from_min + span_min]`.
pub(super) fn make_wait_record(
    id: i64,
    from_min: i64,
    span_min: i64,
    dict: &Dictionary,
) -> Result<ActionRecord> {
    let clock = clock_from_abs_min(from_min);
    let mut rec = base_record("wait", id, clock);
    rec.advanced_min = span_min;
    rec.start_min = Some(from_min); // exact span start — fold asserts continuation
    let mut s = Sentence::bare(SentenceKind::Wait, Some(clock));
    s.span_min = Some(span_min);
    rec.sentence = format_sentence(&s, dict)?;
    Ok(rec)
}

/// Build the EVENT row for one fired trigger at one crossed boundary.
pub(super) fn make_event_record(
    id: i64,
    trigger: &str,
    abs_slot: i64,
    outcomes: Vec<Outcome>,
    dict: &Dictionary,
) -> Result<ActionRecord> {
    let clock = clock_from_abs_min(abs_slot * MIN_PER_TICK);
    let mut rec = base_record("event", id, clock);
    rec.trigger = Some(trigger.to_string());
    rec.outcomes = outcomes;
    let mut s = Sentence::bare(SentenceKind::Event, Some(clock));
    s.trigger = Some(trigger.to_string());
    s.outcomes = rec.outcomes.clone();
    rec.sentence = format_sentence(&s, dict)?;
    Ok(rec)
}

// ---------- arithmetic / format helpers (borrow-shape glue) ----------

/// §9.2 accumulation discipline: rng counters, deck cursors, advance
/// targets, window offsets and wait spans accumulate as exact integers.
/// i64 `checked_add` overflow is unreachable for any real log — the defined
/// overflow direction is saturation, with a debug assert on the way.
pub(super) fn checked_step(a: i64, b: i64, what: &str) -> i64 {
    match a.checked_add(b) {
        Some(v) => v,
        None => {
            debug_assert!(false, "{what}: i64 overflow {a} + {b} (saturating at the i64 bound)");
            i64::MAX
        }
    }
}
