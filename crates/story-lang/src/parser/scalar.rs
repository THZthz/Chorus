//! Scalar literal and write-op helpers: durations, signed integers, counts, and set operators.

use super::arith::parse_arith;
use super::TC;
use crate::ast::{AExpr, ASetOp};
use crate::lexer::{Line, P, Tok};
use crate::span::Span;
use story_content::{code, Diagnostic};

pub(super) fn duration_value(line: &Line) -> Option<(Span, i64)> {
    let mut tc = TC::new(&line.toks);
    tc.next(); // the field keyword
    match tc.peek() {
        Some((s, Tok::Dur(v))) => Some((s.clone(), *v)),
        Some((s, Tok::Int(v))) => Some((s.clone(), *v)),
        _ => None,
    }
}

/// An integer literal with an optional unary minus, for declaration
/// positions that admit negative values (defaults, priorities); the lexer
/// spells `-5` as `Minus` + `Int`.
pub(super) fn signed_int(tc: &mut TC, what: &str, diags: &mut Vec<Diagnostic>) -> Option<(Span, i64)> {
    let minus = tc.eat_p(&P::Minus);
    let (sp, n) = tc.expect_int(what, diags)?;
    let v = if minus.is_some() { -n } else { n };
    Some((minus.unwrap_or(sp), v))
}

/// A non-negative count: a negative value is [E246], never a silent clamp.
fn count_arg(sp: &Span, n: i64, what: &str, diags: &mut Vec<Diagnostic>) -> Option<u32> {
    if n < 0 {
        diags.push(sp.error(code::E246, format!("{what} must be non-negative")));
        None
    } else {
        Some(n as u32)
    }
}

/// A count literal: the optional unary minus is accepted so the negative
/// value gets its [E246] instead of a spurious syntax error.
pub(super) fn count_int(tc: &mut TC, what: &str, diags: &mut Vec<Diagnostic>) -> Option<(Span, u32)> {
    let (sp, n) = signed_int(tc, what, diags)?;
    let c = count_arg(&sp, n, what, diags)?;
    Some((sp, c))
}

/// An effect count: a full integer expression with the roll-operand grammar,
/// evaluated at effect time (language.md §4.3). Effects are line-oriented, so
/// the expression ends at end-of-line; a following clause keyword (`into`,
/// `as`, `to`, `via`) is no operator and stops the arithmetic parse on its
/// own.
pub(super) fn count_expr(tc: &mut TC, at: &Span, what: &str, diags: &mut Vec<Diagnostic>) -> Option<AExpr> {
    let before = diags.len();
    match parse_arith(tc, diags, false) {
        Some(e) => Some(e),
        None => {
            if diags.len() == before {
                diags.push(at.clone().error(code::E201, format!("expected {what}")));
            }
            None
        }
    }
}

pub(super) fn parse_set_op(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<(Span, ASetOp)> {
    match tc.peek() {
        Some((s, Tok::P(P::Eq))) => {
            let r = Some((s.clone(), ASetOp::Assign));
            tc.next();
            r
        }
        Some((s, Tok::P(P::PlusEq))) => {
            let r = Some((s.clone(), ASetOp::AddAssign));
            tc.next();
            r
        }
        Some((s, Tok::P(P::MinusEq))) => {
            let r = Some((s.clone(), ASetOp::SubAssign));
            tc.next();
            r
        }
        Some((s, t)) => {
            diags.push(s.clone().error(
                code::E201,
                format!("expected `=`, `+=`, or `-=`; found `{t:?}`"),
            ));
            None
        }
        None => None,
    }
}
