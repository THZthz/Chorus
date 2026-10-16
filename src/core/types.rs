//! Core data model (spec §4.2, §7).
//!
//! Ownership model: entities live in `State.entities` and are
//! never removed; slot bindings hold entity IDS, never cloned entities; no
//! Rc/RefCell anywhere. Iteration order is observable: entities/relations/
//! windows/decks are `IndexMap`, `fired` an `IndexSet`; `by_name`'s OUTER
//! map is a std `HashMap` (deliberate: no unsorted iteration point over it —
//! snap and script-play both sort first),
//! while the inner pool VECTORS carry the real order (head = oldest).

use std::collections::HashMap;

use indexmap::{IndexMap, IndexSet};

use crate::core::jsonw::clone_map_in_key_order;
use crate::core::r#ref::Ref;
use crate::core::roles::Role;
use crate::error::{ChorusError, Result};

/// The mechanics clock: `day` + `halfhour` (re-exported by `clock`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub day: i64,
    /// halfhours since day start (0..47); one halfhour = 30 minutes
    pub halfhour: i64,
}

/// One aspect value: a number, or the one-level
/// dot sub-map (`{secret:4, language:{latin:2}}`). QUIRK(Q13): the
/// model is deliberately NOT a nested tree — `'a.b.c'` stores a sub-map key
/// literally spelled `b.c`, and a later whole-value write clobbers a
/// sub-map (and vice versa) in both directions.
#[derive(Debug, Clone, PartialEq)]
pub enum AspectVal {
    Num(i64),
    Sub(IndexMap<String, i64>),
}

/// A scalar attr value: a JSON string, number, or boolean. Cross-variant
/// `PartialEq` is `false` — variants are disjoint types, so a cross-variant
/// comparison never reads as equal (the frozen strict-equality judgment).
/// The serde form is untagged (the bare JSON primitive — the `pair` arg's
/// `v` in the log carries the primitive directly);
/// the variants are disjoint JSON types so untagged is total.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum AttrValue {
    Str(String),
    Num(i64),
    Bool(bool),
}

impl AttrValue {
    /// The canonical string spelling of a primitive — e.g. a predicate's
    /// container default (a missing attr spells as the empty string)
    /// and the format/fn_expand arg spellings.
    #[must_use]
    pub fn to_canonical_string(&self) -> String {
        match self {
            AttrValue::Str(s) => s.clone(),
            AttrValue::Num(n) => n.to_string(),
            AttrValue::Bool(b) => b.to_string(),
        }
    }
}

/// The engine's frozen truthiness judgment over a primitive or a missing
/// value: a missing value, the empty string, `0` and `false` are falsy;
/// everything else is truthy. Every `if (…)`-shaped read of an attr or
/// aspect goes through this predicate.
#[must_use]
pub fn is_truthy(v: Option<&AttrValue>) -> bool {
    match v {
        None => false,                            // a missing value
        Some(AttrValue::Str(s)) => !s.is_empty(), // the empty string
        Some(AttrValue::Num(n)) => *n != 0,       // zero
        Some(AttrValue::Bool(b)) => *b,           // false
    }
}

/// The `Aspects` map type.
pub type Aspects = IndexMap<String, AspectVal>;
/// The `Attrs` map type.
pub type Attrs = IndexMap<String, AttrValue>;

/// One entity in the world.
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
    // The hand-rolled clone mirrors an object-spread copy. The attrs/aspects
    // SPREADS reorder integer-like keys to the front (the
    // OrdinaryOwnPropertyKeys order) — a plain IndexMap clone would keep insertion
    // order and drift after `SET(x, "5", true)` + clone.
    // We implement Clone (not just a free fn) so no clone path can skip the
    // reorder.
    fn clone(&self) -> Self {
        Entity {
            id: self.id.clone(),
            kind: self.kind.clone(),
            name: self.name.clone(),
            aspects: clone_aspects(&self.aspects),
            attrs: clone_map_in_key_order(&self.attrs),
            born_action: self.born_action,
            dead_action: self.dead_action,
            stackable: self.stackable,
        }
    }
}

/// One relation edge between two entities.
#[derive(Debug, Clone, PartialEq)]
pub struct Relation {
    pub a: String,
    pub b: String,
    /// Rust name `type_` (`type` is a keyword); the JSON name stays `type`
    /// where serde maps it.
    pub type_: String,
    pub value: i64,
    pub born_action: i64,
    pub dead_action: Option<i64>,
}

/// A story's ending (§9.35): set by the first
/// ENDING outcome to apply; play and fold resolve through the same
/// `apply_outcome` path.
#[derive(Debug, Clone, PartialEq)]
pub struct Ending {
    pub id: String,
    pub title: String,
    pub flavour: String,
}

/// One open processing window (§6.7, v0.5).
#[derive(Debug, Clone, PartialEq)]
pub struct OpenWindow {
    /// `<begin record id>:<clause index>` — unique per window, orders with its row
    pub key: String,
    pub begin_id: i64,
    pub clause: i64,
    pub verb: String,
    pub start: i64,
    pub end: i64,
    /// the recipe id selected AT COMMIT — selection is a commit-time snapshot
    pub recipe: String,
    /// the clause's role bindings as authored — re-resolved at completion
    /// (same shape as the log row's refs: `at/about/to` keep their 0-or-1 arrays)
    pub refs: IndexMap<Role, Vec<Ref>>,
    /// ids bound in committed roles — the lock table (actors excluded)
    pub locked: Vec<String>,
}

/// The whole world state.
#[derive(Debug)]
pub struct State {
    pub entities: IndexMap<String, Entity>,
    /// name -> living ids in birth order (head = OLDEST copy, §9.38 pool
    /// addressing); unique-name law for non-stackable names still holds (§9.14).
    /// Outer map std-HashMap (no order-observable iteration
    /// over it anywhere); the pool vectors ARE the order.
    pub by_name: HashMap<String, Vec<String>>,
    /// key: `<a>|<b>|<type>` with a<=b for symmetric types.
    /// QUIRK(Q14): the key embeds '|' — an id containing '|' could
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
    /// §9.50: recipe execution counts — recipe id → registered windows.
    /// Maintained by the ONE window-registration path play and fold share;
    /// selection reads only this map, never the log (the `fired`
    /// precedent, §9.29).
    pub executed: IndexMap<String, i64>,
    /// v0.5: the open processing windows (§6.7) — the lock table.
    pub windows: IndexMap<String, OpenWindow>,
}

/// A fresh empty state at absolute minute `now`.
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
        executed: IndexMap::new(),
        windows: IndexMap::new(),
    }
}

impl Clone for State {
    // `State::clone`: every container is deep-copied so
    // a discarded draft can never mutate real state (law 6 comments below);
    // the dict-valued copies go through the canonical key order (see Entity::clone).
    fn clone(&self) -> Self {
        // law 6 (§9.38): the pool ARRAYS are deep-copied — a shallow map
        // clone would share arrays, so a discarded draft's pool destroy would
        // mutate real state
        let mut by_name = HashMap::with_capacity(self.by_name.len());
        for (k, v) in &self.by_name {
            by_name.insert(k.clone(), v.clone());
        }
        // law 6 (§9.42): cursors are copied — a discarded draft's draws must
        // not move the real deck
        // law 6 (§9.50): execution counts are copied — a discarded draft's
        // window registrations must not spend the real budget
        // law 6 (v0.5): windows are copied ENTRY-BY-ENTRY — `locked` is an
        // array a discarded draft must not share with the real state;
        // `refs` likewise (IndexMap of Vec<Ref>)
        State {
            entities: self.entities.clone(), // values clone through Entity::clone (canonical key order)
            by_name,
            relations: self.relations.clone(),
            now: self.now,
            fired: self.fired.clone(),
            ended: self.ended.clone(),
            rng: self.rng,
            decks: self.decks.clone(),
            executed: self.executed.clone(),
            windows: self.windows.clone(),
        }
    }
}

/// Free-function alias of [`State::clone`] — the canonical clone entry point.
#[must_use]
pub fn clone_state(s: &State) -> State {
    s.clone()
}

/// The aspects clone: also goes through the canonical key order (like the
/// object-spread of a sub-map).
#[must_use]
fn clone_aspects(a: &Aspects) -> Aspects {
    clone_map_in_key_order(a)
}

/// §9.14 + §9.38: names are unique among the
/// living — except stackable mints, which append to the pool. A stackable
/// mint may not collide with a DISTINCT non-stackable living name (shared
/// namespace); checking the existing members for stackability is exact
/// because a mixed pool is impossible under the mint rules.
pub fn add_entity(s: &mut State, e: Entity) -> Result<()> {
    if e.dead_action.is_none() {
        if let Some(pool) = s.by_name.get(&e.name) {
            if !pool.is_empty() {
                // the whole pool must be stackable; a missing
                // entity reads as non-stackable
                let existing_stackable =
                    pool.iter().all(|id| s.entities.get(id).is_some_and(|x| x.stackable));
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

/// §9.14/§9.38 name release on death: remove
/// the id from the name's pool; the name only frees when the LAST copy dies.
pub fn release_name(s: &mut State, e: &Entity) {
    release_name_entry(s, &e.name, &e.id);
}

/// The (name, id) form of [`release_name`] — the engine's DESTROY/REPLACE
/// paths read the entity off the same `State` they mutate (stamp
/// `dead_action`, then release), so the `(name, id)` pair avoids the
/// double borrow.
pub fn release_name_entry(s: &mut State, name: &str, id: &str) {
    let Some(pool) = s.by_name.get_mut(name) else {
        return;
    };
    if let Some(i) = pool.iter().position(|pid| pid == id) {
        pool.remove(i);
    }
    if pool.is_empty() {
        s.by_name.remove(name);
    }
}

/// Oldest living copy bearing a name (§9.38
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

// NOTE: a liveness-blind `get_entity` lookup is deliberately absent —
// dead code. `get_alive` below is the live accessor.

/// The entity with this id, when still alive.
#[must_use]
pub fn get_alive<'a>(s: &'a State, id: &str) -> Option<&'a Entity> {
    let e = s.entities.get(id)?;
    if e.dead_action.is_none() {
        Some(e)
    } else {
        None
    }
}

/// The borrow-shaped result of [`aspect_entry`] —
/// a number, a sub-map borrow, or missing.
#[derive(Debug, Clone, PartialEq)]
pub enum AspectEntry<'a> {
    Num(i64),
    Sub(&'a IndexMap<String, i64>),
}

/// Raw aspect entry by dotted path
/// (`secret`, `language.latin`; missing → None). Unlike [`aspect_get`] this
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
        Some(AspectVal::Num(_)) | None => None, // a scalar head reads as a miss, as does a missing sub-key (both read as undefined)
        Some(AspectVal::Sub(m)) => m.get(&path[dot + 1..]).map(|n| AspectEntry::Num(*n)),
    }
}

/// Aspect lookup by dotted path (missing → None).
#[must_use]
pub fn aspect_get(e: &Entity, path: &str) -> Option<i64> {
    match aspect_entry(e, path) {
        Some(AspectEntry::Num(n)) => Some(n),
        _ => None,
    }
}

/// Aspect write by dotted path — creates the
/// sub-object when missing (mirrors aspect_get). QUIRK(Q13): a later scalar
/// write replaces a whole sub-map and vice versa — both directions silent,
/// frozen.
pub fn aspect_set(e: &mut Entity, path: &str, v: i64) {
    let Some(dot) = path.find('.') else {
        e.aspects.insert(path.to_string(), AspectVal::Num(v));
        return;
    };
    let head = &path[..dot];
    let rest = &path[dot + 1..];
    // an existing Num head is REPLACED by a fresh
    // sub-map; a missing head creates one.
    if !matches!(e.aspects.get(head), Some(AspectVal::Sub(_))) {
        e.aspects.insert(head.to_string(), AspectVal::Sub(IndexMap::new()));
    }
    if let Some(AspectVal::Sub(m)) = e.aspects.get_mut(head) {
        m.insert(rest.to_string(), v);
    }
}

/// Symmetric edges get one canonical key
/// (sorted); directed edges keep the stored direction — swapping directed
/// endpoints loses the direction. The `a > b` comparison is UTF-16
/// code-unit order → `cmp_utf16`.
#[must_use]
pub fn relation_key(a: &str, b: &str, type_: &str, symmetric: bool) -> String {
    let (a, b) = if symmetric && crate::core::text::cmp_utf16(a, b) == std::cmp::Ordering::Greater {
        (b, a)
    } else {
        (a, b)
    };
    // QUIRK(Q14): '|' embedded in an id can alias two distinct edges
    // onto one key; frozen by the engine-minted-id convention.
    format!("{a}|{b}|{type_}")
}

/// Relations are stored under one canonical
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

// NOTE: a plain `attr_get` read helper is deliberately absent — dead code.
