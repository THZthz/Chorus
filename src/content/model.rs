//! The §6 content model: authored raw definitions ([`ContentRaw`] and the
//! per-section `*Def` types) and their compiled counterparts
//! ([`CompiledContent`] and friends). Plain data with string
//! queries/predicates/outcomes; [`crate::content::compile_content`] parses
//! them once and fails loudly at load time.

use indexmap::{IndexMap, IndexSet};

use crate::core::predicate::Pred;
use crate::core::query::Query;
use crate::core::sentence::Outcome;
use crate::core::types::{Aspects, Attrs};

// ---------------------------------------------------------------------------
// authored (raw) content types
// ---------------------------------------------------------------------------

/// A slot role, kept as the RAW string (Q4): role matching never normalizes
/// — the exact authored text is the match key. (Q4's silent never-match half
/// is FIXED: the script parser validates the six roles at load; a hand-built
/// `ContentRaw` skips that gate.)
pub type SlotRole = String;

/// An authored slot. `max: None` is the open-ended count spec (`2..`) —
/// no upper bound (slots never serialize).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlotDef {
    pub role: SlotRole,
    pub min: i64,
    /// Open-ended allowed (§6.1); `None` = no upper bound.
    pub max: Option<i64>,
    pub query: Option<String>,
    /// default true (§6.2) — resolved at compile
    pub shown: Option<bool>,
}

/// An authored recipe.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecipeDef {
    pub id: Option<String>,
    /// None or an empty string → the default recipe
    pub when: Option<String>,
    /// gates default hidden (§6.2)
    pub shown: Option<bool>,
    pub hint: Option<String>,
    /// §6.4: the execution limit (`max N`, N ≥ 1) — None = unlimited
    pub max: Option<i64>,
    /// §6.7: the `aspects` presence list (`aspects <name> [, <name>]*`) —
    /// content only, never logged; the `on complete <aspect>` gate reads it
    pub aspects: Vec<String>,
    /// outcome strings, parsed at load
    pub out: Vec<String>,
}

/// An authored verb.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VerbContent {
    pub slots: Vec<SlotDef>,
    /// minutes, consumed automatically on success (§6.6)
    pub duration: i64,
    /// first match wins; a `when`-less recipe is the default
    pub recipes: Vec<RecipeDef>,
}

/// A clock trigger (§6.6).
#[derive(Debug, Clone, PartialEq)]
pub struct SlotTriggerDef {
    pub id: String,
    /// ticks-of-day slot 0..143 (`at hour N` in the script is slot 6N)
    pub slot: i64,
    pub once: Option<bool>,
    pub out: Vec<String>,
}

/// An expiry trigger (`on expire of NAME`, §6.6).
#[derive(Debug, Clone, PartialEq)]
pub struct ExpireTriggerDef {
    pub id: String,
    pub on_expire_of: String,
    pub out: Vec<String>,
}

/// A completion trigger (`on complete [<aspect>]`, §6.7). ID-less in the
/// script — the id is engine-minted (`complete:{home}`, then
/// `complete:{home}#2`, … in declaration order). `home` is the
/// entity/prototype NAME the clause was authored on: the gate matches it
/// against a locked card's CURRENT name, which is exactly how a prototype's
/// trigger reaches its mints (they carry the prototype's name, the same
/// name-keyed read by which mints take the prototype's aspects — §6.7
/// "inherits like other prototype members"; a `derive`-renamed mint carries
/// its new name and no longer fires the prototype's triggers).
#[derive(Debug, Clone, PartialEq)]
pub struct CompleteTriggerDef {
    pub id: String,
    pub home: String,
    pub aspect: Option<String>,
    pub out: Vec<String>,
}

/// A trigger is a [`SlotTriggerDef`], an [`ExpireTriggerDef`], or a
/// [`CompleteTriggerDef`].
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerDef {
    Slot(SlotTriggerDef),
    Expire(ExpireTriggerDef),
    Complete(CompleteTriggerDef),
}

impl TriggerDef {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            TriggerDef::Slot(t) => &t.id,
            TriggerDef::Expire(t) => &t.id,
            TriggerDef::Complete(t) => &t.id,
        }
    }

    #[must_use]
    pub fn out(&self) -> &[String] {
        match self {
            TriggerDef::Slot(t) => &t.out,
            TriggerDef::Expire(t) => &t.out,
            TriggerDef::Complete(t) => &t.out,
        }
    }
}

/// The `derive` clause's per-aspect entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeriveAspect {
    pub from_object_aspect: String,
    /// optional number; parse always materializes one (default 0)
    pub minus: i64,
}

/// The `derive` clause of a prototype.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrototypeDerive {
    /// minted name shape: `<name>_<object.name>` — the only legal
    /// value is the literal `object`
    pub name_from: Option<String>,
    pub aspects: Option<IndexMap<String, DeriveAspect>>,
}

/// A prototype: prototype-parameterized spawn (§6.4, §9.2).
/// Shared by the raw and compiled faces (the compiled
/// `prototypes` map stores the same struct).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrototypeDef {
    pub kind: String,
    /// §4.2: mints from this prototype may share their name with living
    /// copies. Every read is a truthiness test, so a
    /// plain `bool` (default false) is exact.
    pub stackable: bool,
    pub aspects: Option<Aspects>,
    pub attrs: Option<Attrs>,
    pub derive: Option<PrototypeDerive>,
}

/// A story ending (§4.4). The flavour is an OPEN
/// vocabulary (any lowercase word) — never tighten to an enum.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EndingDef {
    pub title: String,
    pub flavour: Option<String>,
}

/// A function declaration (§10): a parametric
/// outcome MACRO, compiled to templates before verbs/triggers parse.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    /// [] for a parameterless function
    pub params: Vec<String>,
    /// raw outcome-list source after `->`
    pub body: String,
}

/// An `ops:` declaration (§10): a script-defined outcome operator.
/// Structurally a [`FunctionDef`] — the same head grammar, the same
/// template shape, the same expansion pass — but it joins the OP namespace:
/// an UPPER_SNAKE name callable exactly like a built-in op, arity-checked
/// at every call site, and legal to call from other op bodies (a function
/// body may never call one). The load keeps the two tables disjoint by a
/// name-collision check (§10).
pub type OpDef = FunctionDef;

/// A deck declaration (§4.4).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeckRaw {
    pub id: String,
    /// entries, spec order; repeats are legal (weighting)
    pub spec: Vec<String>,
    /// served forever once the spec is exhausted (no `reset`)
    pub defaultcard: Option<String>,
    /// wrap the permutation when exhausted
    pub reset: bool,
}

/// The raw content tree. Every optional
/// collection becomes `Option`; the maps are IndexMaps (insertion order IS
/// observable: compile-time canonical key-order walks and the `.recipe<i>`
/// minted ids).
#[derive(Debug, Clone, Default)]
pub struct ContentRaw {
    /// §4.4: the dice seed (default 0). Meta line `seed: N` (§10).
    pub seed: Option<i64>,
    pub verbs: IndexMap<String, VerbContent>,
    /// §4.1: the script's `kinds:` declarations, section order —
    /// the record of what the loader registers into its per-load dictionary
    /// clone. Code-level raw builders that use a custom kind must register
    /// it into their dictionary themselves (the same discipline as verbs).
    pub kinds: IndexSet<String>,
    pub triggers: Option<Vec<TriggerDef>>,
    pub prototypes: Option<IndexMap<String, PrototypeDef>>,
    /// prose templates per verb (§5.3)
    pub templates: Option<IndexMap<String, String>>,
    /// the ending table (§4.4)
    pub endings: Option<IndexMap<String, EndingDef>>,
    /// parametric outcome macros (§10) — declaration order matters for
    /// error reports only
    pub functions: Option<Vec<FunctionDef>>,
    /// script-defined outcome operators (§10) — the same record
    /// shape as `functions`, a different table; declaration order matters
    /// for error reports only
    pub ops: Option<Vec<OpDef>>,
    /// deck declarations (§4.4) — order matters for error reports only
    pub decks: Option<Vec<DeckRaw>>,
}

// ---------------------------------------------------------------------------
// compiled content types
// ---------------------------------------------------------------------------

/// A compiled recipe.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRecipe {
    pub id: String,
    /// None → default
    pub when: Option<Pred>,
    pub shown: bool,
    pub hint: Option<String>,
    /// §6.4: the execution limit — None = unlimited
    pub max: Option<i64>,
    /// §6.7: the `aspects` presence list — the emitter gate an
    /// `on complete <aspect>` trigger reads; content, never logged
    pub aspects: Vec<String>,
    pub out: Vec<Outcome>,
}

/// A compiled slot. `max: None` = no upper bound.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSlot {
    pub role: SlotRole,
    pub min: i64,
    pub max: Option<i64>,
    pub shown: bool,
    pub query: Option<Query>,
}

/// A compiled verb.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledVerb {
    pub name: String,
    pub slots: Vec<CompiledSlot>,
    /// Keyed by the RAW role string (Q4) — role matching never normalizes
    pub slot_by_role: IndexMap<SlotRole, CompiledSlot>,
    pub duration: i64,
    pub recipes: Vec<CompiledRecipe>,
}

/// A compiled ending: flavour resolved (default 'melancholy', §10).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEnding {
    pub title: String,
    pub flavour: String,
}

/// The compiled trigger forms (inline variants).
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledTrigger {
    Slot {
        id: String,
        slot: i64,
        once: bool,
        out: Vec<Outcome>,
    },
    Expire {
        id: String,
        on_expire_of: String,
        out: Vec<Outcome>,
    },
    /// §6.7: `on complete [<aspect>]` — home/aspect semantics mirror
    /// [`CompleteTriggerDef`]; fires in fire_window's completion, never as an
    /// event (no EVENT row, no `state.fired`).
    Complete {
        id: String,
        home: String,
        aspect: Option<String>,
        out: Vec<Outcome>,
    },
}

impl CompiledTrigger {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            CompiledTrigger::Slot { id, .. }
            | CompiledTrigger::Expire { id, .. }
            | CompiledTrigger::Complete { id, .. } => id,
        }
    }

    /// The compiled outcome list.
    #[must_use]
    pub fn out(&self) -> &[Outcome] {
        match self {
            CompiledTrigger::Slot { out, .. }
            | CompiledTrigger::Expire { out, .. }
            | CompiledTrigger::Complete { out, .. } => out,
        }
    }
}

/// One resolvable deck entry: the card NAME and the
/// kind its mint carries (prototype kind, or the declared entity line's kind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckCard {
    pub name: String,
    pub kind: String,
}

/// A compiled deck (§4.4): the fixed draw order is
/// a PERMUTATION OVER POSITIONS of the spec (multiset preserved), derived
/// purely from (seed, deck id) at load. It lives on compiled content, never
/// on State: identical at play and fold, consuming NO rng counter (law 5).
#[derive(Debug, Clone)]
pub struct CompiledDeck {
    pub id: String,
    /// the draw order: spec positions shuffled, entries resolved to (name, kind)
    pub order: Vec<DeckCard>,
    /// served forever once the order is exhausted and `reset` is absent
    pub defaultcard: Option<DeckCard>,
    pub reset: bool,
}
/// `CompiledContent`. All maps IndexMap
/// (compile error order, the `.recipe<i>` minted ids and UI traversal depend
/// on it).
#[derive(Debug, Clone)]
pub struct CompiledContent {
    /// §4.4: every die is `f(seed, counter)`; the run's dice sequence is
    /// fixed at load and replay is counter-based (no re-derivation)
    pub seed: i64,
    pub verbs: IndexMap<String, CompiledVerb>,
    pub triggers: Vec<CompiledTrigger>,
    pub prototypes: IndexMap<String, PrototypeDef>,
    pub templates: IndexMap<String, String>,
    pub endings: IndexMap<String, CompiledEnding>,
    /// §4.2 stackability, derived from `stackable` declarations at load:
    /// declared stackable prototype/entity NAMES, and the KINDS of those
    /// declarations (a `kind:K` mint is stackable when its kind carries one).
    /// Only membership is ever read, so the single build (prototype side
    /// + script entity side, once, here) is behavior-preserving.
    pub stackable_names: IndexSet<String>,
    pub stackable_kinds: IndexSet<String>,
    /// §4.4 the deck table: id → compiled deck (permutation + exhaustion)
    pub decks: IndexMap<String, CompiledDeck>,
    /// §6.7 the GLOBAL recipe-id table: recipe id → owning verb name.
    /// Chain-op candidates (RUN/FORK) resolve against it in every home —
    /// recipe bodies and trigger bodies alike — which is why it demands
    /// GLOBAL id uniqueness: the build is where a cross-verb collision
    /// fails loud.
    pub recipe_verbs: IndexMap<String, String>,
}

impl CompiledContent {
    /// §4.2 stackability of a mint, decided from the RESOLVED name+kind: a
    /// NAME declared stackable, or a KIND carrying a stackable declaration.
    /// Concrete SPAWN rows always carry the resolved `kind:K`, so the fold
    /// recomputes from the same (name, kind) pair: play must decide from the
    /// resolved kind too, or a plain spawn of a non-stackable prototype in a
    /// stackable KIND would mint non-stackable at play but stackable at fold
    /// (snap would diverge). The reach census applies the same rule to every
    /// site it registers.
    #[must_use]
    pub fn mint_is_stackable(&self, name: &str, kind: &str) -> bool {
        self.stackable_names.contains(name) || self.stackable_kinds.contains(kind)
    }
}

/// One declared entity line. The `stackable` flag rides
/// along so the §4.2 sets build ONCE here instead of `compile_script`
/// mutating the compiled content afterwards (behavior-preserving — set
/// membership is order- and structure-free).
#[derive(Debug, Clone)]
pub struct DeclaredEntity {
    pub name: String,
    pub kind: String,
    pub stackable: bool,
}
