//! L1 Dictionary (spec §4, §9.14 reserved words,
//! §4.4 normative outcome arities).

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use crate::core::regex_util::static_regex;
use crate::error::{ChorusError, Result};

/// The twelve core verbs every script gets for free (spec §4).
pub const CORE_VERBS: [&str; 12] = [
    "READ",
    "STUDY",
    "WORK",
    "TALK",
    "EXPLORE",
    "MANSUS",
    "TRAVEL_TO",
    "LOCATE_AT",
    "DREAM",
    "USE_ON",
    "ACQUIRE",
    "WAIT",
];

/// Noun classes, spec §4.1.
pub const KINDS: [&str; 8] =
    ["character", "place", "item", "text", "idea", "state", "rite", "resource"];

/// The reserved list: 18 keywords + every
/// CORE_VERB appended (spelled out; a unit test below pins the tail equal to
/// CORE_VERBS so the double-write cannot drift).
pub const RESERVED_WORDS: [&str; 30] = [
    "EVENT",
    "TO",
    "WITH",
    "AT",
    "ABOUT",
    "INTO",
    "AS",
    "AND",
    "OR",
    "NOT",
    "IF",
    "ELSE", // §9.39: conditional outcome lists (v0.3.2)
    "ROLL",
    "ROLLED", // §9.41: the dice op, and the engine-written record of one (v0.3.4)
    "DRAW",
    "DRAWN",    // §9.42: the deck-draw op, and the engine-written record of one (v0.3.5)
    "RUN", // §9.47: the recipe-chain op — authored, zero-concrete (the follow-up's BEGIN row is the record)
    "COMPLETE", // v0.5: the engine-written completion row of a processing window
    "READ",
    "STUDY",
    "WORK",
    "TALK",
    "EXPLORE",
    "MANSUS",
    "TRAVEL_TO",
    "LOCATE_AT",
    "DREAM",
    "USE_ON",
    "ACQUIRE",
    "WAIT",
];

/// The outcome op vocabulary. An outcome's `op` is kept as a plain
/// `String` (the engine's unknown-op throw needs the raw text) with this
/// array as the single membership/append-order
/// source. Declaration order is load-bearing: the unknown-call error message
/// (fn_expand) joins it in this order.
pub const OUTCOME_OPS: [&str; 18] = [
    "SPAWN", "DESTROY", "REPLACE", "SET", "MOVE", "REVEAL", "ADVANCE", "NARRATE", "PROGRESS",
    "RELATE", "UNRELATE", "REFUSE", "ENDING",
    "ROLL",   // authored: ROLL(count, sides[, bonus]) — concretizes to ROLLED (§9.41)
    "ROLLED", // engine-written ONLY: ROLLED(total, ctrAfter) — the fold's counter row (§9.41)
    "DRAW",   // authored: DRAW <DECK> [INTO target] — concretizes to DRAWN+SPAWN (§9.42)
    "DRAWN",  // engine-written ONLY: DRAWN(<DECK>, <name>) — the fold's cursor row (§9.42)
    "RUN", // authored: RUN "recipe-id" [, …] — zero-concrete; the follow-up's BEGIN row is the record (§9.47)
];

/// Definition of a relation type: whether its edges are symmetric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationTypeDef {
    pub symmetric: bool,
}

/// The nameable-word registry: verbs, relation types, reserved words. All
/// three keep insertion order (IndexSet/IndexMap).
#[derive(Debug, Clone)]
pub struct Dictionary {
    pub verbs: IndexSet<String>,
    pub relation_types: IndexMap<String, RelationTypeDef>,
    pub reserved: IndexSet<String>,
}

impl Default for Dictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl Dictionary {
    /// New dictionary with `reserved` pre-populated from RESERVED_WORDS.
    #[must_use]
    pub fn new() -> Dictionary {
        Dictionary {
            verbs: IndexSet::new(),
            relation_types: IndexMap::new(),
            reserved: RESERVED_WORDS.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// Register a verb lexeme (must be nameable; also added to `reserved`).
    pub fn register_verb(&mut self, lexeme: &str) -> Result<()> {
        self.assert_nameable(lexeme, "verb")?;
        self.verbs.insert(lexeme.to_string());
        self.reserved.insert(lexeme.to_string());
        Ok(())
    }

    /// NO `assert_nameable` check here: relation registration is
    /// unvalidated by design; the asymmetry with verbs is frozen (an
    /// arbitrary string is a legal relation type).
    pub fn register_relation(&mut self, type_: impl Into<String>, def: RelationTypeDef) {
        self.relation_types.insert(type_.into(), def);
    }

    /// Verb membership test.
    #[must_use]
    pub fn is_verb(&self, tok: &str) -> bool {
        self.verbs.contains(tok)
    }

    /// Outcome-op membership over OUTCOME_OPS.
    #[must_use]
    pub fn is_op(&self, tok: &str) -> bool {
        OUTCOME_OPS.contains(&tok)
    }

    /// Look up a relation type's definition, or error if unregistered.
    pub fn relation_def(&self, type_: &str) -> Result<RelationTypeDef> {
        self.relation_types.get(type_).copied().ok_or_else(|| {
            ChorusError::new(format!(
                "unknown relation type '{type_}' (register it in the dictionary)"
            ))
        })
    }

    /// Reserved words cannot be registered as names/verbs
    /// (spec §5.1, §9.14).
    fn assert_nameable(&self, lexeme: &str, what: &str) -> Result<()> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = static_regex(&RE, || Regex::new(r"^[A-Z][A-Z0-9_]*$"));
        if !re.is_match(lexeme) {
            return Err(ChorusError::new(format!("{what} '{lexeme}' must be UPPER_SNAKE")));
        }
        if self.verbs.contains(lexeme) {
            return Err(ChorusError::new(format!("{what} '{lexeme}' collides with a verb")));
        }
        Ok(())
    }
}

/// The built-in dictionary: the core verbs registered plus the eight
/// built-in relation types.
#[must_use]
pub fn core_dictionary() -> Dictionary {
    let mut d = Dictionary::new();
    for v in CORE_VERBS {
        // CORE_VERBS are UPPER_SNAKE and mutually unique — registration
        // cannot fail; a failure would be a defect in this literal table.
        if let Err(e) = d.register_verb(v) {
            unreachable!("core verb '{v}' failed to register: {e}");
        }
    }
    d.register_relation("parent_child", RelationTypeDef { symmetric: true });
    d.register_relation("parent_of", RelationTypeDef { symmetric: false });
    d.register_relation("neighbor", RelationTypeDef { symmetric: true });
    // the four directions of the table: `A is south of B` — stored edges keep
    // their authored direction, so a layout can be read back from the log
    d.register_relation("north", RelationTypeDef { symmetric: false });
    d.register_relation("south", RelationTypeDef { symmetric: false });
    d.register_relation("east", RelationTypeDef { symmetric: false });
    d.register_relation("west", RelationTypeDef { symmetric: false });
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_words_tail_is_core_verbs_and_core_dict_registers_clean() {
        // the appended CORE_VERBS tail, pinned so the double-write cannot drift
        assert_eq!(RESERVED_WORDS[RESERVED_WORDS.len() - CORE_VERBS.len()..], CORE_VERBS);
        let d = core_dictionary();
        assert_eq!(d.verbs.len(), CORE_VERBS.len());
        assert!(d.is_verb("READ"));
        assert!(d.is_op("SPAWN"));
        assert!(!d.is_op("READ")); // verbs are not ops
        assert!(d.reserved.contains("IF"));
        assert_eq!(d.relation_def("parent_child").map(|def| def.symmetric), Ok(true));
        assert_eq!(d.relation_def("parent_of").map(|def| def.symmetric), Ok(false));
        let err = d.relation_def("nope").unwrap_err();
        assert_eq!(err.msg, "unknown relation type 'nope' (register it in the dictionary)");
    }

    #[test]
    fn assert_nameable_messages_are_byte_exact() {
        let mut d = Dictionary::new();
        let e1 = d.register_verb("lower").unwrap_err();
        assert_eq!(e1.msg, "verb 'lower' must be UPPER_SNAKE");
        d.register_verb("VALID").unwrap();
        let e2 = d.register_verb("VALID").unwrap_err();
        assert_eq!(e2.msg, "verb 'VALID' collides with a verb");
        // `register_relation` has NO validation — frozen asymmetry
        d.register_relation("x y z", RelationTypeDef { symmetric: true });
        assert!(d.relation_types.contains_key("x y z"));
    }
}
