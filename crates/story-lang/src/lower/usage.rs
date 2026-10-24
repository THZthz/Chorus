//! Parameter usage analysis: classifying each predicate and effectset parameter as a card or integer use.

use super::Compiler;
use crate::ast;
use std::collections::{BTreeMap, HashSet};

// ---------------------------------------------------------------------------
// Parameter usage analysis (language.md §2.5, §6: parameters are integer
// expressions or card references). A bare parameter passed as a call argument
// inherits the callee's declared typing for that parameter, so forwarding
// chains classify correctly; the callee's flags are resolved on demand with a
// cycle guard (a cycle is [E230]'s business).
// ---------------------------------------------------------------------------

type Usage = BTreeMap<String, (bool, bool)>;

impl Compiler {
    /// The parameter-usage flags of one predicate, memoized.
    pub(super) fn pred_usage_of(&mut self, name: &str, stack: &mut Vec<String>) -> Usage {
        if let Some(m) = self.pred_usage_memo.get(name) {
            return m.clone();
        }
        if stack.iter().any(|s| s == name) {
            return BTreeMap::new();
        }
        let Some(reg) = self.preds.get(name) else {
            return BTreeMap::new();
        };
        let params: HashSet<String> = reg.params.iter().map(|(_, n)| n.clone()).collect();
        let body = reg.body.clone();
        stack.push(name.to_string());
        let mut usage: Usage = params.iter().map(|n| (n.clone(), (false, false))).collect();
        if let Some(body) = body {
            self.usage_cond(&body, &params, &mut usage, stack);
        }
        stack.pop();
        self.pred_usage_memo.insert(name.to_string(), usage.clone());
        usage
    }

    /// The parameter-usage flags of one effectset, memoized.
    pub(super) fn set_usage_of(&mut self, name: &str, stack: &mut Vec<String>) -> Usage {
        if let Some(m) = self.set_usage_memo.get(name) {
            return m.clone();
        }
        if stack.iter().any(|s| s == name) {
            return BTreeMap::new();
        }
        let Some(reg) = self.sets.get(name) else {
            return BTreeMap::new();
        };
        let params: HashSet<String> = reg.params.iter().map(|(_, n)| n.clone()).collect();
        let body = reg.body.clone();
        stack.push(name.to_string());
        let mut usage: Usage = params.iter().map(|n| (n.clone(), (false, false))).collect();
        if let Some((_, body)) = body {
            for eff in &body {
                self.usage_effect(eff, &params, &mut usage, stack);
            }
        }
        stack.pop();
        self.set_usage_memo.insert(name.to_string(), usage.clone());
        usage
    }

    /// Is the k-th argument of a call to predicate `callee` a card reference?
    fn pred_arg_is_card(&mut self, callee: &str, k: usize, stack: &mut Vec<String>) -> bool {
        let usage = self.pred_usage_of(callee, stack);
        self.preds
            .get(callee)
            .and_then(|r| r.params.get(k))
            .and_then(|(_, pn)| usage.get(pn))
            .is_some_and(|(c, _)| *c)
    }

    /// Is the k-th argument of a call to effectset `callee` a card reference?
    fn set_arg_is_card(&mut self, callee: &str, k: usize, stack: &mut Vec<String>) -> bool {
        let usage = self.set_usage_of(callee, stack);
        self.sets
            .get(callee)
            .and_then(|r| r.params.get(k))
            .and_then(|(_, pn)| usage.get(pn))
            .is_some_and(|(c, _)| *c)
    }

    fn usage_cond(
        &mut self,
        c: &ast::ACond,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match c {
            ast::ACond::Or(l, r) | ast::ACond::And(l, r) => {
                self.usage_cond(l, params, usage, stack);
                self.usage_cond(r, params, usage, stack);
            }
            ast::ACond::Not(l) => self.usage_cond(l, params, usage, stack),
            ast::ACond::Rel { from, to, .. } => {
                self.usage_card_ref(from, params, usage, stack);
                self.usage_card_ref(to, params, usage, stack);
            }
            ast::ACond::Cmp { l, r, .. } => {
                self.usage_expr(l, params, usage, stack);
                self.usage_expr(r, params, usage, stack);
            }
            ast::ACond::Call { name, args, .. } => {
                if let Some((card, int)) = usage.get_mut(&name.1) {
                    *int = true;
                    let _ = card;
                }
                for (k, a) in args.iter().enumerate() {
                    self.usage_pred_arg(a, &name.1, k, params, usage, stack);
                }
            }
            ast::ACond::Name(_, (sp, n)) => {
                if let Some((card, int)) = usage.get_mut(n) {
                    *int = true;
                    let _ = card;
                }
                let _ = sp;
            }
        }
    }

    /// One call argument: a bare parameter forwarded to a card-typed callee
    /// parameter is a card use; anything else is an integer expression.
    fn usage_pred_arg(
        &mut self,
        a: &ast::AExpr,
        callee: &str,
        k: usize,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        if let ast::AExpr::Ident(_, n) = a {
            if params.contains(n) && self.pred_arg_is_card(callee, k, stack) {
                if let Some((card, _)) = usage.get_mut(n) {
                    *card = true;
                }
                return;
            }
        }
        self.usage_expr(a, params, usage, stack);
    }

    fn usage_expr(
        &mut self,
        e: &ast::AExpr,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match e {
            ast::AExpr::Int(..) | ast::AExpr::Dur(..) | ast::AExpr::Clock(_) => {}
            ast::AExpr::NamePath { card, .. } => {
                if let Some((card_u, _)) = usage.get_mut(&card.1) {
                    *card_u = true;
                }
            }
            ast::AExpr::Ident(_, n) => {
                if let Some((_, int_u)) = usage.get_mut(n) {
                    *int_u = true;
                }
            }
            ast::AExpr::Legacy { .. } => {}
            ast::AExpr::Count { scope, terms, .. } => {
                self.usage_scope(scope, params, usage, stack);
                for t in terms {
                    self.usage_term(t, params, usage, stack);
                }
            }
            ast::AExpr::Total { scope, .. } => {
                self.usage_scope(scope, params, usage, stack);
            }
            ast::AExpr::Alive { card, .. } => {
                if let Some((card_u, _)) = usage.get_mut(&card.1) {
                    *card_u = true;
                }
            }
            ast::AExpr::Rel { from, to, .. } => {
                self.usage_card_ref(from, params, usage, stack);
                self.usage_card_ref(to, params, usage, stack);
            }
            ast::AExpr::Neg(_, inner) => self.usage_expr(inner, params, usage, stack),
            ast::AExpr::Bin { l, r, .. } => {
                self.usage_expr(l, params, usage, stack);
                self.usage_expr(r, params, usage, stack);
            }
        }
    }

    fn usage_term(
        &mut self,
        t: &ast::AQueryTerm,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match t {
            ast::AQueryTerm::Aspect { .. }
            | ast::AQueryTerm::Name(..)
            | ast::AQueryTerm::Kind(..) => {}
            ast::AQueryTerm::Not(inner) => self.usage_term(inner, params, usage, stack),
            ast::AQueryTerm::Call { name, args, .. } => {
                if let Some((_, int_u)) = usage.get_mut(&name.1) {
                    *int_u = true;
                }
                for (k, a) in args.iter().enumerate() {
                    self.usage_pred_arg(a, &name.1, k, params, usage, stack);
                }
            }
        }
    }

    fn usage_effect(
        &mut self,
        e: &ast::AEffect,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match e {
            ast::AEffect::Spawn { card, .. } => {
                if let Some((card_u, _)) = usage.get_mut(&card.1) {
                    *card_u = true;
                }
            }
            ast::AEffect::Destroy { target, .. }
            | ast::AEffect::Consume { target, .. }
            | ast::AEffect::Move { target, .. } => self.usage_target(target, params, usage, stack),
            ast::AEffect::Relate { from, to, .. } | ast::AEffect::Unrelate { from, to, .. } => {
                self.usage_target(from, params, usage, stack);
                self.usage_target(to, params, usage, stack);
            }
            ast::AEffect::Swap {
                target, to, into, ..
            } => {
                self.usage_target(target, params, usage, stack);
                if let Some((card_u, _)) = usage.get_mut(&to.1) {
                    *card_u = true;
                }
                if let Some(ast::AInto::Id(_, id)) = into {
                    if let Some((card_u, _)) = usage.get_mut(id) {
                        *card_u = true;
                    }
                }
            }
            ast::AEffect::Set { target, expr, .. } => {
                self.usage_target(target, params, usage, stack);
                self.usage_expr(expr, params, usage, stack);
            }
            ast::AEffect::Legacy { expr, .. } => self.usage_expr(expr, params, usage, stack),
            ast::AEffect::Narrate { .. } => {}
            ast::AEffect::Roll {
                count,
                sides,
                bonus,
                ..
            } => {
                self.usage_expr(count, params, usage, stack);
                self.usage_expr(sides, params, usage, stack);
                if let Some(b) = bonus {
                    self.usage_expr(b, params, usage, stack);
                }
            }
            ast::AEffect::Draw { .. } => {}
            ast::AEffect::Start { .. } => {}
            ast::AEffect::Continue { .. } => {}
            ast::AEffect::Signal { .. } | ast::AEffect::Ending { .. } => {}
            ast::AEffect::If {
                cond, then, els, ..
            } => {
                self.usage_cond(cond, params, usage, stack);
                for e in then {
                    self.usage_effect(e, params, usage, stack);
                }
                for e in els {
                    self.usage_effect(e, params, usage, stack);
                }
            }
            ast::AEffect::Call { name, args, .. } => {
                if let Some((_, int_u)) = usage.get_mut(&name.1) {
                    *int_u = true;
                }
                for (k, a) in args.iter().enumerate() {
                    // A single-ident argument forwards the parameter; its
                    // typing follows the callee effectset's parameter.
                    if let [(sp, crate::lexer::Tok::Ident(w))] = a.toks.as_slice() {
                        if params.contains(w) {
                            if self.set_arg_is_card(&name.1, k, stack) {
                                if let Some((card, _)) = usage.get_mut(w) {
                                    *card = true;
                                }
                            } else if let Some((_, int_u)) = usage.get_mut(w) {
                                *int_u = true;
                            }
                        }
                        let _ = sp;
                        continue;
                    }
                    for (sp, t) in &a.toks {
                        if let crate::lexer::Tok::Ident(w) = t {
                            if let Some((_, int_u)) = usage.get_mut(w) {
                                *int_u = true;
                            }
                        }
                        let _ = sp;
                    }
                }
            }
        }
    }

    fn usage_target(
        &mut self,
        t: &ast::ATarget,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match t {
            ast::ATarget::Name(_, (sp, n)) => {
                if let Some((card_u, _)) = usage.get_mut(n) {
                    *card_u = true;
                }
                let _ = sp;
            }
            ast::ATarget::NamePath { card, .. } => {
                if let Some((card_u, _)) = usage.get_mut(&card.1) {
                    *card_u = true;
                }
            }
            ast::ATarget::Slot(..) => {}
            ast::ATarget::Query { terms, .. } => {
                for t in terms {
                    self.usage_term(t, params, usage, stack);
                }
            }
            ast::ATarget::Bare { terms, .. } => {
                for t in terms {
                    self.usage_term(t, params, usage, stack);
                }
            }
        }
    }

    /// A relation endpoint's parameter use: a bare parameter name in
    /// card-ref position is a card use.
    fn usage_card_ref(
        &mut self,
        r: &ast::ACardRef,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match r {
            ast::ACardRef::Name(_, (sp, n)) => {
                if let Some((card_u, _)) = usage.get_mut(n) {
                    *card_u = true;
                }
                let _ = sp;
            }
            ast::ACardRef::Subject(_) => {}
            ast::ACardRef::First { scope, terms, .. } => {
                self.usage_scope(scope, params, usage, stack);
                for t in terms {
                    self.usage_term(t, params, usage, stack);
                }
            }
        }
    }

    /// A query scope's parameter use: only `related:<kind> of <card-ref>`
    /// carries a card reference; the other scopes name zones, slots, or the
    /// whole world.
    fn usage_scope(
        &mut self,
        s: &ast::AScope,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        if let ast::AScope::Related { of, .. } = s {
            self.usage_card_ref(of, params, usage, stack);
        }
    }
}
