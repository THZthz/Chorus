//! Pass 2: naming and the per-recipe slot plans.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::naming::mangle;

use super::source::{pairs, s, truthy, verb_slot_of, PLAYER_VERBS, TIME_VERB};
use super::{Converter, PlannedSlot, RecipePlan, SlotKind};

impl<'a> Converter<'a> {
    // -- pass 2: naming -----------------------------------------------------

    pub(super) fn name_everything(&mut self) {
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
        // bindable.
        for (k, v) in &reqs {
            if *v <= 0 || covered.contains(k) || plan.costs.contains_key(k) {
                continue;
            }
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
    pub(super) fn by_verb(&self) -> Vec<(String, Vec<String>)> {
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
}
