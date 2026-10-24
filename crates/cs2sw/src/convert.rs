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

use source::{pairs, s, PLAYER_VERBS, TIME_VERB};

pub use gaps::{Class, Conversion, GapRow, Gaps, Stats};
pub use source::SEED;

mod chains;
mod census;
mod emit;
mod gaps;
mod inventory;
mod naming;
mod recipe;
mod source;

// ---------------------------------------------------------------------------
// the converter
// ---------------------------------------------------------------------------

pub fn convert(d: &CsData) -> Conversion {
    // An element a recipe's effects write positively but the tree never
    // declares (a DLC-side id the core data leaks) becomes a minimal stub
    // card, so the spawn lands the way it does against CS's full data. Each
    // minted stub is named in the gap report.
    let mut owned = d.clone();
    let tree_elements = d.elements.len();
    let mut stubs: Vec<String> = Vec::new();
    for (_, r) in &owned.recipes {
        for (k, v) in pairs(r, "effects") {
            if v > 0 && !k.starts_with("lever_") && !owned.elements.contains_key(&k)
                && !stubs.contains(&k)
            {
                stubs.push(k);
            }
        }
    }
    for k in &stubs {
        owned
            .elements
            .insert(k.clone(), serde_json::json!({ "id": k, "label": k }));
    }
    let d = &owned;
    let mut c = Converter {
        d,
        names: Names::new(),
        gaps: Gaps::default(),
        stats: Stats {
            elements_in: tree_elements,
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
        started: BTreeSet::new(),
        continued: BTreeSet::new(),
        dead_verb_detail: Vec::new(),
        alt_targets_with_warmup: BTreeSet::new(),
    };
    for k in &stubs {
        c.gaps.hit(
            "tree-absent element stubbed as a card",
            Class::ExtensionSeam,
            "the effect writes an element the loaded tree never declares (DLC-side id); a minimal card is minted so the spawn lands",
            k,
        );
    }
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
    pub(super) fn carriers(&self) -> Vec<String> {
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
    pub(super) fn is_aspect(&self, id: &str) -> bool {
        self.aspect_ids.contains(id)
    }

    pub(super) fn card_name(&self, id: &str) -> Option<String> {
        self.names.card.get(id).cloned()
    }

    pub(super) fn key(&self, id: &str) -> String {
        self.names
            .key
            .get(id)
            .cloned()
            .unwrap_or_else(|| mangle(id))
    }

    pub(super) fn aspects_of(&self, id: &str) -> Vec<(String, i64)> {
        self.d
            .element(id)
            .map(|e| pairs(e, "aspects"))
            .unwrap_or_default()
    }

    pub(super) fn craftable(&self, r: &Value) -> bool {
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
    pub(super) fn verb_of(&self, action: &str, _craftable: bool, warmup: i64) -> Option<String> {
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
    pub(super) fn action_of(&self, rid: &str) -> String {
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
    pub(super) fn arm_term(&self, k: &str, v: i64) -> String {
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
    pub(super) fn slot_filter_terms(&self, rid: &str, sl: &Value) -> Vec<String> {
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
    pub(super) fn verb_slot_filter_terms(&self, action: &str, sl: &Value) -> Vec<String> {
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
}
