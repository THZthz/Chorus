//! Core data model — ts: src/core/types.ts (spec §4.2, §7).
//!
//! Ownership model (plan §2.6): entities live in `State.entities` and are
//! never removed; slot bindings hold entity IDS, never cloned entities; no
//! Rc/RefCell anywhere. Iteration order is observable: entities/relations/
//! windows/decks are `IndexMap`, `fired` an `IndexSet`; `by_name`'s OUTER
//! map is a std `HashMap` on the plan's explicit blessing (§3.7: no
//! unsorted iteration point over it — snap and script-play both sort first),
//! while the inner pool VECTORS carry the real order (head = oldest).

use std::collections::HashMap;

use indexmap::{IndexMap, IndexSet};

use crate::core::r#ref::Ref;
use crate::core::roles::Role;
use crate::js::{clone_map_in_js_order, ChorusError, Result};

/// ts: src/core/types.ts:5-9 (declared here, re-exported by clock.ts — the
/// Rust tree mirrors that: clock.rs does `pub use types::Clock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub day: i64,
    /// halfhours since day start (0..47); one halfhour = 30 minutes
    pub halfhour: i64,
}

/// ts: src/core/types.ts:12 — one aspect value: a number, or the one-level
/// dot sub-map (`{secret:4, language:{latin:2}}`). QUIRK(CORE-18/Q13): the
/// model is deliberately NOT a nested tree — `'a.b.c'` stores a sub-map key
/// literally spelled `b.c`, and a later whole-value write clobbers a
/// sub-map (and vice versa) in both directions.
#[derive(Debug, Clone, PartialEq)]
pub enum AspectVal {
    Num(i64),
    Sub(IndexMap<String, i64>),
}

/// ts: src/core/types.ts:14 — `string | number | boolean`. Cross-variant
/// `PartialEq` is `false`, exactly like JS `===` across types (plan §3.10).
#[derive(Debug, Clone, PartialEq)]
pub enum AttrValue {
    Str(String),
    Num(i64),
    Bool(bool),
}

impl AttrValue {
    /// ts: JS `String(v)` on a primitive — predicate.ts:416's
    /// `String(e.attrs['container'] ?? '')` and format/fn_expand spellings.
    #[must_use]
    pub fn to_js_string(&self) -> String {
        match self {
            AttrValue::Str(s) => s.clone(),
            AttrValue::Num(n) => n.to_string(),
            AttrValue::Bool(b) => b.to_string(),
        }
    }
}

/// The `Aspects` map type (ts: types.ts:12).
pub type Aspects = IndexMap<String, AspectVal>;
/// The `Attrs` map type (ts: types.ts:15).
pub type Attrs = IndexMap<String, AttrValue>;

/// ts: src/core/types.ts:17-29
#[derive(Debug, PartialEq)]
pub struct Entity {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub aspects: Aspects,
    pub attrs: Attrs,
    pub born_action: Option<i64>,
    pub dead_action: Option<i64>,
    /// minted from a `stackable` declaration (§9.38): the name may be held by
    /// several living copies. Every mint site computes it from the compiled
    /// stackable sets; the fold recomputes it identically from the logged ref.
    pub stackable: bool,
}

impl Clone for Entity {
    // ts: types.ts:118 — the hand-rolled clone: `{ ...e, aspects:
    // cloneAspects(e.aspects), attrs: { ...e.attrs } }`. The attrs/aspects
    // SPREADS reorder integer-like keys to the front (JS
    // OrdinaryOwnPropertyKeys) — a plain IndexMap clone would keep insertion
    // order and drift after `SET(x, "5", true)` + clone (plan §3.7, PR2-05).
    // We implement Clone (not just a free fn) so no clone path can skip the
    // reorder.
    fn clone(&self) -> Self {
        Entity {
            id: self.id.clone(),
            kind: self.kind.clone(),
            name: self.name.clone(),
            aspects: clone_aspects(&self.aspects),
            attrs: clone_map_in_js_order(&self.attrs),
            born_action: self.born_action,
            dead_action: self.dead_action,
            stackable: self.stackable,
        }
    }
}

/// ts: src/core/types.ts:31-38
#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    pub a: String,
    pub b: String,
    /// TS field `type` (keyword in Rust); JSON name stays `type` when the
    /// W3 serde structs map it.
    pub type_: String,
    pub value: i64,
    pub born_action: i64,
    pub dead_action: Option<i64>,
}

/// ts: src/core/types.ts:44-48 — a story's ending (§9.35): set by the first
/// ENDING outcome to apply; play and fold resolve through the same
/// applyOutcome path.
#[derive(Debug, Clone, PartialEq)]
pub struct Ending {
    pub id: String,
    pub title: String,
    pub flavour: String,
}

/// ts: src/core/types.ts:59-74 — one open processing window (§6.7, v0.5).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenWindow {
    /// `${beginRecordId}:${clauseIndex}` — unique per window, orders with its row
    pub key: String,
    pub begin_id: i64,
    pub clause: i64,
    pub verb: String,
    pub start: i64,
    pub end: i64,
    /// the recipe id selected AT COMMIT — selection is a commit-time snapshot
    pub recipe: String,
    /// the clause's role bindings as authored — re-resolved at completion
    /// (same shape as clauseRefs: `at/about/to` keep their 0-or-1 arrays)
    pub refs: IndexMap<Role, Vec<Ref>>,
    /// ids bound in committed roles — the lock table (actors excluded)
    pub locked: Vec<String>,
}

/// ts: src/core/types.ts:76-109
#[derive(Debug)]
pub struct State {
    pub entities: IndexMap<String, Entity>,
    /// name -> living ids in birth order (head = OLDEST copy, §9.38 pool
    /// addressing); unique-name law for non-stackable names still holds (§9.14).
    /// Outer map std-HashMap per plan §3.7 (no order-observable iteration
    /// over it anywhere); the pool vectors ARE the order.
    pub by_name: HashMap<String, Vec<String>>,
    /// key: `${a}|${b}|${type}` with a<=b for symmetric types.
    /// QUIRK(CORE-19/Q14): the key embeds '|' — an id containing '|' could
    /// collide with a different edge; frozen by the engine convention that
    /// ids are engine-minted (`#hex`), never authored.
    pub relations: IndexMap<String, Relation>,
    /// engine clock, absolute minutes since day 0, tick 0
    pub now: i64,
    /// once-triggers that have fired (spec §9.29); rebuilt by fold from EVENT rows
    pub fired: IndexSet<String>,
    /// the ending once told (spec §9.35); None until an ENDING outcome applies,
    /// cleared naturally when the fold no longer reaches its record (undo)
    pub ended: Option<Ending>,
    /// §9.41: the seeded-RNG counter — how many die values the whole history
    /// has consumed. Play consumes counters ON THE DRAFT inside the ROLL
    /// outcome; the fold lands the same spot from the concrete
    /// `ROLLED(<total>, <ctrAfter>)` row (law 3).
    pub rng: i64,
    /// §9.42: deck cursors — for each deck id, how many cards it has served.
    pub decks: IndexMap<String, i64>,
    /// v0.5: the open processing windows (§6.7) — the lock table.
    pub windows: IndexMap<String, OpenWindow>,
}

/// ts: src/core/types.ts:111-113 (`newState(now = 0)`)
#[must_use]
pub fn new_state(now: i64) -> State {
    State {
        entities: IndexMap::new(),
        by_name: HashMap::new(),
        relations: IndexMap::new(),
        now,
        fired: IndexSet::new(),
        ended: None,
        rng: 0,
        decks: IndexMap::new(),
        windows: IndexMap::new(),
    }
}

impl Clone for State {
    // ts: types.ts:115-134 — `cloneState`. Every container is deep-copied so
    // a discarded draft can never mutate real state (law 6 comments below);
    // the dict-valued spreads go through the JS key order (see Entity::clone).
    fn clone(&self) -> Self {
        // law 6 (§9.38): the pool ARRAYS are deep-copied — a shallow map
        // clone would share arrays, so a discarded draft's pool destroy would
        // mutate real state (types.ts:122-125)
        let mut by_name = HashMap::with_capacity(self.by_name.len());
        for (k, v) in &self.by_name {
            by_name.insert(k.clone(), v.clone());
        }
        // law 6 (§9.42): cursors are copied — a discarded draft's draws must
        // not move the real deck (types.ts:126-128)
        // law 6 (v0.5): windows are copied ENTRY-BY-ENTRY — `locked` is an
        // array a discarded draft must not share with the real state
        // (types.ts:129-132); `refs` likewise (IndexMap of Vec<Ref>)
        State {
            entities: self.entities.clone(), // values clone through Entity::clone (JS key order)
            by_name,
            relations: self.relations.clone(),
            now: self.now,
            fired: self.fired.clone(),
            ended: self.ended.clone(),
            rng: self.rng,
            decks: self.decks.clone(),
            windows: self.windows.clone(),
        }
    }
}

/// ts: src/core/types.ts:115-134 — public alias mirroring the TS export.
#[must_use]
pub fn clone_state(s: &State) -> State {
    s.clone()
}

/// ts: src/core/types.ts:136-140 — the sub-map spread `{ ...v }` also
/// reorders integer-like keys (PR2-05 clone point types.ts:138).
#[must_use]
fn clone_aspects(a: &Aspects) -> Aspects {
    clone_map_in_js_order(a)
}

/// ts: src/core/types.ts:142-160 — §9.14 + §9.38: names are unique among the
/// living — except stackable mints, which append to the pool. A stackable
/// mint may not collide with a DISTINCT non-stackable living name (shared
/// namespace); checking the existing members for stackability is exact
/// because a mixed pool is impossible under the mint rules.
pub fn add_entity(s: &mut State, e: Entity) -> Result<()> {
    if e.dead_action.is_none() {
        if let Some(pool) = s.by_name.get(&e.name) {
            if !pool.is_empty() {
                // `pool.every(id => s.entities.get(id)?.stackable === true)`:
                // a missing entity reads as non-stackable
                let existing_stackable = pool
                    .iter()
                    .all(|id| s.entities.get(id).is_some_and(|x| x.stackable));
                if !e.stackable || !existing_stackable {
                    return Err(ChorusError::new(format!(
                        "duplicate name '{}' among alive entities",
                        e.name
                    )));
                }
            }
        }
        match s.by_name.get_mut(&e.name) {
            Some(pool) => pool.push(e.id.clone()),
            None => {
                s.by_name.insert(e.name.clone(), vec![e.id.clone()]);
            }
        }
    }
    let id = e.id.clone();
    s.entities.insert(id, e);
    Ok(())
}

/// ts: src/core/types.ts:162-170 — §9.14/§9.38 name release on death: remove
/// the id from the name's pool; the name only frees when the LAST copy dies.
pub fn release_name(s: &mut State, e: &Entity) {
    let Some(pool) = s.by_name.get_mut(&e.name) else {
        return;
    };
    if let Some(i) = pool.iter().position(|id| id == &e.id) {
        pool.remove(i);
    }
    if pool.is_empty() {
        s.by_name.remove(&e.name);
    }
}

/// ts: src/core/types.ts:172-180 — oldest living copy bearing a name (§9.38
/// pool addressing: a name ref with N > 1 living copies addresses the HEAD
/// of its pool — never id-string order).
#[must_use]
pub fn oldest_named<'a>(s: &'a State, name: &str) -> Option<&'a Entity> {
    let pool = s.by_name.get(name)?;
    for id in pool {
        if let Some(e) = s.entities.get(id) {
            if e.dead_action.is_none() {
                return Some(e);
            }
        }
    }
    None
}

// NOTE: `getEntity` (types.ts:182-184) is dead code and deliberately NOT
// ported (plan §1.2 "明确禁止" list). `getAlive` below is live (engine.ts:115).

/// ts: src/core/types.ts:186-189
#[must_use]
pub fn get_alive<'a>(s: &'a State, id: &str) -> Option<&'a Entity> {
    let e = s.entities.get(id)?;
    if e.dead_action.is_none() {
        Some(e)
    } else {
        None
    }
}

/// The borrow-shaped result of [`aspect_entry`] — TS returns
/// `number | Aspects | undefined` (types.ts:194).
#[derive(Debug, Clone, PartialEq)]
pub enum AspectEntry<'a> {
    Num(i64),
    Sub(&'a IndexMap<String, i64>),
}

/// ts: src/core/types.ts:191-200 — raw aspect entry by dotted path:
/// 'secret' | 'language.latin' (missing → None). Unlike [`aspect_get`] this
/// surfaces sub-maps, so a delta can refuse instead of silently clobbering
/// one (§9.31). Only the FIRST dot splits (QUIRK Q13: 'a.b.c' addresses the
/// sub-map key 'b.c').
#[must_use]
pub fn aspect_entry<'a>(e: &'a Entity, path: &str) -> Option<AspectEntry<'a>> {
    let Some(dot) = path.find('.') else {
        return e.aspects.get(path).map(|v| match v {
            AspectVal::Num(n) => AspectEntry::Num(*n),
            AspectVal::Sub(m) => AspectEntry::Sub(m),
        });
    };
    let sub = e.aspects.get(&path[..dot]);
    match sub {
        Some(AspectVal::Num(_)) | None => None, // `typeof sub === 'number'` → undefined; `sub?.[rest]` on missing → undefined
        Some(AspectVal::Sub(m)) => m.get(&path[dot + 1..]).map(|n| AspectEntry::Num(*n)),
    }
}

/// ts: src/core/types.ts:202-206 — aspect lookup by dotted path (missing → None).
#[must_use]
pub fn aspect_get(e: &Entity, path: &str) -> Option<i64> {
    match aspect_entry(e, path) {
        Some(AspectEntry::Num(n)) => Some(n),
        _ => None,
    }
}

/// ts: src/core/types.ts:208-219 — aspect write by dotted path — creates the
/// sub-object when missing (mirrors aspect_get). QUIRK(Q13): a later scalar
/// write replaces a whole sub-map and vice versa (`typeof sub === 'object' ?
/// sub : (e.aspects[head] = {})`) — both directions silent, frozen.
pub fn aspect_set(e: &mut Entity, path: &str, v: i64) {
    let Some(dot) = path.find('.') else {
        e.aspects.insert(path.to_string(), AspectVal::Num(v));
        return;
    };
    let head = &path[..dot];
    let rest = &path[dot + 1..];
    // `typeof sub === 'object' ? sub : (e.aspects[head] = {})` — an existing
    // Num is REPLACED by a fresh sub-map; a missing head creates one.
    if !matches!(e.aspects.get(head), Some(AspectVal::Sub(_))) {
        e.aspects.insert(head.to_string(), AspectVal::Sub(IndexMap::new()));
    }
    if let Some(AspectVal::Sub(m)) = e.aspects.get_mut(head) {
        m.insert(rest.to_string(), v);
    }
}

/// ts: src/core/types.ts:225-230 — symmetric edges get one canonical key
/// (sorted); directed edges keep the stored direction — swapping directed
/// endpoints loses the direction. The `a > b` comparison is JS string order
/// = UTF-16 code-unit order → `cmp_utf16` (plan §3.6, CORE-05).
#[must_use]
pub fn relation_key(a: &str, b: &str, type_: &str, symmetric: bool) -> String {
    let (a, b) = if symmetric && crate::js::cmp_utf16(a, b) == std::cmp::Ordering::Greater {
        (b, a)
    } else {
        (a, b)
    };
    // QUIRK(CORE-19/Q14): '|' embedded in an id can alias two distinct edges
    // onto one key; frozen by the engine-minted-id convention.
    format!("{a}|{b}|{type_}")
}

/// ts: src/core/types.ts:232-240 — relations are stored under one canonical
/// key; check both orders so this stays correct regardless of
/// symmetric/directed storage (the double-check is belt-and-braces).
#[must_use]
pub fn relation_alive(s: &State, id_a: &str, id_b: &str, type_: &str) -> bool {
    for k in [format!("{id_a}|{id_b}|{type_}"), format!("{id_b}|{id_a}|{type_}")] {
        if let Some(r) = s.relations.get(&k) {
            if r.dead_action.is_none() {
                return true;
            }
        }
    }
    false
}

// NOTE: `attrGet` (types.ts:221-223) is dead code and deliberately NOT
// ported (plan §1.2 "明确禁止" list).
