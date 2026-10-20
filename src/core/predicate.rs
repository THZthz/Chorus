//! Condition predicates for recipe `when` (spec
//! §6.3): the AST (`Pred`/`ValExpr`/`ScalarCond`/`ScalarPath`/`PredField`),
//! the evaluation context, and the evaluator. The SAME grammar serves the IF
//! conditions of outcome lists (§6.8): parsing lives in `pred_parse` —
//! `parse_predicate` string-parses `when:` lines, `parse_predicate_toks`
//! parses an IF condition out of the SENTENCE token stream — one grammar,
//! two front-ends, no duplicated semantics; the load-time lints live in
//! `pred_lint`. Both are re-exported here, which stays the seam's public
//! face.

use indexmap::IndexMap;

use crate::core::query::{match_query, CmpOp, Query};
use crate::core::roles::Role;
use crate::core::types::{
    aspect_get, is_parked, number_attr, relation_alive, AttrValue, Entity, State,
};
use crate::error::{ChorusError, Result};

pub use crate::core::pred_lint::{
    lint_pred, lint_roll_may_bind, lint_trigger_pred, lint_trigger_val_expr_what, lint_val_expr,
};
pub use crate::core::pred_parse::{
    parse_predicate, parse_predicate_toks, parse_value_expr_toks, PredView,
};

/// Role bindings: role → bound entity IDS (`IndexMap<Role, Vec<String>>`),
/// never
/// cloned entities: role_cards and eval_pred must read the CURRENT dead_action
/// through `state.entities.get` mid-list (dead-stamp reads false silently).
pub type RoleBindings = IndexMap<Role, Vec<String>>;

/// The evaluation context: bindings, state, strictness, dice.
pub struct PredCtx<'a> {
    pub b: &'a RoleBindings,
    pub state: &'a State,
    /// §6.3 asymmetric failure discipline: IF conditions evaluate STRICTLY —
    /// a path operand whose role has no bound card REJECTS the step (§9.2),
    /// where a `when:` predicate would silently read false.
    pub strict: bool,
    /// §4.4: the apply context's per-clause dice binding — the last ROLL of
    /// this outcome list, last-wins. Absent during recipe `when:` selection
    /// and at fold.
    pub roll: Option<i64>,
}

/// The path field: 'aspect' | 'attr'.
/// Serde derives: type-graph completeness only (predicates never serialize
/// into the log — `when:` lives in compiled content, IF arms are authored
/// form only), so the default wire shape is fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PredField {
    Aspect,
    Attr,
}

impl PredField {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PredField::Aspect => "aspect",
            PredField::Attr => "attr",
        }
    }

    /// The two-word field validation, shared by every path grammar site.
    #[must_use]
    pub fn from_name(s: &str) -> Option<PredField> {
        match s {
            "aspect" => Some(PredField::Aspect),
            "attr" => Some(PredField::Attr),
            _ => None,
        }
    }
}

/// Serde derives: type-graph completeness
/// only (see PredField — never on the wire).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Pred {
    And(Box<Pred>, Box<Pred>),
    Or(Box<Pred>, Box<Pred>),
    Not(Box<Pred>),
    Cmp {
        l: ValExpr,
        op: CmpOp,
        r: ValExpr,
    },
    Rel {
        rtype: String,
        role: String,
        role2: Option<String>,
    },
    Every {
        role: String,
        cond: ScalarCond,
    },
    Any {
        role: String,
        cond: ScalarCond,
    },
    /// ∃ an ALIVE, UNPARKED entity bearing this exact name (§6.3's
    /// visibility law — a parked copy keeps the name addressable but not
    /// world-existent)
    Exists {
        name: String,
    },
    /// ∃ an alive, unparked entity matching a §4.5 slot query
    ExistsQuery {
        query: Query,
    },
}

/// Serde derives: type-graph completeness
/// only (Arg::Vexpr's payload; ROLL args are authored-only and the log
/// records computed ROLLED ints, so this is never on the wire).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ValExpr {
    Int {
        v: i64,
    },
    Count {
        role: String,
    },
    /// §4.5: count over the whole living world (any §4.5 query)
    CountQuery {
        query: Query,
    },
    Total {
        role: String,
        path: String,
    },
    /// §6.3: `total(bound, aspect:k)` — the summed level over every card
    /// bound to the current window across ALL its roles (the per-binding
    /// bookkeeping: a card bound to two roles counts twice; a trigger's
    /// event bindings are empty, so the scope reads 0 there — §6.6).
    TotalBound {
        path: String,
    },
    /// §6.3: `total(table, aspect:k)` — the summed level over every ALIVE,
    /// UNPARKED card on the world projection (the same card set `count(name:X)`
    /// and `exists(name:X)` see, §4.5 — the visibility law keeps parked cards
    /// out of the world scopes), missing aspects reading 0.
    TotalTable {
        path: String,
    },
    /// §6.3 (v0.10.0): `total(parked, aspect:k)` — the summed level over every
    /// ALIVE PARKED card (the interior's pool — the exact complement of the
    /// `table` scope's card set), missing aspects reading 0.
    TotalParked {
        path: String,
    },
    Path {
        role: String,
        field: PredField,
        key: String,
    },
    /// F3 authored-only: a function parameter standing in a cmp operand
    /// (§10); expansion replaces it with the call's number — it never
    /// reaches evaluation.
    Param {
        name: String,
    },
    /// §4.4: the per-clause dice binding from the APPLY context. Parses only
    /// in the Tok front-end (IF conditions, ROLL args).
    Roll,
}

/// Serde derives: type-graph completeness
/// only (see PredField).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScalarCond {
    pub path: ScalarPath,
    pub op: CmpOp,
    pub value: i64,
}

/// `{ field: 'aspect' | 'attr', key }`. Serde derives:
/// type-graph completeness only (see PredField).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScalarPath {
    pub field: PredField,
    pub key: String,
}

// ---------- evaluation ----------

/// A role's bound cards, ALIVE only
/// (§6.3 draft reads). Reads the CURRENT entities through the id handles:
/// a dead-stamped entity reads as absent mid-list —
/// `DESTROY OBJECT` then `IF object.aspect.x` is a silent false.
fn role_cards<'a>(ctx: &PredCtx<'a>, role: &str) -> Vec<&'a Entity> {
    let Some(r) = Role::from_lower(role) else {
        return Vec::new(); // a missing role binds nothing — reads as empty
    };
    ctx.b
        .get(&r)
        .map(|ids| {
            ids.iter()
                .filter_map(|id| ctx.state.entities.get(id))
                .filter(|e| e.dead_action.is_none())
                .collect()
        })
        .unwrap_or_default()
}

/// Evaluate one predicate node.
pub fn eval_pred(p: &Pred, ctx: &PredCtx) -> Result<bool> {
    match p {
        Pred::And(a, b) => {
            // `&&` short-circuits: b is not evaluated (and may error)
            if eval_pred(a, ctx)? {
                eval_pred(b, ctx)
            } else {
                Ok(false)
            }
        }
        Pred::Or(a, b) => {
            if eval_pred(a, ctx)? {
                Ok(true)
            } else {
                eval_pred(b, ctx)
            }
        }
        Pred::Not(a) => Ok(!eval_pred(a, ctx)?),
        Pred::Rel { rtype, role, role2 } => {
            // rel(type, role) pairs cards within one role; rel(type, a, b)
            // crosses two roles — e.g. rel(neighbor, actors.container, object)
            let cards_of = |role: &str| -> Vec<&Entity> {
                let Some(base) = role.strip_suffix(".container") else {
                    return role_cards(ctx, role);
                };
                let cards = role_cards(ctx, base);
                cards
                    .into_iter()
                    .map(|e| {
                        // container redirect: the container attr's stringified
                        // value names the container entity, else the card itself
                        // (the canonical string spelling)
                        let cid = e
                            .attrs
                            .get("container")
                            .map(|v| v.to_canonical_string())
                            .unwrap_or_default();
                        ctx.state.entities.get(&cid).unwrap_or(e)
                    })
                    .collect()
            };
            let a = cards_of(role);
            let b = cards_of(role2.as_deref().unwrap_or(role));
            for (i, ca) in a.iter().enumerate() {
                let start = if role2.is_none() { i + 1 } else { 0 };
                for cb in b.iter().skip(start) {
                    if role2.is_none() && ca.id == cb.id {
                        continue;
                    }
                    if relation_alive(ctx.state, &ca.id, &cb.id, rtype) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        Pred::Every { role, cond } => {
            let cards = role_cards(ctx, role);
            Ok(cards.iter().all(|e| scalar_ok(e, cond)))
        }
        Pred::Any { role, cond } => {
            let cards = role_cards(ctx, role);
            Ok(cards.iter().any(|e| scalar_ok(e, cond)))
        }
        Pred::Exists { name } => {
            // `by_name` holds ALIVE entities only (§5.1); the `dead_action`
            // re-check is the same belt-and-braces `resolve_ref` wears. Under
            // pools (§4.2) the existential is ANY living copy of the name —
            // and the §6.3 visibility law reads the WORLD-VISIBLE pool: a
            // parked copy keeps the name alive (name/#id refs still reach it)
            // but does not keep the name's world-existence true.
            match ctx.state.by_name.get(name) {
                Some(pool) => Ok(pool.iter().any(|id| {
                    ctx.state
                        .entities
                        .get(id)
                        .is_some_and(|e| e.dead_action.is_none() && !is_parked(e))
                })),
                None => Ok(false),
            }
        }
        Pred::ExistsQuery { query } => {
            // the WHOLE world, not the step's bound roles — CS extantreqs
            // ("anywhere in play"); entities insertion order is the scan
            // order (match_query is order-insensitive per-card). §6.3's
            // visibility law: UNPARKED alive cards only.
            for e in ctx.state.entities.values() {
                if e.dead_action.is_none() && !is_parked(e) && match_query(e, query) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Pred::Cmp { l, op, r } => {
            // a numeric attr entry ABSENT from an ALIVE bound card compares
            // as 0, BOTH operands (cmp_operand — the §4.4 SET-delta rule
            // and total()'s unwrap_or(0)); every other unresolved operand
            // still never compares (either side None → false)
            let lv = cmp_operand(l, ctx)?;
            let rv = cmp_operand(r, ctx)?;
            Ok(match (lv, rv) {
                (Some(lv), Some(rv)) => op.num_cmp(lv, rv),
                _ => false,
            })
        }
    }
}

/// One `Pred::Cmp` operand read: `eval_expr`, plus the E1 default — a
/// numeric attr entry ABSENT from an ALIVE bound card compares as 0, so
/// `attr:x < 3` holds from missing and `attr:x >= 3` fails (§4.4's
/// SET-delta rule "a missing entry counts as 0" and total()'s
/// `unwrap_or(0)`; script entities mint empty attrs, so a `silver < cost`
/// broke-gate holds for a first-time buyer). Everything else keeps
/// `eval_expr`'s discipline: an ASPECT path stays §6.3-frozen
/// missing→false ("a missing aspect on a bound card reads false"); a role
/// with no bound card is the §6.3 unresolvable ref (strict rejects inside
/// `eval_expr`); all-dead bindings stay silent-false (the dead-stamp rule);
/// and a PRESENT non-numeric entry never compares.
fn cmp_operand(x: &ValExpr, ctx: &PredCtx) -> Result<Option<i64>> {
    let ValExpr::Path { role, field: PredField::Attr, key } = x else {
        return eval_expr(x, ctx);
    };
    let bound: &[String] = match Role::from_lower(role) {
        Some(r) => ctx.b.get(&r).map(|v| v.as_slice()).unwrap_or(&[]),
        None => &[], // an undeclared role binds nothing
    };
    let Some(e) =
        bound.iter().filter_map(|id| ctx.state.entities.get(id)).find(|e| e.dead_action.is_none())
    else {
        // no bindings (§6.3: strict rejects inside eval_expr) or all-dead
        // (the silent dead-stamp false) — the un-coerced paths
        return eval_expr(x, ctx);
    };
    Ok(match e.attrs.get(key) {
        Some(AttrValue::Num(n)) => Some(*n),
        None => Some(0), // the E1 default: absent entry on a living card
        Some(_) => None, // present non-numeric: unresolved, never compares
    })
}

/// Evaluate a scalar condition (missing aspect → false).
fn scalar_ok(e: &Entity, cond: &ScalarCond) -> bool {
    let v = match cond.path.field {
        PredField::Aspect => aspect_get(e, &cond.path.key),
        PredField::Attr => number_attr(e, &cond.path.key),
    };
    match v {
        Some(v) => cond.op.num_cmp(v, cond.value),
        None => false,
    }
}

/// Evaluate one value expression (None = unresolved).
pub fn eval_expr(x: &ValExpr, ctx: &PredCtx) -> Result<Option<i64>> {
    match x {
        ValExpr::Int { v } => Ok(Some(*v)),
        ValExpr::Roll => {
            // §4.4: unbound in a taken branch is a step rejection (§9.2) —
            // never a silent false or an implicit 0
            match ctx.roll {
                Some(roll) => Ok(Some(roll)),
                None => Err(ChorusError::new(
                    "'roll' is unbound — no ROLL has run in this outcome list before this point",
                )),
            }
        }
        ValExpr::Count { role } => Ok(Some(role_cards(ctx, role).len() as i64)),
        ValExpr::CountQuery { query } => {
            // world scope, like `ExistsQuery` (§6.3): every ALIVE, UNPARKED
            // card matching — the visibility law keeps the interior out of
            // the world counts
            let mut n: i64 = 0;
            for e in ctx.state.entities.values() {
                if e.dead_action.is_none() && !is_parked(e) && match_query(e, query) {
                    n += 1;
                }
            }
            Ok(Some(n))
        }
        ValExpr::Total { role, path } => {
            // reduce along role_cards order; saturating add — only absurd
            // magnitudes overflow, and no log pins their sums
            let sum = role_cards(ctx, role)
                .iter()
                .fold(0i64, |sum, e| sum.saturating_add(aspect_get(e, path).unwrap_or(0)));
            Ok(Some(sum))
        }
        ValExpr::TotalBound { path } => {
            // §6.3: the window's bindings across ALL roles, per-binding — the
            // same id under two roles contributes twice (the bindings map is
            // the bookkeeping). Missing aspects read 0 (total()'s
            // unwrap_or(0)); dead-stamped ids drop like every role read.
            let sum = ctx
                .b
                .values()
                .flatten()
                .filter_map(|id| ctx.state.entities.get(id))
                .filter(|e| e.dead_action.is_none())
                .fold(0i64, |sum, e| sum.saturating_add(aspect_get(e, path).unwrap_or(0)));
            Ok(Some(sum))
        }
        ValExpr::TotalTable { path } => {
            // §6.3 world scope, like `CountQuery`: every ALIVE, UNPARKED card
            // (the visibility law), missing aspect reading 0
            let sum = ctx
                .state
                .entities
                .values()
                .filter(|e| e.dead_action.is_none() && !is_parked(e))
                .fold(0i64, |sum, e| sum.saturating_add(aspect_get(e, path).unwrap_or(0)));
            Ok(Some(sum))
        }
        ValExpr::TotalParked { path } => {
            // §6.3 (v0.10.0) interior scope — the exact complement of the
            // table scope's card set: every ALIVE PARKED card, missing aspect
            // reading 0. The two pools partition the living world, so a death
            // gate on parked dread and a table-scope gate never double-count
            // a mint.
            let sum = ctx
                .state
                .entities
                .values()
                .filter(|e| e.dead_action.is_none() && is_parked(e))
                .fold(0i64, |sum, e| sum.saturating_add(aspect_get(e, path).unwrap_or(0)));
            Ok(Some(sum))
        }
        ValExpr::Path { role, field, key } => {
            // the oldest ALIVE bound card: first id, in binding order, that
            // is still alive
            let bound: &[String] = match Role::from_lower(role) {
                Some(r) => ctx.b.get(&r).map(|v| v.as_slice()).unwrap_or(&[]),
                None => &[], // an undeclared role binds nothing
            };
            let found = bound
                .iter()
                .filter_map(|id| ctx.state.entities.get(id))
                .find(|e| e.dead_action.is_none());
            match found {
                Some(e) => Ok(match field {
                    PredField::Aspect => aspect_get(e, key),
                    PredField::Attr => number_attr(e, key),
                }),
                None => {
                    // §6.3: an IF condition names a CARD that is not there —
                    // an unresolvable ref, a step rejection (§9.2) — but ONLY
                    // when the role has NO bindings at all; all-dead bindings
                    // read false silently (the dead-stamp rule)
                    if ctx.strict && bound.is_empty() {
                        return Err(ChorusError::new(format!(
                            "IF condition: operand '{role}.{field}.{key}' cannot resolve — role '{role}' has no bound card",
                            field = field.as_str()
                        )));
                    }
                    Ok(None)
                }
            }
        }
        ValExpr::Param { name } => {
            // expansion (§10) replaces every param before compile finishes;
            // this is a defensive tripwire, never a gameplay path
            Err(ChorusError::new(format!(
                "internal: unresolved function parameter '{name}' was evaluated"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dict::core_dictionary;
    use crate::core::pred_parse::parse_predicate;
    use crate::core::types::{new_state, AspectVal, Aspects, Attrs};

    fn ent(id: &str, name: &str, kind: &str, aspects: Aspects, attrs: Attrs) -> Entity {
        Entity {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects,
            attrs,
            born_action: Some(0),
            dead_action: None,
            stackable: false,
            desc: None,
        }
    }

    #[test]
    fn parses_and_evaluates_the_doc_grammar() {
        let dict = core_dictionary();
        // count(actors) = 2 AND NOT any(actors, aspect:occult >= 2)
        let p = parse_predicate("count(actors) = 2 AND NOT any(actors, aspect:occult >= 2)", &dict)
            .unwrap();
        let mut state = new_state(0);
        let mut e1 = ent("a1", "A", "character", Aspects::new(), Attrs::new());
        e1.aspects.insert("occult".into(), AspectVal::Num(3));
        let _ = state.entities.insert("a1".into(), e1.clone());
        let mut e2 = ent("a2", "B", "character", Aspects::new(), Attrs::new());
        e2.aspects.insert("occult".into(), AspectVal::Num(1));
        let _ = state.entities.insert("a2".into(), e2.clone());
        let mut bindings = RoleBindings::new();
        bindings.insert(Role::Actors, vec!["a1".into(), "a2".into()]);
        let ctx = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        // count(actors)=2 holds, any(occult>=2) holds (a1:3) → NOT any fails:
        // the conjunction is FALSE
        assert!(!eval_pred(&p, &ctx).unwrap());
        // the same tree with a predicate that passes end to end
        let p_pos = parse_predicate("count(actors) = 2 AND any(actors, aspect:occult >= 2)", &dict)
            .unwrap();
        assert!(eval_pred(&p_pos, &ctx).unwrap());
        let p2 = parse_predicate("any(actors, aspect:occult >= 2)", &dict).unwrap();
        assert!(eval_pred(&p2, &ctx).unwrap());
        let p3 = parse_predicate("every(actors, aspect:occult >= 2)", &dict).unwrap();
        assert!(!eval_pred(&p3, &ctx).unwrap()); // e2 has 1 — every fails
    }

    #[test]
    fn three_valued_boundaries_are_frozen() {
        let dict = core_dictionary();
        // every([]) is vacuously true, any([]) vacuously false
        let p = parse_predicate("every(actors, aspect:x >= 1)", &dict).unwrap();
        let p2 = parse_predicate("any(actors, aspect:x >= 1)", &dict).unwrap();
        let mut state = new_state(0);
        let bindings = RoleBindings::new();
        let ctx = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        assert!(eval_pred(&p, &ctx).unwrap());
        assert!(!eval_pred(&p2, &ctx).unwrap());
        // all-dead bindings read false silently, never a strict throw
        let mut dead = ent("d1", "D", "character", Aspects::new(), Attrs::new());
        dead.dead_action = Some(9);
        let _ = state.entities.insert("d1".into(), dead);
        let mut bindings2 = RoleBindings::new();
        bindings2.insert(Role::Object, vec!["d1".into()]);
        let ctx2 = PredCtx { b: &bindings2, state: &state, strict: true, roll: None };
        let p3 = parse_predicate("object.aspect.x >= 1", &dict).unwrap();
        assert!(!eval_pred(&p3, &ctx2).unwrap());
        // strict + ZERO bindings → the exact rejection message
        let bindings3 = RoleBindings::new();
        let ctx3 = PredCtx { b: &bindings3, state: &state, strict: true, roll: None };
        let e = eval_pred(&p3, &ctx3).unwrap_err();
        assert_eq!(
            e.msg,
            "IF condition: operand 'object.aspect.x' cannot resolve — role 'object' has no bound card"
        );
    }

    #[test]
    fn exists_and_count_query_forms() {
        let dict = core_dictionary();
        let p = parse_predicate("exists(THE_SEALED_DOOR)", &dict).unwrap();
        let pq = parse_predicate("exists(aspect:occult >= 2)", &dict).unwrap();
        let pc = parse_predicate("count(aspect:occult >= 2) = 1", &dict).unwrap();
        let mut state = new_state(0);
        let mut door = ent("x1", "THE_SEALED_DOOR", "place", Aspects::new(), Attrs::new());
        door.aspects.insert("occult".into(), AspectVal::Num(5));
        // `by_name` pool drives exists(NAME)
        let _ = state.entities.insert("x1".into(), door.clone());
        let _ = state.by_name.insert("THE_SEALED_DOOR".into(), vec!["x1".into()]);
        let bindings = RoleBindings::new();
        let ctx = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        assert!(eval_pred(&p, &ctx).unwrap());
        assert!(eval_pred(&pq, &ctx).unwrap());
        assert!(eval_pred(&pc, &ctx).unwrap());
        // dead entity: exists(NAME) false, count drops it (rebuild the ctx
        // after the mutation to end the shared borrow)
        let _ = state.entities.get_mut("x1").map(|e| e.dead_action = Some(3));
        let ctx = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        assert!(!eval_pred(&p, &ctx).unwrap());
        assert!(!eval_pred(&pc, &ctx).unwrap());
    }
}
