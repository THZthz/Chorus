//! L1 Dictionary (spec §4, §5.1 reserved words,
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

/// The eight built-in noun classes (§4.1), in suggestion order. The
/// vocabulary itself is EXTENSIBLE — a script's `kinds:` section registers
/// more — so this table only seeds `core_dictionary`; every
/// membership/suggestion read goes through `Dictionary::kinds`. The four
/// semantic roles only the built-ins play are written into §4.1 (player:
/// names a `character`; the actors pool defaults same-kind; `place` zones;
/// the `<someone>` phrasing) — nothing in the engine special-cases a
/// declared kind.
pub const CORE_KINDS: [&str; 8] =
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
    "ELSE", // §6.8: conditional outcome lists (v0.3.2)
    "ROLL",
    "ROLLED", // §4.4: the dice op, and the engine-written record of one (v0.3.4)
    "DRAW",
    "DRAWN",    // §4.4: the deck-draw op, and the engine-written record of one (v0.3.5)
    "RUN", // §4.4: the recipe-chain op — authored, zero-concrete (the follow-up's BEGIN row is the record)
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
    "ROLL",   // authored: ROLL(count, sides[, bonus]) — concretizes to ROLLED (§4.4)
    "ROLLED", // engine-written ONLY: ROLLED(total, ctrAfter) — the fold's counter row (§4.4)
    "DRAW",   // authored: DRAW <DECK> [INTO target] — concretizes to DRAWN+SPAWN (§4.4)
    "DRAWN",  // engine-written ONLY: DRAWN(<DECK>, <name>) — the fold's cursor row (§4.4)
    "RUN", // authored: RUN "recipe-id" [, …] — zero-concrete; the follow-up's BEGIN row is the record (§4.4)
];

/// Definition of a relation type: whether its edges are symmetric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelationTypeDef {
    pub symmetric: bool,
}

/// The nameable-word registry: verbs, kinds, relation types, reserved words.
/// All keep insertion order (IndexSet/IndexMap) — for kinds, insertion order
/// IS the error-message/suggestion order (built-ins first, then `kinds:`
/// declarations in section order).
#[derive(Debug, Clone)]
pub struct Dictionary {
    pub verbs: IndexSet<String>,
    pub kinds: IndexSet<String>,
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
            kinds: IndexSet::new(),
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

    /// Register a kind lexeme (§4.1): `lower_snake`
    /// (`^[a-z][a-z0-9_]*$`), never a registered relation type (the entity
    /// line's relation dispatch precedes the kind check, so such a kind
    /// could never be declared on an entity line), never a duplicate — the
    /// caller owns idempotency, and the script loader rejects re-declaration
    /// loudly (built-ins included), uniform with every other §10 duplicate
    /// rule. Kinds are NOT added to `reserved`: they never collide with the
    /// UPPER_SNAKE name/reserved namespace (§5.1), so an entity may still be
    /// named "character".
    pub fn register_kind(&mut self, lexeme: &str) -> Result<()> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = static_regex(&RE, || Regex::new(r"^[a-z][a-z0-9_]*$"));
        if !re.is_match(lexeme) {
            return Err(ChorusError::new(format!("kind '{lexeme}' must be lower_snake")));
        }
        if self.relation_types.contains_key(lexeme) {
            return Err(ChorusError::new(format!("kind '{lexeme}' collides with a relation type")));
        }
        if self.kinds.contains(lexeme) {
            return Err(ChorusError::new(format!("kind '{lexeme}' is already registered")));
        }
        self.kinds.insert(lexeme.to_string());
        Ok(())
    }

    /// The kind vocabulary, insertion order — the exact list the
    /// unknown-kind errors join into their suggestions (query.rs, decls.rs).
    #[must_use]
    pub fn kinds_list(&self) -> String {
        self.kinds.iter().cloned().collect::<Vec<_>>().join(", ")
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

    /// Outcome-op membership over OUTCOME_OPS. A static test on purpose —
    /// the op vocabulary is fixed, never per-dictionary state; `&self`
    /// exists only so call sites read uniformly next to `is_verb`.
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

    /// Verb lexemes cannot be registered as names/verbs when the shape or
    /// the §5.1 namespace says no (spec §5.1): UPPER_SNAKE only, never a
    /// registered verb, and never a reserved word (§5.1: reserved words
    /// cannot be registered). The collision check precedes the reserved
    /// check (a re-register of
    /// an already-registered verb is a `collides with a verb`, never a
    /// reserved complaint — it IS in `reserved`, registration put it there),
    /// and the reserved check exempts CORE_VERBS: RESERVED_WORDS spells the
    /// core verbs into the seed (the tail the unit test pins), so a core
    /// verb is reserved *by being a verb* — the exemption is what lets
    /// `core_dictionary` register its own table and a caller configure a
    /// core lexeme. It cannot smuggle a duplicate past the collision check
    /// above.
    fn assert_nameable(&self, lexeme: &str, what: &str) -> Result<()> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = static_regex(&RE, || Regex::new(r"^[A-Z][A-Z0-9_]*$"));
        if !re.is_match(lexeme) {
            return Err(ChorusError::new(format!("{what} '{lexeme}' must be UPPER_SNAKE")));
        }
        if self.verbs.contains(lexeme) {
            return Err(ChorusError::new(format!("{what} '{lexeme}' collides with a verb")));
        }
        if self.reserved.contains(lexeme) && !CORE_VERBS.contains(&lexeme) {
            return Err(ChorusError::new(format!("{what} '{lexeme}' is a reserved word (§5.1)")));
        }
        Ok(())
    }
}

/// The built-in dictionary: the core verbs registered plus the eight
/// built-in kinds and the eight built-in relation types.
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
    for k in CORE_KINDS {
        // CORE_KINDS are lower_snake and name no relation type — same
        // literal-table argument as the verbs above.
        if let Err(e) = d.register_kind(k) {
            unreachable!("core kind '{k}' failed to register: {e}");
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
        // the collision check outranks the reserved check: VALID joined
        // `reserved` at its own registration, and the re-register must still
        // read as a verb collision, not a reserved complaint
        assert_eq!(e2.msg, "verb 'VALID' collides with a verb");
        // §5.1: a reserved word cannot be registered (the keyword half of
        // RESERVED_WORDS)
        let e3 = d.register_verb("EVENT").unwrap_err();
        assert_eq!(e3.msg, "verb 'EVENT' is a reserved word (§5.1)");
        let e4 = d.register_verb("ROLL").unwrap_err();
        assert_eq!(e4.msg, "verb 'ROLL' is a reserved word (§5.1)");
        // the CORE_VERBS exemption: a core lexeme is reserved *by being a
        // verb* — a fresh dictionary registers it (a code-level caller
        // configuring a core verb), and against the seeded core dictionary a
        // re-register is the collision above
        let mut fresh = Dictionary::new();
        fresh.register_verb("READ").unwrap();
        assert!(fresh.is_verb("READ"));
        let mut core = core_dictionary();
        assert_eq!(core.register_verb("READ").unwrap_err().msg, "verb 'READ' collides with a verb");
        core.register_verb("SHOUT").unwrap();
        assert!(core.reserved.contains("SHOUT"));
        // `register_relation` has NO validation — frozen asymmetry
        d.register_relation("x y z", RelationTypeDef { symmetric: true });
        assert!(d.relation_types.contains_key("x y z"));
    }

    #[test]
    fn core_kinds_seed_in_suggestion_order_and_are_not_reserved() {
        // the seeds land in table order — the order every unknown-kind
        // suggestion list joins (query.rs, decls.rs)
        let d = core_dictionary();
        let got: Vec<&str> = d.kinds.iter().map(String::as_str).collect();
        assert_eq!(got, CORE_KINDS);
        assert_eq!(d.kinds_list(), "character, place, item, text, idea, state, rite, resource");
        // kinds never join the reserved namespace: the UPPER_SNAKE name
        // grammar and the lower_snake kind grammar cannot collide (§5.1)
        for k in CORE_KINDS {
            assert!(!d.reserved.contains(k));
        }
    }

    #[test]
    fn register_kind_messages_are_byte_exact() {
        let mut d = core_dictionary();
        // the shape rule
        let e1 = d.register_kind("Ritual").unwrap_err();
        assert_eq!(e1.msg, "kind 'Ritual' must be lower_snake");
        let e2 = d.register_kind("3rd").unwrap_err();
        assert_eq!(e2.msg, "kind '3rd' must be lower_snake");
        // the relation-type collision: the entity line dispatches relation
        // types BEFORE kinds (decls.rs), so such a kind is unreachable there
        let e3 = d.register_kind("neighbor").unwrap_err();
        assert_eq!(e3.msg, "kind 'neighbor' collides with a relation type");
        // duplicates — built-ins and fresh registrations alike — reject
        let e4 = d.register_kind("text").unwrap_err();
        assert_eq!(e4.msg, "kind 'text' is already registered");
        d.register_kind("omen").unwrap();
        let e5 = d.register_kind("omen").unwrap_err();
        assert_eq!(e5.msg, "kind 'omen' is already registered");
        assert_eq!(d.kinds.len(), CORE_KINDS.len() + 1);
    }
}
