//! The never-matching-`when` detector (E11): a three-valued static verdict
//! over a compiled predicate — a `when` provably false for every §6.6
//! selection binding is the lint's WARNING class.

use indexmap::{IndexMap, IndexSet};

use crate::core::predicate::{Pred, ValExpr};
use crate::core::query::CmpOp;

/// One statically-typed scalar of a `when` (§6.3): a role's binding count or
/// the `total(role, path)` pool sum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum WhenVar {
    Count(String),
    Total(String, String),
}

impl WhenVar {
    /// The authored spelling (message text pins it).
    fn text(&self) -> String {
        match self {
            WhenVar::Count(role) => format!("count({role})"),
            WhenVar::Total(role, path) => format!("total({role}, aspect:{path})"),
        }
    }
    /// The role the variable reads.
    fn role(&self) -> &str {
        match self {
            WhenVar::Count(role) | WhenVar::Total(role, _) => role,
        }
    }
}

/// The values a [`WhenVar`] can still take under one conjunction: `lo..hi`
/// with `hi: None` = unbounded above; `lo == NEG_INF` = unbounded below.
/// (Counts are cardinalities — always finite-below; a total sums authored
/// aspect values, and aspects parse signed, so a total is unbounded below.)
pub(super) type WhenRange = (i64, Option<i64>);

/// The `lo` sentinel for "no lower bound" (a total over signed aspects).
const NEG_INF: i64 = i64::MIN;

fn range_text(r: &WhenRange) -> String {
    let lo = if r.0 == NEG_INF { "-∞".to_string() } else { r.0.to_string() };
    match r.1 {
        Some(hi) => format!("{lo}..{hi}"),
        None => format!("{lo}..∞"),
    }
}

/// Narrow `r` to the values satisfying `value {op} k`; `None` = none do.
/// `Ne` cannot narrow an interval — the point-range case is decided by the
/// satisfiability helpers below.
fn range_narrow(r: WhenRange, op: CmpOp, k: i64) -> Option<WhenRange> {
    let (lo, hi) = match op {
        CmpOp::Ge => (r.0.max(k), r.1),
        CmpOp::Gt => (r.0.max(k.saturating_add(1)), r.1),
        CmpOp::Le => (r.0, Some(r.1.map_or(k, |h| h.min(k)))),
        CmpOp::Lt => (r.0, Some(r.1.map_or(k.saturating_sub(1), |h| h.min(k.saturating_sub(1))))),
        CmpOp::Eq => (r.0.max(k), Some(r.1.map_or(k, |h| h.min(k)))),
        CmpOp::Ne => r,
    };
    if hi.is_some_and(|hi| lo > hi) {
        None
    } else {
        Some((lo, hi))
    }
}

/// Does EVERY value of `r` satisfy `value {op} k`?
fn range_all_satisfy(r: WhenRange, op: CmpOp, k: i64) -> bool {
    let (lo, hi) = r;
    match op {
        CmpOp::Ge => lo >= k,
        CmpOp::Gt => lo > k,
        CmpOp::Le => hi.is_some_and(|h| h <= k),
        CmpOp::Lt => hi.is_some_and(|h| h < k),
        CmpOp::Eq => lo == k && hi == Some(k),
        CmpOp::Ne => k < lo || hi.is_some_and(|h| k > h) || (hi == Some(lo) && lo != k),
    }
}

/// Does SOME value of `r` satisfy `value {op} k`?
fn range_any_satisfies(r: WhenRange, op: CmpOp, k: i64) -> bool {
    let (lo, hi) = r;
    match op {
        CmpOp::Ge => hi.is_none_or(|h| h >= k),
        CmpOp::Gt => hi.is_none_or(|h| h > k),
        CmpOp::Le => lo <= k,
        CmpOp::Lt => lo < k,
        CmpOp::Eq => lo <= k && hi.is_none_or(|h| h >= k),
        CmpOp::Ne => !(lo == k && hi == Some(k)),
    }
}

/// The comparison flipped operand-order (`3 > count(a)` ≡ `count(a) < 3`).
fn mirror_op(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Ge => CmpOp::Le,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Eq => CmpOp::Eq,
        CmpOp::Ne => CmpOp::Ne,
    }
}

/// Three-valued verdict for one (polarity-applied) predicate node. `Dead`
/// and `Always` are EXACT (provable over the whole §6.6 domain), so NOT
/// complements them and OR/AND combine them classically; `Unknown` is the
/// honest third value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WhenVerdict {
    /// False for every selection binding — the WARNING fires.
    Dead(String),
    /// True for every selection binding (a NOT-wrapped dead atom).
    Always,
    /// Statically undecidable — world reads (`exists`, `count(<query>)`),
    /// `every(...)`'s vacuous-true corner, non-int comparisons.
    Unknown,
}

/// What a role's binding state is under the current conjunction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleBound {
    /// The slot declares it and no conjunct pinned it empty.
    Bindable,
    /// A conjunct pinned the count to 0 (`NOT (count(r) >= 1)`,
    /// `count(r) = 0`, a narrowed interval `..0`).
    PinnedEmpty,
    /// No slot for the role on this verb (or a `0..0` slot) — the role
    /// never binds anything (§6.6: validate_slots refuses any ref to an
    /// undeclared role; refs_to_clause zeroes it on RUN projection).
    NeverBinds,
}

/// The conjunction state: working ranges per typed variable plus the roles
/// pinned empty. Cloned per OR branch (small maps, never hot).
#[derive(Clone)]
pub(super) struct WhenCx<'a> {
    /// each declared role's §6.1 cardinality, spelled lowercase
    slots: &'a IndexMap<String, WhenRange>,
    vars: IndexMap<WhenVar, WhenRange>,
    empty: IndexSet<String>,
    bound: IndexSet<String>,
}

impl<'a> WhenCx<'a> {
    pub(super) fn new(slots: &'a IndexMap<String, WhenRange>) -> Self {
        WhenCx { slots, vars: IndexMap::new(), empty: IndexSet::new(), bound: IndexSet::new() }
    }

    /// The §6.1 domain of one variable: a declared role's slot range; an
    /// undeclared role binds nothing, so its count is exactly 0; a total
    /// sums 0 over an empty role (§6.3's fold) and signed aspect values
    /// above — pinned to {0} only when the role can never bind.
    fn initial(&self, var: &WhenVar) -> WhenRange {
        match var {
            WhenVar::Count(role) => match self.slots.get(role) {
                Some(range) => *range,
                None => (0, Some(0)),
            },
            WhenVar::Total(role, _) => match self.slots.get(role) {
                Some((_, Some(0))) => (0, Some(0)),
                Some(_) => (NEG_INF, None),
                None => (0, Some(0)),
            },
        }
    }

    fn working(&self, var: &WhenVar) -> WhenRange {
        self.vars.get(var).copied().unwrap_or_else(|| self.initial(var))
    }

    /// A role's binding state under the current conjunction.
    fn role_bound(&self, role: &str) -> RoleBound {
        if self.empty.contains(role) {
            return RoleBound::PinnedEmpty;
        }
        match self.slots.get(role) {
            Some((_, Some(0))) | None => RoleBound::NeverBinds,
            Some(_) => RoleBound::Bindable,
        }
    }

    /// Record that an atom proved the role bound; `Some(reason)` = this
    /// contradicts a conjunct that pinned the role empty.
    fn require_bound(&mut self, role: &str) -> Option<String> {
        if self.empty.contains(role) {
            return Some(format!("the when requires {role} both empty and bound (§6.3)"));
        }
        self.bound.insert(role.to_string());
        None
    }
}

/// The statically-typed variable of a cmp operand, if one side is.
fn typed_var(x: &ValExpr) -> Option<WhenVar> {
    match x {
        ValExpr::Count { role } => Some(WhenVar::Count(role.clone())),
        ValExpr::Total { role, path } => Some(WhenVar::Total(role.clone(), path.clone())),
        _ => None,
    }
}

/// The never-matching-`when` detector's core (E11). SOUNDNESS BOUNDARY: a
/// WARNING fires only when the `when` is provably false for EVERY §6.6
/// selection binding, over this static model — the count-gate check's own
/// domain (§6.1): each declared role's count is its slot's `min..max`, an
/// undeclared role binds nothing (count ≡ 0), a total sums over signed
/// aspects (0 exactly when the role cannot bind). Within one conjunction
/// the typed atoms narrow per-variable intervals and couple role-emptiness:
/// a count pinned to 0 (the `NOT (count(r) >= 1)` shape) vs anything
/// needing the role bound — `total(…) >= 1`, a positive `r.aspect.x` path,
/// `any(r, …)`, `rel(…, r…)`. NOT is exact (op complement on leaves, De
/// Morgan on AND/OR — the verdicts are exact); OR is dead only when EVERY
/// branch is.
///
/// Deliberately NOT caught (the documented boundary): world reads —
/// `exists(...)` and the `count(<query>)` pool form — stay `Unknown` (no
/// world model, so `exists(A) AND NOT exists(A)` passes silently);
/// `every(...)` is `Unknown` (vacuously true over an empty role, §6.3);
/// non-int comparisons (`count(a) > object.attr.seats`) are `Unknown`; and
/// the E7 RUN corner — a cross-verb RUN candidate re-evaluates its when
/// against the PARENT's committed bindings (refs_to_clause), outside the
/// §6.6 domain — exactly the simplification the §6.1 count-gate check has
/// always made.
pub(super) fn when_verdict(p: &Pred, neg: bool, cx: &mut WhenCx) -> WhenVerdict {
    match p {
        Pred::Not(a) => when_verdict(a, !neg, cx),
        Pred::And(a, b) | Pred::Or(a, b) => {
            // AND shares the narrowing state when positive; OR forks it.
            // Under NOT the shapes swap (De Morgan): ¬(a∧b) = ¬a∨¬b forks,
            // ¬(a∨b) = ¬a∧¬b shares.
            let is_and = matches!(p, Pred::And(..));
            if is_and != neg {
                // conjunction: Dead if EITHER side is; Always only if BOTH are
                // (an Unknown side keeps the whole Unknown — an Always that
                // leaks through NOT would fabricate a warning)
                let va = when_verdict(a, neg, cx);
                if let WhenVerdict::Dead(reason) = va {
                    return WhenVerdict::Dead(reason);
                }
                match when_verdict(b, neg, cx) {
                    WhenVerdict::Dead(reason) => WhenVerdict::Dead(reason),
                    WhenVerdict::Always if va == WhenVerdict::Always => WhenVerdict::Always,
                    _ => WhenVerdict::Unknown,
                }
            } else {
                // disjunction: Dead only if BOTH sides are; Always if either is
                let mut a_cx = cx.clone();
                let mut b_cx = cx.clone();
                match (when_verdict(a, neg, &mut a_cx), when_verdict(b, neg, &mut b_cx)) {
                    (WhenVerdict::Dead(ra), WhenVerdict::Dead(_)) => WhenVerdict::Dead(ra),
                    (WhenVerdict::Always, _) | (_, WhenVerdict::Always) => WhenVerdict::Always,
                    _ => WhenVerdict::Unknown,
                }
            }
        }
        Pred::Cmp { l, r, op } => when_cmp(l, r, *op, neg, cx),
        Pred::Any { role, .. } => {
            // any(...) is false over an empty role (§6.3) — a positive atom
            // proves the role bound; negated it proves nothing
            if neg {
                return WhenVerdict::Unknown;
            }
            role_bound_verdict(cx, role, "any(…) needs a bound member")
        }
        Pred::Every { .. } => {
            // every(...) is VACUOUSLY TRUE over an empty role (§6.3) — no
            // static verdict either way (the documented boundary)
            WhenVerdict::Unknown
        }
        Pred::Rel { role, role2, .. } => {
            // ∃ a pair — needing one is the sound half (a true rel needs
            // every named role bound); negated proves nothing
            if neg {
                return WhenVerdict::Unknown;
            }
            for role in [Some(role.as_str()), role2.as_deref()].into_iter().flatten() {
                if let WhenVerdict::Dead(reason) =
                    role_bound_verdict(cx, role, "rel(…) needs bound members")
                {
                    return WhenVerdict::Dead(reason);
                }
            }
            WhenVerdict::Unknown
        }
        // world reads: no world model at load — the documented boundary
        Pred::Exists { .. } | Pred::ExistsQuery { .. } => WhenVerdict::Unknown,
    }
}

/// A positive role-reading atom (`any`, `rel`, a path operand): Dead against
/// a pinned-empty or never-binding role, Unknown otherwise (with the role
/// recorded bound for the emptiness coupling).
fn role_bound_verdict(cx: &mut WhenCx, role: &str, why: &str) -> WhenVerdict {
    match cx.role_bound(role) {
        RoleBound::PinnedEmpty => WhenVerdict::Dead(format!(
            "the when requires {role} both empty and bound — this clause needs a bound card, another pinned it empty ({why}, §6.3)"
        )),
        RoleBound::NeverBinds => WhenVerdict::Dead(format!(
            "{role} never binds — the verb declares no slot for it (or a 0..0 slot), so this clause is always false ({why}, §6.6)"
        )),
        RoleBound::Bindable => {
            // recording is pointless after an Unknown — but free and exact
            cx.require_bound(role);
            WhenVerdict::Unknown
        }
    }
}

/// One cmp leaf, polarity applied. The statically-decidable core: a typed
/// variable vs an int literal (either operand order). A path operand reads
/// the role's first bound card — unresolvable over an empty role reads
/// FALSE in the when-discipline (§6.3) — so a positive path atom also
/// proves its role bound.
fn when_cmp(l: &ValExpr, r: &ValExpr, op: CmpOp, neg: bool, cx: &mut WhenCx) -> WhenVerdict {
    // positive path atoms prove the role bound (the dead-stamp/unresolvable
    // rule reads false over an empty role, §6.3); a negated path atom is
    // satisfiable via the empty role and undecidable otherwise
    if !neg {
        for x in [l, r] {
            if let ValExpr::Path { role, field, key } = x {
                let why = format!("{role}.{}.{key} reads a bound card", field.as_str());
                if let WhenVerdict::Dead(reason) = role_bound_verdict(cx, role, &why) {
                    return WhenVerdict::Dead(reason);
                }
            }
        }
    }
    // the typed-var-vs-int core (either operand order)
    let (var, op, k) = match (typed_var(l), typed_var(r)) {
        (Some(v), _) if matches!(r, ValExpr::Int { .. }) => (v, op, int_of(r)),
        (_, Some(v)) if matches!(l, ValExpr::Int { .. }) => (v, mirror_op(op), int_of(l)),
        _ => return WhenVerdict::Unknown,
    };
    let op = if neg {
        // NOT over a leaf: the exact op complement (≥↔<, =↔≠)
        match op {
            CmpOp::Ge => CmpOp::Lt,
            CmpOp::Lt => CmpOp::Ge,
            CmpOp::Gt => CmpOp::Le,
            CmpOp::Le => CmpOp::Gt,
            CmpOp::Eq => CmpOp::Ne,
            CmpOp::Ne => CmpOp::Eq,
        }
    } else {
        op
    };
    let init = cx.initial(&var);
    let lit = format!("{} {} {k}", var.text(), op.as_str());
    if !range_any_satisfies(init, op, k) {
        // self-contained: no §6.1 value satisfies the literal
        return WhenVerdict::Dead(match (&var, cx.slots.get(var.role())) {
            (WhenVar::Count(_), None) => format!(
                "{lit} can never hold — the verb declares no {} slot, the role binds nothing (§6.6)",
                var.role()
            ),
            _ => format!(
                "{lit} can never hold — {} takes {} here (§6.1)",
                var.role(),
                range_text(&init)
            ),
        });
    }
    // conjunction narrowing: this literal vs what earlier conjuncts pinned
    let prior = cx.working(&var);
    let Some(narrowed) = range_narrow(prior, op, k) else {
        return WhenVerdict::Dead(format!(
            "{lit} can never hold — the when already pins {} to {} (§6.3)",
            var.text(),
            range_text(&prior)
        ));
    };
    cx.vars.insert(var.clone(), narrowed);
    // the emptiness coupling flows both ways through the count
    match &var {
        WhenVar::Count(role) => {
            if narrowed.1 == Some(0) {
                cx.empty.insert(role.clone());
            }
            if narrowed.0 >= 1 {
                cx.bound.insert(role.clone());
            }
        }
        WhenVar::Total(role, _) => {
            if narrowed.0 >= 1 {
                cx.bound.insert(role.clone());
            }
        }
    }
    if let Some(role) = cx.empty.intersection(&cx.bound).next() {
        return WhenVerdict::Dead(format!("the when requires {role} both empty and bound (§6.3)"));
    }
    if range_all_satisfy(init, op, k) {
        WhenVerdict::Always
    } else {
        WhenVerdict::Unknown
    }
}

fn int_of(x: &ValExpr) -> i64 {
    match x {
        ValExpr::Int { v } => *v,
        _ => 0, // unreachable: every caller guards with matches!(.., Int)
    }
}
