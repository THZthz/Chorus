//! The compiled content database (spec.md §3): read-only queries over one
//! expanded, validated IR. Serializing it is the round-trip face; the
//! fingerprint is the snapshot-envelope identity.

use crate::ir::{
    AspectDecl, AspectKey, Book, CardDecl, DeckDecl, EndingDecl, GameHeader, LegacyDecl, LegacyKey,
    RecipeDecl, RelationKindDecl, VerbDecl, ZoneDecl,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use story_core::CardId;

#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Content {
    pub game: Option<GameHeader>,
    pub zones: BTreeMap<ZoneName, ZoneDecl>,
    /// The declared aspects registry, key -> declaration with read default.
    pub aspects: BTreeMap<AspectKey, AspectDecl>,
    /// Kinds collected from card `kind` lines; `kind:` terms validate against
    /// this set.
    pub kinds: BTreeSet<String>,
    /// The declared relation-kind registry (`relations` blocks); every read
    /// and write of a relation kind validates against it. Its own namespace,
    /// disjoint from card `kind` tags.
    pub relation_kinds: BTreeMap<String, RelationKindDecl>,
    pub cards: BTreeMap<CardId, CardDecl>,
    pub verbs: BTreeMap<String, VerbDecl>,
    pub decks: BTreeMap<String, DeckDecl>,
    /// The declared legacy registry, key -> declaration with default.
    pub legacies: BTreeMap<LegacyKey, LegacyDecl>,
    pub endings: BTreeMap<String, EndingDecl>,
    /// Top-level event books; ignited only by `signal`.
    pub events: BTreeMap<String, Book>,
}

pub use crate::ir::ZoneName;

impl Content {
    /// FNV-1a 64 over the canonical serialization of this database
    /// (kernel-api.md §2). Stable across serialize -> deserialize and across
    /// recompilation of identical content.
    pub fn fingerprint(&self) -> u64 {
        story_core::fingerprint::fingerprint(self)
    }

    pub fn card(&self, id: &CardId) -> Option<&CardDecl> {
        self.cards.get(id)
    }

    /// The declared flags of a relation kind.
    pub fn relation_kind(&self, name: &str) -> Option<&RelationKindDecl> {
        self.relation_kinds.get(name)
    }

    pub fn recipe(&self, id: &str) -> Option<&RecipeDecl> {
        let (verb, name) = id.split_once('.')?;
        self.verbs.get(verb)?.recipes.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::Content;
    use crate::ir::{GameHeader, SeedSpec};

    fn sample() -> Content {
        let mut c = Content::default();
        c.game = Some(GameHeader {
            title: "T".into(),
            seed: SeedSpec::Host,
            turn: 1,
            home: "tower".into(),
            meta: Default::default(),
        });
        c.aspects.insert(
            "heat".into(),
            crate::ir::AspectDecl {
                key: "heat".into(),
                default: 0,
                meta: Default::default(),
            },
        );
        c
    }

    #[test]
    fn fingerprint_survives_json_round_trip() {
        let c = sample();
        let bytes = serde_json::to_vec(&c).unwrap();
        let back: Content = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(c, back);
        assert_eq!(c.fingerprint(), back.fingerprint());
        assert_ne!(c.fingerprint(), 0);
    }
}
