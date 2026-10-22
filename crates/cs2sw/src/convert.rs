//! The mapping passes: CS core content -> `.sw` text. The converter maps, it
//! never interprets: every emission is a table-driven transformation of the
//! source shape, every construct the language cannot carry becomes a named
//! gap row, and every iteration is sorted (or content-ordered for recipes,
//! which is CS's own first-match order).

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
    /// Dead census: emitted recipes on uncommittable, non-ambient verbs with
    /// no incoming `start` and no inline coverage, and the verbs whose every
    /// recipe is dead.
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

/// How deep same-context linked continuations inline into the parent's
/// effect list. One level carries the classic case (`needs` ->
/// `discardseason` sees the funds card bound in the expenses slot); deeper
/// CS continuations are sequential situation executions and open fresh —
/// inlining the whole tree would duplicate every branch exponentially.
const INLINE_LINKED_DEPTH: usize = 1;

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
        started: BTreeSet::new(),
        inlined: BTreeSet::new(),
        dead_verb_detail: Vec::new(),
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
    /// (verb id, CS recipe id) targets of emitted `start` effects (the census).
    started: BTreeSet<(String, String)>,
    /// CS recipe ids whose behavior was inlined as a same-context linked
    /// continuation (the census counts them reached).
    inlined: BTreeSet<String>,
    /// Per dead verb: (verb id, dead recipe count).
    dead_verb_detail: Vec<(String, usize)>,
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
    fn verb_of(&self, action: &str, craftable: bool, warmup: i64) -> Option<String> {
        if action == "(none)" {
            return None;
        }
        Some(if PLAYER_VERBS.contains(&action) {
            if craftable {
                action.to_string()
            } else {
                format!("{action}_chains")
            }
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
            .map(|r| {
                s(r, "actionid")
                    .or_else(|| s(r, "actionId"))
                    .unwrap_or("(none)")
                    .to_string()
            })
            .unwrap_or_else(|| "(none)".to_string())
    }

    /// The positive requirement arms of one recipe slot: the `required` map
    /// (CS matches any of its arms) plus `essential` keys, in sorted order.
    fn slot_positive_arms(sl: &Value) -> Vec<(String, i64)> {
        let mut arms = pairs(sl, "required");
        arms.extend(pairs(sl, "essential"));
        arms.sort();
        arms.dedup();
        arms
    }

    /// The positive arms of a recipe's requirement dict, sorted.
    fn requirement_arms(r: &Value) -> Vec<(String, i64)> {
        pairs(r, "requirements")
            .into_iter()
            .filter(|(_, v)| *v > 0)
            .collect()
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
        }

        for (rid, r) in &self.d.recipes {
            // The single-offer-slot flattening: a requirement dict with
            // several keys cannot route all its cards through the one offer
            // slot, so its positive arms keep the three-scope union in
            // `when` — the world arm is the documented approximation.
            let reqs = pairs(r, "requirements");
            if reqs.len() > 1 && reqs.iter().any(|(_, v)| *v > 0) {
                self.gaps.hit(
                    "multi-key requirement dict keeps the world arm",
                    Class::ConverterIdiom,
                    "the offer slot binds one card, so a multi-key requirement dict cannot drop the world arm from `when` like a single-key dict does; a tabletop card can satisfy one arm of the dict",
                    rid,
                );
            }
            // The world-scope flattening for the context dicts: CS reads
            // tablereqs against AspectsOnTable (World spheres plus every
            // non-greedy threshold and output sphere — Sphere.IsExteriorSphere
            // and its ThresholdSphere/OutputSphere overrides) and extantreqs
            // against AspectsExtant (the table pool plus every situation's
            // storage/greedy-threshold contents, verb aspects, and running
            // recipe aspects — HornedAxe.GetAspectsInContext). The language's
            // `world` scope covers only unbound public cards; a card locked
            // in another situation's slot is invisible to it.
            if r.get("tablereqs").is_some() || r.get("extantreqs").is_some() {
                self.gaps.hit(
                    "tablereqs/extantreqs read the world scope",
                    Class::ConverterIdiom,
                    "CS's AspectsOnTable/AspectsExtant pools include cards locked in situation slots, situation storage, and running-recipe aspects; the language has no cross-situation scope, so `total(world, ...)` under-counts by exactly the bound set",
                    rid,
                );
            }
            for dk in ["requirements", "tablereqs", "extantreqs"] {
                for (k, _) in pairs(r, dk) {
                    self.registry.insert(k.clone());
                    self.note_group_read(&k);
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
        self.induce_ids.sort();

        // Verb assignment and ending-variant scan.
        for (rid, r) in &self.d.recipes {
            let action = s(r, "actionid")
                .or_else(|| s(r, "actionId"))
                .unwrap_or("(none)");
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

        // Disjunctive slot filters: a slot whose positive requirement arms
        // exceed one matches any arm (CS any-of), which the query grammar
        // carries as a unary derived predicate over the candidate card.
        // Predicates share the one global namespace, so their ids reserve
        // here, in per-verb content order.
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
                    let arms = Self::slot_positive_arms(sl);
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
            }
        }

        self.stats.aspect_elements = self.aspect_ids.len();
        self.stats.aspect_keys = self.names.key.len();
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
        self.emit_verbs(&mut out);
        out.join("\n") + "\n"
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
            // card keeps its contents instead of burning them.
            let has_storage_slot = e
                .get("slots")
                .and_then(|x| x.as_array())
                .map(|a| a.iter().any(|sl| !truthy(sl, "consumes")))
                .unwrap_or(false);
            if has_storage_slot {
                self.gaps.hit(
                    "non-consuming element slot",
                    Class::ConverterIdiom,
                    "storage-only element slots bind nothing in the map; where the behavior matters it is expressible via a card on complete book (consuming slots are emitted as books)",
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

            // Consuming element slots -> an unqualified completion book that
            // burns one bound card matching the slot filter. Non-consuming
            // (storage-only) slots bind nothing and are reported.
            let slots: Vec<Vec<String>> = e
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
                            keys
                        })
                        .collect()
                })
                .unwrap_or_default();
            for keys in slots {
                if keys.is_empty() {
                    self.gaps.hit(
                        "consuming element slot without a filter",
                        Class::NonGoal,
                        "nothing identifies the burned card; no rule is emitted",
                        &id,
                    );
                    continue;
                }
                let k = self.key(&keys[0]);
                let rule = format!("cons_{name}_{}", mangle(&keys[0]));
                let body = vec![
                    format!("if count(interior, aspect:{k}) >= 1"),
                    format!("  consume interior aspect:{k} 1"),
                ];
                push_rule(&mut books, &mut rule_ids, None, rule, body);
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
    }

    fn emit_recipe(
        &mut self,
        out: &mut Vec<String>,
        verb: &str,
        rid: &str,
        local: &str,
        index: usize,
        r: &Value,
    ) {
        let craftable = self.craftable(r);
        let on_player = PLAYER_VERBS.contains(&verb);
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

        if let Some(when) = self.when_of(r) {
            out.push(format!("      when {when}"));
        }

        let mut slot_lines: Vec<String> = Vec::new();
        if on_player && craftable {
            // The offer slot routes the requirement-bearing card into the
            // situation when the requirement dict is a single conjunct, so
            // the completing completion binds the card its books key on.
            let arms = Self::requirement_arms(r);
            let mut offer = String::from("offer: 0..1 open");
            if arms.len() == 1 {
                let (k, v) = &arms[0];
                offer.push(' ');
                offer.push_str(&self.arm_term(k, *v));
            }
            slot_lines.push(offer);
        }
        let mut taken: BTreeSet<String> = BTreeSet::new();
        taken.insert("offer".to_string());
        for sl in r
            .get("slots")
            .and_then(|x| x.as_array())
            .into_iter()
            .flatten()
        {
            let raw = s(sl, "id").unwrap_or("slot");
            let base = {
                let b = mangle(raw);
                if b.is_empty() {
                    "slot".to_string()
                } else {
                    b
                }
            };
            let mut sname = base.clone();
            let mut i = 2;
            while !taken.insert(sname.clone()) {
                sname = format!("{base}_{i}");
                i += 1;
            }
            let mut line = format!("{sname}: 0..1");
            if truthy(sl, "greedy") {
                line.push_str(" greedy");
            }
            if truthy(sl, "consumes") {
                line.push_str(" consuming");
            }
            line.push_str(" open");
            let mut terms: Vec<String> = Vec::new();
            // The positive arms match any-of in CS; a single arm is a plain
            // filter term, several arms become a derived predicate call.
            let arms = Self::slot_positive_arms(sl);
            if arms.len() == 1 {
                let (k, v) = &arms[0];
                terms.push(self.arm_term(k, *v));
            } else if arms.len() > 1 {
                if let Some(pred) = self.pred_names.get(&(rid.to_string(), raw.to_string())) {
                    terms.push(format!("{pred}()"));
                }
            }
            for (k, _) in pairs(sl, "forbidden") {
                terms.push(format!("-aspect:{}", self.key(&k)));
            }
            if !terms.is_empty() {
                line.push(' ');
                line.push_str(&terms.join(" "));
            }
            slot_lines.push(line);
        }
        if !slot_lines.is_empty() {
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

        let plain_alts: Vec<Value> = entries(r, "alt")
            .into_iter()
            .filter(|e| !(e.is_object() && truthy(e, "additional")))
            .collect();
        let mut stack: Vec<String> = vec![rid.to_string()];
        self.emit_alt_cascade(&mut body, &plain_alts, 0, ind, r, rid, &mut stack);

        for (i, line) in body {
            out.push(format!("{}{}", " ".repeat(i), line));
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

    fn pos3(&self, k: &str, v: i64) -> String {
        let k = self.key(k);
        format!("(total(interior, aspect:{k}) >= {v} or total(slot:offer, aspect:{k}) >= {v} or total(world, aspect:{k}) >= {v})")
    }

    /// The interior+offer sum arm for a single-key positive requirement: CS
    /// checks requirements against the situation's combined interior
    /// dictionary — `Situation.GetAspects` sums every interior sphere
    /// (thresholds, storage, output) plus the verb's aspects into one
    /// dictionary, so the two counts add before the comparison — never the
    /// world total (a tabletop card would hijack routing).
    fn pos2(&self, k: &str, v: i64) -> String {
        let k = self.key(k);
        format!("(total(interior, aspect:{k}) + total(slot:offer, aspect:{k})) >= {v}")
    }

    /// The interior+offer sum arm for a negative requirement: the same
    /// combined interior dictionary — the two counts add before the
    /// comparison. The sum form degrades gracefully
    /// for multi-key dicts: the worst case is a recipe CS would refuse
    /// becomes openable, never the reverse.
    fn neg_sum(&self, k: &str, n: i64) -> String {
        let k = self.key(k);
        format!("(total(interior, aspect:{k}) + total(slot:offer, aspect:{k})) < {n}")
    }

    /// CS CanExecuteInContext over the three requirement dicts only: the
    /// recipe `when`. Slot requirements do not gate the context — slot
    /// routing lives in the slot filters (the offer slot filter and the
    /// slot queries).
    fn when_of(&self, r: &Value) -> Option<String> {
        let mut parts: Vec<String> = Vec::new();
        let reqs = pairs(r, "requirements");
        // A single-key requirement adds the situation interior and the bound
        // offer card into one count, mirroring CS's single combined
        // dictionary; the world total never enters. A multi-key dict keeps
        // the three-scope union — the one offer slot cannot bind several
        // cards (the flattening gap row covers the approximation).
        let single = reqs.len() == 1;
        for (k, v) in &reqs {
            if *v > 0 {
                if single {
                    parts.push(self.pos2(k, *v));
                } else {
                    parts.push(self.pos3(k, *v));
                }
            } else if *v < 0 {
                // An anti-requirement in the requirements dict reads the
                // situation's own aspect pool (interior spheres plus the
                // bound offer card) as an arithmetic sum; the world total
                // would make e.g. a rite on the table brick every job.
                parts.push(self.neg_sum(k, -*v));
            }
        }
        for dk in ["tablereqs", "extantreqs"] {
            for (k, v) in pairs(r, dk) {
                let kk = self.key(&k);
                if v > 0 {
                    parts.push(format!("total(world, aspect:{kk}) >= {v}"));
                } else if v < 0 {
                    parts.push(format!("total(world, aspect:{kk}) < {}", -v));
                }
            }
        }
        if parts.is_empty() {
            return None;
        }
        Some(
            parts
                .iter()
                .map(|p| format!("({p})"))
                .collect::<Vec<_>>()
                .join(" and "),
        )
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

    /// CS chance/challenges gate: (roll line, condition over the binding).
    /// CS's hidden roll becomes roll 1d100 plus the aspect-banded success
    /// table; the binding is visible to the if-condition only.
    fn chance_gate(&mut self, e: &Value, rid: &str) -> (Option<String>, Option<String>) {
        let chance = e.get("chance").and_then(|x| x.as_i64()).unwrap_or(100);
        let challenges = e.get("challenges").and_then(|x| x.as_object()).cloned();
        if chance >= 100 && challenges.is_none() {
            return (None, None);
        }
        let bind = self.roll_bind();
        let mut arms: Vec<String> = Vec::new();
        if (0..100).contains(&chance) {
            arms.push(format!("{bind} <= {chance}"));
        }
        let mut ch: Vec<(String, String)> = challenges
            .map(|o| {
                o.into_iter()
                    .map(|(k, v)| (k, v.as_str().unwrap_or("stealth").to_string()))
                    .collect()
            })
            .unwrap_or_default();
        ch.sort();
        for (ak, kind) in &ch {
            let k = self.key(ak);
            let table: &[(i64, i64)] = if kind == "base" {
                &[(1, 30), (5, 70), (10, 90)]
            } else {
                &[(5, 10), (10, 30), (15, 70), (20, 90)]
            };
            for (i, (lo, val)) in table.iter().enumerate() {
                let mut band = format!("total(bound, aspect:{k}) >= {lo}");
                if let Some((hi, _)) = table.get(i + 1) {
                    band.push_str(&format!(" and total(bound, aspect:{k}) < {hi}"));
                }
                arms.push(format!("({band} and {bind} <= {val})"));
            }
        }
        if arms.is_empty() {
            return (None, None);
        }
        self.gaps.hit(
            "chance/challenges gate mapped to a roll",
            Class::ConverterIdiom,
            "CS's hidden roll becomes roll 1d100 plus the aspect-banded success table; deviation: CS rolls each challenge separately and requires every challenge to pass, while the map rolls one shared 1d100 and ORs the aspect bands (shared-roll, ANY vs ALL)",
            rid,
        );
        (
            Some(format!("roll 1d100 as {bind}")),
            Some(format!("({})", arms.join(" or "))),
        )
    }

    /// Record a `start` target for the dead census.
    fn note_start(&mut self, tid: &str) {
        if let Some(host) = self.host.get(tid) {
            self.started.insert((host.clone(), tid.to_string()));
        }
    }

    /// The non-additional alt cascade: the first executable entry replaces the
    /// recipe's own effects (CS alternative resolution), so each arm guards a
    /// `start` and the deepest `else` carries the recipe's own body — which
    /// includes its linked cascade.
    #[allow(clippy::too_many_arguments)]
    fn emit_alt_cascade(
        &mut self,
        body: &mut Vec<(usize, String)>,
        alts: &[Value],
        i: usize,
        ind: usize,
        r: &Value,
        rid: &str,
        stack: &mut Vec<String>,
    ) {
        if i >= alts.len() {
            self.emit_core(body, ind, r, rid, stack);
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
            self.emit_alt_cascade(body, alts, i + 1, ind, r, rid, stack);
            return;
        };
        let Some(target) = self.recipe_target(&tid) else {
            self.emit_alt_cascade(body, alts, i + 1, ind, r, rid, stack);
            return;
        };
        let gate = self.d.recipe(&tid).and_then(|t| self.when_of(t));
        let (roll_line, cond) = self.chance_gate(&e, rid);
        let gates: Vec<String> = gate.into_iter().chain(cond).collect();
        if let Some(rl) = roll_line {
            body.push((ind, rl));
        }
        if gates.is_empty() {
            // An unconditional alternative fully replaces the recipe.
            body.push((ind, format!("start {} via {}", target.0, target.1)));
            self.note_start(&tid);
            self.gaps.hit(
                "unconditional alt replaces the recipe body",
                Class::ConverterIdiom,
                "CS resolves the alternative over the recipe's own effects; the core body never runs",
                rid,
            );
            return;
        }
        if gates.len() > 1 {
            body.push((ind, format!("if ({})", gates.join(") and ("))));
        } else {
            body.push((ind, format!("if {}", gates[0])));
        }
        body.push((ind + 2, format!("start {} via {}", target.0, target.1)));
        self.note_start(&tid);
        body.push((ind, "else".into()));
        self.emit_alt_cascade(body, alts, i + 1, ind + 2, r, rid, stack);
    }

    /// The recipe's own resolution body, in CS completion order: the
    /// additional-alt gates (RequiresExecutionState.Continue's first
    /// statement, GetValidOverride), then mutations, deck draws, and the
    /// effects table (RecipeCompletionEffectCommand.Execute), then the
    /// ending, then the linked cascade and the inductions.
    fn emit_core(
        &mut self,
        body: &mut Vec<(usize, String)>,
        ind: usize,
        r: &Value,
        rid: &str,
        stack: &mut Vec<String>,
    ) {
        // Additional alts: CS decides their gates in GetValidOverride, before
        // the recipe's own mutations and effects and before any linked
        // continuation's effects run (the starvation anti-requirement must
        // still see the funds card the expenses slot holds). The start both
        // gates and spawns, so the start lines go here — ahead of everything
        // this recipe does (CS spawns the queued additions only at the
        // inductions pass; one `start` line cannot split gate from spawn, so
        // the gate — the semantics-carrying half — keeps CS's position).
        self.emit_additional_starts(body, ind, r, rid);

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
            body.push((ind, format!("set aspect:{fk} aspect:{mk} {op}")));
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
                let n = v.clamp(1, 20);
                body.push((ind, format!("spawn {pname} count {n} into table")));
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

        // linked: CS walks the entries in order and executes the first whose
        // requirements pass; later entries never run.
        let linked = entries(r, "linked");
        self.emit_linked_cascade(body, &linked, 0, ind, rid, stack);

        // Inductions: CS gates them after the effects (the inductions pass
        // runs post-ExecuteCurrentRecipe), so they stay behind the linked
        // cascade.
        self.emit_induction_starts(body, ind, r, rid);
    }

    /// The exclusive first-executable cascade over a linked list. Each arm
    /// guards the entry's target `when` plus its chance roll; a same-context
    /// target (same CS action id) inlines its behavior into the parent's
    /// effect list — the continuation runs in the parent situation with the
    /// parent's bindings in scope — and a cross-context target gates a
    /// `start` of the other verb's situation.
    fn emit_linked_cascade(
        &mut self,
        body: &mut Vec<(usize, String)>,
        linked: &[Value],
        i: usize,
        ind: usize,
        rid: &str,
        stack: &mut Vec<String>,
    ) {
        if i >= linked.len() {
            return;
        }
        let e = linked[i].clone();
        let Some(tid) = entry_id(&e) else {
            self.gaps.hit(
                "linked entry without an id",
                Class::NonGoal,
                "the entry names no recipe; it is dropped",
                rid,
            );
            self.emit_linked_cascade(body, linked, i + 1, ind, rid, stack);
            return;
        };
        if tid == rid {
            self.gaps.hit(
                "self-link (CS situation persistence)",
                Class::ConverterIdiom,
                "player-verb self-links re-commit (the player re-runs the verb); only ambient-verb self-links ride the heartbeat; no start is emitted",
                rid,
            );
            self.emit_linked_cascade(body, linked, i + 1, ind, rid, stack);
            return;
        }
        let Some(target) = self.recipe_target(&tid) else {
            self.emit_linked_cascade(body, linked, i + 1, ind, rid, stack);
            return;
        };
        let same_context = self.action_of(rid) == self.action_of(&tid);
        let gate = self.d.recipe(&tid).and_then(|t| self.when_of(t));
        let (roll_line, cond) = self.chance_gate(&e, rid);
        let gates: Vec<String> = gate.into_iter().chain(cond).collect();
        if let Some(rl) = roll_line {
            body.push((ind, rl));
        }
        // Same-context continuations run in the parent situation, but only
        // the first continuation level inlines; deeper levels open fresh.
        let inline = same_context && stack.len() <= INLINE_LINKED_DEPTH;
        if gates.is_empty() {
            // The first executable entry always runs; later entries never do.
            if inline {
                self.emit_target_inline(body, ind, &tid, stack);
            } else {
                if same_context {
                    self.gaps.hit(
                        "deep linked continuation starts fresh",
                        Class::ConverterIdiom,
                        "the inline map carries the parent context one continuation deep; deeper same-context linked steps open a fresh situation and parent-bound cards do not carry over",
                        &format!("{rid}:{tid}"),
                    );
                }
                body.push((ind, format!("start {} via {}", target.0, target.1)));
                self.note_start(&tid);
            }
            return;
        }
        if gates.len() > 1 {
            body.push((ind, format!("if ({})", gates.join(") and ("))));
        } else {
            body.push((ind, format!("if {}", gates[0])));
        }
        if inline {
            self.emit_target_inline(body, ind + 2, &tid, stack);
        } else {
            if same_context {
                self.gaps.hit(
                    "deep linked continuation starts fresh",
                    Class::ConverterIdiom,
                    "the inline map carries the parent context one continuation deep; deeper same-context linked steps open a fresh situation and parent-bound cards do not carry over",
                    &format!("{rid}:{tid}"),
                );
            } else {
                self.gaps.hit(
                    "cross-context linked step starts fresh",
                    Class::ConverterIdiom,
                    "cross-verb linked steps run in a fresh situation (context handoff is explicit in CS via slot re-binding); parent-bound cards do not carry over",
                    &format!("{rid}:{tid}"),
                );
            }
            body.push((ind + 2, format!("start {} via {}", target.0, target.1)));
            self.note_start(&tid);
        }
        body.push((ind, "else".into()));
        self.emit_linked_cascade(body, linked, i + 1, ind + 2, rid, stack);
    }

    /// Inline a same-context linked continuation: the target recipe's
    /// behavior runs as effects of the parent completion, so the parent's
    /// bound cards stay in scope (the classic `needs` -> `discardseason`
    /// funds wipe sees the funds card bound in the expenses slot). Only the
    /// first continuation level reaches here.
    fn emit_target_inline(
        &mut self,
        body: &mut Vec<(usize, String)>,
        ind: usize,
        tid: &str,
        stack: &mut Vec<String>,
    ) {
        if stack.iter().any(|s| s == tid) {
            self.gaps.hit(
                "linked continuation re-enters the chain",
                Class::ConverterIdiom,
                "CS re-runs the linked loop over sequential situation executions; the inline map runs each context once and cuts re-entry (player verbs re-commit, the ambient heartbeat re-opens)",
                &format!("{}->{tid}", stack.last().map(String::as_str).unwrap_or("?")),
            );
            return;
        }
        let Some(r) = self.d.recipe(tid).cloned() else {
            return;
        };
        stack.push(tid.to_string());
        if let Some(sd) = s(&r, "startdescription") {
            let t = escape_sw(sd);
            if !t.is_empty() {
                body.push((ind, format!("narrate \"{t}\"")));
            }
        }
        let plain_alts: Vec<Value> = entries(&r, "alt")
            .into_iter()
            .filter(|e| !(e.is_object() && truthy(e, "additional")))
            .collect();
        // The alt cascade's deepest else carries the target's own body: its
        // effects, its linked cascade, and its additional starts.
        self.emit_alt_cascade(body, &plain_alts, 0, ind, &r, tid, stack);
        stack.pop();
        self.inlined.insert(tid.to_string());
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
        let (roll_line, cond) = self.chance_gate(e, rid);
        if let Some(rl) = roll_line {
            body.push((ind, rl));
        }
        match cond {
            Some(cl) => {
                body.push((ind, format!("if {cl}")));
                body.push((ind + 2, format!("start {} via {}", target.0, target.1)));
            }
            None => body.push((ind, format!("start {} via {}", target.0, target.1))),
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
        match default {
            Some(cname) if v > 0 => {
                let n = v.clamp(1, 20);
                body.push((ind, format!("spawn {cname} count {n} into table")));
                self.gaps.hit(
                    "lever redirect spawns the lever default",
                    Class::ConverterIdiom,
                    "CS resolves the lever key against cross-run history; a single run lands on defaultValue",
                    &format!("{rid}:{key}"),
                );
            }
            _ => {
                self.gaps.hit(
                    "lever effect with no resolvable default",
                    Class::ExtensionSeam,
                    "cross-run card redirects need a legacy-to-card seam; the effect is dropped. Designed disposition (recommended idiom): an autonomous legacy-gated resolver verb — `when legacy.<lever> >= n` choosing the spawn — kept as the one genuine seam",
                    &format!("{rid}:{key}"),
                );
            }
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
    /// same-context linked continuation inlines — plus the verbs whose every
    /// recipe is dead.
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
                    || self.inlined.contains(rid);
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
