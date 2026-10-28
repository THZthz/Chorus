//! The syntactic AST (parser output). Name resolution happens at lowering;
//! every node keeps its span for diagnostics.

use crate::lexer::{Line, Tok, P};
use crate::span::Span;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MetaLine {
    pub span: Span,
    pub key: String,
    pub value: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IncludeAst {
    pub span: Span,
    pub path: (Span, String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GameAst {
    pub span: Span,
    pub title: (Span, String),
    pub seed: Option<SeedAst>,
    pub turn: Option<(Span, i64)>,
    pub home: Option<(Span, String)>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SeedAst {
    Fixed((Span, u64)),
    Host(Span),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ZoneAst {
    pub span: Span,
    pub name: String,
    pub parent: Option<(Span, String)>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AspectAst {
    pub span: Span,
    pub key: String,
    pub value: i64,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LegacyAst {
    pub span: Span,
    pub key: String,
    pub value: i64,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RelationKindAst {
    pub span: Span,
    pub name: (Span, String),
    pub symmetric: Option<Span>,
    pub exclusive: Option<Span>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PredAst {
    pub span: Span,
    pub name: (Span, String),
    pub params: Vec<(Span, String)>,
    pub body: Option<ACond>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum UniqAst {
    Stackable(Span),
    Unique(Span),
    Group(Span, (Span, String)),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BootAst {
    pub span: Span,
    pub zone: (Span, String),
    pub count: Option<(Span, u32)>,
}

/// One boot `relates <kind> <card>, ...` line: `(kind, object card names)`
/// in declaration order.
pub type RelatesAst = ((Span, String), Vec<(Span, String)>);

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuleAst {
    pub span: Span,
    pub name: (Span, String),
    pub priority: Option<(Span, i64)>,
    pub once: Option<Span>,
    pub when: Option<(Span, ACond)>,
    pub effects: Option<(Span, Vec<AEffect>)>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CardAst {
    pub span: Span,
    pub name: (Span, String),
    pub kind: Option<(Span, String)>,
    /// Uniqueness flags spelled on the card; `unique` together with
    /// `stackable`/`group` is [E221].
    pub uniq: Vec<UniqAst>,
    pub aspects: Vec<(Span, String, i64)>,
    pub boot: Option<BootAst>,
    /// Boot `relates <kind> <card>, ...` lines, in declaration order.
    pub relates: Vec<RelatesAst>,
    pub desc: Option<(Span, String)>,
    pub lifetime: Option<(Span, i64)>,
    pub mint: Option<(Span, String)>,
    /// `on complete [<aspect>]` books; the qualifier is `None` for the
    /// unqualified form.
    pub completes: Vec<(Span, Option<(Span, String)>, Vec<RuleAst>)>,
    pub expire: Option<(Span, Vec<RuleAst>)>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DeckAst {
    pub span: Span,
    pub name: (Span, String),
    pub entries: Vec<(Span, String)>,
    pub fallback: Option<(Span, String)>,
    pub wrap: bool,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EndingAst {
    pub span: Span,
    pub name: (Span, String),
    pub title: (Span, String),
    pub tier: (Span, String),
    pub text: Option<(Span, String)>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeAst {
    Committable,
    Autonomous,
    Uncommittable,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InstancesAst {
    Any(Span),
    N(Span, u32),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GuardAst {
    pub span: Span,
    pub prose: (Span, String),
    pub when: ACond,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum RangeAst {
    Exact(Span, u32),
    Bounded(Span, u32, u32),
    Open(Span, u32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotFlagAst {
    Greedy,
    Consuming,
    Open,
    Take,
    Internal,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SlotAst {
    pub span: Span,
    pub name: (Span, String),
    pub range: RangeAst,
    pub flags: Vec<(Span, SlotFlagAst)>,
    pub query: Vec<AQueryTerm>,
    /// `shown when <cond>` sub-line: the player-facing presence condition.
    pub shown: Option<(Span, ACond)>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RecipeAst {
    pub span: Span,
    pub name: (Span, String),
    pub label: Option<(Span, String)>,
    pub priority: Option<(Span, i64)>,
    pub craftable: Option<Span>,
    pub chain_only: Option<Span>,
    pub max: Option<(Span, u32)>,
    pub duration: Option<(Span, i64)>,
    pub declares: Vec<(Span, String)>,
    pub when: Option<(Span, ACond)>,
    pub guards: Vec<GuardAst>,
    pub allow_shadow: Option<Span>,
    pub slots: Vec<SlotAst>,
    pub effects: Option<(Span, Vec<AEffect>)>,
    pub complete: Option<(Span, Vec<RuleAst>)>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerbAst {
    pub span: Span,
    pub name: (Span, String),
    pub mode: (Span, ModeAst),
    pub instances: Option<InstancesAst>,
    pub title: Option<(Span, String)>,
    pub releases: Option<(Span, String)>,
    pub recipes: Vec<RecipeAst>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EffectsetAst {
    pub span: Span,
    pub name: (Span, String),
    pub params: Vec<(Span, String)>,
    /// Declaration-time parse of the body: the syntax check (a malformed line
    /// is an error even if the macro is never called) and the source of the
    /// parameter-usage analysis.
    pub body: Option<(Span, Vec<AEffect>)>,
    /// The body's raw token lines; expansion splices argument tokens into
    /// these and re-parses the result as ordinary effects (language.md §6).
    pub body_lines: Vec<Line>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EventBookAst {
    pub span: Span,
    pub name: (Span, String),
    pub rules: Vec<RuleAst>,
    pub meta: Vec<MetaLine>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TopItem {
    Game(GameAst),
    Zones(Vec<ZoneAst>),
    Aspects(Vec<AspectAst>),
    Predicates(Vec<PredAst>),
    Relations(Vec<RelationKindAst>),
    Cards(Vec<CardAst>),
    Decks(Vec<DeckAst>),
    Legacies(Vec<LegacyAst>),
    Endings(Vec<EndingAst>),
    Verbs(Vec<VerbAst>),
    Effectsets(Vec<EffectsetAst>),
    Events(Vec<EventBookAst>),
    Include(IncludeAst),
}

// ---------------------------------------------------------------------------
// Expressions, queries, targets, effects
// ---------------------------------------------------------------------------

pub type Toks = Vec<(Span, Tok)>;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AScope {
    Interior(Span),
    Table(Span),
    World(Span),
    Bound(Span),
    Anywhere(Span),
    Zone(Span, (Span, String)),
    Slot(Span, (Span, String)),
    /// `related:<kind> (of | to) <card-ref>`.
    Related {
        span: Span,
        kind: (Span, String),
        of: Box<ACardRef>,
        incoming: bool,
    },
}

impl AScope {
    pub fn span(&self) -> &Span {
        match self {
            AScope::Interior(s)
            | AScope::Table(s)
            | AScope::World(s)
            | AScope::Bound(s)
            | AScope::Anywhere(s)
            | AScope::Zone(s, _)
            | AScope::Slot(s, _) => s,
            AScope::Related { span, .. } => span,
        }
    }
}

/// A relation endpoint (language.md §3): a unique card name, the context
/// subject, or `first(<scope> [<terms>])`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ACardRef {
    Name(Span, (Span, String)),
    Subject(Span),
    First {
        span: Span,
        scope: AScope,
        terms: Vec<AQueryTerm>,
    },
}

impl ACardRef {
    pub fn span(&self) -> &Span {
        match self {
            ACardRef::Name(s, _) | ACardRef::Subject(s) => s,
            ACardRef::First { span, .. } => span,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ACmpOp {
    Ge,
    Le,
    Gt,
    Lt,
    Eq,
    Ne,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ABinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AQueryTerm {
    Aspect {
        span: Span,
        key: (Span, String),
        op: Option<ACmpOp>,
        value: i64,
    },
    Name(Span, (Span, String)),
    Kind(Span, (Span, String)),
    Not(Box<AQueryTerm>),
    Call {
        span: Span,
        name: (Span, String),
        args: Vec<AExpr>,
    },
}

impl AQueryTerm {
    pub fn span(&self) -> &Span {
        match self {
            AQueryTerm::Aspect { span, .. }
            | AQueryTerm::Name(span, _)
            | AQueryTerm::Kind(span, _)
            | AQueryTerm::Call { span, .. } => span,
            AQueryTerm::Not(inner) => inner.span(),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AExpr {
    Int(Span, i64),
    Dur(Span, i64),
    NamePath {
        span: Span,
        card: (Span, String),
        aspect: (Span, String),
    },
    /// Bare identifier: roll binding, predicate parameter, or unknown name —
    /// resolved at lowering.
    Ident(Span, String),
    Clock(Span),
    Legacy {
        span: Span,
        key: (Span, String),
    },
    Count {
        span: Span,
        scope: AScope,
        terms: Vec<AQueryTerm>,
    },
    Total {
        span: Span,
        scope: AScope,
        key: (Span, String),
    },
    Alive {
        span: Span,
        card: (Span, String),
    },
    /// `rel(<kind>, <card-ref>, <card-ref>)`: reads 0/1.
    Rel {
        span: Span,
        kind: (Span, String),
        from: Box<ACardRef>,
        to: Box<ACardRef>,
    },
    Neg(Span, Box<AExpr>),
    Bin {
        span: Span,
        op: ABinOp,
        l: Box<AExpr>,
        r: Box<AExpr>,
    },
}

impl AExpr {
    pub fn span(&self) -> &Span {
        match self {
            AExpr::Int(s, _)
            | AExpr::Dur(s, _)
            | AExpr::Ident(s, _)
            | AExpr::Clock(s)
            | AExpr::Neg(s, _) => s,
            AExpr::NamePath { span, .. }
            | AExpr::Legacy { span, .. }
            | AExpr::Count { span, .. }
            | AExpr::Total { span, .. }
            | AExpr::Alive { span, .. }
            | AExpr::Rel { span, .. }
            | AExpr::Bin { span, .. } => span,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ACond {
    Or(Box<ACond>, Box<ACond>),
    And(Box<ACond>, Box<ACond>),
    Not(Box<ACond>),
    Cmp {
        span: Span,
        op: ACmpOp,
        l: AExpr,
        r: AExpr,
    },
    /// A bare `rel(...)` operand in condition position.
    Rel {
        span: Span,
        kind: (Span, String),
        from: Box<ACardRef>,
        to: Box<ACardRef>,
    },
    Call {
        span: Span,
        name: (Span, String),
        args: Vec<AExpr>,
    },
    /// Zero-argument predicate spelled as a bare identifier.
    Name(Span, (Span, String)),
}

impl ACond {
    pub fn span(&self) -> &Span {
        match self {
            ACond::Or(l, _) | ACond::And(l, _) | ACond::Not(l) => l.span(),
            ACond::Cmp { span, .. }
            | ACond::Rel { span, .. }
            | ACond::Call { span, .. }
            | ACond::Name(span, _) => span,
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ATarget {
    /// Bare identifier: a spawn handle or a unique card name.
    Name(Span, (Span, String)),
    NamePath {
        span: Span,
        card: (Span, String),
        aspect: (Span, String),
    },
    Slot(Span, (Span, String)),
    Query {
        span: Span,
        scope: AScope,
        terms: Vec<AQueryTerm>,
    },
    Bare {
        span: Span,
        terms: Vec<AQueryTerm>,
    },
}

impl ATarget {
    pub fn span(&self) -> &Span {
        match self {
            ATarget::Name(s, _) | ATarget::Slot(s, _) | ATarget::Bare { span: s, .. } => s,
            ATarget::NamePath { span, .. } | ATarget::Query { span, .. } => span,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ASetOp {
    Assign,
    AddAssign,
    SubAssign,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AInto {
    Interior(Span),
    Id(Span, String),
}

impl AInto {
    pub fn span(&self) -> &Span {
        match self {
            AInto::Interior(s) | AInto::Id(s, _) => s,
        }
    }
}

/// One raw effectset argument: the argument's token stream, substituted at
/// expansion (language.md §6).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ArgAst {
    pub span: Span,
    pub toks: Toks,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AEffect {
    Spawn {
        span: Span,
        card: (Span, String),
        count: Option<AExpr>,
        into: Option<AInto>,
        handle: Option<(Span, String)>,
    },
    Destroy {
        span: Span,
        target: ATarget,
        count: Option<(Span, DestroyCountAst)>,
    },
    Consume {
        span: Span,
        target: ATarget,
        count: Option<AExpr>,
    },
    Swap {
        span: Span,
        target: ATarget,
        count: Option<AExpr>,
        to: (Span, String),
        into: Option<AInto>,
    },
    Move {
        span: Span,
        target: ATarget,
        into: AInto,
    },
    Set {
        span: Span,
        target: ATarget,
        key: (Span, String),
        op: (Span, ASetOp),
        expr: AExpr,
        unclamped: Option<Span>,
    },
    Legacy {
        span: Span,
        key: (Span, String),
        op: (Span, ASetOp),
        expr: AExpr,
    },
    Narrate {
        span: Span,
        text: (Span, String),
    },
    Roll {
        span: Span,
        count: AExpr,
        sides: AExpr,
        bonus: Option<AExpr>,
        bind: (Span, String),
    },
    Draw {
        span: Span,
        deck: (Span, String),
        count: Option<AExpr>,
        into: Option<AInto>,
    },
    Start {
        span: Span,
        verb: (Span, String),
        via: Option<(Span, String)>,
        count: Option<AExpr>,
    },
    /// `continue via <recipe>` — a recipe's effects only ([E247] elsewhere).
    Continue {
        span: Span,
        recipe: (Span, String),
    },
    /// `relate <target> to <target> as <kind>`.
    Relate {
        span: Span,
        from: ATarget,
        to: ATarget,
        kind: (Span, String),
    },
    /// `unrelate <target> from <target> [as <kind>]`; the omitted kind
    /// severs every edge between each pair.
    Unrelate {
        span: Span,
        from: ATarget,
        to: ATarget,
        kind: Option<(Span, String)>,
    },
    Signal {
        span: Span,
        event: (Span, String),
    },
    Ending {
        span: Span,
        id: (Span, String),
    },
    If {
        span: Span,
        cond: ACond,
        then: Vec<AEffect>,
        els: Vec<AEffect>,
    },
    Call {
        span: Span,
        name: (Span, String),
        args: Vec<ArgAst>,
    },
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DestroyCountAst {
    All,
    N(AExpr),
}

/// True when the token is the `d`/`dN` spelling a roll's dice separator.
pub fn is_dice_ident(name: &str) -> bool {
    name == "d"
        || (name.starts_with('d')
            && name.len() > 1
            && name[1..].bytes().all(|b| b.is_ascii_digit()))
}

pub fn punct_of(tok: &Tok) -> Option<&P> {
    match tok {
        Tok::P(p) => Some(p),
        _ => None,
    }
}
