//! Engine-side expression evaluation, scope resolution, and the shared target
//! grammar (kernel-api.md §5, §8). Evaluation is total: absent reads are the
//! declared default (0 unless declared otherwise), dead entities read the
//! default, empty scopes count 0, division by zero is 0.

use crate::Engine;
use std::collections::{BTreeMap, BTreeSet};
use story_content::ir::{AspectTerm, CardRef, CmpOp, Expr, QueryTerm, Scope, Target};
use story_core::{EntityId, RecipeId, SituationId, SlotName, ZoneId};

/// The evaluation context: which situation's scopes read live, whose card the
/// `subject` reads address, the last roll binding, and — in commit selection
/// only — the submitted bindings standing in for the slot scopes.
#[derive(Clone, Debug, Default)]
pub(crate) struct Ctx {
    pub sit: Option<SituationId>,
    pub sel_bindings: Option<Vec<(SlotName, EntityId)>>,
    pub subject: Option<EntityId>,
    pub roll: Option<i64>,
    pub recipe: Option<RecipeId>,
}

impl Engine {
    /// The declared read default of an aspect key (0 when undeclared; every
    /// authored key is declared, so the fallback is defensive).
    pub(crate) fn aspect_default(&self, key: &str) -> i64 {
        self.content
            .aspects
            .get(key)
            .map(|a| a.default)
            .unwrap_or(0)
    }

    /// A card's read for a key: the entity's value, else the declared
    /// default. A dead or absent entity reads the default (T1).
    pub fn read(&self, e: EntityId, key: &str) -> i64 {
        self.state
            .entities
            .get(&e)
            .and_then(|ent| ent.aspects.get(key).copied())
            .unwrap_or_else(|| self.aspect_default(key))
    }

    pub fn read_aspect(&self, e: EntityId, key: &str) -> i64 {
        self.read(e, key)
    }

    pub(crate) fn read_attr(&self, e: EntityId, key: &str) -> i64 {
        self.state
            .entities
            .get(&e)
            .and_then(|ent| ent.attrs.get(key).copied())
            .unwrap_or(0)
    }

    /// The live entity of a `unique` card name; at most one exists.
    pub fn named_entity(&self, card: &str) -> Option<EntityId> {
        self.state
            .entities
            .values()
            .find(|ent| ent.name == card)
            .map(|ent| ent.id)
    }

    /// Total expression evaluation; booleans are 0/1 (T1).
    pub(crate) fn eval_expr_ctx(&self, e: &Expr, ctx: &Ctx) -> i64 {
        match e {
            Expr::Lit(n) => *n,
            Expr::Bool(b) => i64::from(*b),
            Expr::NamePath { card, key } => match self.named_entity(card.as_str()) {
                Some(id) => self.read(id, key),
                None => self.aspect_default(key),
            },
            Expr::Subject(key) => match ctx.subject {
                Some(s) => self.read(s, key),
                None => self.aspect_default(key),
            },
            Expr::Aspect { scope, query, key } => self
                .scope_entities_ctx(scope, ctx)
                .into_iter()
                .filter(|e| self.matches_ctx(e, query, ctx))
                .map(|e| self.read(e, key))
                .sum(),
            Expr::Count { scope, query } => self
                .scope_entities_ctx(scope, ctx)
                .into_iter()
                .filter(|e| self.matches_ctx(e, query, ctx))
                .count() as i64,
            Expr::Exists { scope, query } => i64::from(
                self.scope_entities_ctx(scope, ctx)
                    .into_iter()
                    .any(|e| self.matches_ctx(&e, query, ctx)),
            ),
            Expr::Attr { scope, query, key } => self
                .scope_entities_ctx(scope, ctx)
                .into_iter()
                .filter(|e| self.matches_ctx(e, query, ctx))
                .map(|e| self.read_attr(e, key))
                .sum(),
            Expr::Legacy(key) => self.state.legacy.get(key).copied().unwrap_or(0),
            Expr::Roll => ctx.roll.unwrap_or(0),
            Expr::Counter => ctx
                .recipe
                .as_ref()
                .and_then(|r| self.state.recipe_runs.get(r))
                .map(|v| *v as i64)
                .unwrap_or(0),
            Expr::Clock => self.state.now,
            Expr::Alive(card) => i64::from(self.named_entity(card.as_str()).is_some()),
            Expr::Rel { kind, from, to } => i64::from(self.eval_rel(kind, from, to, ctx)),
            Expr::Arith { op, left, right } => {
                // Fold the left spine iteratively: a `when` summing hundreds
                // of carriers evaluates without per-node recursion.
                let mut spine: Vec<(story_content::ir::ArithOp, &Expr)> = Vec::with_capacity(8);
                spine.push((*op, right.as_ref()));
                let mut cur: &Expr = left.as_ref();
                while let Expr::Arith { op, left, right } = cur {
                    spine.push((*op, right.as_ref()));
                    cur = left.as_ref();
                }
                let mut acc = self.eval_expr_ctx(cur, ctx);
                for (op, r) in spine.into_iter().rev() {
                    let b = self.eval_expr_ctx(r, ctx);
                    acc = match op {
                        story_content::ir::ArithOp::Add => acc.saturating_add(b),
                        story_content::ir::ArithOp::Sub => acc.saturating_sub(b),
                        story_content::ir::ArithOp::Mul => acc.saturating_mul(b),
                        story_content::ir::ArithOp::Div => {
                            if b == 0 {
                                0
                            } else {
                                acc.checked_div_euclid(b).unwrap_or(0)
                            }
                        }
                    };
                }
                acc
            }
            Expr::Cmp { op, left, right } => {
                let a = self.eval_expr_ctx(left, ctx);
                let b = self.eval_expr_ctx(right, ctx);
                let holds = match op {
                    CmpOp::Ge => a >= b,
                    CmpOp::Le => a <= b,
                    CmpOp::Gt => a > b,
                    CmpOp::Lt => a < b,
                    CmpOp::Eq => a == b,
                    CmpOp::Ne => a != b,
                };
                i64::from(holds)
            }
            Expr::Logic { op, left, right } => {
                let a = self.eval_expr_ctx(left, ctx) != 0;
                let holds = match op {
                    story_content::ir::LogicOp::And => a && self.eval_expr_ctx(right, ctx) != 0,
                    story_content::ir::LogicOp::Or => a || self.eval_expr_ctx(right, ctx) != 0,
                };
                i64::from(holds)
            }
            Expr::Not(inner) => i64::from(self.eval_expr_ctx(inner, ctx) == 0),
        }
    }

    /// A condition holds when its evaluation is nonzero.
    pub(crate) fn eval_cond_ctx(&self, e: &Expr, ctx: &Ctx) -> bool {
        self.eval_expr_ctx(e, ctx) != 0
    }

    // -- context-free evaluation face (story-analyze spec §2.2) ---------------
    //
    // Read-only wrappers for analysis tooling: they fix the context-free
    // context (`sit = None`, no subject, no roll binding, no submitted
    // bindings, no recipe context — `counter` reads 0). Totality (T1) and
    // the context-free scope semantics (T9) stay kernel-side.

    /// Evaluate one expression context-free (T1 totality).
    pub fn eval_expr(&self, e: &Expr) -> i64 {
        self.eval_expr_ctx(e, &Ctx::default())
    }

    /// Evaluate one condition context-free; a condition holds when its
    /// evaluation is nonzero.
    pub fn eval_cond(&self, e: &Expr) -> bool {
        self.eval_cond_ctx(e, &Ctx::default())
    }

    /// Query-term match over one entity, context-free (T9 scope semantics).
    pub fn matches(&self, e: EntityId, terms: &[QueryTerm]) -> bool {
        self.matches_ctx(&e, terms, &Ctx::default())
    }

    /// Entities of one scope in the scope's deterministic order, context-free:
    /// `Interior`, `Slot`, and `Bindings` read empty (T9).
    pub fn scope_entities(&self, scope: &Scope) -> Vec<EntityId> {
        self.scope_entities_ctx(scope, &Ctx::default())
    }

    // -- relations (kernel-api.md §5, spec.md §4.9) ---------------------------

    /// Resolve one card ref: a unique card's live entity, the context's
    /// subject, or `first(...)` over a scope. Total: an unresolved ref is a
    /// dead endpoint (`None`).
    pub(crate) fn resolve_card_ref(&self, r: &CardRef, ctx: &Ctx) -> Option<EntityId> {
        match r {
            CardRef::Name(card) => self.named_entity(card.as_str()),
            CardRef::Subject => ctx.subject,
            CardRef::Query { scope, terms } => self
                .scope_entities_ctx(scope, ctx)
                .into_iter()
                .find(|e| self.matches_ctx(e, terms, ctx)),
        }
    }

    /// The `rel(...)` edge test: symmetric kinds hold either way; a dead
    /// endpoint or an undeclared kind reads false (T17).
    fn eval_rel(&self, kind: &str, from: &CardRef, to: &CardRef, ctx: &Ctx) -> bool {
        let (Some(a), Some(b)) = (
            self.resolve_card_ref(from, ctx),
            self.resolve_card_ref(to, ctx),
        ) else {
            return false;
        };
        let symmetric = self
            .content
            .relation_kind(kind)
            .is_some_and(|k| k.symmetric);
        story_core::edge_exists(&self.state.relations, a, kind, b, symmetric)
    }

    /// One endpoint's edges of one kind, deduplicated, ascending entity id
    /// (mint order). Forward (`of`) reads the endpoint's subject entry;
    /// incoming (`to`) scans for rows pointing at the endpoint; a symmetric
    /// kind reads its partners under either spelling. A dead endpoint
    /// enumerates nothing.
    pub fn related_entities(&self, x: EntityId, kind: &str, incoming: bool) -> Vec<EntityId> {
        let symmetric = self
            .content
            .relation_kind(kind)
            .is_some_and(|k| k.symmetric);
        let mut out: BTreeSet<EntityId> = BTreeSet::new();
        if !incoming || symmetric {
            if let Some(objects) = self
                .state
                .relations
                .get(&x)
                .and_then(|kinds| kinds.get(kind))
            {
                out.extend(objects.iter().copied());
            }
        }
        if incoming || symmetric {
            for (subject, kinds) in &self.state.relations {
                if let Some(objects) = kinds.get(kind) {
                    if objects.contains(&x) {
                        out.insert(*subject);
                    }
                }
            }
        }
        out.into_iter().collect()
    }

    // -- scopes (kernel-api.md §5, spec.md §4.7) -----------------------------

    /// Entities of one scope, in the scope's deterministic order. `Interior`,
    /// `Slot`, and `Bindings` read empty outside a running situation (T9);
    /// commit selection substitutes the submitted bindings for slot scopes.
    pub(crate) fn scope_entities_ctx(&self, scope: &Scope, ctx: &Ctx) -> Vec<EntityId> {
        match scope {
            Scope::Interior => match ctx.sit.and_then(|s| self.state.situations.get(&s)) {
                Some(sit) => self.zone_entities(sit.interior),
                None => Vec::new(),
            },
            Scope::Bindings => self.all_bindings(ctx.sit),
            Scope::Slot(name) => self.slot_bindings(ctx.sit, ctx.sel_bindings.as_deref(), name),
            Scope::Zone(name) => match self.zone_ids.get(name) {
                Some(z) => self.zone_subtree_entities(*z),
                None => Vec::new(),
            },
            Scope::Related { kind, of, incoming } => match self.resolve_card_ref(of, ctx) {
                Some(x) => self.related_entities(x, kind, *incoming),
                None => Vec::new(),
            },
            Scope::Table => self.table_entities(),
            Scope::World => self.public_entities(),
            Scope::Anywhere => self.state.entities.values().map(|e| e.id).collect(),
        }
    }

    /// The `table` scope: every card in a public zone plus every situation's
    /// non-greedy slot bindings; greedy bindings and situation interiors are
    /// excluded (spec.md §4.7). Public cards come first in mint order, then
    /// the bindings by situation id, slot declaration order, and bind order.
    pub(crate) fn table_entities(&self) -> Vec<EntityId> {
        let mut ids = self.public_entities();
        let mut seen: BTreeSet<EntityId> = ids.iter().copied().collect();
        for sit in self.state.situations.values() {
            let Some(rec) = self.content.recipe(sit.recipe.as_str()) else {
                continue;
            };
            for sd in &rec.slots {
                if sd.greedy {
                    continue;
                }
                let Some(binds) = sit.bindings.get(&sd.name) else {
                    continue;
                };
                let mut ordered: Vec<(u64, EntityId)> =
                    binds.iter().map(|b| (b.seq, b.entity)).collect();
                ordered.sort_by_key(|(seq, _)| *seq);
                for (_, e) in ordered {
                    if seen.insert(e) {
                        ids.push(e);
                    }
                }
            }
        }
        ids
    }

    pub fn zone_entities(&self, z: ZoneId) -> Vec<EntityId> {
        self.state
            .entities
            .values()
            .filter(|e| e.zone == z)
            .map(|e| e.id)
            .collect()
    }

    /// Public live cards (world scope), mint order.
    pub(crate) fn public_entities(&self) -> Vec<EntityId> {
        self.state
            .entities
            .values()
            .filter(|e| self.state.zones.get(&e.zone).is_some_and(|z| z.public))
            .map(|e| e.id)
            .collect()
    }

    /// A declared zone with its nested subtree (declared nesting), mint order.
    pub(crate) fn zone_subtree_entities(&self, root: ZoneId) -> Vec<EntityId> {
        let mut inside: BTreeMap<ZoneId, bool> = BTreeMap::new();
        for z in self.state.zones.keys() {
            inside.insert(*z, self.zone_under(*z, root));
        }
        self.state
            .entities
            .values()
            .filter(|e| inside.get(&e.zone).copied().unwrap_or(false))
            .map(|e| e.id)
            .collect()
    }

    fn zone_under(&self, z: ZoneId, root: ZoneId) -> bool {
        let mut cur = Some(z);
        while let Some(c) = cur {
            if c == root {
                return true;
            }
            cur = self.zone_parent.get(&c).copied().flatten();
        }
        false
    }

    /// Every binding of the situation across all slots, per binding, in bind
    /// order (a card bound in two slots appears twice).
    pub(crate) fn all_bindings(&self, sit: Option<SituationId>) -> Vec<EntityId> {
        let Some(sit) = sit.and_then(|s| self.state.situations.get(&s)) else {
            return Vec::new();
        };
        let mut binds: Vec<(u64, EntityId)> = sit
            .bindings
            .values()
            .flatten()
            .map(|b| (b.seq, b.entity))
            .collect();
        binds.sort_by_key(|(seq, _)| *seq);
        binds.into_iter().map(|(_, e)| e).collect()
    }

    /// One slot's bound entities in bind order. Outside a running situation,
    /// commit selection substitutes the submitted bindings; otherwise empty.
    pub(crate) fn slot_bindings(
        &self,
        sit: Option<SituationId>,
        sel: Option<&[(SlotName, EntityId)]>,
        slot: &SlotName,
    ) -> Vec<EntityId> {
        if let Some(binds) = sel {
            return binds
                .iter()
                .filter(|(s, _)| s == slot)
                .map(|(_, e)| *e)
                .collect();
        }
        match sit.and_then(|s| self.state.situations.get(&s)) {
            Some(sit) => sit
                .bindings
                .get(slot)
                .map(|v| {
                    let mut b: Vec<(u64, EntityId)> = v.iter().map(|b| (b.seq, b.entity)).collect();
                    b.sort_by_key(|(seq, _)| *seq);
                    b.into_iter().map(|(_, e)| e).collect()
                })
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }

    // -- queries --------------------------------------------------------------

    /// Space-conjoined query terms over one candidate.
    pub(crate) fn matches_ctx(&self, e: &EntityId, terms: &[QueryTerm], ctx: &Ctx) -> bool {
        terms.iter().all(|t| self.matches_term(e, t, ctx))
    }

    fn matches_term(&self, e: &EntityId, t: &QueryTerm, ctx: &Ctx) -> bool {
        match t {
            QueryTerm::Aspect(a) => self.matches_aspect(e, a),
            QueryTerm::Name(c) => self
                .state
                .entities
                .get(e)
                .is_some_and(|ent| ent.name == c.as_str()),
            QueryTerm::Kind(k) => self
                .state
                .entities
                .get(e)
                .and_then(|ent| ent.kind.clone())
                .is_some_and(|kd| &kd == k),
            QueryTerm::Not(inner) => !self.matches_term(e, inner, ctx),
            QueryTerm::Pred(body) => {
                let mut sub = ctx.clone();
                sub.subject = Some(*e);
                self.eval_cond_ctx(body, &sub)
            }
        }
    }

    /// A bare `aspect:<k>` term matches a read value >= 1; an operator term
    /// compares the read against the literal.
    fn matches_aspect(&self, e: &EntityId, a: &AspectTerm) -> bool {
        let v = self.read(*e, &a.key);
        match a.op {
            None => v >= 1,
            Some(op) => match op {
                CmpOp::Ge => v >= a.value,
                CmpOp::Le => v <= a.value,
                CmpOp::Gt => v > a.value,
                CmpOp::Lt => v < a.value,
                CmpOp::Eq => v == a.value,
                CmpOp::Ne => v != a.value,
            },
        }
    }

    /// Slot-filter match (the same term grammar as any query).
    pub(crate) fn matches_slot(
        &self,
        e: &EntityId,
        terms: &story_content::ir::SlotDecl,
        ctx: &Ctx,
    ) -> bool {
        self.matches_ctx(e, &terms.query, ctx)
    }

    // -- targets (kernel-api.md §8) --------------------------------------------

    /// One target grammar for `set`, `move`, `destroy`, `swap`, `consume`.
    /// `Slot` and `Bindings` act per bound card; scoped queries resolve
    /// oldest-first within the scope; bare queries resolve interior-first,
    /// then world; handles index the unit's pool.
    pub(crate) fn resolve_target(
        &self,
        t: &Target,
        ctx: &Ctx,
        handles: &BTreeMap<String, Vec<EntityId>>,
    ) -> Vec<EntityId> {
        match t {
            Target::Name { card, .. } => self.named_entity(card.as_str()).into_iter().collect(),
            Target::Slot(s) => self.slot_bindings(ctx.sit, ctx.sel_bindings.as_deref(), s),
            Target::Query { scope, terms } => self
                .scope_entities_ctx(scope, ctx)
                .into_iter()
                .filter(|e| self.matches_ctx(e, terms, ctx))
                .collect(),
            Target::BareQuery(terms) => {
                let interior = self.scope_entities_ctx(&Scope::Interior, ctx);
                let world = self.scope_entities_ctx(&Scope::World, ctx);
                interior
                    .into_iter()
                    .chain(world)
                    .filter(|e| self.matches_ctx(e, terms, ctx))
                    .collect()
            }
            Target::Handle(h) => handles.get(h).cloned().unwrap_or_default(),
        }
    }
}
