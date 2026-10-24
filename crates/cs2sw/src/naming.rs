//! Bijective, stable naming: CS ids map to `lower_snake` identifiers through
//! one mangling rule and one deterministic dedupe chain. Aspect keys (their
//! own registry) and global ids (cards, verbs, decks, zones, endings — the
//! one namespace) are two independent taken-sets.

use std::collections::{BTreeMap, BTreeSet};

/// Mangle a CS id to a `lower_snake` identifier: lowercase, non-identifier
/// characters become `_`, runs collapse, edges trim, a digit-led id gains an
/// `a` prefix, the empty mangle becomes `x`.
pub fn mangle(raw: &str) -> String {
    let mut s = String::new();
    for ch in raw.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' {
            s.push(ch);
        } else {
            s.push('_');
        }
    }
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    let s = if s.is_empty() { "x".to_string() } else { s };
    if s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        format!("a{s}")
    } else {
        s
    }
}

/// The two identifier registries.
#[derive(Default)]
pub struct Names {
    /// The one global namespace: zones, verbs, decks, endings, cards.
    taken: BTreeSet<String>,
    /// Aspect keys (a separate declared registry).
    keys_taken: BTreeSet<String>,
    /// Collisions seen while deduping, for the report.
    pub collisions: Vec<(String, String)>,

    pub card: BTreeMap<String, String>,
    pub key: BTreeMap<String, String>,
    pub verb: BTreeMap<String, String>,
    pub deck: BTreeMap<String, String>,
    pub ending: BTreeMap<String, String>,
    /// Ending flavour variants: (ending id, flavour) -> id.
    pub ending_variant: BTreeMap<(String, String), String>,
    /// Recipe local name per (verb id, CS recipe id).
    pub recipe: BTreeMap<(String, String), String>,
    pub group: BTreeMap<String, String>,
}

impl Names {
    pub fn new() -> Self {
        let mut n = Names::default();
        n.reserve_id("table");
        n
    }

    fn dedupe(&mut self, base: String, suffix: &str) -> String {
        if !self.taken.contains(&base) {
            self.taken.insert(base.clone());
            return base;
        }
        let mut i = 2;
        loop {
            let candidate = format!("{base}_{suffix}{i}");
            if !self.taken.contains(&candidate) {
                self.taken.insert(candidate.clone());
                return candidate;
            }
            i += 1;
        }
    }

    /// Reserve one id in the global namespace, deduping on collision.
    pub fn reserve_id(&mut self, raw: &str) -> String {
        let base = mangle(raw);
        if !self.taken.contains(&base) {
            self.taken.insert(base.clone());
            return base;
        }
        let name = self.dedupe(base.clone(), "x");
        self.collisions.push((raw.to_string(), name.clone()));
        name
    }

    /// Reserve one aspect key, deduping inside the key registry.
    pub fn reserve_key(&mut self, raw: &str) -> String {
        let base = mangle(raw);
        if let Some(existing) = self.key.get(raw) {
            return existing.clone();
        }
        let name = if !self.keys_taken.contains(&base) {
            self.keys_taken.insert(base.clone());
            base.clone()
        } else {
            let mut i = 2;
            loop {
                let candidate = format!("{base}_k{i}");
                if !self.keys_taken.contains(&candidate) {
                    self.keys_taken.insert(candidate.clone());
                    break candidate;
                }
                i += 1;
            }
        };
        if name != base {
            self.collisions.push((raw.to_string(), name.clone()));
        }
        self.key.insert(raw.to_string(), name.clone());
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mangles_deterministically() {
        assert_eq!(mangle("influencewinter"), "influencewinter");
        assert_eq!(mangle("portal.mansus"), "portal_mansus");
        assert_eq!(mangle("1_debug"), "a1_debug");
        assert_eq!(mangle("__weird--id__"), "weird_id");
        assert_eq!(mangle("---"), "x");
    }

    #[test]
    fn dedupes_bijectively() {
        let mut n = Names::new();
        assert_eq!(n.reserve_id("work"), "work");
        assert_eq!(n.reserve_id("work"), "work_x2");
        assert_eq!(n.reserve_id("work"), "work_x3");
    }
}
