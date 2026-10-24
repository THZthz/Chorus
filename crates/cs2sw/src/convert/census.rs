//! The dead census: emitted recipes that no route reaches.

use std::collections::{BTreeMap, BTreeSet};

use super::source::{entries, entry_id, PLAYER_VERBS, TIME_VERB};
use super::Converter;

impl<'a> Converter<'a> {
    /// The dead census: emitted recipes that no route reaches — player verbs
    /// commit, the ambient heartbeat re-opens, a `start` opens, or a
    /// `continue` adopts — plus the verbs whose every recipe is dead.
    pub(super) fn census(&mut self) {
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
