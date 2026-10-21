//! Expression lowering (language.md §3): scopes, queries, and conditions into
//! the engine-side IR, with derived predicates inlined at the call site —
//! the kernel never sees one (spec.md §4.7). Recursion is [E230]; the shared
//! inlining depth cap is 16 and exceeding it is [E235] (kernel-api.md §6).

use super::{Compiler, Env, ParamVal};
use crate::ast;
use crate::span::Span;
use story_content::code;
use story_content::ir::{self, ArithOp, CmpOp, Expr, LogicOp, QueryTerm, Scope};
use story_core::CardId;

pub(crate) const INLINE_DEPTH_CAP: u32 = 16;

impl Compiler {
    pub(crate) fn lower_cond(&mut self, c: &ast::ACond, env: &mut Env) -> Option<Expr> {
        match c {
            ast::ACond::Or(l, r) => {
                let l = self.lower_cond(l, env)?;
                let r = self.lower_cond(r, env)?;
                Some(Expr::Logic {
                    op: LogicOp::Or,
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            ast::ACond::And(l, r) => {
                let l = self.lower_cond(l, env)?;
                let r = self.lower_cond(r, env)?;
                Some(Expr::Logic {
                    op: LogicOp::And,
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            ast::ACond::Not(l) => Some(Expr::Not(Box::new(self.lower_cond(l, env)?))),
            ast::ACond::Cmp { op, l, r, .. } => {
                let l = self.lower_arith(l, env)?;
                let r = self.lower_arith(r, env)?;
                Some(Expr::Cmp {
                    op: cmp_op(*op),
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
            ast::ACond::Call { span, name, args } => self.pred_call(span, name, args, env),
            ast::ACond::Name(sp, name) => {
                if self.preds.contains_key(&name.1) {
                    self.pred_call(sp, name, &[], env)
                } else {
                    self.err(
                        name.0
                            .error(code::E244, format!("unknown predicate `{}`", name.1)),
                    );
                    None
                }
            }
        }
    }

    pub(crate) fn lower_arith(&mut self, e: &ast::AExpr, env: &mut Env) -> Option<Expr> {
        match e {
            ast::AExpr::Int(_, v) => Some(Expr::Lit(*v)),
            // A duration literal denotes its minute count.
            ast::AExpr::Dur(_, v) => Some(Expr::Lit(*v)),
            ast::AExpr::Clock(_) => Some(Expr::Clock),
            ast::AExpr::NamePath { card, aspect, .. } => self.lower_name_path(card, aspect, env),
            ast::AExpr::Ident(sp, n) => {
                if let Some((_, val)) = env.params.iter().rev().find(|(p, _)| p == n) {
                    return match val {
                        ParamVal::Int(x) => Some(x.clone()),
                        ParamVal::Card(_) | ParamVal::Subject => {
                            self.err(sp.error(
                                code::E245,
                                format!("parameter `{n}` is bound to a card reference here, not an integer expression"),
                            ));
                            None
                        }
                    };
                }
                if env.rolls.contains(n) {
                    return Some(Expr::Roll);
                }
                self.err(
                    sp.error(
                        code::E244,
                        format!("unknown name `{n}` in expression (no roll binding or parameter of that name is visible)"),
                    ),
                );
                None
            }
            ast::AExpr::Legacy { key, .. } => {
                if self.check_legacy_key(&key.1, &key.0) {
                    Some(Expr::Legacy(key.1.clone()))
                } else {
                    None
                }
            }
            ast::AExpr::Count { span, scope, terms } => {
                let scope = self.lower_scope(scope)?;
                let terms = self.lower_query(terms, env);
                let _ = span;
                Some(Expr::Count {
                    scope,
                    query: terms,
                })
            }
            ast::AExpr::Total { scope, key, .. } => {
                let scope = self.lower_scope(scope)?;
                if !self.check_aspect_key(&key.1, &key.0) {
                    return None;
                }
                Some(Expr::Aspect {
                    scope,
                    query: Vec::new(),
                    key: key.1.clone(),
                })
            }
            ast::AExpr::Alive { card, .. } => {
                if self.check_card_unique(&card.1, &card.0) {
                    Some(Expr::Alive(CardId::from(card.1.clone())))
                } else {
                    None
                }
            }
            ast::AExpr::Neg(_, inner) => {
                let inner = self.lower_arith(inner, env)?;
                Some(Expr::Arith {
                    op: ArithOp::Sub,
                    left: Box::new(Expr::Lit(0)),
                    right: Box::new(inner),
                })
            }
            ast::AExpr::Bin { span, op, l, r } => {
                let l = self.lower_arith(l, env)?;
                let r = self.lower_arith(r, env)?;
                // [E242]: a constant zero divisor is a compile error, while
                // a dynamic `x / 0` evaluates to 0 (totality).
                if matches!(op, ast::ABinOp::Div) && ir::const_eval(&r) == Some(0) {
                    self.err(span.error(code::E242, "expression divides by a constant zero"));
                    return None;
                }
                Some(Expr::Arith {
                    op: arith_op(*op),
                    left: Box::new(l),
                    right: Box::new(r),
                })
            }
        }
    }

    fn lower_name_path(
        &mut self,
        card: &(Span, String),
        aspect: &(Span, String),
        env: &mut Env,
    ) -> Option<Expr> {
        if let Some((_, val)) = env.params.iter().rev().find(|(p, _)| p == &card.1) {
            return match val {
                ParamVal::Card(id) => {
                    if self.check_aspect_key(&aspect.1, &aspect.0) {
                        Some(Expr::NamePath {
                            card: id.clone(),
                            key: aspect.1.clone(),
                        })
                    } else {
                        None
                    }
                }
                // A query-position predicate parameter reads the candidate.
                ParamVal::Subject => {
                    if self.check_aspect_key(&aspect.1, &aspect.0) {
                        Some(Expr::Subject(aspect.1.clone()))
                    } else {
                        None
                    }
                }
                ParamVal::Int(_) => {
                    self.err(card.0.error(
                        code::E245,
                        format!(
                            "parameter `{}` is not bound to a card reference here",
                            card.1
                        ),
                    ));
                    None
                }
            };
        }
        if env.subject && card.1 == "subject" {
            if self.check_aspect_key(&aspect.1, &aspect.0) {
                return Some(Expr::Subject(aspect.1.clone()));
            }
            return None;
        }
        if self.check_card_unique(&card.1, &card.0) && self.check_aspect_key(&aspect.1, &aspect.0) {
            Some(Expr::NamePath {
                card: CardId::from(card.1.clone()),
                key: aspect.1.clone(),
            })
        } else {
            None
        }
    }

    pub(crate) fn lower_scope(&mut self, s: &ast::AScope) -> Option<Scope> {
        match s {
            ast::AScope::Interior(_) => Some(Scope::Interior),
            ast::AScope::World(_) => Some(Scope::World),
            ast::AScope::Bound(_) => Some(Scope::Bindings),
            ast::AScope::Anywhere(_) => Some(Scope::Anywhere),
            ast::AScope::Zone(_, (_, z)) => {
                if self.check_zone(z, s.span()) {
                    Some(Scope::Zone(z.clone()))
                } else {
                    None
                }
            }
            // Slot scopes read empty wherever the slot is not bound; no
            // compile-time registry check applies (totality, spec.md §4.7).
            ast::AScope::Slot(_, (_, n)) => Some(Scope::Slot(n.clone().into())),
        }
    }

    pub(crate) fn lower_query(
        &mut self,
        terms: &[ast::AQueryTerm],
        env: &mut Env,
    ) -> Vec<QueryTerm> {
        terms
            .iter()
            .filter_map(|t| self.lower_query_term(t, env))
            .collect()
    }

    fn lower_query_term(&mut self, t: &ast::AQueryTerm, env: &mut Env) -> Option<QueryTerm> {
        match t {
            ast::AQueryTerm::Aspect { key, op, value, .. } => {
                if !self.check_aspect_key(&key.1, &key.0) {
                    return None;
                }
                Some(QueryTerm::Aspect(ir::AspectTerm {
                    key: key.1.clone(),
                    op: op.map(|o| cmp_op(o)),
                    value: *value,
                }))
            }
            ast::AQueryTerm::Name(_, (sp, n)) => {
                if self.check_card_exists(n, sp) {
                    Some(QueryTerm::Name(CardId::from(n.clone())))
                } else {
                    None
                }
            }
            ast::AQueryTerm::Kind(_, (sp, k)) => {
                if self.check_kind(k, sp) {
                    Some(QueryTerm::Kind(k.clone()))
                } else {
                    None
                }
            }
            ast::AQueryTerm::Not(inner) => self
                .lower_query_term(inner, env)
                .map(|x| QueryTerm::Not(Box::new(x))),
            ast::AQueryTerm::Call { span, name, args } => self.query_pred(span, name, args, env),
        }
    }

    /// A predicate in query position is unary over the candidate card: its
    /// single parameter binds to the candidate (language.md §2.5).
    fn query_pred(
        &mut self,
        span: &Span,
        name: &(Span, String),
        args: &[ast::AExpr],
        env: &mut Env,
    ) -> Option<QueryTerm> {
        let n = &name.1;
        let reg = match self.preds.get(n) {
            Some(r) => r.clone(),
            None => {
                self.err(name.0.error(code::E244, format!("unknown predicate `{n}`")));
                return None;
            }
        };
        if self.pred_stack.contains(n) {
            self.err(span.error(
                code::E230,
                format!("predicate `{n}` expansion reaches itself"),
            ));
            return None;
        }
        if env.pred_depth >= INLINE_DEPTH_CAP {
            self.err(span.error(
                code::E235,
                format!("predicate inlining exceeds the depth cap of {INLINE_DEPTH_CAP}"),
            ));
            return None;
        }
        if !args.is_empty() {
            self.err(span.error(
                code::E245,
                format!("a query-position predicate call is unary; `{n}` takes no arguments here"),
            ));
            return None;
        }
        if reg.params.len() != 1 {
            self.err(span.error(
                code::E245,
                format!(
                    "a query-position predicate takes exactly one parameter; `{n}` has {}",
                    reg.params.len()
                ),
            ));
            return None;
        }
        let (card_use, int_use) = reg
            .usage
            .get(&reg.params[0].1)
            .copied()
            .unwrap_or((false, false));
        if !card_use || int_use {
            self.err(span.error(
                code::E245,
                format!(
                    "the body of `{n}` must read its parameter as a card reference (`p.<aspect>`)"
                ),
            ));
            return None;
        }
        let body = reg.body.clone()?;
        let pname = reg.params[0].1.clone();
        let saved = std::mem::take(&mut env.params);
        env.params = vec![(pname, ParamVal::Subject)];
        // Query-position predicates lower roll-free: a roll binding is
        // visible to `if` conditions and prose tokens only (language.md
        // §4.3), so a roll name inside a query term is the E244 unknown-name
        // refusal, exactly as in slot filters and recipe `when`s.
        let saved_rolls = std::mem::take(&mut env.rolls);
        env.pred_depth += 1;
        self.pred_stack.push(n.clone());
        let out = self.lower_cond(&body, env).map(Box::new);
        self.pred_stack.pop();
        env.pred_depth -= 1;
        env.rolls = saved_rolls;
        env.params = saved;
        out.map(QueryTerm::Pred)
    }

    /// Inline a predicate at its call site (language.md §2.5).
    fn pred_call(
        &mut self,
        span: &Span,
        name: &(Span, String),
        args: &[ast::AExpr],
        env: &mut Env,
    ) -> Option<Expr> {
        let n = &name.1;
        let reg = match self.preds.get(n) {
            Some(r) => r.clone(),
            None => {
                self.err(name.0.error(code::E244, format!("unknown predicate `{n}`")));
                return None;
            }
        };
        if self.pred_stack.contains(n) {
            self.err(span.error(
                code::E230,
                format!("predicate `{n}` expansion reaches itself"),
            ));
            return None;
        }
        if env.pred_depth >= INLINE_DEPTH_CAP {
            self.err(span.error(
                code::E235,
                format!("predicate inlining exceeds the depth cap of {INLINE_DEPTH_CAP}"),
            ));
            return None;
        }
        let body = reg.body.clone()?;
        if args.len() != reg.params.len() {
            self.err(span.error(
                code::E245,
                format!(
                    "predicate `{n}` expects {} argument(s), got {}",
                    reg.params.len(),
                    args.len()
                ),
            ));
            return None;
        }
        let mut bound = Vec::new();
        for ((psp, pname), arg) in reg.params.iter().zip(args) {
            let (card_use, int_use) = reg.usage.get(pname).copied().unwrap_or((false, false));
            if card_use && int_use {
                self.err(
                    psp.error(
                        code::E245,
                        format!(
                            "parameter `{pname}` of `{n}` is used both as a card reference and as an integer expression"
                        ),
                    ),
                );
                return None;
            }
            if card_use {
                match arg {
                    ast::AExpr::Ident(_, card) => {
                        // A visible parameter bound to a card forwards its
                        // binding: pass-through (`outer(q) = inner(q)`).
                        if let Some((_, val)) = env.params.iter().rev().find(|(p, _)| p == card) {
                            match val.clone() {
                                ParamVal::Card(id) => {
                                    bound.push((pname.clone(), ParamVal::Card(id)));
                                }
                                ParamVal::Subject => {
                                    bound.push((pname.clone(), ParamVal::Subject));
                                }
                                ParamVal::Int(_) => {
                                    self.err(
                                        arg.span().clone().error(
                                            code::E245,
                                            format!(
                                                "parameter `{card}` is not bound to a card reference here"
                                            ),
                                        ),
                                    );
                                    return None;
                                }
                            }
                            continue;
                        }
                        if self.check_card_unique(card, arg.span()) {
                            bound.push((pname.clone(), ParamVal::Card(CardId::from(card.clone()))));
                        } else {
                            return None;
                        }
                    }
                    _ => {
                        self.err(arg.span().clone().error(
                            code::E245,
                            format!("argument for `{pname}` of `{n}` must be a card name"),
                        ));
                        return None;
                    }
                }
            } else {
                match self.lower_arith(arg, env) {
                    Some(x) => bound.push((pname.clone(), ParamVal::Int(x))),
                    None => return None,
                }
            }
        }
        let saved = std::mem::take(&mut env.params);
        env.params = bound;
        env.pred_depth += 1;
        self.pred_stack.push(n.clone());
        let out = self.lower_cond(&body, env);
        self.pred_stack.pop();
        env.pred_depth -= 1;
        env.params = saved;
        out
    }
}

fn cmp_op(o: ast::ACmpOp) -> CmpOp {
    match o {
        ast::ACmpOp::Ge => CmpOp::Ge,
        ast::ACmpOp::Le => CmpOp::Le,
        ast::ACmpOp::Gt => CmpOp::Gt,
        ast::ACmpOp::Lt => CmpOp::Lt,
        ast::ACmpOp::Eq => CmpOp::Eq,
        ast::ACmpOp::Ne => CmpOp::Ne,
    }
}

fn arith_op(o: ast::ABinOp) -> ArithOp {
    match o {
        ast::ABinOp::Add => ArithOp::Add,
        ast::ABinOp::Sub => ArithOp::Sub,
        ast::ABinOp::Mul => ArithOp::Mul,
        ast::ABinOp::Div => ArithOp::Div,
    }
}
