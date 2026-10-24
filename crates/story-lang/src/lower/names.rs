//! Declaration-time checks: predicate recursion and the declared-name law for predicate bodies.

use super::Compiler;
use crate::ast;
use std::collections::{BTreeSet, HashSet};
use story_content::code;

impl Compiler {
    /// Declaration-time predicate checks: recursion [E230], key registration
    /// (every aspect, legacy, kind, and card name the body reads must be
    /// declared — language.md §2.5).
    pub(super) fn check_predicate_decl(&mut self, p: &ast::PredAst) {
        let mut stack = vec![p.name.1.clone()];
        let mut visiting: BTreeSet<String> = BTreeSet::new();
        self.pred_cycle_dfs(&p.name.1, &mut visiting, &mut stack);
        if let Some(body) = &p.body {
            let params: HashSet<String> = p.params.iter().map(|(_, n)| n.clone()).collect();
            check_cond_names(self, body, &params);
        }
    }

    fn pred_cycle_dfs(
        &mut self,
        name: &str,
        visiting: &mut BTreeSet<String>,
        stack: &mut Vec<String>,
    ) {
        if visiting.contains(name) {
            if let Some(reg) = self.preds.get(name) {
                if !reg.cycle_checked {
                    let sp = reg.span.clone();
                    let chain = stack.join(" -> ");
                    self.err(sp.error(
                        code::E230,
                        format!("predicate `{name}` expansion reaches itself ({chain})"),
                    ));
                    if let Some(reg) = self.preds.get_mut(name) {
                        reg.cycle_checked = true;
                    }
                }
            }
            return;
        }
        // Do not re-descend into predicates already proven acyclic.
        let body_calls = match self.preds.get(name) {
            Some(reg) if !reg.cycle_checked => reg
                .body
                .as_ref()
                .map(|b| {
                    let mut calls = Vec::new();
                    collect_pred_calls(b, &mut calls);
                    calls
                })
                .unwrap_or_default(),
            _ => return,
        };
        visiting.insert(name.to_string());
        stack.push(name.to_string());
        for call in body_calls {
            if self.preds.contains_key(&call) {
                self.pred_cycle_dfs(&call, visiting, stack);
            }
        }
        stack.pop();
        visiting.remove(name);
        if let Some(reg) = self.preds.get_mut(name) {
            reg.cycle_checked = true;
        }
    }
}

fn collect_pred_calls(c: &ast::ACond, out: &mut Vec<String>) {
    match c {
        ast::ACond::Or(l, r) | ast::ACond::And(l, r) => {
            collect_pred_calls(l, out);
            collect_pred_calls(r, out);
        }
        ast::ACond::Not(l) => collect_pred_calls(l, out),
        ast::ACond::Rel { from, to, .. } => {
            collect_ref_calls(from, out);
            collect_ref_calls(to, out);
        }
        ast::ACond::Cmp { l, r, .. } => {
            collect_expr_calls(l, out);
            collect_expr_calls(r, out);
        }
        ast::ACond::Call { name, args, .. } => {
            out.push(name.1.clone());
            for a in args {
                collect_expr_calls(a, out);
            }
        }
        ast::ACond::Name(_, (sp, n)) => {
            out.push(n.clone());
            let _ = sp;
        }
    }
}

fn collect_expr_calls(e: &ast::AExpr, out: &mut Vec<String>) {
    match e {
        ast::AExpr::Count { terms, .. } => {
            for t in terms {
                if let ast::AQueryTerm::Call { name, args, .. } = t {
                    out.push(name.1.clone());
                    for a in args {
                        collect_expr_calls(a, out);
                    }
                }
            }
        }
        ast::AExpr::Rel { from, to, .. } => {
            collect_ref_calls(from, out);
            collect_ref_calls(to, out);
        }
        ast::AExpr::Neg(_, inner) => collect_expr_calls(inner, out),
        ast::AExpr::Bin { l, r, .. } => {
            collect_expr_calls(l, out);
            collect_expr_calls(r, out);
        }
        _ => {}
    }
}

fn collect_ref_calls(r: &ast::ACardRef, out: &mut Vec<String>) {
    match r {
        ast::ACardRef::Name(..) | ast::ACardRef::Subject(_) => {}
        ast::ACardRef::First { terms, .. } => {
            for t in terms {
                if let ast::AQueryTerm::Call { name, args, .. } = t {
                    out.push(name.1.clone());
                    for a in args {
                        collect_expr_calls(a, out);
                    }
                }
            }
        }
    }
}

/// Declaration-time name checks inside a predicate body: every aspect key,
/// legacy key, kind, card name, and relation kind the body reads must be
/// declared.
fn check_cond_names(c: &mut Compiler, cond: &ast::ACond, params: &HashSet<String>) {
    match cond {
        ast::ACond::Or(l, r) | ast::ACond::And(l, r) => {
            check_cond_names(c, l, params);
            check_cond_names(c, r, params);
        }
        ast::ACond::Not(l) => check_cond_names(c, l, params),
        ast::ACond::Cmp { l, r, .. } => {
            check_expr_names(c, l, params);
            check_expr_names(c, r, params);
        }
        ast::ACond::Rel { kind, from, to, .. } => {
            c.check_relation_kind(&kind.1, &kind.0);
            check_card_ref_names(c, from, params);
            check_card_ref_names(c, to, params);
        }
        ast::ACond::Call { name, args, .. } => {
            if !params.contains(&name.1) && !c.preds.contains_key(&name.1) {
                c.err(
                    name.0
                        .error(code::E244, format!("unknown predicate `{}`", name.1)),
                );
            }
            for a in args {
                check_expr_names(c, a, params);
            }
        }
        ast::ACond::Name(_, (sp, n)) => {
            if !params.contains(n) {
                c.err(sp.error(code::E244, format!("unknown predicate `{n}`")));
            }
        }
    }
}

/// Declaration-time card-ref checks: a bare name must be a declared `unique`
/// card unless it is a visible parameter.
fn check_card_ref_names(c: &mut Compiler, r: &ast::ACardRef, params: &HashSet<String>) {
    match r {
        ast::ACardRef::Name(_, (sp, n)) => {
            if !params.contains(n) {
                c.check_card_unique(n, sp);
            }
        }
        ast::ACardRef::Subject(_) => {}
        ast::ACardRef::First { scope, terms, .. } => {
            check_scope_names(c, scope, params);
            for t in terms {
                check_term_names(c, t, params);
            }
        }
    }
}

fn check_expr_names(c: &mut Compiler, e: &ast::AExpr, params: &HashSet<String>) {
    match e {
        ast::AExpr::NamePath { card, aspect, .. } => {
            if !params.contains(&card.1) {
                c.check_card_unique(&card.1, &card.0);
            }
            c.check_aspect_key(&aspect.1, &aspect.0);
        }
        ast::AExpr::Ident(_, n) => {
            if !params.contains(n) {
                // Bare identifiers resolve to roll bindings at call sites;
                // at declaration they cannot be checked further.
                let _ = n;
            }
        }
        ast::AExpr::Legacy { key, .. } => {
            c.check_legacy_key(&key.1, &key.0);
        }
        ast::AExpr::Count { scope, terms, .. } => {
            check_scope_names(c, scope, params);
            for t in terms {
                check_term_names(c, t, params);
            }
        }
        ast::AExpr::Total { scope, key, .. } => {
            check_scope_names(c, scope, params);
            c.check_aspect_key(&key.1, &key.0);
        }
        ast::AExpr::Alive { card, .. } => {
            if !params.contains(&card.1) {
                c.check_card_unique(&card.1, &card.0);
            }
        }
        ast::AExpr::Rel { kind, from, to, .. } => {
            c.check_relation_kind(&kind.1, &kind.0);
            check_card_ref_names(c, from, params);
            check_card_ref_names(c, to, params);
        }
        ast::AExpr::Neg(_, inner) => check_expr_names(c, inner, params),
        ast::AExpr::Bin { l, r, .. } => {
            check_expr_names(c, l, params);
            check_expr_names(c, r, params);
        }
        ast::AExpr::Int(..) | ast::AExpr::Dur(..) | ast::AExpr::Clock(_) => {}
    }
}

fn check_scope_names(c: &mut Compiler, s: &ast::AScope, params: &HashSet<String>) {
    match s {
        ast::AScope::Zone(_, (sp, z)) => {
            c.check_zone(z, sp);
        }
        ast::AScope::Related { kind, of, .. } => {
            c.check_relation_kind(&kind.1, &kind.0);
            check_card_ref_names(c, of, params);
        }
        _ => {}
    }
}

fn check_term_names(c: &mut Compiler, t: &ast::AQueryTerm, params: &HashSet<String>) {
    match t {
        ast::AQueryTerm::Aspect { key, .. } => {
            c.check_aspect_key(&key.1, &key.0);
        }
        ast::AQueryTerm::Name(_, (sp, n)) => {
            if !params.contains(n) {
                c.check_card_exists(n, sp);
            }
        }
        ast::AQueryTerm::Kind(_, (sp, k)) => {
            c.check_kind(k, sp);
        }
        ast::AQueryTerm::Not(inner) => check_term_names(c, inner, params),
        ast::AQueryTerm::Call { name, args, .. } => {
            if !params.contains(&name.1) && !c.preds.contains_key(&name.1) {
                c.err(
                    name.0
                        .error(code::E244, format!("unknown predicate `{}`", name.1)),
                );
            }
            for a in args {
                check_expr_names(c, a, params);
            }
        }
    }
}
