//! Pass 1: the inventory scan.

use std::collections::{BTreeMap, BTreeSet};

use super::source::{entries, entry_id, num, pairs, s, truthy};
use super::{Class, Converter};

impl<'a> Converter<'a> {
    // -- pass 1: inventory --------------------------------------------------

    pub(super) fn inventory(&mut self) {
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
}
