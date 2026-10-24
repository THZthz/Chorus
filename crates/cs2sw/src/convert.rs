//! The mapping passes: CS core content -> `.sw` text. The converter maps, it
//! never interprets: every emission is a table-driven transformation of the
//! source shape, every construct the language cannot carry becomes a named
//! gap row, and every iteration is sorted (or content-ordered for recipes,
//! which is CS's own first-match order).
//!
//! The three-scope story, per the decompile-verified semantics:
//! - Requirements read the situation's combined interior; every requirement
//!   key is made bindable through a mapped or synthesized slot, so the `when`
//!   sums the interior plus the slots that can carry the key (the commit
//!   selection context sees submitted bindings only through slot scopes).
//! - `linked` arrays become exclusive `continue via` cascades: the
//!   same-situation continuation preserves the interior, so chained recipes
//!   pay and gate out of the parent's storage exactly as CS's situation
//!   persistence does. Non-additional `alternativerecipes` adopt the same
//!   shape, gated ahead of the recipe's own body — the override decision is
//!   CS's first completion statement, and the adopting override's body
//!   replaces the original's.
//! - `tablereqs` read the `table` scope (public cards plus non-greedy slot
//!   bindings) and `extantreqs` the `anywhere` scope.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::model::CsData;
use crate::naming::{mangle, Names};

// ---------------------------------------------------------------------------
// gaps, stats, result
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Class {
    /// The language needs a new seam to carry this construct.
    ExtensionSeam,
    /// The construct is outside the converter's scope by design.
    NonGoal,
    /// A converter-side approximation of CS semantics.
    ConverterIdiom,
}

impl Class {
    pub fn label(&self) -> &'static str {
        match self {
            Class::ExtensionSeam => "extension-seam candidate",
            Class::NonGoal => "permanent non-goal",
            Class::ConverterIdiom => "converter idiom",
        }
    }
}

#[derive(Clone)]
pub struct GapRow {
    pub construct: String,
    pub classification: Class,
    pub note: String,
    /// Every affected id, uncapped (the authoritative count).
    pub ids: Vec<String>,
}

#[derive(Default)]
pub struct Gaps {
    pub rows: Vec<GapRow>,
    index: BTreeMap<String, usize>,
}

impl Gaps {
    pub fn row(&mut self, construct: &str, class: Class, note: &str) -> &mut GapRow {
        let i = match self.index.get(construct) {
            Some(&i) => i,
            None => {
                let i = self.rows.len();
                self.rows.push(GapRow {
                    construct: construct.to_string(),
                    classification: class,
                    note: note.to_string(),
                    ids: Vec::new(),
                });
                self.index.insert(construct.to_string(), i);
                i
            }
        };
        &mut self.rows[i]
    }

    pub fn hit(&mut self, construct: &str, class: Class, note: &str, id: &str) {
        let i = match self.index.get(construct) {
            Some(&i) => i,
            None => {
                self.row(construct, class, note);
                self.index[construct]
            }
        };
        self.rows[i].ids.push(id.to_string());
    }

    pub fn count(&self, construct: &str) -> usize {
        self.index
            .get(construct)
            .map(|&i| self.rows[i].ids.len())
            .unwrap_or(0)
    }
}

#[derive(Default, Clone)]
pub struct Stats {
    pub elements_in: usize,
    pub aspect_elements: usize,
    pub cards_out: usize,
    pub recipes_in: usize,
    pub recipes_out: usize,
    pub verbs_out: usize,
    pub decks_out: usize,
    pub endings_out: usize,
    pub aspect_keys: usize,
    pub output_lines: usize,
    /// Emitted slot lines across all recipes (source + verb + synthesized).
    pub slots_out: usize,
    /// Dead census: emitted recipes on uncommittable, non-ambient verbs with
    /// no incoming `start` or `continue`, and the verbs whose every recipe is
    /// dead.
    pub dead_recipes: usize,
    pub dead_recipes_referenced: usize,
    pub dead_verbs: usize,
}

pub struct Conversion {
    pub sw: String,
    pub stats: Stats,
    pub gaps: Gaps,
    /// Files that failed to load or were excluded, (kind, name, reason).
    pub skipped: Vec<(String, String, String)>,
    /// Naming collisions, (CS id, assigned name).
    pub collisions: Vec<(String, String)>,
    /// The dead census detail: (verb id, dead recipe count) per verb whose
    /// every recipe is unreachable, in verb order.
    pub dead_verb_detail: Vec<(String, usize)>,
}

// ---------------------------------------------------------------------------
// source-data accessors (the CS JSON dialect, defensively read)
// ---------------------------------------------------------------------------

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k).and_then(|x| x.as_str())
}

fn truthy(v: &Value, k: &str) -> bool {
    match v.get(k) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(st)) => st.eq_ignore_ascii_case("true"),
        Some(Value::Number(n)) => n.as_i64() != Some(0),
        _ => false,
    }
}

fn num(v: &Value, k: &str) -> Option<i64> {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Some(Value::String(st)) => st.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// An object of CS `aspect: level` pairs, sorted by key, numeric only.
fn pairs(v: &Value, k: &str) -> Vec<(String, i64)> {
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

fn str_list(v: &Value, k: &str) -> Vec<String> {
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
fn entries(v: &Value, k: &str) -> Vec<Value> {
    match v.get(k) {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    }
}

fn entry_id(e: &Value) -> Option<String> {
    e.as_str()
        .map(String::from)
        .or_else(|| s(e, "id").map(String::from))
}

fn escape_sw(text: &str) -> String {
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
const KIND_ORDER: &[(&str, &str)] = &[
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
const START_IDS: &[&str] = &["health", "reason", "passion", "funds", "introjob"];

/// The player-facing verbs, in table order.
const PLAYER_VERBS: &[&str] = &["work", "dream", "study", "talk", "explore"];

/// The ambient verb.
const TIME_VERB: &str = "time";

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
const GUARDED_SLOTS: &[(&str, &str, &str, &str)] = &[(
    "cultfoundation",
    "doctrine",
    "lore",
    "Choose Founding Principles: bind a Lore card as the cult's Doctrine.",
)];

/// The cap on synthesized requirement slots per recipe; anything beyond is
/// gap-reported with its keys.
const MAX_SYNTH_SLOTS: usize = 6;

/// The actions whose CS verb declares its own drop slot (`verbs` table, the
/// singular `slot` field); mapped onto every recipe of that action.
fn verb_slot_of<'a>(d: &'a CsData, action: &str) -> Option<&'a Value> {
    d.verbs.get(action).and_then(|v| v.get("slot"))
}

// ---------------------------------------------------------------------------
// the converter
// ---------------------------------------------------------------------------

pub fn convert(d: &CsData) -> Conversion {
    let mut c = Converter {
        d,
        names: Names::new(),
        gaps: Gaps::default(),
        stats: Stats {
            elements_in: d.elements.len(),
            recipes_in: d.recipes.len(),
            ..Stats::default()
        },
        card_ids: Vec::new(),
        registry: BTreeSet::new(),
        read_groups: BTreeSet::new(),
        aspect_ids: BTreeSet::new(),
        induce_ids: Vec::new(),
        ending_variants: BTreeSet::new(),
        host: BTreeMap::new(),
        internal_deck: BTreeMap::new(),
        roll_counter: 0,
        preds: Vec::new(),
        pred_names: BTreeMap::new(),
        verb_preds: BTreeMap::new(),
        slot_donors: BTreeMap::new(),
        merged_element_slots: BTreeSet::new(),
        merge_preds: BTreeMap::new(),
        plans: BTreeMap::new(),
        levers_used: BTreeMap::new(),
        started: BTreeSet::new(),
        continued: BTreeSet::new(),
        dead_verb_detail: Vec::new(),
        alt_targets_with_warmup: BTreeSet::new(),
    };
    c.inventory();
    c.name_everything();
    let sw = c.emit();
    c.stats.output_lines = sw.lines().count();
    c.census();
    Conversion {
        sw,
        stats: c.stats.clone(),
        gaps: c.gaps,
        skipped: d.skipped.clone(),
        collisions: c.names.collisions.clone(),
        dead_verb_detail: c.dead_verb_detail,
    }
}

/// One planned slot of one recipe: the emitted name, its kind (which fixes
/// the range, flags, and filter at emission), and for a synthesized slot the
/// requirement key it carries (the name alone can lose the key to dedupe
/// suffixes).
#[derive(Clone)]
struct PlannedSlot {
    name: String,
    kind: SlotKind,
    key: Option<String>,
    /// The precomputed emission line for a merged element slot (its filter
    /// terms are fixed at plan time); `None` for every other kind.
    line: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SlotKind {
    /// A CS recipe slot: `0..1` plus the CS greedy/consuming flags.
    Source,
    /// The owning verb's own slot, name-qualified: `0..1 open`.
    Verb,
    /// A requirement key the recipe's own effects pay for: `1..N consuming`.
    Cost,
    /// A synthesized requirement slot: `0..1 open <term>`.
    Req,
    /// An element slot merged from a referenced card's CS `slots`:
    /// `0..1 [consuming] open <term>` (CS opens it as a dependent threshold
    /// of the situation when the card is present).
    Merged,
}

/// The slot plan of one recipe: mapped source slots, the owning verb's own
/// slot, cost slots (requirement key paid by a paired negative effect), and
/// synthesized requirement slots — every requirement key is bindable.
#[derive(Default, Clone)]
struct RecipePlan {
    slots: Vec<PlannedSlot>,
    /// Requirement key -> cost slot name (a `1..N consuming` slot).
    costs: BTreeMap<String, String>,
}

impl RecipePlan {
    /// The planned slot names. Every planned slot is a `when` carrier: CS
    /// sums the whole situation pool, so a card bound in ANY slot contributes
    /// all its aspects to the requirement check (at commit selection the
    /// interior is empty and the submitted bindings are visible only through
    /// the slot scopes).
    fn carriers(&self) -> Vec<String> {
        self.slots.iter().map(|s| s.name.clone()).collect()
    }
}

struct Converter<'a> {
    d: &'a CsData,
    names: Names,
    gaps: Gaps,
    stats: Stats,
    /// Sorted non-aspect element ids (the cards).
    card_ids: Vec<String>,
    /// Every aspect key referenced anywhere.
    registry: BTreeSet<String>,
    /// uniquenessgroup ids read as aspects somewhere (the CS auto-aspect face).
    read_groups: BTreeSet<String>,
    /// Element ids with `isAspect`.
    aspect_ids: BTreeSet<String>,
    /// Element ids carrying `induces`, sorted.
    induce_ids: Vec<String>,
    /// (ending id, flavour) variants referenced by recipes.
    ending_variants: BTreeSet<(String, String)>,
    /// Recipe verb assignment: CS recipe id -> emitted verb id.
    host: BTreeMap<String, String>,
    /// CS recipe id -> emitted internal deck name.
    internal_deck: BTreeMap<String, String>,
    roll_counter: usize,
    /// Slot-filter predicates to emit, (name, body), in emission order.
    preds: Vec<(String, String)>,
    /// (CS recipe id, raw slot id) -> predicate name for its disjunctive
    /// slot filter.
    pred_names: BTreeMap<(String, String), String>,
    /// CS action id -> predicate name for the verb slot's disjunctive filter.
    verb_preds: BTreeMap<String, String>,
    /// Non-aspect elements carrying CS element `slots`, indexed by their own
    /// id and by each of their aspect keys — the admission keys a recipe's
    /// requirements and slot filters can present.
    slot_donors: BTreeMap<String, BTreeSet<String>>,
    /// (element id, slot id) pairs merged into some recipe's slot block.
    merged_element_slots: BTreeSet<(String, String)>,
    /// (recipe id, donor element id, donor slot id) -> predicate name for a
    /// merged slot's disjunctive filter.
    merge_preds: BTreeMap<(String, String, String), String>,
    /// Per-recipe slot plans (built in the naming pass).
    plans: BTreeMap<String, RecipePlan>,
    /// Lever id -> max positive effect count over its sites (the resolver's
    /// spawn count).
    levers_used: BTreeMap<String, i64>,
    /// (verb id, CS recipe id) targets of emitted `start` effects (the census).
    started: BTreeSet<(String, String)>,
    /// (verb id, CS recipe id) targets of emitted `continue` effects (the
    /// census).
    continued: BTreeSet<(String, String)>,
    /// Per dead verb: (verb id, dead recipe count).
    dead_verb_detail: Vec<(String, usize)>,
    /// CS recipe ids targeted by non-additional alt with warmup > 0 (they need an instant override recipe).
    alt_targets_with_warmup: BTreeSet<String>,
}

impl<'a> Converter<'a> {
    fn is_aspect(&self, id: &str) -> bool {
        self.aspect_ids.contains(id)
    }

    fn card_name(&self, id: &str) -> Option<String> {
        self.names.card.get(id).cloned()
    }

    fn key(&self, id: &str) -> String {
        self.names
            .key
            .get(id)
            .cloned()
            .unwrap_or_else(|| mangle(id))
    }

    fn aspects_of(&self, id: &str) -> Vec<(String, i64)> {
        self.d
            .element(id)
            .map(|e| pairs(e, "aspects"))
            .unwrap_or_default()
    }

    fn craftable(&self, r: &Value) -> bool {
        // CS craftable defaults to false (Recipe.cs).
        match r.get("craftable") {
            Some(Value::Bool(b)) => *b,
            Some(Value::String(st)) => st.eq_ignore_ascii_case("true"),
            Some(Value::Number(n)) => n.as_i64() != Some(0),
            _ => false,
        }
    }

    /// The emitted verb of a recipe: player verbs carry craftable recipes on
    /// the commit face and non-craftable recipes on a shadow chain verb; the
    /// ambient verb keeps its warmup recipes and hands instant helpers to its
    /// shadow; every other action id is its own popup verb.
    fn verb_of(&self, action: &str, _craftable: bool, warmup: i64) -> Option<String> {
        if action == "(none)" {
            return None;
        }
        Some(if PLAYER_VERBS.contains(&action) {
            action.to_string()
        } else if action == TIME_VERB {
            if warmup > 0 {
                TIME_VERB.to_string()
            } else {
                "time_chains".to_string()
            }
        } else {
            action.to_string()
        })
    }

    /// The CS action id of a recipe (the context a linked continuation runs
    /// in): same action id means the continuation runs in the parent
    /// situation; a different one genuinely moves to another verb.
    fn action_of(&self, rid: &str) -> String {
        self.d
            .recipe(rid)
            .and_then(|r| {
                s(r, "actionid")
                    .or_else(|| s(r, "actionId"))
                    .map(String::from)
            })
            .or_else(|| self.host.get(rid).cloned())
            .unwrap_or_else(|| "(none)".to_string())
    }

    /// The positive `required` arms of one CS slot (CS matches any of them),
    /// in sorted order. `essential` keys are conjunction terms, not arms
    /// (CS per-card evaluation: forbidden rejects, essential ANDs, required
    /// ORs, empty required accepts).
    fn slot_required_arms(sl: &Value) -> Vec<(String, i64)> {
        pairs(sl, "required")
    }

    /// One slot-filter term for a requirement arm: element-presence filters by
    /// `name:`, aspect thresholds by `aspect:k[>=v]`.
    fn arm_term(&self, k: &str, v: i64) -> String {
        if !self.is_aspect(k) && self.d.element(k).is_some() {
            return format!("name:{}", self.card_name(k).unwrap_or_else(|| mangle(k)));
        }
        let kk = self.key(k);
        if v > 1 {
            format!("aspect:{kk}>={v}")
        } else {
            format!("aspect:{kk}")
        }
    }

    /// The filter terms of one CS slot: the required arms (a single arm as a
    /// direct term, several as a derived-predicate call), then the `essential`
    /// conjunction terms, then the negated `forbidden` terms.
    fn slot_filter_terms(&self, rid: &str, sl: &Value) -> Vec<String> {
        let mut terms: Vec<String> = Vec::new();
        let raw = s(sl, "id").unwrap_or("slot");
        let arms = Self::slot_required_arms(sl);
        if arms.len() == 1 {
            let (k, v) = &arms[0];
            terms.push(self.arm_term(k, *v));
        } else if arms.len() > 1 {
            if let Some(pred) = self.pred_names.get(&(rid.to_string(), raw.to_string())) {
                terms.push(format!("{pred}()"));
            }
        }
        for (k, v) in pairs(sl, "essential") {
            terms.push(self.arm_term(&k, v));
        }
        for (k, _) in pairs(sl, "forbidden") {
            terms.push(format!("-aspect:{}", self.key(&k)));
        }
        terms
    }

    /// The filter terms of a verb's own slot, sharing the recipe-slot shape
    /// (its disjunctive filter is the action's shared predicate).
    fn verb_slot_filter_terms(&self, action: &str, sl: &Value) -> Vec<String> {
        let mut terms: Vec<String> = Vec::new();
        let arms = Self::slot_required_arms(sl);
        if arms.len() == 1 {
            let (k, v) = &arms[0];
            terms.push(self.arm_term(k, *v));
        } else if arms.len() > 1 {
            if let Some(pred) = self.verb_preds.get(action) {
                terms.push(format!("{pred}()"));
            }
        }
        for (k, v) in pairs(sl, "essential") {
            terms.push(self.arm_term(&k, v));
        }
        for (k, _) in pairs(sl, "forbidden") {
            terms.push(format!("-aspect:{}", self.key(&k)));
        }
        terms
    }

    // -- pass 1: inventory --------------------------------------------------

    fn inventory(&mut self) {
        for (id, e) in &self.d.elements {
            if truthy(e, "isAspect") {
                self.aspect_ids.insert(id.clone());
            }
            if e.get("induces").is_some() {
                self.induce_ids.push(id.clone());
            }
            for (k, _) in pairs(e, "aspects") {
                self.registry.insert(k);
            }
            for sl in e
                .get("slots")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
            {
                for dk in ["required", "forbidden", "essential"] {
                    for (k, _) in pairs(sl, dk) {
                        self.registry.insert(k.clone());
                        self.note_group_read(&k);
                    }
                }
            }
            if let Some(o) = e.get("xtriggers").and_then(|x| x.as_object()) {
                for k in o.keys() {
                    self.registry.insert(k.clone());
                }
            }
            if let Some(g) = s(e, "uniquenessgroup") {
                self.registry.insert(g.to_string());
            }
            // Element-slot donors: only cards can sit in a situation, so only
            // non-aspect elements with CS `slots` can hand their slots to a
            // recipe; indexed by their own id and every aspect key.
            let has_slots = e
                .get("slots")
                .and_then(|x| x.as_array())
                .is_some_and(|a| !a.is_empty());
            if has_slots && !self.is_aspect(&id) {
                let mut keys: BTreeSet<String> = BTreeSet::from([(id.clone())]);
                for (k, _) in pairs(e, "aspects") {
                    keys.insert(k);
                }
                for k in keys {
                    self.slot_donors.entry(k).or_default().insert(id.clone());
                }
            }
        }

        for (_, r) in &self.d.recipes {
            for dk in ["requirements", "tablereqs", "extantreqs"] {
                for (k, _) in pairs(r, dk) {
                    self.registry.insert(k.clone());
                    self.note_group_read(&k);
                }
            }
            // Lever effect sites feed the resolver verb (one legacy-gated
            // recipe pair per lever, `start`-routed from the site).
            for (k, v) in pairs(r, "effects") {
                if let Some(lever) = k.strip_prefix("lever_") {
                    let lever = lever.to_lowercase();
                    let e = self.levers_used.entry(lever).or_insert(0);
                    if *e < v {
                        *e = v;
                    }
                }
            }
            for sl in r
                .get("slots")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
            {
                for dk in ["required", "forbidden", "essential"] {
                    for (k, _) in pairs(sl, dk) {
                        self.registry.insert(k.clone());
                        self.note_group_read(&k);
                    }
                }
            }
            for m in r
                .get("mutations")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
            {
                for dk in ["filter", "mutate"] {
                    if let Some(k) = s(m, dk) {
                        self.registry.insert(k.to_string());
                        self.note_group_read(k);
                    }
                }
            }
            for ch in entries(r, "alt")
                .into_iter()
                .chain(entries(r, "linked"))
                .filter_map(|e| e.get("challenges").cloned())
            {
                if let Some(o) = ch.as_object() {
                    for k in o.keys() {
                        self.registry.insert(k.clone());
                    }
                }
            }
        }

        // The verb slot tables: their filter keys read as aspects wherever a
        // mapped verb slot carries them.
        for v in self.d.verbs.values() {
            if let Some(sl) = v.get("slot") {
                for dk in ["required", "forbidden", "essential"] {
                    for (k, _) in pairs(sl, dk) {
                        self.registry.insert(k.clone());
                        self.note_group_read(&k);
                    }
                }
            }
        }
        self.induce_ids.sort();

        // Verb assignment and ending-variant scan.
        let mut parent_actions: BTreeMap<String, String> = BTreeMap::new();
        for (_pid, pr) in &self.d.recipes {
            let pact = s(pr, "actionid").or_else(|| s(pr, "actionId"));
            if let Some(act) = pact {
                for k in ["linked", "alt", "inductions"] {
                    for e in entries(pr, k) {
                        if let Some(cid) = entry_id(&e) {
                            parent_actions.entry(cid).or_insert_with(|| act.to_string());
                        }
                    }
                }
            }
        }

        for (_pid, pr) in &self.d.recipes {
            for e in entries(pr, "alt") {
                if !truthy(&e, "additional") {
                    if let Some(cid) = entry_id(&e) {
                        let w = self.d.recipe(&cid).and_then(|r| num(r, "warmup")).unwrap_or(0);
                        if w > 0 {
                            self.alt_targets_with_warmup.insert(cid);
                        }
                    }
                }
            }
        }

        for (rid, r) in &self.d.recipes {
            let action_str = s(r, "actionid")
                .or_else(|| s(r, "actionId"))
                .map(String::from)
                .or_else(|| parent_actions.get(rid).cloned())
                .unwrap_or_else(|| "(none)".to_string());
            let action = action_str.as_str();
            let craftable = self.craftable(r);
            let warmup = num(r, "warmup").unwrap_or(0);
            match self.verb_of(action, craftable, warmup) {
                Some(v) => {
                    self.host.insert(rid.clone(), v);
                }
                None => {
                    self.gaps.hit(
                        "recipe without an actionId",
                        Class::NonGoal,
                        "no verb site exists for it; the recipe is not emitted",
                        rid,
                    );
                }
            }
            if let Some(end) = s(r, "ending") {
                let base = self
                    .d
                    .endings
                    .get(end)
                    .and_then(|e| s(e, "flavour"))
                    .unwrap_or("Melancholy")
                    .to_string();
                if let Some(f) = s(r, "signalEndingFlavour") {
                    if f != base {
                        self.ending_variants
                            .insert((end.to_string(), f.to_string()));
                    }
                }
            }
        }
    }

    fn note_group_read(&mut self, k: &str) {
        let is_group = self
            .d
            .elements
            .values()
            .any(|e| s(e, "uniquenessgroup") == Some(k));
        if is_group {
            self.read_groups.insert(k.to_string());
        }
    }

    // -- pass 2: naming -----------------------------------------------------

    fn name_everything(&mut self) {
        for v in PLAYER_VERBS {
            let name = self.names.reserve_id(v);
            self.names.verb.insert(v.to_string(), name);
        }
        let time_name = self.names.reserve_id(TIME_VERB);
        self.names.verb.insert(TIME_VERB.to_string(), time_name);
        for v in PLAYER_VERBS {
            let raw = format!("{v}_chains");
            let name = self.names.reserve_id(&raw);
            self.names.verb.insert(raw, name);
        }
        {
            let name = self.names.reserve_id("time_chains");
            self.names.verb.insert("time_chains".to_string(), name);
        }

        // Popup verbs: every other assigned host verb.
        let popups: BTreeSet<String> = self
            .host
            .values()
            .filter(|v| !self.names.verb.contains_key(v.as_str()))
            .cloned()
            .collect();
        for p in popups {
            let name = self.names.reserve_id(&p);
            self.names.verb.insert(p, name);
        }

        // The cross-run lever resolver verb and its recipe locals (spec.md
        // §7.8): reserved in content order so emission and reference agree.
        if !self.levers_used.is_empty() {
            let name = self.names.reserve_id("lever_resolver");
            self.names.verb.insert("lever_resolver".to_string(), name);
            for lever in self.levers_used.keys() {
                for suffix in ["routed", "default"] {
                    let raw = format!("lever_{lever}_{suffix}");
                    let name = self.names.reserve_id(&raw);
                    self.names.recipe.insert(("lever_resolver".to_string(), raw), name);
                }
            }
        }

        for id in self.d.decks.keys() {
            let raw = format!("deck_{id}");
            let name = self.names.reserve_id(&raw);
            self.names.deck.insert(id.clone(), name);
        }

        for id in self.d.endings.keys() {
            let name = self.names.reserve_id(id);
            self.names.ending.insert(id.clone(), name);
        }
        for (end, flavour) in self.ending_variants.clone() {
            let raw = format!("{end}_{flavour}");
            let name = self.names.reserve_id(&raw);
            self.names.ending_variant.insert((end, flavour), name);
        }

        // Uniquenessgroup names, only where the group is read as an aspect.
        for g in self.read_groups.clone() {
            let name = self.names.reserve_id(&g);
            self.names.group.insert(g, name);
        }

        for id in self.d.elements.keys() {
            self.names.reserve_key(id);
        }
        for k in &self.registry {
            if !self.d.elements.contains_key(k.as_str()) {
                self.names.reserve_key(k);
            }
        }

        for id in self.d.elements.keys() {
            if self.is_aspect(id) {
                continue;
            }
            let name = self.names.reserve_id(id);
            self.names.card.insert(id.clone(), name);
        }
        self.card_ids = self
            .d
            .elements
            .keys()
            .filter(|id| !self.is_aspect(id))
            .cloned()
            .collect();

        // Recipe local names, in per-verb content order (priority = index).
        for (verb, recipes) in self.by_verb() {
            for rid in recipes {
                let local = self.names.reserve_id(&rid);
                self.names.recipe.insert((verb.clone(), rid), local);
            }
        }

        // Slot-filter predicates and per-recipe slot plans. A slot whose
        // positive `required` arms exceed one matches any arm (CS any-of),
        // which the query grammar carries as a unary derived predicate over
        // the candidate card (`essential` keys are conjunct terms, not
        // predicate arms). Predicates share the one global namespace, so
        // their ids reserve here: one per action's verb-slot filter, then the
        // recipe slot filters in per-verb content order.
        for action in PLAYER_VERBS
            .iter()
            .map(|v| v.to_string())
            .chain([TIME_VERB.to_string()])
        {
            let Some(sl) = verb_slot_of(self.d, &action) else {
                continue;
            };
            let arms = pairs(sl, "required");
            if arms.len() <= 1 {
                continue;
            }
            let body = arms
                .iter()
                .map(|(k, v)| {
                    let kk = self.key(k);
                    if *v > 1 {
                        format!("p.{kk} >= {v}")
                    } else {
                        format!("p.{kk} >= 1")
                    }
                })
                .collect::<Vec<_>>()
                .join(" or ");
            let name = self.names.reserve_id(&format!("fits.{action}_slot"));
            self.verb_preds.insert(action.clone(), name.clone());
            self.preds.push((name, body));
        }
        for (_verb, recipes) in self.by_verb() {
            for rid in recipes {
                let Some(r) = self.d.recipe(&rid).cloned() else {
                    continue;
                };
                for sl in r
                    .get("slots")
                    .and_then(|x| x.as_array())
                    .into_iter()
                    .flatten()
                {
                    let arms = pairs(sl, "required");
                    if arms.len() <= 1 {
                        continue;
                    }
                    let raw = s(sl, "id").unwrap_or("slot");
                    let pred_body = arms
                        .iter()
                        .map(|(k, v)| {
                            let kk = self.key(k);
                            if *v > 1 {
                                format!("p.{kk} >= {v}")
                            } else {
                                format!("p.{kk} >= 1")
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" or ");
                    let name = self.names.reserve_id(&format!("fits.{rid}.{raw}"));
                    self.pred_names
                        .insert((rid.clone(), raw.to_string()), name.clone());
                    self.preds.push((name, pred_body));
                }
                let plan = self.plan_recipe(&rid, &r);
                self.plans.insert(rid, plan);
            }
        }

        self.stats.aspect_elements = self.aspect_ids.len();
        self.stats.aspect_keys = self.names.key.len();
    }

    /// The slot plan of one recipe: its source slots, the owning verb's own
    /// slot (name-qualified `<action>_<slotid>`), a consuming cost slot per
    /// requirement key the recipe's own effects pay for, and a synthesized
    /// filtered slot per requirement key no mapped source slot admits — every
    /// requirement is bindable.
    fn plan_recipe(&mut self, rid: &str, r: &Value) -> RecipePlan {
        let action = self.action_of(rid);
        let mut plan = RecipePlan {
            slots: Vec::new(),
            costs: BTreeMap::new(),
        };
        let mut taken: BTreeSet<String> = BTreeSet::new();

        // Source slots. A key is "covered" when a source slot's positive
        // filter admits it (required any-of or essential); covered keys need
        // no synthesized slot.
        let source: Vec<Value> = r
            .get("slots")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        let mut covered: BTreeSet<String> = BTreeSet::new();
        for sl in &source {
            // A key is covered when a source slot's positive filter admits it
            // (required any-of or essential); covered keys need no
            // synthesized slot.
            let mut admitted: Vec<String> = pairs(sl, "required").into_iter().map(|(k, _)| k).collect();
            admitted.extend(pairs(sl, "essential").into_iter().map(|(k, _)| k));
            for k in admitted {
                covered.insert(k);
            }
            let raw = s(sl, "id").unwrap_or("slot");
            let base = {
                let b = mangle(raw);
                if b.is_empty() {
                    "slot".to_string()
                } else {
                    b
                }
            };
            let mut name = base.clone();
            let mut i = 2;
            while !taken.insert(name.clone()) {
                name = format!("{base}_{i}");
                i += 1;
            }
            plan.slots.push(PlannedSlot {
                name,
                kind: SlotKind::Source,
                key: None,
                line: None,
            });
        }

        // The owning verb's own slot, mapped onto every recipe of the action.
        if let Some(sl) = verb_slot_of(self.d, &action) {
            let raw = s(sl, "id").unwrap_or("slot");
            let base = mangle(&format!("{action}_{raw}"));
            let mut name = base.clone();
            let mut i = 2;
            while !taken.insert(name.clone()) {
                name = format!("{base}_{i}");
                i += 1;
            }
            plan.slots.push(PlannedSlot {
                name,
                kind: SlotKind::Verb,
                key: None,
                line: None,
            });
        }

        let reqs = pairs(r, "requirements");

        // Merged element slots. CS opens a card's element `slots` as dependent
        // threshold spheres of the situation when that card is present
        // (SituationDominion.AddDependentSpheresForToken -> Sphere.
        // GetChildSpheresSpecsToAddIfThisTokenAdded, actionid-matched against
        // the situation's verb), and their `consumes` slots burn at the
        // completion flush (ConsumingAngel.MinisterToDepartingToken) — never
        // at a book. A requirement key or slot filter key of this recipe that
        // names (or admits, through an aspect) such an element therefore
        // pulls that element's slots into the block: consuming slots become
        // `0..1 consuming`, the rest `0..1 open` with their CS filter. The
        // merged slots cover their own filter keys, so no cost or synthesized
        // slot duplicates the payment route (the bookdealer's funds Cost slot
        // is the one consume, not a drain paired with an open slot).
        let mut admission: BTreeSet<String> = reqs
            .iter()
            .filter(|(_, v)| *v > 0)
            .map(|(k, _)| k.clone())
            .collect();
        for sl in &source {
            for dk in ["required", "essential"] {
                for (k, _) in pairs(sl, dk) {
                    admission.insert(k);
                }
            }
        }
        let mut shapes: BTreeMap<
            (bool, Vec<(String, i64)>, Vec<(String, i64)>, Vec<(String, i64)>),
            String,
        > = BTreeMap::new();
        for k in &admission {
            let Some(donors) = self.slot_donors.get(k).cloned() else {
                continue;
            };
            for eid in donors {
                let Some(e) = self.d.element(&eid).cloned() else {
                    continue;
                };
                let slots: Vec<Value> = e
                    .get("slots")
                    .and_then(|x| x.as_array())
                    .cloned()
                    .unwrap_or_default();
                for sl in &slots {
                    if !Self::slot_fits_action(&action, sl) {
                        continue;
                    }
                    let raw = s(sl, "id").unwrap_or("slot").to_string();
                    let required = pairs(sl, "required");
                    let essential = pairs(sl, "essential");
                    let forbidden = pairs(sl, "forbidden");
                    let consumes = truthy(sl, "consumes");
                    self.merged_element_slots
                        .insert((eid.clone(), raw.clone()));
                    let shape = (
                        consumes,
                        required.clone(),
                        essential.clone(),
                        forbidden.clone(),
                    );
                    if shapes.contains_key(&shape) {
                        continue;
                    }
                    let base = mangle(&format!("{eid}_{raw}"));
                    let mut name = base.clone();
                    let mut i = 2;
                    while !taken.insert(name.clone()) {
                        name = format!("{base}_{i}");
                        i += 1;
                    }
                    let mut terms: Vec<String> = Vec::new();
                    if required.len() == 1 {
                        let (k, v) = &required[0];
                        terms.push(self.arm_term(k, *v));
                    } else if required.len() > 1 {
                        let key = (rid.to_string(), eid.clone(), raw.clone());
                        let pred = match self.merge_preds.get(&key) {
                            Some(p) => p.clone(),
                            None => {
                                let body = required
                                    .iter()
                                    .map(|(k, v)| {
                                        if *v > 1 {
                                            format!("p.{} >= {v}", self.key(k))
                                        } else {
                                            format!("p.{} >= 1", self.key(k))
                                        }
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" or ");
                                let name = self
                                    .names
                                    .reserve_id(&format!("fits.{rid}.{eid}.{raw}"));
                                self.merge_preds.insert(key.clone(), name.clone());
                                self.preds.push((name.clone(), body));
                                name
                            }
                        };
                        terms.push(format!("{pred}()"));
                    }
                    for (k, v) in &essential {
                        terms.push(self.arm_term(k, *v));
                    }
                    for (k, _) in &forbidden {
                        terms.push(format!("-aspect:{}", self.key(k)));
                    }
                    let mut line = format!("{name}: 0..1");
                    if consumes {
                        line.push_str(" consuming");
                    }
                    line.push_str(" open");
                    if !terms.is_empty() {
                        line.push(' ');
                        line.push_str(&terms.join(" "));
                    }
                    shapes.insert(shape, name.clone());
                    for (ck, _) in required.iter().chain(essential.iter()) {
                        covered.insert(ck.clone());
                    }
                    plan.slots.push(PlannedSlot {
                        name,
                        kind: SlotKind::Merged,
                        key: None,
                        line: Some(line),
                    });
                }
            }
        }

        // Cost slots: a requirement key the recipe's own effects pay for
        // (positive requirement, paired negative effect) becomes one
        // `1..N consuming` slot when no source slot already binds the payer;
        // the paired negative effect drains the interior at the effects pass
        // (effects run before consuming settlement, so the pay happens once).
        let effects: std::collections::BTreeMap<String, i64> = pairs(r, "effects").into_iter().collect();
        let mut cost_keys: Vec<(String, i64)> = Vec::new();
        for (k, v) in &reqs {
            if *v <= 0 || covered.contains(k) {
                continue;
            }
            if let Some(n) = effects.get(k) {
                if -*n >= 1 && -*n <= 12 {
                    cost_keys.push((k.clone(), -*n));
                }
            }
        }
        cost_keys.sort();
        for (k, _) in cost_keys {
            let base = mangle(&format!("cost_{k}"));
            let mut name = base.clone();
            let mut i = 2;
            while !taken.insert(name.clone()) {
                name = format!("{base}_{i}");
                i += 1;
            }
            plan.costs.insert(k.clone(), name.clone());
            plan.slots.push(PlannedSlot {
                name,
                kind: SlotKind::Cost,
                key: Some(k.clone()),
                line: None,
            });
        }

        // Synthesized requirement slots: every remaining positive requirement
        // key gets `req_<key>: 0..1 open <term>` so the card its gate reads is
        // bindable. Keys beyond the cap are gap-reported.
        let mut synth = 0usize;
        for (k, v) in &reqs {
            if *v <= 0 || covered.contains(k) || plan.costs.contains_key(k) {
                continue;
            }
            if synth >= MAX_SYNTH_SLOTS {
                self.gaps.hit(
                    "requirement keys beyond the synthesized-slot cap",
                    Class::ConverterIdiom,
                    &format!(
                        "the recipe needs more bindable requirement routes than the {MAX_SYNTH_SLOTS}-slot sanity cap; the excess keys gate through the interior sum alone"
                    ),
                    &format!("{rid}:{k}"),
                );
                continue;
            }
            synth += 1;
            let base = mangle(&format!("req_{k}"));
            let mut name = base.clone();
            let mut i = 2;
            while !taken.insert(name.clone()) {
                name = format!("{base}_{i}");
                i += 1;
            }
            plan.slots.push(PlannedSlot {
                name,
                kind: SlotKind::Req,
                key: Some(k.clone()),
                line: None,
            });
        }
        plan
    }

    /// Whether a CS element slot opens for a situation of `action` (CS
    /// SituationDominion -> Sphere.GetChildSpheresSpecsToAddIfThisTokenAdded:
    /// `WildcardMatchId(verbId, cs.ActionId)` — equality, a trailing-`*`
    /// prefix match, or an empty actionid).
    fn slot_fits_action(action: &str, sl: &Value) -> bool {
        let Some(a) = s(sl, "actionid").or_else(|| s(sl, "actionId")) else {
            return true;
        };
        if a.is_empty() {
            return true;
        }
        if a.eq_ignore_ascii_case(action) {
            return true;
        }
        match a.strip_suffix('*') {
            Some(prefix) => action
                .to_lowercase()
                .starts_with(&prefix.to_lowercase()),
            None => false,
        }
    }

    /// Recipes grouped per emitted verb, CS content order preserved (the
    /// recipe `priority` is the per-verb content index: CS's own first-match
    /// order).
    fn by_verb(&self) -> Vec<(String, Vec<String>)> {
        let mut by_verb: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (rid, _) in &self.d.recipes {
            if let Some(h) = self.host.get(rid) {
                by_verb.entry(h.clone()).or_default().push(rid.clone());
            }
        }
        let mut order: Vec<String> = PLAYER_VERBS
            .iter()
            .map(|v| v.to_string())
            .chain([TIME_VERB.to_string()])
            .collect();
        for v in PLAYER_VERBS {
            order.push(format!("{v}_chains"));
        }
        order.push("time_chains".to_string());
        for v in by_verb.keys() {
            if !order.contains(v) {
                order.push(v.clone());
            }
        }
        order
            .into_iter()
            .filter_map(|v| by_verb.remove_entry(&v).map(|(k, rs)| (k, rs)))
            .collect()
    }

    // -- pass 3: emission ---------------------------------------------------

    fn emit(&mut self) -> String {
        let mut out: Vec<String> = Vec::new();
        out.push(
            "# cultist-simulator.sw — converted from the Cultist Simulator core content tree"
                .into(),
        );
        out.push("# by cs2sw: a mechanical, table-driven map of elements, recipes, decks,".into());
        out.push("# endings, and levers onto the .sw grammar. Generated file; edit the".into());
        out.push("# converter, not this text.".into());
        out.push(String::new());
        out.push("game \"Cultist Simulator\"".into());
        out.push(format!("  seed {SEED}"));
        out.push("  turn 1m".into());
        out.push("  home table".into());
        out.push(String::new());
        out.push("zones".into());
        out.push("  table".into());
        out.push(String::new());

        out.push("aspects".into());
        let mut seen_keys = BTreeSet::new();
        for id in self.d.elements.keys() {
            let k = self.key(id);
            if seen_keys.insert(k.clone()) {
                out.push(format!("  {k}: 0"));
            }
        }
        for k in &self.registry {
            if self.d.elements.contains_key(k.as_str()) {
                continue;
            }
            let kk = self.key(k);
            if seen_keys.insert(kk.clone()) {
                out.push(format!("  {kk}: 0"));
            }
        }
        out.push(String::new());

        self.emit_predicates(&mut out);
        self.emit_cards(&mut out);
        self.emit_decks(&mut out);
        self.emit_endings(&mut out);
        self.emit_legacies(&mut out);
        self.emit_verbs(&mut out);
        out.join("\n") + "\n"
    }

    /// The lever keys the resolver verb routes on (spec.md §7.8): one legacy
    /// key per CS lever with effect sites, declared at 0 — a single run
    /// routes every lever through the default arm; a host folding incoming
    /// legacy flips the gate onto the routed arm.
    fn emit_legacies(&mut self, out: &mut Vec<String>) {
        if self.levers_used.is_empty() {
            return;
        }
        out.push("legacies".into());
        for lever in self.levers_used.keys() {
            out.push(format!("  {}: 0", mangle(lever)));
        }
        out.push(String::new());
    }

    /// The derived predicates the disjunctive slot filters call (language.md
    /// §2.5); emitted beside the cards, before any use.
    fn emit_predicates(&mut self, out: &mut Vec<String>) {
        if self.preds.is_empty() {
            return;
        }
        out.push("predicates".into());
        for (name, body) in self.preds.clone() {
            out.push(format!("  predicate {name}(p) = {body}"));
        }
        out.push(String::new());
    }

    fn kind_of(&self, id: &str, e: &Value) -> &'static str {
        let own = pairs(e, "aspects");
        for (marker, kind) in KIND_ORDER {
            if *marker == id || own.iter().any(|(k, _)| k == *marker) {
                return kind;
            }
        }
        "state"
    }

    fn uniqueness(&self, e: &Value) -> String {
        if let Some(g) = s(e, "uniquenessgroup") {
            let gname = self
                .names
                .group
                .get(g)
                .cloned()
                .unwrap_or_else(|| mangle(g));
            format!("group {gname}")
        } else if truthy(e, "unique") {
            "unique".to_string()
        } else {
            "stackable".to_string()
        }
    }

    /// A card to mint or swap to: a resolvable, non-aspect element.
    fn target_card(&self, id: &str) -> Option<String> {
        if self.is_aspect(id) {
            return None;
        }
        self.card_name(id)
    }

    fn roll_bind(&mut self) -> String {
        self.roll_counter += 1;
        format!("roll{}", self.roll_counter)
    }

    fn emit_cards(&mut self, out: &mut Vec<String>) {
        out.push("cards".into());
        for id in self.card_ids.clone() {
            let Some(e) = self.d.element(&id).cloned() else {
                continue;
            };
            let Some(name) = self.card_name(&id) else {
                continue;
            };
            let mut lines: Vec<String> = Vec::new();
            let kind = self.kind_of(&id, &e);
            let uniq = self.uniqueness(&e);
            lines.push(format!("  card {name}"));
            lines.push(format!("    kind {kind}  {uniq}"));
            let mut pair_list: Vec<String> = vec![format!("{} 1", self.key(&id))];
            for (k, v) in self.aspects_of(&id) {
                pair_list.push(format!("{} {v}", self.key(&k)));
            }
            if let Some(g) = s(&e, "uniquenessgroup") {
                if self.read_groups.contains(g) {
                    pair_list.push(format!("{} 1", self.key(g)));
                }
            }
            lines.push(format!("    aspects {}", pair_list.join(", ")));
            if let Some(life) = num(&e, "lifetime") {
                if life > 0 {
                    lines.push(format!("    lifetime {life}m"));
                }
            }
            if START_IDS.contains(&id.as_str()) {
                lines.push("    start in table".into());
            }
            if let Some(desc) = s(&e, "description") {
                let d = escape_sw(desc);
                if !d.is_empty() {
                    lines.push(format!("    desc \"{d}\""));
                }
            }

            // Storage-only (non-consuming) element slots are reported: the
            // card keeps its contents instead of burning them. Consuming
            // element slots burn at CS's completion flush, never at a book:
            // the converter merges them into the slot block of every recipe
            // whose requirements or slot filters admit the element, and only
            // the unmergeable ones are named gaps below.
            let has_storage_slot = e
                .get("slots")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().any(|sl| !truthy(sl, "consumes")))
                .unwrap_or(false);
            if has_storage_slot {
                self.gaps.hit(
                    "non-consuming element slot",
                    Class::ConverterIdiom,
                    "storage-only element slots bind nothing in the map; where the behavior matters it is expressible via a card on complete book (consuming slots merge into referencing recipes' slot blocks)",
                    &id,
                );
            }

            // Completion books, merged per qualifier (one `on complete`
            // block per aspect key): element xtriggers, element induces, and
            // consuming element slots.
            let mut books: Vec<(Option<String>, Vec<(String, Vec<String>)>)> = Vec::new();
            let mut rule_ids: BTreeSet<String> = BTreeSet::new();
            let push_rule = |books: &mut Vec<(Option<String>, Vec<(String, Vec<String>)>)>,
                             rule_ids: &mut BTreeSet<String>,
                             qual: Option<String>,
                             rule: String,
                             body: Vec<String>| {
                let mut rule = rule;
                let mut i = 2;
                while !rule_ids.insert(rule.clone()) {
                    rule = format!("{rule}_{i}");
                    i += 1;
                }
                match books.iter_mut().find(|(q, _)| *q == qual) {
                    Some((_, rules)) => rules.push((rule, body)),
                    None => books.push((qual, vec![(rule, body)])),
                }
            };

            // Element xtriggers -> aspect-qualified completion books; the
            // catalyst aspect must be declared by the completing recipe.
            let xtrs: Vec<(String, Value)> = e
                .get("xtriggers")
                .and_then(|x| x.as_object())
                .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                .unwrap_or_default();
            for (cat, tgt) in xtrs {
                let Some(tgt_id) = tgt.as_str().map(String::from) else {
                    self.gaps.hit(
                        "xtrigger with a non-element target",
                        Class::NonGoal,
                        "the target is not an element id; no book is emitted",
                        &format!("{id}:{cat}"),
                    );
                    continue;
                };
                let Some(tname) = self.target_card(&tgt_id) else {
                    self.gaps.hit(
                        "xtrigger target is an aspect or unknown element",
                        Class::NonGoal,
                        "the replacement has no card; no book is emitted",
                        &format!("{id}:{cat} -> {tgt_id}"),
                    );
                    continue;
                };
                let rule = format!("xtr_{}_{}", name, self.key(&cat));
                let body = vec![format!("swap subject.{} to {tname}", self.key(&id))];
                push_rule(&mut books, &mut rule_ids, Some(self.key(&cat)), rule, body);
            }

            // decayTo -> an expiry book swapping to the decay target.
            if let Some(dt) = s(&e, "decayTo").map(String::from) {
                match self.target_card(&dt) {
                    Some(tname) => {
                        let rule = format!("decay_{name}");
                        lines.push("    on expire".into());
                        lines.push(format!("      rule {rule}"));
                        lines.push("        effects".into());
                        lines.push(format!(
                            "          swap subject.{} to {tname}",
                            self.key(&id)
                        ));
                    }
                    None => {
                        self.gaps.hit(
                            "decayTo target is an aspect or unknown element",
                            Class::NonGoal,
                            "the decay replacement has no card; no book is emitted",
                            &format!("{id} -> {dt}"),
                        );
                    }
                }
            }

            // Element `induces` -> an aspect-qualified completion book that
            // starts the induced recipe.
            let induces = e
                .get("induces")
                .and_then(|x| x.as_array())
                .cloned()
                .unwrap_or_default();
            for ind in &induces {
                let Some(tid) = s(ind, "id").map(String::from) else {
                    continue;
                };
                let Some((verb, local)) = self.recipe_target(&tid) else {
                    continue;
                };
                let chance = num(ind, "chance").unwrap_or(100);
                let rule = format!("ind_{name}_{}", mangle(&tid));
                let mut body: Vec<String> = Vec::new();
                if chance < 100 {
                    let bind = self.roll_bind();
                    body.push(format!("roll 1d100 as {bind}"));
                    body.push(format!("if {bind} <= {chance}"));
                    body.push(format!("  start {verb} via {local}"));
                } else {
                    body.push(format!("start {verb} via {local}"));
                }
                self.note_start(&tid);
                push_rule(&mut books, &mut rule_ids, Some(self.key(&id)), rule, body);
            }

            // Consuming element slots: the merge sites are settled in the
            // plan pass; anything unmerged is named here. The old
            // unqualified consume-book approximation is gone — it fired
            // after the completion's swap books and ate cards a consuming
            // slot would have burned earlier (the rite eating its own
            // freshly summoned spirit).
            let slots: Vec<(String, Vec<String>)> = e
                .get("slots")
                .and_then(|x| x.as_array())
                .map(|a| {
                    a.iter()
                        .filter(|sl| truthy(sl, "consumes"))
                        .map(|sl| {
                            let mut keys: Vec<String> = Vec::new();
                            for dk in ["required", "essential"] {
                                for (k, _) in pairs(sl, dk) {
                                    keys.push(k);
                                }
                            }
                            keys.sort();
                            let raw = s(sl, "id").unwrap_or("slot").to_string();
                            (raw, keys)
                        })
                        .collect()
                })
                .unwrap_or_default();
            for (raw, keys) in slots {
                if self
                    .merged_element_slots
                    .contains(&(id.clone(), raw.clone()))
                {
                    continue;
                }
                if keys.is_empty() {
                    self.gaps.hit(
                        "consuming element slot without a filter",
                        Class::NonGoal,
                        "nothing identifies the burned card; no merge site can carry it",
                        &id,
                    );
                    continue;
                }
                self.gaps.hit(
                    "consuming element slot with no host recipe",
                    Class::ConverterIdiom,
                    "no emitted recipe's requirements or slot filters admit this element, so the consuming slot has no slot block to merge into and its burn is dropped",
                    &format!("{id}:{raw}"),
                );
            }

            for (qual, rules) in books {
                match &qual {
                    Some(q) => lines.push(format!("    on complete [{q}]")),
                    None => lines.push("    on complete".into()),
                }
                for (rule, body) in rules {
                    lines.push(format!("      rule {rule}"));
                    lines.push("        effects".into());
                    for l in body {
                        lines.push(format!("          {l}"));
                    }
                }
            }

            out.extend(lines);
            out.push(String::new());
            self.stats.cards_out += 1;
        }
    }

    fn emit_decks(&mut self, out: &mut Vec<String>) {
        let has_regular = !self.d.decks.is_empty();
        let internal_ids: Vec<String> = self
            .d
            .recipes
            .iter()
            .filter(|(_, r)| r.get("internaldeck").is_some())
            .map(|(id, _)| id.clone())
            .collect();
        if !has_regular && internal_ids.is_empty() {
            return;
        }
        out.push("decks".into());
        for (id, dk) in self.d.decks.clone() {
            let Some(dname) = self.names.deck.get(&id).cloned() else {
                continue;
            };
            let Some(mut lines) = self.deck_lines(&dname, &dk, &id) else {
                self.gaps.hit(
                    "deck with an empty spec",
                    Class::NonGoal,
                    "nothing resolvable to deal; no deck is emitted",
                    &id,
                );
                continue;
            };
            self.gap_relief(&dk, &id);
            let entries_line = lines.remove(1);
            out.push(lines.remove(0));
            out.push(entries_line);
            out.extend(lines);
            self.stats.decks_out += 1;
        }
        for rid in internal_ids {
            let Some(r) = self.d.recipe(&rid).cloned() else {
                continue;
            };
            let idk = r.get("internaldeck").cloned().unwrap_or_default();
            let raw = format!("deck_{rid}");
            let dname = self.names.reserve_id(&raw);
            self.internal_deck.insert(rid.clone(), dname.clone());
            let Some(mut lines) = self.deck_lines(&dname, &idk, &rid) else {
                self.gaps.hit(
                    "internaldeck with an empty spec",
                    Class::NonGoal,
                    "nothing resolvable to deal; no deck is emitted",
                    &rid,
                );
                self.internal_deck.remove(&rid);
                continue;
            };
            self.gap_relief(&idk, &rid);
            let entries_line = lines.remove(1);
            out.push(lines.remove(0));
            out.push(entries_line);
            out.extend(lines);
            self.stats.decks_out += 1;
        }
        out.push(String::new());
    }

    /// The lines for one deck; `None` drops the deck (empty spec).
    fn deck_lines(&mut self, dname: &str, dk: &Value, id: &str) -> Option<Vec<String>> {
        let mut entries: Vec<String> = Vec::new();
        for sp in str_list(dk, "spec") {
            if self.is_aspect(&sp) {
                self.gaps.hit(
                    "deck spec entry that is an aspect",
                    Class::NonGoal,
                    "aspects have no cards; the entry is dropped",
                    &format!("{id}:{sp}"),
                );
                continue;
            }
            match self.card_name(&sp) {
                Some(n) => entries.push(n),
                None => {
                    self.gaps.hit(
                        "deck spec entry with no card",
                        Class::NonGoal,
                        "the element is unknown; the entry is dropped",
                        &format!("{id}:{sp}"),
                    );
                }
            }
        }
        if entries.is_empty() {
            return None;
        }
        let mut lines = vec![
            format!("  deck {dname}"),
            format!("    entries {}", entries.join(", ")),
        ];
        let mut relieved = false;
        if let Some(dc) = s(dk, "defaultcard").and_then(|dc| self.target_card(dc)) {
            lines.push(format!("    fallback {dc}"));
            relieved = true;
        }
        if truthy(dk, "resetonexhaustion") {
            lines.push("    wrap".into());
            relieved = true;
        }
        if !relieved {
            // Every deck needs a relief path: the deck's own first entry
            // serves when CS declares none.
            let first = entries[0].clone();
            lines.push(format!("    fallback {first}"));
        }
        Some(lines)
    }

    fn gap_relief(&mut self, dk: &Value, id: &str) {
        let relieved = s(dk, "defaultcard")
            .map(|dc| self.target_card(dc).is_some())
            .unwrap_or(false)
            || truthy(dk, "resetonexhaustion");
        if !relieved {
            self.gaps.hit(
                "deck without a declared relief path",
                Class::ConverterIdiom,
                "the deck's first entry is emitted as the fallback so exhaustion never fails",
                id,
            );
        }
    }

    fn emit_endings(&mut self, out: &mut Vec<String>) {
        out.push("endings".into());
        for (id, e) in self.d.endings.clone() {
            let Some(name) = self.names.ending.get(&id).cloned() else {
                continue;
            };
            let title = escape_sw(s(&e, "label").unwrap_or(&id));
            let tier = mangle(s(&e, "flavour").unwrap_or("Melancholy"));
            out.push(format!("  ending {name} \"{title}\" {tier}"));
            if let Some(text) = s(&e, "description") {
                let t = escape_sw(text);
                if !t.is_empty() {
                    out.push(format!("    text \"{t}\""));
                }
            }
            self.stats.endings_out += 1;
        }
        for ((end, flavour), name) in self.names.ending_variant.clone() {
            let Some(base) = self.d.endings.get(&end).cloned() else {
                continue;
            };
            let title = escape_sw(s(&base, "label").unwrap_or(&end));
            let tier = mangle(&flavour);
            out.push(format!("  ending {name} \"{title}\" {tier}"));
            self.stats.endings_out += 1;
        }
        out.push(String::new());
    }

    fn verb_title(&self, action: &str) -> String {
        let label = self
            .d
            .verbs
            .get(action)
            .and_then(|v| s(v, "label"))
            .unwrap_or(action);
        escape_sw(label)
    }

    fn emit_verbs(&mut self, out: &mut Vec<String>) {
        out.push("verbs".into());
        for (verb, recipes) in self.by_verb() {
            let vname = self
                .names
                .verb
                .get(&verb)
                .cloned()
                .unwrap_or_else(|| verb.clone());
            let (mode, instances) = if PLAYER_VERBS.contains(&verb.as_str()) {
                ("committable", "1")
            } else if verb == TIME_VERB {
                ("autonomous", "1")
            } else {
                ("uncommittable", "any")
            };
            let action = verb.strip_suffix("_chains").unwrap_or(&verb);
            let title = self.verb_title(action);
            out.push(format!("  verb {vname} {mode} instances {instances}"));
            out.push(format!("    title \"{title}\""));
            out.push(String::new());

            for (i, rid) in recipes.iter().enumerate() {
                let Some(local) = self
                    .names
                    .recipe
                    .get(&(verb.clone(), rid.to_string()))
                    .cloned()
                else {
                    continue;
                };
                let Some(r) = self.d.recipe(rid).cloned() else {
                    continue;
                };
                self.emit_recipe(out, &verb, rid, &local, i, &r);
                self.stats.recipes_out += 1;
            }
            self.stats.verbs_out += 1;
            out.push(String::new());
        }
        self.emit_lever_resolver(out);
    }

    /// The cross-run lever resolver (spec.md §7.8): one autonomous verb whose
    /// recipes are chain-only (the heartbeat cannot open them; only the
    /// `start` emitted at a lever effect site can), one legacy-gated pair per
    /// lever — the routed arm answers a host-fed legacy value, the default
    /// arm spawns the lever's `defaultValue`. The selection law picks the
    /// arm, so each site emits a plain `start lever_resolver`.
    fn emit_lever_resolver(&mut self, out: &mut Vec<String>) {
        if self.levers_used.is_empty() {
            return;
        }
        let vname = self
            .names
            .verb
            .get("lever_resolver")
            .cloned()
            .unwrap_or_else(|| "lever_resolver".to_string());
        out.push(format!("  verb {vname} autonomous instances any"));
        out.push("    title \"Cross-Run Legacy Resolver\"".into());
        out.push(String::new());
        for (lever, count) in self.levers_used.clone() {
            let default = self
                .d
                .levers
                .get(&lever)
                .and_then(|l| s(l, "defaultValue"))
                .and_then(|dv| self.target_card(dv));
            let Some(cname) = default else {
                self.gaps.hit(
                    "lever with no resolvable default",
                    Class::ExtensionSeam,
                    "the lever's defaultValue names no card; no resolver recipes are emitted and the lever sites drop their effect",
                    &lever,
                );
                continue;
            };
            let n = count.clamp(1, 20);
            let lkey = mangle(&lever);
            for (suffix, prio, when) in [
                ("routed", 0usize, format!("legacy.{lkey} >= 1")),
                ("default", 1usize, format!("legacy.{lkey} < 1")),
            ] {
                let local = self
                    .names
                    .recipe
                    .get(&("lever_resolver".to_string(), format!("lever_{lever}_{suffix}")))
                    .cloned()
                    .unwrap_or_else(|| mangle(&format!("lever_{lever}_{suffix}")));
                out.push(format!("    recipe {local}"));
                out.push(format!("      priority {prio}"));
                out.push("      chain-only".into());
                out.push("      duration 0".into());
                out.push(format!("      when {when}"));
                out.push("      effects".into());
                out.push(format!("        spawn {cname} count {n} into table"));
            }
        }
        out.push(String::new());
    }

    fn emit_recipe(&mut self, out: &mut Vec<String>, verb: &str, rid: &str, local: &str, index: usize, r: &Value) {
        let warmup = num(r, "warmup").unwrap_or(0);

        out.push(format!("    recipe {local}"));
        if let Some(l) = s(r, "label") {
            let l = escape_sw(l);
            if !l.is_empty() {
                out.push(format!("      label \"{l}\""));
            }
        }
        if let Some(desc) = s(r, "description") {
            let d = escape_sw(desc);
            if !d.is_empty() {
                out.push(format!("      meta description \"{d}\""));
            }
        }
        if PLAYER_VERBS.contains(&verb) && !self.craftable(r) {
            out.push("      chain-only".into());
        }
        out.push(format!("      priority {index}"));
        if let Some(mx) = num(r, "maxexecutions") {
            if mx > 0 {
                out.push(format!("      max {mx}"));
            }
        }
        out.push(format!("      duration {warmup}m"));

        let declares = self.declares_of(r);
        if !declares.is_empty() {
            out.push(format!("      declares {}", declares.join(", ")));
        }

        let plan = self.plans.get(rid).cloned().unwrap_or_else(RecipePlan::default);

        let when = self.when_of(r, &plan);
        if let Some(ref gate) = when {
            out.push(format!("      when {gate}"));
        }

        // The doctrine guard: the one recipe whose completion demonstrably
        // requires a bound card in a named slot (the founding alt cascade
        // keys on the doctrine's aspects) refuses the doctrine-less opening.
        for (cs_id, slot, aspect, prose) in GUARDED_SLOTS {
            if *cs_id == rid {
                let planned = plan.slots.iter().any(|p| p.name == *slot);
                if planned {
                    out.push(format!(
                        "      guard \"{prose}\" when count(slot:{slot}) < 1 and total(interior, aspect:{aspect}) < 1"
                    ));
                }
            }
        }

        let mut slot_lines: Vec<String> = Vec::new();
        let source: Vec<Value> = r
            .get("slots")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        for (sl, planned) in source.iter().zip(plan.slots.iter()) {
            let mut line = format!("{}: 0..1", planned.name);
            if truthy(sl, "greedy") {
                line.push_str(" greedy");
            }
            if truthy(sl, "consumes") {
                line.push_str(" consuming");
            }
            line.push_str(" open");
            let terms = self.slot_filter_terms(rid, sl);
            if !terms.is_empty() {
                line.push(' ');
                line.push_str(&terms.join(" "));
            }
            slot_lines.push(line);
        }
        // The verb slot and the synthesized slots follow the source slots, in
        // plan order (verb slot first, then cost, then requirement slots).
        for planned in plan.slots.iter().skip(source.len()) {
            let line = match planned.kind {
                SlotKind::Source => continue,
                SlotKind::Verb => {
                    let action = self.action_of(rid);
                    let mut line = format!("{}: 0..1 open", planned.name);
                    if let Some(sl) = verb_slot_of(self.d, &action) {
                        let terms = self.verb_slot_filter_terms(&action, sl);
                        if !terms.is_empty() {
                            line.push(' ');
                            line.push_str(&terms.join(" "));
                        }
                    }
                    line
                }
                SlotKind::Cost => {
                    // `cost_<key>: 1..N consuming <term>` — the bind is the
                    // payment route; the paired negative effect drains the
                    // interior at the effects pass, and the consuming
                    // settlement burns the bound payers.
                    let k = planned.key.clone().unwrap_or_default();
                    let n = pairs(r, "effects")
                        .into_iter()
                        .find(|(ek, _)| *ek == k)
                        .map(|(_, v)| -v)
                        .unwrap_or(1)
                        .clamp(1, 12);
                    let term = self.arm_term(&k, 1);
                    format!("{}: 1..{n} consuming open {term}", planned.name)
                }
                SlotKind::Req => {
                    // A synthesized requirement slot: the key's own term.
                    let k = planned.key.clone().unwrap_or_default();
                    let term = self.arm_term(&k, 1);
                    format!("{}: 0..1 open {term}", planned.name)
                }
                SlotKind::Merged => {
                    // A merged element slot: its line is fixed at plan time.
                    planned.line.clone().unwrap_or_else(|| {
                        format!("{}: 0..1 open", planned.name)
                    })
                }
            };
            slot_lines.push(line);
        }
        if !slot_lines.is_empty() {
            self.stats.slots_out += slot_lines.len();
            out.push("      slots".into());
            for l in slot_lines {
                out.push(format!("        {l}"));
            }
        }

        out.push("      effects".into());
        let mut body: Vec<(usize, String)> = Vec::new();
        let ind = 8;
        if let Some(sd) = s(r, "startdescription") {
            let t = escape_sw(sd);
            if !t.is_empty() {
                body.push((ind, format!("narrate \"{t}\"")));
            }
        }

        // Additional alts decide ahead of everything (CS GetValidOverride, the
        // completion's first statement) and their queued spawns fire at the
        // inductions pass regardless of which recipe the override installed —
        // so their gated starts sit ahead of the override cascade, evaluated
        // whether or not an override adopts.
        self.emit_additional_starts(&mut body, ind, r, rid);

        let plain_alts: Vec<Value> = entries(r, "alt")
            .into_iter()
            .filter(|e| !(e.is_object() && truthy(e, "additional")))
            .collect();
        self.emit_alt_cascade(&mut body, &plain_alts, 0, ind, r, rid);

        for (i, line) in &body {
            out.push(format!("{}{}", " ".repeat(*i), line));
        }

        if self.alt_targets_with_warmup.contains(rid) {
            let instant_local = format!("{local}__instant");
            out.push(format!("    recipe {instant_local}"));
            if let Some(l) = s(r, "label") {
                let l = escape_sw(l);
                if !l.is_empty() {
                    out.push(format!("      label \"{l}\""));
                }
            }
            if let Some(desc) = s(r, "description") {
                let d = escape_sw(desc);
                if !d.is_empty() {
                    out.push(format!("      meta description \"{d}\""));
                }
            }
            out.push("      chain-only".into());
            out.push("      allow shadow".into());
            out.push(format!("      priority {index}"));
            if let Some(mx) = num(r, "maxexecutions") {
                if mx > 0 {
                    out.push(format!("      max {mx}"));
                }
            }
            out.push("      duration 0m".into());
            if !declares.is_empty() {
                out.push(format!("      declares {}", declares.join(", ")));
            }
            if let Some(ref gate) = when {
                out.push(format!("      when {gate}"));
            }
            out.push("      effects".into());
            for (i, line) in &body {
                out.push(format!("{}{}", " ".repeat(*i), line));
            }
            self.stats.recipes_out += 1;
        }
    }

    /// The catalyst aspects this recipe declares for aspect-qualified card
    /// books: the CS trigger-gate analysis (recipe aspects, positive
    /// requirement keys, slot requirements, table reqs, mutation keys), plus
    /// the induce catalysts (recipe aspects plus positive-effect outputs and
    /// their aspects — CS induces from outputs, not requirement bearers).
    fn declares_of(&self, r: &Value) -> Vec<String> {
        let mut keys: BTreeSet<String> = BTreeSet::new();
        for (k, _) in pairs(r, "aspects") {
            keys.insert(self.key(&k));
        }
        for (k, v) in pairs(r, "requirements") {
            if v > 0 {
                keys.insert(self.key(&k));
            }
        }
        for sl in r
            .get("slots")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            for (k, _) in pairs(sl, "required") {
                keys.insert(self.key(&k));
            }
        }
        for (k, _) in pairs(r, "tablereqs") {
            keys.insert(self.key(&k));
        }
        for m in r
            .get("mutations")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(k) = s(m, "mutate") {
                keys.insert(self.key(k));
            }
        }
        let mut catalysts: BTreeSet<String> =
            pairs(r, "aspects").into_iter().map(|(k, _)| k).collect();
        for (k, v) in pairs(r, "effects") {
            if v > 0 && !self.is_aspect(&k) && self.d.element(&k).is_some() {
                catalysts.insert(k.clone());
                for (a, _) in self.aspects_of(&k) {
                    catalysts.insert(a);
                }
            }
        }
        for ind in &self.induce_ids {
            let ik = self.key(ind);
            if catalysts.contains(&ik) {
                keys.insert(ik);
            }
        }
        keys.into_iter().collect()
    }

    /// CS CanExecuteInContext over the three requirement dicts, scoped per
    /// the verified semantics:
    /// - `requirements` read the situation's combined interior
    ///   (`Situation.GetAspects`: every interior sphere plus the verb's
    ///   aspects). The bound cards across ALL slots live in the interior, so
    ///   each positive key emits one sum arm — the interior total plus the
    ///   planned slots whose filter can carry the key (at commit selection
    ///   the interior is empty and only the submitted bindings, visible
    ///   through the slot scopes, can satisfy the arm; at a continuation the
    ///   released storage carries it). Anti-requirements (`-1`) read the
    ///   interior sum alone: the situation pool, where a bound card counts
    ///   once.
    /// - `tablereqs` read the `table` scope (public cards plus every
    ///   situation's non-greedy slot bindings — AspectsOnTable).
    /// - `extantreqs` read the `anywhere` scope (all live cards —
    ///   AspectsExtant, up to running-recipe aspects the language cannot
    ///   see).
    fn when_of(&self, r: &Value, plan: &RecipePlan) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        for (k, v) in pairs(r, "requirements") {
            let kk = self.key(&k);
            if v > 0 {
                let mut sum = format!("total(interior, aspect:{kk})");
                for c in plan.carriers() {
                    sum.push_str(&format!(" + total(slot:{c}, aspect:{kk})"));
                }
                parts.push(format!("({sum} >= {v})"));
            } else if v < 0 {
                let mut sum = format!("total(interior, aspect:{kk})");
                for c in plan.carriers() {
                    sum.push_str(&format!(" + total(slot:{c}, aspect:{kk})"));
                }
                parts.push(format!("({sum} < {})", -v));
            }
        }
        for dk in ["tablereqs", "extantreqs"] {
            let scope = if dk == "tablereqs" { "table" } else { "anywhere" };
            for (k, v) in pairs(r, dk) {
                let kk = self.key(&k);
                if v > 0 {
                    parts.push(format!("(total({scope}, aspect:{kk}) >= {v})"));
                } else if v < 0 {
                    parts.push(format!("(total({scope}, aspect:{kk}) < {})", -v));
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        Some(parts.join(" and "))
    }

    /// Deck draws for a recipe: deckeffects, then internaldeck wiring, then
    /// portal consequences. CS draws before effects, and RunDeckEffect deals
    /// into the situation's storage sphere: the drawn cards land in the
    /// interior, count toward the situation's aspects, and ride the normal
    /// interior-leftover release at completion (the table ejection is the
    /// completion's job, not the draw's).
    fn draws_of(&mut self, r: &Value, rid: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut decks: Vec<(String, i64)> = pairs(r, "deckeffects");
        if decks.is_empty() {
            if let Some(idk) = r.get("internaldeck") {
                let draws = num(idk, "draws").unwrap_or(1).max(1);
                decks.push((rid.to_string(), draws));
            }
        }
        for (deck, n) in &decks {
            let Some(dname) = self.deck_name(deck) else {
                self.gaps.hit(
                    "deckeffect on an unresolvable deck",
                    Class::NonGoal,
                    "no deck is emitted under that id; the draw is dropped",
                    &format!("{rid}:{deck}"),
                );
                continue;
            };
            let count = (*n).clamp(1, 12);
            out.push(format!("draw from {dname} count {count} into interior"));
        }
        if let Some(pe) = s(r, "portaleffect").map(String::from) {
            match self.d.portals.get(&pe).cloned() {
                Some(portal) => {
                    for cons in portal
                        .get("consequences")
                        .and_then(|x| x.as_array())
                        .into_iter()
                        .flatten()
                    {
                        for (deck, n) in pairs(cons, "deckeffects") {
                            if let Some(dname) = self.deck_name(&deck) {
                                let count = n.clamp(1, 12);
                                out.push(format!("draw from {dname} count {count} into interior"));
                            }
                        }
                    }
                }
                None => {
                    self.gaps.hit(
                        "portaleffect with no portal table entry",
                        Class::NonGoal,
                        "the portal is unknown; its draws are dropped",
                        &format!("{rid}:{pe}"),
                    );
                }
            }
        }
        out
    }

    fn deck_name(&self, id: &str) -> Option<String> {
        self.names
            .deck
            .get(id)
            .cloned()
            .or_else(|| self.internal_deck.get(id).cloned())
    }

    /// A chain start for a recipe target: (verb name, local recipe name).
    /// Targets on the ambient verb are heartbeat-served and never chained.
    fn recipe_target(&mut self, tid: &str) -> Option<(String, String)> {
        let host = match self.host.get(tid) {
            Some(h) => h.clone(),
            None => {
                self.gaps.hit(
                    "chain target is not an emitted recipe",
                    Class::NonGoal,
                    "the target was skipped upstream; the chain entry is dropped",
                    tid,
                );
                return None;
            }
        };
        if host == TIME_VERB {
            self.gaps.hit(
                "chain target on the ambient verb",
                Class::ConverterIdiom,
                "the autonomous heartbeat re-opens ambient recipes, so no start is emitted",
                tid,
            );
            return None;
        }
        let vname = self.names.verb.get(&host).cloned()?;
        self.names
            .recipe
            .get(&(host, tid.to_string()))
            .cloned()
            .map(|l| (vname, l))
    }

    /// CS chance/challenges gate: the roll lines to emit and the success
    /// condition over their bindings. With `challenges`, CS
    /// (ChallengeArbiter.GetArbitratedChance) reads each challenge's
    /// aspect-banded chance over the situation's interior (the table replaces
    /// the flat chance), takes the MAX across every challenge, and rolls ONCE
    /// — `Rolld100() > arbitrated` fails. The emission keeps that shape: one
    /// d100 per gated entry, and the condition ORs every `(challenge × band)`
    /// pair of `(aspect-level-band and roll <= bandChance)` over the shared
    /// binding, which collapses at runtime to `roll <= max band chance`. A
    /// flat `chance` without challenges is one d100 against the entry's
    /// percentage, sharing the walk's roll binding when one is provided.
    fn gate_of(
        &mut self,
        e: &Value,
        shared: Option<&str>,
        rid: &str,
    ) -> (Vec<String>, Option<String>) {
        let chance = e.get("chance").and_then(|x| x.as_i64()).unwrap_or(100);
        let challenges = e.get("challenges").and_then(|x| x.as_object()).cloned();
        if chance >= 100 && challenges.is_none() {
            return (Vec::new(), None);
        }
        let mut ch: Vec<(String, String)> = challenges
            .map(|o| {
                o.into_iter()
                    .map(|(k, v)| (k, v.as_str().unwrap_or("stealth").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        ch.sort();
        let mut rolls: Vec<String> = Vec::new();
        let mut conds: Vec<String> = Vec::new();
        if ch.is_empty() {
            let bind = match shared {
                Some(b) => b.to_string(),
                None => {
                    let b = self.roll_bind();
                    rolls.push(format!("roll 1d100 as {b}"));
                    b
                }
            };
            conds.push(format!("({bind} <= {chance})"));
        } else {
            let bind = self.roll_bind();
            rolls.push(format!("roll 1d100 as {bind}"));
            let mut pairs: Vec<String> = Vec::new();
            for (ak, kind) in &ch {
                let k = self.key(ak);
                let table: &[(i64, i64)] = if kind == "base" {
                    &[(1, 30), (5, 70), (10, 90)]
                } else {
                    &[(5, 10), (10, 30), (15, 70), (20, 90)]
                };
                for (i, (lo, val)) in table.iter().enumerate() {
                    let mut band = format!("total(interior, aspect:{k}) >= {lo}");
                    if let Some((hi, _)) = table.get(i + 1) {
                        band.push_str(&format!(" and total(interior, aspect:{k}) < {hi}"));
                    }
                    pairs.push(format!("({band} and {bind} <= {val})"));
                }
            }
            conds.push(format!("({})", pairs.join(" or ")));
        }
        self.gaps.hit(
            "chance/challenges gate mapped to rolls",
            Class::ConverterIdiom,
            "CS's hidden roll becomes a roll 1d100 line plus the aspect-banded success table; CS's arbiter takes the max challenge chance and rolls once (Rolld100() > arbitrated fails), so the challenge bands OR over one shared binding; deviation: one shared d100 covers a whole linked walk (CS rolls per entry)",
            rid,
        );
        (rolls, Some(conds.join(" and ")))
    }

    /// Record a `start` target for the dead census.
    fn note_start(&mut self, tid: &str) {
        if let Some(host) = self.host.get(tid) {
            self.started.insert((host.clone(), tid.to_string()));
        }
    }

    /// Record a `continue` target for the dead census.
    fn note_continue(&mut self, tid: &str) {
        if let Some(host) = self.host.get(tid) {
            self.continued.insert((host.clone(), tid.to_string()));
        }
    }

    /// A continuation target: (verb name, local recipe name), spelled
    /// globally. Any emitted recipe qualifies — a continuation adopts the
    /// target's verb, so ambient-verb targets are the heartbeat loop itself.
    fn continue_target(&mut self, tid: &str) -> Option<(String, String)> {
        let host = match self.host.get(tid) {
            Some(h) => h.clone(),
            None => {
                self.gaps.hit(
                    "chain target is not an emitted recipe",
                    Class::NonGoal,
                    "the target was skipped upstream; the chain entry is dropped",
                    tid,
                );
                return None;
            }
        };
        let vname = self.names.verb.get(&host).cloned()?;
        self.names
            .recipe
            .get(&(host, tid.to_string()))
            .cloned()
            .map(|l| (vname, l))
    }

    /// The non-additional alt cascade: the first executable entry REPLACES the
    /// recipe being executed, in place, in the same situation with the same
    /// spheres (CS RequiresExecutionState.Continue: `situation.CurrentRecipe =
    /// validOverride; situation.ExecuteCurrentRecipe();` — the override's body
    /// runs INSTEAD of the original's, whose effects never fire). Each arm is
    /// therefore a gated `continue via` — the same-situation adoption — and
    /// the deepest `else` carries the recipe's own body, linked cascade
    /// included, which runs only when no override adopts. The decision sits
    /// ahead of the body because CS resolves the override as the completion's
    /// first statement, before ExecuteCurrentRecipe. A target with a warmup
    /// re-arms the timer for it (a 0-warmup target — the ladder case —
    /// resolves inline at once); CS's override executes without a warmup, so
    /// the re-arm is the mapped shape's one pacing deviation.
    fn emit_alt_cascade(
        &mut self,
        body: &mut Vec<(usize, String)>,
        alts: &[Value],
        i: usize,
        ind: usize,
        r: &Value,
        rid: &str,
    ) {
        if i >= alts.len() {
            self.emit_core(body, ind, r, rid);
            return;
        }
        let e = alts[i].clone();
        let Some(tid) = entry_id(&e) else {
            self.gaps.hit(
                "alt entry without an id",
                Class::NonGoal,
                "the entry names no recipe; it is dropped",
                rid,
            );
            self.emit_alt_cascade(body, alts, i + 1, ind, r, rid);
            return;
        };
        let Some(target) = self.continue_target(&tid) else {
            self.emit_alt_cascade(body, alts, i + 1, ind, r, rid);
            return;
        };
        let plan = self.plans.get(&tid).cloned().unwrap_or_default();
        let target_recipe = self.d.recipe(&tid);
        let gate = target_recipe
            .and_then(|t| self.when_of(t, &plan));
        let (rolls, cond) = self.gate_of(&e, None, rid);
        let gates: Vec<String> = gate.into_iter().chain(cond).collect();
        for rl in rolls {
            body.push((ind, rl));
        }
        let target_local = if self.alt_targets_with_warmup.contains(&tid) {
            format!("{}__instant", target.1)
        } else {
            target.1.clone()
        };
        if gates.is_empty() {
            // An unconditional alternative adopts the situation outright; the
            // core body never runs.
            body.push((ind, format!("continue via {}.{target_local}", target.0)));
            self.note_continue(&tid);
            self.gaps.hit(
                "unconditional alt replaces the recipe body",
                Class::ConverterIdiom,
                "CS resolves the alternative over the recipe's own effects; the arm's unconditional continue adopts the situation and the core body never runs",
                rid,
            );
            return;
        }
        if gates.len() > 1 {
            body.push((ind, format!("if ({})", gates.join(") and ("))));
        } else {
            body.push((ind, format!("if {}", gates[0])));
        }
        body.push((
            ind + 2,
            format!("continue via {}.{target_local}", target.0),
        ));
        self.note_continue(&tid);
        body.push((ind, "else".into()));
        let mark = body.len();
        self.emit_alt_cascade(body, alts, i + 1, ind + 2, r, rid);
        if body.len() == mark {
            // The cascade is exhausted past this arm: the else has no
            // branch to fill, so the arm ends the walk.
            body.pop();
        }
    }

    /// The recipe's own resolution body, in CS completion order: mutations,
    /// deck draws, and the effects table (RecipeCompletionEffectCommand.
    /// Execute), then the ending, then the linked cascade and the inductions.
    /// It runs only when no non-additional alt adopted the situation (the
    /// alt cascade guards it as its deepest else — CS executes the override's
    /// body instead of this one).
    fn emit_core(&mut self, body: &mut Vec<(usize, String)>, ind: usize, r: &Value, rid: &str) {
        // Mutations.
        for m in r
            .get("mutations")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            let (Some(filt), Some(mutate)) = (s(m, "filter"), s(m, "mutate")) else {
                self.gaps.hit(
                    "mutation without a filter or mutate key",
                    Class::NonGoal,
                    "nothing identifies the mutated cards; the mutation is dropped",
                    rid,
                );
                continue;
            };
            let level = num(m, "level").unwrap_or(1);
            let additive = truthy(m, "additive");
            let fk = self.key(filt);
            let mk = self.key(mutate);
            let op = if additive {
                if level >= 0 {
                    format!("+= {level}")
                } else {
                    format!("-= {}", -level)
                }
            } else {
                format!("= {level}")
            };
            body.push((ind, format!("set interior aspect:{fk} aspect:{mk} {op}")));
        }

        // Deck draws: CS RunDeckEffect deals into the situation's storage
        // sphere after the mutation and xtrigger passes and before the
        // recipe's own effects — so draws come here, after mutations and
        // before the effects table (and after the alt gates above, which CS
        // decides in GetValidOverride before ExecuteCurrentRecipe runs).
        for line in self.draws_of(r, rid) {
            body.push((ind, line));
        }

        // The effects table.
        for (k, v) in pairs(r, "effects") {
            if k.starts_with("lever_") {
                self.emit_lever(body, ind, rid, &k, v);
                continue;
            }
            if v == 0 {
                continue;
            }
            if v > 0 {
                if self.is_aspect(&k) {
                    self.gaps.hit(
                        "positive effect on an aspect",
                        Class::NonGoal,
                        "CS NullElement semantics: the write is inert",
                        &format!("{rid}:{k}"),
                    );
                    continue;
                }
                let Some(pname) = self.target_card(&k) else {
                    self.gaps.hit(
                        "positive effect on an unknown element",
                        Class::NonGoal,
                        "no card exists under that id; the spawn is dropped",
                        &format!("{rid}:{k}"),
                    );
                    continue;
                };
                // CS RunRecipeEffects writes the situation's storage sphere:
                // the spawn lands in the interior, feeds the completion's
                // books and any continuation's `when` out of the same pool,
                // and rides the interior-leftover release at completion.
                let n = v.clamp(1, 20);
                body.push((ind, format!("spawn {pname} count {n} into interior")));
            } else {
                // A negative effect value drains the executing situation's
                // interior only (CS RunRecipeEffects -> Sphere.ModifyElementQuantity
                // -> ReduceElement): it consumes up to |n| units across the
                // interior's stacks in arrival order, retiring stacks that
                // reach zero, and stops silently at the last matching unit
                // (the elementStack == null early return) — so -1 and -999
                // differ observably whenever the interior holds more than
                // one unit. The .sw destroy count is the same AtMost cap
                // (story-engine effects.rs takes min(count, resolved)).
                let kk = self.key(&k);
                body.push((ind, format!("destroy interior aspect:{kk} count {}", -v)));
            }
        }

        // Ending.
        if let Some(end) = s(r, "ending").map(String::from) {
            match self.ending_name(&end, r) {
                Some(en) => body.push((ind, format!("ending {en}"))),
                None => {
                    self.gaps.hit(
                        "recipe ending with no ending table entry",
                        Class::NonGoal,
                        "the ending is unknown; the effect is dropped",
                        &format!("{rid}:{end}"),
                    );
                }
            }
        }

        // linked: CS scans the entries in order after the effects; the first
        // roll-passing, requirement-satisfying, non-additional entry takes
        // over the same situation, and `additional: true` entries spawn side
        // situations instead.
        let linked = entries(r, "linked");
        self.emit_linked_cascade(body, &linked, ind, rid);

        // Inductions: CS gates them after the effects (the inductions pass
        // runs post-ExecuteCurrentRecipe), so they stay behind the linked
        // cascade.
        self.emit_induction_starts(body, ind, r, rid);
    }

    /// The linked list: the `additional: true` entries spawn gated side
    /// situations (CS GetNextValidLink's spawnAdditionals), then the
    /// non-additional entries walk an exclusive if/else cascade of
    /// `continue via` arms — the same-situation continuation. Each arm gates
    /// the target's `when` plus its chance/challenges; the walk shares one
    /// flat d100 (arms compare the binding, CS's hidden roll made visible);
    /// challenged entries roll their own d100 and pass at the max banded
    /// chance (CS's arbiter rolls once against the max). An
    /// unconditional entry ends the walk. Loops are the engine's to bound:
    /// each step re-arms the timer, and the gated arms make every CS cycle
    /// strictly progress (the needs/discardseason/drawseason heartbeat).
    fn emit_linked_cascade(
        &mut self,
        body: &mut Vec<(usize, String)>,
        linked: &[Value],
        ind: usize,
        rid: &str,
    ) {
        // Side situations first: they never take over the walk.
        for e in linked {
            if e.is_object() && truthy(e, "additional") {
                self.emit_side_start(body, ind, rid, e);
            }
        }
        let walk: Vec<&Value> = linked
            .iter()
            .filter(|e| !(e.is_object() && truthy(e, "additional")))
            .collect();
        // One shared d100 per walk when any arm carries a flat chance.
        let shared = walk
            .iter()
            .any(|e| {
                let chance = e.get("chance").and_then(|x| x.as_i64()).unwrap_or(100);
                (0..100).contains(&chance) && e.get("challenges").is_none()
            })
            .then(|| {
                let b = self.roll_bind();
                body.push((ind, format!("roll 1d100 as {b}")));
                b
            });
        self.emit_linked_walk(body, &walk, 0, ind, rid, shared.as_deref());
    }

    /// The exclusive walk over the non-additional linked entries.
    fn emit_linked_walk(
        &mut self,
        body: &mut Vec<(usize, String)>,
        walk: &[&Value],
        i: usize,
        ind: usize,
        rid: &str,
        shared: Option<&str>,
    ) {
        if i >= walk.len() {
            return;
        }
        let e = walk[i].clone();
        let Some(tid) = entry_id(&e) else {
            self.gaps.hit(
                "linked entry without an id",
                Class::NonGoal,
                "the entry names no recipe; it is dropped",
                rid,
            );
            self.emit_linked_walk(body, walk, i + 1, ind, rid, shared);
            return;
        };
        let Some(target) = self.continue_target(&tid) else {
            self.emit_linked_walk(body, walk, i + 1, ind, rid, shared);
            return;
        };
        let plan = self.plans.get(&tid).cloned().unwrap_or_default();
        let gate = self
            .d
            .recipe(&tid)
            .and_then(|t| self.when_of(t, &plan));
        let (rolls, cond) = self.gate_of(&e, shared, rid);
        let gates: Vec<String> = gate.into_iter().chain(cond).collect();
        for rl in rolls {
            body.push((ind, rl));
        }
        if gates.is_empty() {
            // The first executable entry always runs; later entries never do.
            body.push((ind, format!("continue via {}.{}", target.0, target.1)));
            self.note_continue(&tid);
            return;
        }
        if gates.len() > 1 {
            body.push((ind, format!("if ({})", gates.join(") and ("))));
        } else {
            body.push((ind, format!("if {}", gates[0])));
        }
        body.push((ind + 2, format!("continue via {}.{}", target.0, target.1)));
        self.note_continue(&tid);
        body.push((ind, "else".into()));
        let mark = body.len();
        self.emit_linked_walk(body, walk, i + 1, ind + 2, rid, shared);
        if body.len() == mark {
            // The walk is exhausted past this arm: the else has no branch
            // to fill, so the arm ends the walk.
            body.pop();
        }
    }

    /// The additional-alt fan-out: every matching entry starts its own
    /// situation (gates decided pre-effects, CS GetValidOverride order).
    fn emit_additional_starts(
        &mut self,
        body: &mut Vec<(usize, String)>,
        ind: usize,
        r: &Value,
        rid: &str,
    ) {
        let additional: Vec<Value> = entries(r, "alt")
            .into_iter()
            .filter(|e| e.is_object() && truthy(e, "additional"))
            .collect();
        for e in additional {
            self.emit_side_start(body, ind, rid, &e);
        }
    }

    /// The induction fan-out: every matching entry starts its own situation
    /// (gates decided post-effects, CS inductions-pass order).
    fn emit_induction_starts(
        &mut self,
        body: &mut Vec<(usize, String)>,
        ind: usize,
        r: &Value,
        rid: &str,
    ) {
        let mut inductions = entries(r, "inductions");
        inductions.retain(|e| e.is_object() || e.is_string());
        for e in inductions {
            self.emit_side_start(body, ind, rid, &e);
        }
    }

    /// One side-situation start: chance gate, then the start line.
    fn emit_side_start(
        &mut self,
        body: &mut Vec<(usize, String)>,
        ind: usize,
        rid: &str,
        e: &Value,
    ) {
        let Some(tid) = entry_id(e) else {
            return;
        };
        let Some(target) = self.recipe_target(&tid) else {
            return;
        };
        let (rolls, cond) = self.gate_of(e, None, rid);
        for rl in rolls {
            body.push((ind, rl));
        }
        let plan = self.plans.get(&tid).cloned().unwrap_or_default();
        let target_gate = self.d.recipe(&tid).and_then(|t| self.when_of(t, &plan));
        let conds: Vec<String> = cond.into_iter().chain(target_gate).collect();
        if conds.is_empty() {
            body.push((ind, format!("start {} via {}", target.0, target.1)));
        } else {
            let expr = if conds.len() > 1 {
                format!("({})", conds.join(") and ("))
            } else {
                conds[0].clone()
            };
            body.push((ind, format!("if {expr}")));
            body.push((ind + 2, format!("start {} via {}", target.0, target.1)));
        }
        self.note_start(&tid);
    }

    fn emit_lever(
        &mut self,
        body: &mut Vec<(usize, String)>,
        ind: usize,
        rid: &str,
        key: &str,
        v: i64,
    ) {
        let lever_id = key.trim_start_matches("lever_").to_lowercase();
        let default = self
            .d
            .levers
            .get(&lever_id)
            .and_then(|l| s(l, "defaultValue"))
            .and_then(|dv| self.target_card(dv));
        if default.is_some() && v > 0 {
            // The site hands off to the resolver verb; the selection law
            // picks the legacy-gated arm (spec.md §7.8).
            let vname = self
                .names
                .verb
                .get("lever_resolver")
                .cloned()
                .unwrap_or_else(|| "lever_resolver".to_string());
            let n = v.clamp(1, 20);
            if n > 1 {
                body.push((ind, format!("start {vname} count {n}")));
            } else {
                body.push((ind, format!("start {vname}")));
            }
            self.gaps.hit(
                "lever redirect routes through the legacy resolver",
                Class::ConverterIdiom,
                "CS resolves the lever key against cross-run history; the site starts the resolver verb, whose legacy-gated recipes spawn the defaultValue (a host folding incoming legacy flips the routed arm)",
                &format!("{rid}:{key}"),
            );
        } else {
            self.gaps.hit(
                "lever effect with no resolvable default",
                Class::ExtensionSeam,
                "the lever's defaultValue names no card (or the effect value is not positive); the resolver has nothing to spawn and the effect is dropped",
                &format!("{rid}:{key}"),
            );
        }
    }

    fn ending_name(&self, end: &str, r: &Value) -> Option<String> {
        let base = self.d.endings.get(end)?;
        let base_flavour = s(base, "flavour").unwrap_or("Melancholy").to_string();
        if let Some(f) = s(r, "signalEndingFlavour") {
            if f != base_flavour {
                return self
                    .names
                    .ending_variant
                    .get(&(end.to_string(), f.to_string()))
                    .cloned();
            }
        }
        self.names.ending.get(end).cloned()
    }

    /// The dead census: emitted recipes that no route reaches — player verbs
    /// commit, the ambient heartbeat re-opens, a `start` opens, or a
    /// `continue` adopts — plus the verbs whose every recipe is dead.
    fn census(&mut self) {
        let mut referenced: BTreeSet<String> = BTreeSet::new();
        for (_, r) in &self.d.recipes {
            for key in ["alt", "linked", "inductions"] {
                for e in entries(r, key) {
                    if let Some(tid) = entry_id(&e) {
                        referenced.insert(tid);
                    }
                }
            }
        }
        let mut dead_referenced = 0usize;
        let mut dead_total = 0usize;
        let mut dead_by_verb: BTreeMap<String, usize> = BTreeMap::new();
        for (verb, rids) in self.by_verb() {
            if PLAYER_VERBS.contains(&verb.as_str()) || verb == TIME_VERB {
                continue;
            }
            let mut dead_here = 0usize;
            for rid in &rids {
                let reached = self.started.contains(&(verb.clone(), rid.clone()))
                    || self.continued.contains(&(verb.clone(), rid.clone()));
                if !reached {
                    dead_here += 1;
                    if referenced.contains(rid) {
                        dead_referenced += 1;
                    }
                }
            }
            if dead_here > 0 {
                dead_total += dead_here;
                // A dead verb is one whose every recipe is unreachable: no
                // route can ever open it.
                if dead_here == rids.len() {
                    dead_by_verb.insert(verb, dead_here);
                }
            }
        }
        self.stats.dead_recipes = dead_total;
        self.stats.dead_recipes_referenced = dead_referenced;
        self.stats.dead_verbs = dead_by_verb.len();
        self.dead_verb_detail = dead_by_verb.into_iter().collect();
    }
}
