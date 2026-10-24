//! Defensive accessors for the CS JSON dialect, plus the declarative
//! mapping tables (kind markers, start ids, verbs, guarded slots).

use serde_json::Value;

use crate::model::CsData;

// ---------------------------------------------------------------------------
// source-data accessors (the CS JSON dialect, defensively read)
// ---------------------------------------------------------------------------

pub(super) fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

pub(super) fn truthy(v: &Value, k: &str) -> bool {
    match v.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(st)) => st.eq_ignore_ascii_case("true"),
        Some(Value::Number(n)) => n.as_i64() != Some(0),
        _ => false,
    }
}

pub(super) fn num(v: &Value, k: &str) -> Option<i64> {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(st)) => st.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// An object of CS `aspect: level` pairs, sorted by key, numeric only.
pub(super) fn pairs(v: &Value, k: &str) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = v
        .get(k)
        .and_then(|x| x.as_object())
        .map(|o| {
            o.iter()
                .filter_map(|(key, val)| {
                    val.as_i64()
                        .or_else(|| val.as_f64().map(|f| f as i64))
                        .map(|n| (key.clone(), n))
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

pub(super) fn str_list(v: &Value, k: &str) -> Vec<String> {
    v.get(k)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// A linked/alt/inductions entry: a bare recipe id or an object with `id`.
pub(super) fn entries(v: &Value, k: &str) -> Vec<Value> {
    match v.get(k) {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

pub(super) fn entry_id(e: &Value) -> Option<String> {
    e.as_str()
        .map(String::from)
        .or_else(|| s(e, "id").map(String::from))
}

pub(super) fn escape_sw(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' | '\r' | '\t' => out.push(' '),
            '{' => out.push('('),
            '}' => out.push(')'),
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out.trim().to_string()
}

/// CS kind markers, first match wins (a declarative table, not a special case).
pub(super) const KIND_ORDER: &[(&str, &str)] = &[
    ("mortal", "character"),
    ("summoned", "character"),
    ("hunter", "character"),
    ("location", "place"),
    ("vault", "place"),
    ("ritual", "rite"),
    ("text", "text"),
    ("tome", "text"),
    ("lore", "idea"),
    ("influence", "idea"),
    ("memory", "idea"),
    ("glimming", "idea"),
    ("ability", "idea"),
    ("ingredient", "item"),
    ("tool", "item"),
    ("funds", "resource"),
    ("health", "resource"),
    ("reason", "resource"),
    ("passion", "resource"),
    ("vitality", "resource"),
    ("erudition", "resource"),
    ("glimmering", "resource"),
    ("contentment", "resource"),
];

/// The cards that instantiate at boot: the classic Aspirant legacy defaults
/// (a declarative table entry — CS's own legacy start set).
pub(super) const START_IDS: &[&str] = &["health", "reason", "passion", "funds", "introjob"];

/// The player-facing verbs, in table order.
pub(super) const PLAYER_VERBS: &[&str] = &["work", "dream", "study", "talk", "explore"];

/// The ambient verb.
pub(super) const TIME_VERB: &str = "time";

pub const SEED: u64 = 1899;

/// Recipes whose completion branches on a named slot's aspects, and the slot
/// whose absence the branch structure cannot survive: CS demonstrably
/// requires a doctrine card by the founding's completion (the alt cascade
/// keys on the doctrine's aspects), so the doctrine-less opening is refused
/// instead of starting a situation that could only wait. The guard holds
/// while no doctrine is bound in the slot AND no lore candidate rides the
/// context's interior — a start fired from a situation that carries a lore
/// (the founding hand-off) still opens, and the bind lands mid-run.
/// (CS id, slot name, slot filter aspect, guard prose.)
pub(super) const GUARDED_SLOTS: &[(&str, &str, &str, &str)] = &[(
    "cultfoundation",
    "doctrine",
    "lore",
    "Choose Founding Principles: bind a Lore card as the cult's Doctrine.",
)];

/// The actions whose CS verb declares its own drop slot (`verbs` table, the
/// singular `slot` field); mapped onto every recipe of that action.
pub(super) fn verb_slot_of<'a>(d: &'a CsData, action: &str) -> Option<&'a Value> {
    d.verbs.get(action).and_then(|v| v.get("slot"))
}
