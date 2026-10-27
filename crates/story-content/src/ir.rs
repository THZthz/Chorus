//! IR types: the compiled shape the kernel sees (kernel-api.md §4–§5).
//! Predicates, effectsets, includes, and meta machinery never appear here:
//! the compiled content stores expanded effect lists and inlined expressions
//! only (spec.md §7).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use story_core::{CardId, DeckId, EndingId, RecipeId, SlotName, TriggerRuleId, VerbId};

pub type AspectKey = String;
pub type LegacyKey = String;
pub type HandleName = String;
pub type ZoneName = String;
/// Annotations carried on every declaration; the kernel never reads them.
pub type Meta = BTreeMap<String, String>;

// ---------------------------------------------------------------------------
// Expressions, scopes, queries (kernel-api.md §5)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    /// The interior of the situation whose resolution is running.
    Interior,
    /// The `bound` scope: every card bound in the running situation, counted
    /// per binding; empty outside a running situation.
    Bindings,
    Slot(SlotName),
    /// A declared zone with its nested subtree.
    Zone(ZoneName),
    /// The `related:<kind> (of|to) <card-ref>` scope: the endpoint's edges of
    /// one kind, forward (`of`) or incoming (`to`). Location-transparent:
    /// related live cards wherever they live. Empty on a dead endpoint.
    Related {
        kind: String,
        of: Box<CardRef>,
        incoming: bool,
    },
    /// Every card in a public zone plus every situation's non-greedy slot
    /// bindings; greedy bindings and situation interiors are excluded.
    Table,
    /// Public live cards.
    World,
    /// All live cards.
    Anywhere,
}

/// The endpoint type of `Expr::Rel` and `Scope::Related` (language.md §3).
/// Resolution is total: an unresolved ref is a dead endpoint — `rel(...)`
/// reads false and a `related:` scope reads empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CardRef {
    /// A `unique` card's one live entity (compile-checked [E239]).
    Name(CardId),
    /// The context's subject: the query-position candidate, the bound card in
    /// a card completion book, the dying card in an expiry book.
    Subject,
    /// `first(<scope> [<terms>])`: the scope's first term-matching candidate
    /// in the scope's own deterministic order, re-resolved per read.
    Query { scope: Scope, terms: Vec<QueryTerm> },
}

/// A bare `aspect:<k>` term matches a candidate whose read value for the key
/// is >= 1 (absent keys read the declared default, usually 0).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AspectTerm {
    pub key: AspectKey,
    pub op: Option<CmpOp>,
    pub value: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryTerm {
    Aspect(AspectTerm),
    Name(CardId),
    Kind(String),
    Not(Box<QueryTerm>),
    /// A unary predicate inlined at compile time: an arbitrary boolean
    /// expression evaluated over the candidate card, whose aspect reads are
    /// `Expr::Subject` reads.
    Pred(Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CmpOp {
    Ge,
    Le,
    Gt,
    Lt,
    Eq,
    Ne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogicOp {
    And,
    Or,
}

/// The write operators of `set` and `legacy` (`=`, `+=`, `-=`); distinct from
/// expression arithmetic, which also has `*` and `/`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetOp {
    Assign,
    Add,
    Sub,
}

/// The engine-side expression grammar (language.md §3). Evaluation is total:
/// absent reads are 0, dead entities read 0, division by zero is 0.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expr {
    Lit(i64),
    Bool(bool),
    /// A read on a `unique` card (`beacon.heat`).
    NamePath {
        card: CardId,
        key: AspectKey,
    },
    /// A read on the implicit subject card: the candidate in a query-position
    /// predicate, the bound card in a card completion book, or the dying card
    /// in an expiry book.
    Subject(AspectKey),
    /// `total(<scope>, <query>)` over the key's read values.
    Aspect {
        scope: Scope,
        query: Vec<QueryTerm>,
        key: AspectKey,
    },
    Count {
        scope: Scope,
        query: Vec<QueryTerm>,
    },
    Exists {
        scope: Scope,
        query: Vec<QueryTerm>,
    },
    /// Kernel-internal attribute read; host ops only, never emitted by the
    /// authored language.
    Attr {
        scope: Scope,
        query: Vec<QueryTerm>,
        key: String,
    },
    Legacy(LegacyKey),
    /// The last roll binding in the current effect list; 0 when unbound.
    Roll,
    /// Per-recipe run counter.
    Counter,
    Clock,
    Alive(CardId),
    /// `rel(<kind>, <card-ref>, <card-ref>)`: the pairwise edge test.
    /// Symmetric kinds hold either way; a dead endpoint reads false.
    Rel {
        kind: String,
        from: Box<CardRef>,
        to: Box<CardRef>,
    },
    Arith {
        op: ArithOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Cmp {
        op: CmpOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Logic {
        op: LogicOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Not(Box<Expr>),
}

/// Constant-fold a pure-literal expression; used for the constant
/// zero-divisor check [E242].
pub fn const_eval(e: &Expr) -> Option<i64> {
    match e {
        Expr::Lit(n) => Some(*n),
        Expr::Arith { op, left, right } => {
            let l = const_eval(left)?;
            let r = const_eval(right)?;
            match op {
                ArithOp::Add => l.checked_add(r),
                ArithOp::Sub => l.checked_sub(r),
                ArithOp::Mul => l.checked_mul(r),
                ArithOp::Div => {
                    if r == 0 {
                        None
                    } else {
                        Some(l.div_euclid(r))
                    }
                }
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Effects and targets (kernel-api.md §4)
// ---------------------------------------------------------------------------

/// The shared destination of `spawn` / `draw` / `move` / `swap`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Into {
    Interior,
    Zone(ZoneName),
    /// A card carrier (`into <card>`).
    Carrier(CardId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Target {
    /// One `unique` card; `aspect` is present on `set` (the write key).
    Name {
        card: CardId,
        aspect: Option<AspectKey>,
    },
    /// The slot's bound cards; effects act per bound card.
    Slot(SlotName),
    Query {
        scope: Scope,
        terms: Vec<QueryTerm>,
    },
    /// Interior-oldest-first, then world-oldest-first.
    BareQuery(Vec<QueryTerm>),
    /// A spawn handle's pool (`subject` is pre-bound by the kernel inside
    /// card completion and expiry books).
    Handle(HandleName),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DestroyCount {
    All,
    AtMost(Expr),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectNode {
    Spawn {
        prototype: CardId,
        count: Expr,
        into: Into,
        handle: Option<HandleName>,
    },
    Destroy {
        target: Target,
        count: DestroyCount,
    },
    Consume {
        target: Target,
        count: Expr,
    },
    Swap {
        target: Target,
        count: Expr,
        to: CardId,
        into: Option<Into>,
    },
    Set {
        target: Target,
        key: AspectKey,
        op: SetOp,
        expr: Expr,
        unclamped: bool,
    },
    Move {
        target: Target,
        into: Into,
    },
    Legacy {
        key: LegacyKey,
        op: SetOp,
        expr: Expr,
    },
    Cue {
        id: String,
    },
    Roll {
        count: Expr,
        sides: Expr,
        bonus: Expr,
        bind: String,
    },
    Draw {
        deck: DeckId,
        count: Expr,
        into: Option<Into>,
    },
    Start {
        verb: VerbId,
        via: Option<RecipeId>,
        count: Expr,
    },
    /// Same-situation continuation: legal only in recipe effects (the
    /// compiler enforces it). At settlement the situation adopts the target
    /// recipe instead of completing (spec.md §4.2).
    Continue {
        recipe: RecipeId,
    },
    /// `relate <target> to <target> as <kind>`: one `Related` per written
    /// logical edge; exclusive kinds steal first (`Unrelated` per victim);
    /// self-pairs are skipped silently. Total — no failure modes.
    Relate {
        from: Box<Target>,
        to: Box<Target>,
        kind: String,
    },
    /// `unrelate <target> from <target> [as <kind>]`: one `Unrelated` per
    /// removed logical edge; an omitted kind severs every edge between each
    /// pair, either direction, each once.
    Unrelate {
        from: Box<Target>,
        to: Box<Target>,
        kind: Option<String>,
    },
    Signal {
        event: String,
    },
    Ending {
        id: EndingId,
    },
    Branch {
        cond: Expr,
        then: Vec<EffectNode>,
        els: Vec<EffectNode>,
    },
    /// Host-registered registry extension; never emitted by story-lang.
    Op {
        name: String,
        args: Vec<Expr>,
    },
}

// ---------------------------------------------------------------------------
// Declarations (spec.md §5, language.md §2)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SeedSpec {
    /// Content-fixed seed; boot must receive the same seed.
    Fixed(u64),
    /// The seed is the boot argument.
    Host,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameHeader {
    pub title: String,
    pub seed: SeedSpec,
    /// The minimum advance unit, in minutes; advance durations must be
    /// positive multiples of it.
    pub turn: i64,
    pub home: ZoneName,
    pub meta: Meta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AspectDecl {
    pub key: AspectKey,
    /// Read default for cards without the key.
    pub default: i64,
    pub meta: Meta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyDecl {
    pub key: LegacyKey,
    pub default: i64,
    pub meta: Meta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneDecl {
    pub name: ZoneName,
    pub parent: Option<ZoneName>,
    pub meta: Meta,
    /// Declaration order; the kernel assigns zone ids and orders greedy
    /// candidates by it.
    pub decl_index: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerbMode {
    Committable,
    Autonomous,
    Uncommittable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstanceCap {
    Any,
    AtMost(u32),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BootFace {
    pub zone: ZoneName,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationKindDecl {
    pub name: String,
    /// Stored once per logical edge under the canonical subject
    /// `min(from, to)`; reads are direction-agnostic.
    pub symmetric: bool,
    /// Write-time exclusivity: directed kinds hold at most one `from` per
    /// object (steal); symmetric kinds give each endpoint valence 1.
    pub exclusive: bool,
    pub meta: Meta,
    pub decl_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDecl {
    pub id: CardId,
    pub kind: Option<String>,
    pub mint: story_core::MintPolicy,
    pub aspects: BTreeMap<AspectKey, i64>,
    /// Every mint arms decay from the mint moment; positive [E246].
    pub lifetime: Option<i64>,
    /// Present when the card instantiates at boot.
    pub boot: Option<BootFace>,
    /// Boot edges: `(kind, object cards)` in declaration order, applied after
    /// all boot mints toward every live same-name entity.
    pub relates: Vec<(String, Vec<CardId>)>,
    pub desc: Option<String>,
    /// Mints of this name use the aliased card's body as template.
    pub mint_alias: Option<CardId>,
    /// `(None, book)` is the unqualified book; qualified entries are unique
    /// per aspect [E236].
    pub on_complete: Vec<(Option<AspectKey>, Book)>,
    pub on_expire: Option<Book>,
    pub meta: Meta,
    pub decl_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guard {
    pub prose: String,
    pub when: Expr,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotDecl {
    pub name: SlotName,
    pub min: u32,
    /// `None` is open-ended (`N..`).
    pub max: Option<u32>,
    pub greedy: bool,
    pub consuming: bool,
    pub open: bool,
    pub take: bool,
    pub query: Vec<QueryTerm>,
    pub decl_index: u32,
}

/// A trigger book: rules walked under the selection law
/// `min_by (priority, decl_index)`, first whose `when` holds fires.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Book {
    pub rules: Vec<Rule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub id: TriggerRuleId,
    pub priority: i64,
    pub once: bool,
    /// Absent `when` always holds.
    pub when: Option<Expr>,
    pub effects: Vec<EffectNode>,
    pub meta: Meta,
    pub decl_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeDecl {
    pub verb: VerbId,
    pub name: String,
    pub id: RecipeId,
    pub label: Option<String>,
    pub priority: i64,
    pub craftable: bool,
    pub chain_only: bool,
    /// Lifetime open-count cap for the whole run.
    pub max: Option<u32>,
    /// Mandatory [E237]; 0 is the instant recipe.
    pub duration: i64,
    /// Aspect keys this recipe declares, for aspect-qualified card books.
    pub declares: Vec<AspectKey>,
    pub when: Option<Expr>,
    pub guards: Vec<Guard>,
    pub slots: Vec<SlotDecl>,
    pub effects: Vec<EffectNode>,
    pub on_complete: Option<Book>,
    pub allow_shadow: bool,
    pub meta: Meta,
    pub decl_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerbDecl {
    pub id: VerbId,
    pub mode: VerbMode,
    pub instances: InstanceCap,
    pub title: Option<String>,
    /// Where interior survivors go at completion; default `home`.
    pub releases: Option<ZoneName>,
    pub recipes: BTreeMap<String, RecipeDecl>,
    /// Recipe declaration order within the verb.
    pub recipe_order: Vec<String>,
    pub meta: Meta,
    pub decl_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckDecl {
    pub id: DeckId,
    /// Repetition is weighting; the permutation order is seeded at boot.
    pub entries: Vec<CardId>,
    pub fallback: Option<CardId>,
    pub wrap: bool,
    pub meta: Meta,
    pub decl_index: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndingDecl {
    pub id: EndingId,
    pub title: String,
    /// Free-form display label; the engine never ranks tiers.
    pub tier: String,
    pub text: Option<String>,
    pub meta: Meta,
    pub decl_index: u32,
}
