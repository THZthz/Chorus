//! Arithmetic expression parsing: precedence, unary minus, and operand forms.

use super::query::{parse_card_ref, parse_query_terms, parse_scope};
use super::TC;
use crate::ast::{ABinOp, AExpr, is_dice_ident};
use crate::lexer::{P, Tok};
use story_content::{code, Diagnostic};

fn bin_op(p: &P) -> Option<ABinOp> {
    match p {
        P::Plus => Some(ABinOp::Add),
        P::Minus => Some(ABinOp::Sub),
        P::Star => Some(ABinOp::Mul),
        P::Slash => Some(ABinOp::Div),
        _ => None,
    }
}

pub fn parse_arith(tc: &mut TC, diags: &mut Vec<Diagnostic>, stop_dice: bool) -> Option<AExpr> {
    let mut l = parse_arith_unary(tc, diags, stop_dice)?;
    loop {
        let op = match tc.peek() {
            Some((_, Tok::P(p))) => bin_op(p),
            _ => None,
        };
        let Some(op) = op else { break };
        if dice_ahead(tc, stop_dice) {
            break;
        }
        tc.next();
        let mut r = parse_arith_unary(tc, diags, stop_dice)?;
        loop {
            let next_op = match tc.peek() {
                Some((_, Tok::P(p))) => bin_op(p),
                _ => None,
            };
            let Some(nop) = next_op else { break };
            let binds_tighter = matches!(nop, ABinOp::Mul | ABinOp::Div);
            if !binds_tighter {
                break;
            }
            if dice_ahead(tc, stop_dice) {
                break;
            }
            tc.next();
            let rr = parse_arith_unary(tc, diags, stop_dice)?;
            let sp = r.span().clone();
            r = AExpr::Bin {
                span: sp,
                op: nop,
                l: Box::new(r),
                r: Box::new(rr),
            };
        }
        let sp = l.span().clone();
        l = AExpr::Bin {
            span: sp,
            op,
            l: Box::new(l),
            r: Box::new(r),
        };
    }
    Some(l)
}

fn dice_ahead(tc: &TC, stop_dice: bool) -> bool {
    stop_dice
        && tc
            .peek_at(1)
            .is_some_and(|(_, t)| matches!(t, Tok::Ident(w) if is_dice_ident(w)))
}

/// A term-level arithmetic expression: `*` and `/` chains, used for roll
/// sides so the bonus's `+` stays in the caller's hands.
pub(super) fn parse_arith_term(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<AExpr> {
    let mut l = parse_arith_unary(tc, diags, false)?;
    loop {
        let op = match tc.peek() {
            Some((_, Tok::P(p))) => bin_op(p),
            _ => None,
        };
        let Some(op @ (ABinOp::Mul | ABinOp::Div)) = op else {
            break;
        };
        tc.next();
        let r = parse_arith_unary(tc, diags, false)?;
        let sp = l.span().clone();
        l = AExpr::Bin {
            span: sp,
            op,
            l: Box::new(l),
            r: Box::new(r),
        };
    }
    Some(l)
}

fn parse_arith_unary(tc: &mut TC, diags: &mut Vec<Diagnostic>, stop_dice: bool) -> Option<AExpr> {
    if tc.at_p(&P::Minus) {
        let (sp, _) = tc.next()?;
        let inner = parse_arith_unary(tc, diags, stop_dice)?;
        let nsp = sp.clone();
        return Some(AExpr::Neg(nsp, Box::new(inner)));
    }
    parse_arith_atom(tc, diags)
}

fn parse_arith_atom(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<AExpr> {
    let (sp, t) = tc.peek().cloned()?;
    match t {
        Tok::Int(v) => {
            tc.next();
            Some(AExpr::Int(sp, v))
        }
        Tok::Dur(v) => {
            tc.next();
            Some(AExpr::Dur(sp, v))
        }
        Tok::P(P::LParen) => {
            tc.next();
            let e = parse_arith(tc, diags, false)?;
            if tc.eat_p(&P::RParen).is_none() {
                diags.push(sp.error(code::E201, "expected `)`"));
            }
            Some(e)
        }
        Tok::Ident(w) => {
            let w = w.clone();
            tc.next();
            match w.as_str() {
                "clock" => Some(AExpr::Clock(sp)),
                "legacy" => {
                    if tc.eat_p(&P::Dot).is_none() {
                        diags.push(sp.error(code::E201, "legacy reads as `legacy.<key>`"));
                        return None;
                    }
                    let key = tc.expect_ident("a legacy key", diags)?;
                    Some(AExpr::Legacy { span: sp, key })
                }
                "count" => {
                    if tc.eat_p(&P::LParen).is_none() {
                        diags.push(sp.error(code::E201, "expected `count(<scope>, <query>)`"));
                        return None;
                    }
                    let Some(scope) = parse_scope(tc, diags) else {
                        return None;
                    };
                    tc.eat_p(&P::Comma);
                    let terms = parse_query_terms(tc, diags);
                    if tc.eat_p(&P::RParen).is_none() {
                        diags.push(sp.error(code::E201, "expected `)` after the query"));
                    }
                    Some(AExpr::Count {
                        span: sp,
                        scope,
                        terms,
                    })
                }
                "total" => {
                    if tc.eat_p(&P::LParen).is_none() {
                        diags.push(sp.error(code::E201, "expected `total(<scope>, aspect:<key>)`"));
                        return None;
                    }
                    let scope = parse_scope(tc, diags)?;
                    tc.eat_p(&P::Comma);
                    let key = if tc.eat_ident("aspect").is_some() {
                        if tc.eat_p(&P::Colon).is_none() {
                            diags.push(sp.error(code::E201, "expected `aspect:<key>`"));
                            return None;
                        }
                        tc.expect_ident("an aspect key", diags)?
                    } else {
                        diags.push(sp.error(code::E201, "expected `aspect:<key>`"));
                        return None;
                    };
                    if tc.eat_p(&P::RParen).is_none() {
                        diags.push(sp.error(code::E201, "expected `)`"));
                    }
                    Some(AExpr::Total {
                        span: sp,
                        scope,
                        key,
                    })
                }
                "alive" => {
                    if tc.eat_p(&P::LParen).is_none() {
                        diags.push(sp.error(code::E201, "expected `alive(<card>)`"));
                        return None;
                    }
                    let card = tc.expect_ident("a card name", diags)?;
                    if tc.eat_p(&P::RParen).is_none() {
                        diags.push(sp.error(code::E201, "expected `)`"));
                    }
                    Some(AExpr::Alive { span: sp, card })
                }
                "rel" => {
                    if tc.eat_p(&P::LParen).is_none() {
                        diags.push(
                            sp.error(code::E201, "expected `rel(<kind>, <card-ref>, <card-ref>)`"),
                        );
                        return None;
                    }
                    let kind = tc.expect_ident("a relation kind", diags)?;
                    if tc.eat_p(&P::Comma).is_none() {
                        diags.push(sp.error(code::E201, "expected `,` after the relation kind"));
                        return None;
                    }
                    let Some(from) = parse_card_ref(tc, diags) else {
                        return None;
                    };
                    if tc.eat_p(&P::Comma).is_none() {
                        diags.push(sp.error(code::E201, "expected `,` after the first endpoint"));
                        return None;
                    }
                    let Some(to) = parse_card_ref(tc, diags) else {
                        return None;
                    };
                    if tc.eat_p(&P::RParen).is_none() {
                        diags.push(sp.error(code::E201, "expected `)`"));
                        return None;
                    }
                    Some(AExpr::Rel {
                        span: sp,
                        kind,
                        from: Box::new(from),
                        to: Box::new(to),
                    })
                }
                _ => {
                    if tc.at_p(&P::Dot) {
                        tc.next();
                        let aspect = tc.expect_ident("an aspect key", diags)?;
                        Some(AExpr::NamePath {
                            span: sp.clone(),
                            card: (sp, w),
                            aspect,
                        })
                    } else {
                        Some(AExpr::Ident(sp, w))
                    }
                }
            }
        }
        other => {
            diags.push(sp.error(
                code::E201,
                format!("expected an operand; found `{other:?}`"),
            ));
            None
        }
    }
}
