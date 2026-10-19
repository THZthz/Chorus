//! Load-time predicate lints, all run at content compile with the runtime
//! path unaffected (§4.5, §4.4): the count()/total() role lint
//! (`lint_pred`/`lint_val_expr`), the trigger-binding lint for IF
//! conditions of outcome lists with empty bindings
//! (`lint_trigger_pred`/`lint_trigger_val_expr_what`), and the best-effort
//! may-bind analysis for the `roll` operand (`lint_roll_may_bind`). The AST
//! and evaluator live in `predicate`, the parser in `pred_parse`.

use crate::core::predicate::{Pred, ValExpr};
use crate::core::roles::{Role, ROLES};
use crate::core::sentence::Outcome;
use crate::error::{ChorusError, Result};

/// `ROLES` joined with ', ' (the ROLE_LIST message text).
#[must_use]
fn role_list() -> String {
    ROLES.iter().map(|r| r.as_str()).collect::<Vec<_>>().join(", ")
}

/// count()/total() arguments must be a
/// DECLARED ROLE or a query-term starter. Called at content compile for
/// every recipe `when`; the runtime path is unaffected.
pub fn lint_pred(where_: &str, p: &Pred) -> Result<()> {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => {
            lint_pred(where_, a)?;
            lint_pred(where_, b)
        }
        Pred::Not(a) => lint_pred(where_, a),
        Pred::Cmp { l, r, .. } => {
            lint_val_expr(where_, l)?;
            lint_val_expr(where_, r)
        }
        // Rel/Every/Any/Exists/ExistsQuery carry no count/total args
        _ => Ok(()),
    }
}

/// Lint the count()/total() operand expressions of a predicate.
pub fn lint_val_expr(where_: &str, x: &ValExpr) -> Result<()> {
    let is_role = |role: &str| Role::from_lower(role).is_some();
    if let ValExpr::Count { role } = x {
        if !is_role(role) {
            return Err(ChorusError::new(format!(
                "{where_}: count() argument '{role}' is neither a role ({}) nor a query (name:x, kind:k, aspect:a>=1, state:s, -term) — did you mean count(name:{role})?",
                role_list()
            )));
        }
    }
    if let ValExpr::Total { role, .. } = x {
        if !is_role(role) {
            return Err(ChorusError::new(format!(
                "{where_}: total() first argument '{role}' is not a declared role ({}) — total pools a role's aspects; the scope forms are total(bound, aspect:k) and total(table, aspect:k), and count(name:x) counts a pool by name",
                role_list()
            )));
        }
    }
    Ok(())
}

/// Trigger (slot + expiry) outcome lists
/// have EMPTY event bindings, so any role-dependent operand in an IF
/// condition there would silently read false/0. Called from content compile.
pub fn lint_trigger_pred(where_: &str, p: &Pred) -> Result<()> {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => {
            lint_trigger_pred(where_, a)?;
            lint_trigger_pred(where_, b)
        }
        Pred::Not(a) => lint_trigger_pred(where_, a),
        Pred::Cmp { l, r, .. } => {
            lint_trigger_val_expr(where_, l)?;
            lint_trigger_val_expr(where_, r)
        }
        Pred::Rel { rtype, role, role2 } => Err(ChorusError::new(format!(
            "{where_}: IF operand rel({rtype}, {role}{role2}) reads a step role — trigger bindings are empty, it would always be false; use world forms (exists(…), count(name:…))",
            role2 = role2.as_ref().map(|r| format!(", {r}")).unwrap_or_default()
        ))),
        Pred::Every { role, .. } | Pred::Any { role, .. } => {
            let is_every = matches!(p, Pred::Every { .. });
            Err(ChorusError::new(format!(
                "{where_}: IF operand {t}({role}, …) reads a step role — trigger bindings are empty, it would always be {b}; use world forms (exists(…), count(name:…))",
                t = if is_every { "every" } else { "any" },
                b = if is_every { "true" } else { "false" }
            )))
        }
        _ => Ok(()), // `Exists`/`ExistsQuery` are world forms
    }
}

/// `what` names the operand kind in the messages
/// ('IF operand' by default; content compile passes 'ROLL argument' for
/// vexpr operands).
pub fn lint_trigger_val_expr_what(where_: &str, x: &ValExpr, what: &str) -> Result<()> {
    match x {
        ValExpr::Count { role } => Err(ChorusError::new(format!(
            "{where_}: {what} count({role}) reads a step role — trigger bindings are empty, it would always be 0; use count(<query>) (e.g. count(name:{role}))"
        ))),
        ValExpr::Total { role, path } => Err(ChorusError::new(format!(
            "{where_}: {what} total({role}, aspect:{path}) reads a step role — trigger bindings are empty, it would always be 0"
        ))),
        // §6.3: `bound` reads the WINDOW's bindings — the same empty bookkeeping
        // an event trigger's IF conditions see, so it would always be 0 too;
        // `table` is a world form and never fires this lint
        ValExpr::TotalBound { path } => Err(ChorusError::new(format!(
            "{where_}: {what} total(bound, aspect:{path}) reads the window's bindings — trigger bindings are empty, it would always be 0"
        ))),
        ValExpr::Path { role, field, key } => Err(ChorusError::new(format!(
            "{where_}: {what} {role}.{field}.{key} reads a step role — trigger bindings are empty, it would always be false; use world forms (exists(…), count(name:…))",
            field = field.as_str()
        ))),
        // `Int` / `CountQuery` are world forms; `roll` (§4.4) reads the list's
        // own dice, not a role
        _ => Ok(()),
    }
}

/// The default-`what` entry point.
fn lint_trigger_val_expr(where_: &str, x: &ValExpr) -> Result<()> {
    lint_trigger_val_expr_what(where_, x, "IF operand")
}

// ---------- §4.4 compile-time may-bind lint for the `roll` operand ----------

/// Does this predicate read the `roll`
/// operand anywhere?
#[must_use]
fn pred_uses_roll(p: &Pred) -> bool {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => pred_uses_roll(a) || pred_uses_roll(b),
        Pred::Not(a) => pred_uses_roll(a),
        Pred::Cmp { l, r, .. } => matches!(l, ValExpr::Roll) || matches!(r, ValExpr::Roll),
        _ => false,
    }
}

/// Best-effort may-bind analysis (§4.4,
/// false NEGATIVES allowed). The scan walks each list linearly, descending
/// into IF branch bodies; a ROLL seen anywhere earlier makes later references
/// lint-clean.
pub fn lint_roll_may_bind(where_: &str, out: &[Outcome]) -> Result<()> {
    walk_roll_may_bind(where_, out, false)?;
    Ok(())
}

/// Walk one list; returns whether it
/// could possibly bind `roll` (contain a ROLL).
fn walk_roll_may_bind(where_: &str, list: &[Outcome], bound: bool) -> Result<bool> {
    let mut may_bind = bound;
    for o in list {
        if o.op == "ROLL" {
            may_bind = true;
        } else if o.op == "IF" {
            if let Some(arms) = &o.arms {
                for arm in arms {
                    if pred_uses_roll(&arm.when) && !may_bind {
                        return Err(ChorusError::new(format!(
                            "{where_}: 'roll' is read but no ROLL can have run before it in this outcome list — author a ROLL earlier in the list (the dice binding is per list/clause, §4.4)"
                        )));
                    }
                }
                // branch bodies may bind: ANY branch containing a possible
                // ROLL makes everything after this IF lint-clean (false
                // negatives allowed); note the recursion sees may_bind as of
                // the IF's entry, before branch updates
                let mut branch_binds = false;
                for arm in arms {
                    if walk_roll_may_bind(where_, &arm.then, may_bind)? {
                        branch_binds = true;
                    }
                }
                if let Some(fallback) = &o.fallback {
                    if walk_roll_may_bind(where_, fallback, may_bind)? {
                        branch_binds = true;
                    }
                }
                if branch_binds {
                    may_bind = true;
                }
            }
        }
    }
    Ok(may_bind)
}

// (the modules' cyclic type use is intra-crate by design)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dict::core_dictionary;
    use crate::core::pred_parse::parse_predicate;

    #[test]
    fn lint_messages_name_roles_and_world_forms() {
        let dict = core_dictionary();
        let p = parse_predicate("count(DREAD) >= 3", &dict).unwrap();
        let e = lint_pred("recipe r1", &p).unwrap_err();
        assert_eq!(
            e.msg,
            "recipe r1: count() argument 'DREAD' is neither a role (actors, object, with, at, about, to) nor a query (name:x, kind:k, aspect:a>=1, state:s, -term) — did you mean count(name:DREAD)?"
        );
        let p2 = parse_predicate("total(BOB, aspect:x) >= 1", &dict).unwrap();
        let e2 = lint_pred("recipe r2", &p2).unwrap_err();
        assert_eq!(
            e2.msg,
            "recipe r2: total() first argument 'BOB' is not a declared role (actors, object, with, at, about, to) — total pools a role's aspects; the scope forms are total(bound, aspect:k) and total(table, aspect:k), and count(name:x) counts a pool by name"
        );
        // trigger lint: rel/every/count over step roles are load errors
        let p3 = parse_predicate("rel(neighbor, actors)", &dict).unwrap();
        let e3 = lint_trigger_pred("trigger t", &p3).unwrap_err();
        assert_eq!(
            e3.msg,
            "trigger t: IF operand rel(neighbor, actors) reads a step role — trigger bindings are empty, it would always be false; use world forms (exists(…), count(name:…))"
        );
        let p4 = parse_predicate("every(actors, aspect:x >= 1)", &dict).unwrap();
        let e4 = lint_trigger_pred("trigger t", &p4).unwrap_err();
        assert_eq!(
            e4.msg,
            "trigger t: IF operand every(actors, …) reads a step role — trigger bindings are empty, it would always be true; use world forms (exists(…), count(name:…))"
        );
        // §6.3 sum scopes in trigger IF conditions: `bound` reads the (empty)
        // event bindings — the same always-0 error; `table` is a world form
        let p5 = parse_predicate("total(bound, aspect:dread) >= 1", &dict).unwrap();
        let e5 = lint_trigger_pred("trigger t", &p5).unwrap_err();
        assert_eq!(
            e5.msg,
            "trigger t: IF operand total(bound, aspect:dread) reads the window's bindings — trigger bindings are empty, it would always be 0"
        );
        let p6 = parse_predicate("total(table, aspect:dread) >= 1", &dict).unwrap();
        assert!(lint_trigger_pred("trigger t", &p6).is_ok());
    }
}
