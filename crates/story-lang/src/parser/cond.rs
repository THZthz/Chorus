//! Condition parsing: or/and/not chains, comparisons, and predicate calls.

use super::arith::parse_arith;
use super::TC;
use crate::ast::{ACond, ACmpOp, AExpr};
use crate::lexer::{P, Tok};
use story_content::{code, Diagnostic};

pub(super) fn is_cmp_p(p: &P) -> bool {
    matches!(p, P::Ge | P::Le | P::Gt | P::Lt | P::EqEq | P::Ne)
}

pub(super) fn cmp_op(p: &P) -> ACmpOp {
    match p {
        P::Ge => ACmpOp::Ge,
        P::Le => ACmpOp::Le,
        P::Gt => ACmpOp::Gt,
        P::Lt => ACmpOp::Lt,
        P::EqEq => ACmpOp::Eq,
        P::Ne => ACmpOp::Ne,
        _ => unreachable!(),
    }
}

pub fn parse_cond(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<ACond> {
    parse_or(tc, diags)
}

fn parse_or(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<ACond> {
    let mut l = parse_and(tc, diags)?;
    while tc.at_ident("or") {
        tc.next();
        let r = parse_and(tc, diags)?;
        l = ACond::Or(Box::new(l), Box::new(r));
    }
    Some(l)
}

fn parse_and(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<ACond> {
    let mut l = parse_not(tc, diags)?;
    while tc.at_ident("and") {
        tc.next();
        let r = parse_not(tc, diags)?;
        l = ACond::And(Box::new(l), Box::new(r));
    }
    Some(l)
}

fn parse_not(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<ACond> {
    if tc.at_ident("not") {
        tc.next();
        let inner = parse_not(tc, diags)?;
        Some(ACond::Not(Box::new(inner)))
    } else {
        parse_unit(tc, diags)
    }
}

fn parse_unit(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<ACond> {
    // Predicate call with explicit arguments?
    if let Some((sp, Tok::Ident(w))) = tc.peek() {
        let sp = sp.clone();
        let w = w.clone();
        if !matches!(
            w.as_str(),
            "count" | "total" | "alive" | "clock" | "legacy" | "rel"
        ) && tc
            .peek_at(1)
            .is_some_and(|(_, t)| matches!(t, Tok::P(P::LParen)))
        {
            tc.next();
            tc.next();
            let mut args = Vec::new();
            loop {
                if tc.done() || tc.eat_p(&P::RParen).is_some() {
                    break;
                }
                match parse_arith(tc, diags, false) {
                    Some(a) => args.push(a),
                    None => break,
                }
                if tc.eat_p(&P::Comma).is_none() {
                    tc.eat_p(&P::RParen);
                    break;
                }
            }
            let nsp = sp.clone();
            return Some(ACond::Call {
                span: sp,
                name: (nsp, w),
                args,
            });
        }
    }
    // Parenthesized condition vs parenthesized arithmetic: try the condition
    // reading first, fall back (dropping its diagnostics) on a failed close.
    if tc.at_p(&P::LParen) {
        let save = tc.i;
        let ndiags = diags.len();
        tc.next();
        if let Some(c) = parse_cond(tc, diags) {
            if tc.eat_p(&P::RParen).is_some() {
                return Some(c);
            }
        }
        tc.i = save;
        diags.truncate(ndiags);
    }
    let first = parse_arith(tc, diags, false)?;
    let cmp = match tc.peek() {
        Some((sp, Tok::P(p))) if is_cmp_p(p) => {
            let sp = sp.clone();
            let op = cmp_op(p);
            tc.next();
            Some((sp, op))
        }
        _ => None,
    };
    match cmp {
        Some((sp, op)) => {
            let r = parse_arith(tc, diags, false)?;
            Some(ACond::Cmp {
                span: sp,
                op,
                l: first,
                r,
            })
        }
        None => match first {
            AExpr::Ident(sp, name) => Some(ACond::Name(sp.clone(), (sp, name))),
            AExpr::Rel {
                span,
                kind,
                from,
                to,
            } => Some(ACond::Rel {
                span,
                kind,
                from,
                to,
            }),
            _ => {
                diags.push(first.span().clone().error(
                    code::E201,
                    "expected a comparison or a predicate call in condition position",
                ));
                None
            }
        },
    }
}
