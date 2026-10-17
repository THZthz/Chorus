//! Shared full-state snapshot for fold-equality assertions (spec §7). ONE
//! helper so play/fold comparisons cannot drift —
//! engine tests, endings tests and nightjar-explorer tests all compare through
//! this. New State fields land here with the state itself.
//!
//! Frozen surface: the output is the exact canonical JSON image of the state —
//! - key order: `ents, rels, byName, now, rng, decks, executed, fired, ended,
//!   windows`
//!   (insertion order; no integer-like keys, so
//!   OrdinaryOwnPropertyKeys agrees);
//! - the SIX map fields
//!   (`ents/rels/byName/decks/executed/windows`) sort their entry
//!   pairs ascending by key
//!   `<` order, which is UTF-16 code-unit order → `cmp_utf16` (never Rust
//!   byte order);
//! - `fired` takes the default string sort (the same UTF-16 order);
//! - `windows` sorts its entries and then SHALLOW-copies each window: the
//!   copy is unobservable behind the JSON image, and
//!   the JSON projection of the Rust `OpenWindow` struct preserves the
//!   window object's insertion key order (no integer-like keys) — see
//!   [`window_json`].
//!
//! Object-valued fields serialized as plain objects (`aspects`,
//! `attrs`, window `refs`) walk [`canonical_key_order`] order —
//! idempotent on maps that were
//! cloned through the key-order clone paths, exact even when they were not.

//! (Shared test lib: every helper is the common discipline, not per-file
//! dead code — each mounting test crate uses a different subset, exactly like
//! harness.rs. The `allow` lives in this one place.)
#![allow(dead_code)]

use chorus::core::jsonw::canonical_key_order;
use chorus::core::text::cmp_utf16;
use chorus::core::types::{
    AspectVal, Aspects, AttrValue, Attrs, Entity, OpenWindow, Relation, State,
};

/// The full-state JSON image — see the module doc for the frozen surface.
#[must_use]
pub fn snap(s: &State) -> String {
    let mut root = serde_json::Map::new();
    root.insert(
        "ents".to_string(),
        sorted_entries(s.entities.iter().map(|(k, e)| (k.clone(), entity_json(e)))),
    );
    root.insert(
        "rels".to_string(),
        sorted_entries(s.relations.iter().map(|(k, r)| (k.clone(), relation_json(r)))),
    );
    root.insert(
        "byName".to_string(),
        sorted_entries(s.by_name.iter().map(|(k, v)| (k.clone(), serde_json::json!(v)))),
    );
    root.insert("now".to_string(), serde_json::json!(s.now));
    // §4.4: the dice counter — play and fold must land it identically
    root.insert("rng".to_string(), serde_json::json!(s.rng));
    // §4.4: the deck cursors
    root.insert(
        "decks".to_string(),
        sorted_entries(s.decks.iter().map(|(k, v)| (k.clone(), serde_json::json!(v)))),
    );
    // §6.4: the recipe execution counts — play and fold must rebuild them
    // identically (the same registration path)
    root.insert(
        "executed".to_string(),
        sorted_entries(s.executed.iter().map(|(k, v)| (k.clone(), serde_json::json!(v)))),
    );
    // §6.6: play and fold must agree on fired too (default string sort)
    let mut fired: Vec<&String> = s.fired.iter().collect();
    fired.sort_by(|a, b| cmp_utf16(a, b));
    root.insert("fired".to_string(), serde_json::json!(fired));
    // §4.4: …and on the story's ending
    root.insert(
        "ended".to_string(),
        match &s.ended {
            None => serde_json::Value::Null,
            Some(e) => serde_json::json!({
                "id": e.id, "title": e.title, "flavour": e.flavour,
            }),
        },
    );
    // v0.5: the open windows — a log may END with them (quit mid-process), so
    // fold equality must see them; sorted by key for a stable snapshot
    root.insert(
        "windows".to_string(),
        sorted_entries(s.windows.iter().map(|(k, w)| (k.clone(), window_json(w)))),
    );
    serde_json::Value::Object(root).to_string()
}

/// Map entries sorted ascending by key
/// in `<` order = UTF-16 code-unit order (`cmp_utf16`; keys are unique).
/// Emits `[key, value]` PAIRS — the JSON image of the entry list.
fn sorted_entries<I>(entries: I) -> serde_json::Value
where
    I: Iterator<Item = (String, serde_json::Value)>,
{
    let mut v: Vec<(String, serde_json::Value)> = entries.collect();
    v.sort_by(|a, b| cmp_utf16(&a.0, &b.0));
    serde_json::Value::Array(
        v.into_iter()
            .map(|(k, val)| serde_json::Value::Array(vec![serde_json::Value::String(k), val]))
            .collect(),
    )
}

/// The entity object in its frozen field order.
fn entity_json(e: &Entity) -> serde_json::Value {
    serde_json::json!({
        "id": e.id,
        "kind": e.kind,
        "name": e.name,
        "aspects": aspects_json(&e.aspects),
        "attrs": attrs_json(&e.attrs),
        "bornAction": e.born_action,
        "deadAction": e.dead_action,
        "stackable": e.stackable,
    })
}

/// The relation object in its frozen field order.
fn relation_json(r: &Relation) -> serde_json::Value {
    serde_json::json!({
        "a": r.a, "b": r.b, "type": r.type_, "value": r.value,
        "bornAction": r.born_action, "deadAction": r.dead_action,
    })
}

/// The window object's JSON image. The frozen semantics keep
/// the window's insertion key order (no integer-like keys), and the `refs`
/// object keeps its insertion order = ROLES order — the Rust
/// struct field order and the refs `IndexMap` mirror both exactly, so this
/// projection IS the window's JSON image.
fn window_json(w: &OpenWindow) -> serde_json::Value {
    let mut refs = serde_json::Map::new();
    for (role, rs) in &w.refs {
        refs.insert(role.as_str().to_string(), serde_json::json!(rs));
    }
    serde_json::json!({
        "key": w.key,
        "beginId": w.begin_id,
        "clause": w.clause,
        "verb": w.verb,
        "start": w.start,
        "end": w.end,
        "recipe": w.recipe,
        "refs": serde_json::Value::Object(refs),
        "locked": w.locked,
    })
}

/// The canonical JSON image of an aspects map: sub-maps one level deep
/// (Q13's flat model), every level walked in `OrdinaryOwnPropertyKeys` order.
fn aspects_json(a: &Aspects) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for k in canonical_key_order(a) {
        let Some(v) = a.get(&k) else {
            continue;
        };
        out.insert(
            k,
            match v {
                AspectVal::Num(n) => serde_json::json!(n),
                AspectVal::Sub(sub) => {
                    // sub-map values are plain ints (the Q13 flat model)
                    let mut m = serde_json::Map::new();
                    for k2 in canonical_key_order(sub) {
                        if let Some(n2) = sub.get(&k2) {
                            m.insert(k2, serde_json::json!(n2));
                        }
                    }
                    serde_json::Value::Object(m)
                }
            },
        );
    }
    serde_json::Value::Object(out)
}

/// The attrs map's JSON image (string, number, or boolean values).
fn attrs_json(a: &Attrs) -> serde_json::Value {
    let mut out = serde_json::Map::new();
    for k in canonical_key_order(a) {
        let Some(v) = a.get(&k) else {
            continue;
        };
        out.insert(
            k,
            match v {
                AttrValue::Str(s) => serde_json::json!(s),
                AttrValue::Num(n) => serde_json::json!(n),
                AttrValue::Bool(b) => serde_json::json!(b),
            },
        );
    }
    serde_json::Value::Object(out)
}
