//! The causal fact domain: facts, provenance sites, hyperedges, and analysis verdicts.

use std::collections::BTreeSet;

use story_content::ir::{AspectKey, LegacyKey, ZoneName};
use story_core::{CardId, DeckId, EndingId, RecipeId};

// ---------------------------------------------------------------------------
// The fact domain (story-analyze spec §4)
// ---------------------------------------------------------------------------

/// One node of the fact domain. Every variant is a capability question about
/// reachable states, never a statement about one state.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Fact {
    /// A live instance of the card can exist.
    CardLive(CardId),
    /// A live instance of the card can sit in the zone.
    CardInZone(CardId, ZoneName),
    /// The card can hold the aspect key (boot declarations and increase
    /// producers ground it; see `CausalGraph::producers_of`).
    CardHasAspect(CardId, AspectKey),
    /// Signed polarity: some effect can raise the aspect's value. `unclamped`
    /// records whether the write may pass below 0 (`set`/`legacy` clamp at 0
    /// unless written `unclamped`), so upper-bound preconditions chain only
    /// through decrease producers.
    AspectCanIncrease {
        card: Option<CardId>,
        key: AspectKey,
        unclamped: bool,
    },
    /// Signed polarity: some effect can lower the aspect's value.
    AspectCanDecrease {
        card: Option<CardId>,
        key: AspectKey,
        unclamped: bool,
    },
    /// Some effect can write the legacy key.
    LegacyCanChange(LegacyKey),
    /// An instance of the recipe can open.
    RecipeCanOpen(RecipeId),
    /// An opened instance of the recipe can run to completion.
    RecipeCanComplete(RecipeId),
    /// The event book can be ignited.
    EventSignaled(String),
    /// The ending can be reached.
    EndingReached(EndingId),
}

impl std::fmt::Display for Fact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CardLive(c) => write!(f, "card \"{c}\" live"),
            Self::CardInZone(c, z) => write!(f, "card \"{c}\" in zone \"{z}\""),
            Self::CardHasAspect(c, k) => write!(f, "card \"{c}\" has aspect \"{k}\""),
            Self::AspectCanIncrease {
                card: Some(c), key, ..
            } => write!(f, "aspect \"{c}.{key}\" can increase"),
            Self::AspectCanIncrease { card: None, key, .. } => {
                write!(f, "aspect \"{key}\" can increase")
            }
            Self::AspectCanDecrease {
                card: Some(c), key, ..
            } => write!(f, "aspect \"{c}.{key}\" can decrease"),
            Self::AspectCanDecrease { card: None, key, .. } => {
                write!(f, "aspect \"{key}\" can decrease")
            }
            Self::LegacyCanChange(k) => write!(f, "legacy \"{k}\" can change"),
            Self::RecipeCanOpen(r) => write!(f, "recipe \"{r}\" can open"),
            Self::RecipeCanComplete(r) => write!(f, "recipe \"{r}\" can complete"),
            Self::EventSignaled(e) => write!(f, "event \"{e}\" signaled"),
            Self::EndingReached(e) => write!(f, "ending \"{e}\" reached"),
        }
    }
}

/// Where a hyperedge was extracted from; the provenance label of the edge.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Site {
    /// A recipe's effect list and its completion book's products.
    Recipe(RecipeId),
    /// The recipe's abort path (T4): effects rolled back, the recipe's
    /// completion book alone walks, `ResolutionFailed` emitted. A failing
    /// context-free book (expire, event) rolls back silently and produces
    /// nothing, so context-free sites emit no abort edge.
    RecipeAbort(RecipeId),
    /// A card-carried completion book; one edge per triggering route (each
    /// recipe that can bind the card or mint it into an interior) and per
    /// book rule, so every requirement set is a necessary conjunction.
    CardBook {
        card: CardId,
        qualifier: Option<AspectKey>,
    },
    /// A card's `on expire` book (context-free; `subject` is the dying card).
    CardExpire(CardId),
    /// A top-level event book.
    Event(String),
    /// A deck's entries and fallback.
    Deck(DeckId),
    /// Boot instantiation of one card (own declaration; mint aliasing never
    /// applies at boot).
    Boot(CardId),
    /// The boot state itself: declared legacy defaults carry values, and the
    /// player and the autonomous heartbeat can open their verbs' recipes.
    BootState,
}

/// One hyperedge: a site whose effect list can run when every requirement
/// fact holds, producing every produced fact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hyperedge {
    pub site: Site,
    /// Necessary preconditions of this route: an effect's target liveness,
    /// the enclosing recipe's open gate and its unique-card `when` reads, a
    /// book's ignition condition. Requirements are conjunctions; disjunctive
    /// routes split into separate edges.
    pub requires: BTreeSet<Fact>,
    /// Every fact this route can produce (an over-approximation: branch arms
    /// and book rules union, never intersect).
    pub produces: BTreeSet<Fact>,
}

/// The verdict of the Tier-1 plausibility pre-check for one goal fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Plausibility {
    /// A producer cone grounds the fact from the boot state.
    Plausible,
    /// No producer cone exists and the content is fully transparent
    /// (`EffectNode::Op` never appears): statically impossible.
    StaticallyImpossible,
    /// No producer cone exists, but the content references host-registered
    /// ops that are opaque to extraction: answer unknown, never impossible
    /// (story-analyze spec §4).
    UnknownHost,
}

/// A backward-provenance witness: the fact, the edge that can produce it,
/// and the provenance of each requirement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub fact: Fact,
    pub site: Site,
    pub requires: Vec<Provenance>,
}
