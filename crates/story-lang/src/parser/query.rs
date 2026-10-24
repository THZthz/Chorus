//! Query scopes and query terms: scope spellings, term lists, and target-shape predicates.

use super::arith::parse_arith;
use super::cond::{cmp_op, is_cmp_p};
use super::TC;
use crate::ast::{ACardRef, AQueryTerm, AScope};
use crate::lexer::{P, Tok};
use story_content::{code, Diagnostic};

pub(super) fn is_name_path(tc: &TC) -> bool {
    matches!(tc.peek(), Some((_, Tok::Ident(_))))
        && tc
            .peek_at(1)
            .is_some_and(|(_, t)| matches!(t, Tok::P(P::Dot)))
        && tc
            .peek_at(2)
            .is_some_and(|(_, t)| matches!(t, Tok::Ident(_)))
}

pub(super) fn starts_scope(tc: &TC) -> bool {
    match tc.peek() {
        Some((_, Tok::Ident(w))) => match w.as_str() {
            "world" | "interior" | "table" | "anywhere" | "bound" | "related" => true,
            "zone" | "slot" => tc
                .peek_at(1)
                .is_some_and(|(_, t)| matches!(t, Tok::P(P::Colon))),
            _ => false,
        },
        _ => false,
    }
}

/// A bare query target starts with a term that cannot be anything else:
/// `aspect:` / `name:` / `kind:`, a negation, or a predicate call with
/// parentheses. A lone identifier is a name/handle target.
pub(super) fn starts_query_term(tc: &TC) -> bool {
    match tc.peek() {
        Some((_, Tok::P(P::Minus))) => true,
        Some((_, Tok::Ident(w))) => match w.as_str() {
            "aspect" | "name" | "kind" => tc
                .peek_at(1)
                .is_some_and(|(_, t)| matches!(t, Tok::P(P::Colon))),
            _ => tc
                .peek_at(1)
                .is_some_and(|(_, t)| matches!(t, Tok::P(P::LParen))),
        },
        _ => false,
    }
}

pub fn parse_scope(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<AScope> {
    let (s, t) = tc.peek().cloned()?;
    let Tok::Ident(w) = t else {
        diags.push(s.error(code::E201, "expected a scope"));
        return None;
    };
    match w.as_str() {
        "interior" => {
            tc.next();
            Some(AScope::Interior(s))
        }
        "table" => {
            tc.next();
            Some(AScope::Table(s))
        }
        "world" => {
            tc.next();
            Some(AScope::World(s))
        }
        "bound" => {
            tc.next();
            Some(AScope::Bound(s))
        }
        "anywhere" => {
            tc.next();
            Some(AScope::Anywhere(s))
        }
        "zone" | "slot" => {
            tc.next();
            if tc.eat_p(&P::Colon).is_none() {
                diags.push(s.error(code::E201, format!("expected `{w}:<name>`")));
                return None;
            }
            let name = tc.expect_ident("a name", diags)?;
            if w == "zone" {
                Some(AScope::Zone(s, name))
            } else {
                Some(AScope::Slot(s, name))
            }
        }
        "related" => {
            tc.next();
            if tc.eat_p(&P::Colon).is_none() {
                diags.push(s.error(code::E201, "expected `related:<kind> (of | to) <card-ref>`"));
                return None;
            }
            let kind = tc.expect_ident("a relation kind", diags)?;
            let incoming = if tc.eat_ident("of").is_some() {
                false
            } else if tc.eat_ident("to").is_some() {
                true
            } else {
                diags.push(s.error(code::E201, "expected `of` or `to` after the relation kind"));
                return None;
            };
            let of = parse_card_ref(tc, diags)?;
            Some(AScope::Related {
                span: s,
                kind,
                of: Box::new(of),
                incoming,
            })
        }
        other => {
            diags.push(s.error(code::E201, format!("`{other}` is not a scope")));
            None
        }
    }
}

/// A relation endpoint (language.md §3): a unique card name, `subject`, or
/// `first(<scope> [<terms>])`.
pub(crate) fn parse_card_ref(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<ACardRef> {
    let (sp, t) = tc.peek().cloned()?;
    let Tok::Ident(w) = &t else {
        diags.push(sp.error(
            code::E201,
            "expected a card reference (a unique card name, `subject`, or `first(...)`)",
        ));
        return None;
    };
    let w = w.clone();
    if w == "subject" {
        tc.next();
        return Some(ACardRef::Subject(sp));
    }
    if w == "first"
        && tc
            .peek_at(1)
            .is_some_and(|(_, t)| matches!(t, Tok::P(P::LParen)))
    {
        tc.next();
        tc.next();
        let Some(scope) = parse_scope(tc, diags) else {
            return None;
        };
        let terms = parse_query_terms(tc, diags);
        if tc.eat_p(&P::RParen).is_none() {
            diags.push(sp.error(
                code::E201,
                "expected `)` after the `first(...)` card reference",
            ));
            return None;
        }
        return Some(ACardRef::First {
            span: sp,
            scope,
            terms,
        });
    }
    tc.next();
    let nsp = sp.clone();
    Some(ACardRef::Name(nsp, (sp, w)))
}

/// Space-conjoined query terms (language.md §3). The list ends at end-of-line,
/// `)`, a stop-clause keyword, or a bare integer (a trailing count).
pub(super) fn parse_query_terms(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Vec<AQueryTerm> {
    let mut out = Vec::new();
    loop {
        match tc.peek() {
            None | Some((_, Tok::Int(_))) | Some((_, Tok::P(P::RParen))) => return out,
            Some((_, Tok::Ident(w))) => {
                if matches!(w.as_str(), "count" | "all" | "to" | "into" | "from" | "as") {
                    return out;
                }
            }
            _ => {}
        }
        match parse_query_term(tc, diags) {
            Some(t) => out.push(t),
            None => return out,
        }
    }
}

/// `set` targets stop additionally at the `aspect:<key>` the write op follows.
pub(super) fn parse_set_query_terms(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Vec<AQueryTerm> {
    let mut out = Vec::new();
    loop {
        match tc.peek() {
            None | Some((_, Tok::Int(_))) | Some((_, Tok::P(P::RParen))) => return out,
            Some((_, Tok::Ident(w))) => {
                if matches!(w.as_str(), "count" | "all" | "to" | "into" | "from" | "as") {
                    return out;
                }
            }
            _ => {}
        }
        // The required trailing `aspect:<key>` immediately before the op is
        // the write key, not a query term.
        if tc.at_ident("aspect")
            && tc
                .peek_at(1)
                .is_some_and(|(_, t)| matches!(t, Tok::P(P::Colon)))
            && tc
                .peek_at(2)
                .is_some_and(|(_, t)| matches!(t, Tok::Ident(_)))
        {
            let after = tc.peek_at(3).map(|(_, t)| t.clone());
            if matches!(
                after,
                Some(Tok::P(P::Eq)) | Some(Tok::P(P::PlusEq)) | Some(Tok::P(P::MinusEq))
            ) {
                return out;
            }
        }
        match parse_query_term(tc, diags) {
            Some(t) => out.push(t),
            None => return out,
        }
    }
}

fn parse_query_term(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<AQueryTerm> {
    if tc.at_p(&P::Minus) {
        tc.next();
        let inner = parse_query_term(tc, diags)?;
        return Some(AQueryTerm::Not(Box::new(inner)));
    }
    let (sp, t) = tc.peek().cloned()?;
    let Tok::Ident(w) = &t else {
        diags.push(sp.error(code::E201, "expected a query term"));
        return None;
    };
    let w = w.clone();
    match w.as_str() {
        "aspect" | "name" | "kind" => {
            tc.next();
            if tc.eat_p(&P::Colon).is_none() {
                diags.push(sp.error(code::E201, format!("expected `{w}:<name>`")));
                return None;
            }
            let name = tc.expect_ident("a name", diags)?;
            match w.as_str() {
                "aspect" => {
                    let mut op = None;
                    let mut value = 1;
                    if let Some((_, Tok::P(p))) = tc.peek() {
                        if is_cmp_p(p) {
                            let Tok::P(p) = tc.next().unwrap().1 else {
                                unreachable!()
                            };
                            op = Some(cmp_op(&p));
                            if let Some((_, n)) = tc.expect_int("an integer", diags) {
                                value = n;
                            }
                        }
                    }
                    Some(AQueryTerm::Aspect {
                        span: sp,
                        key: name,
                        op,
                        value,
                    })
                }
                "name" => Some(AQueryTerm::Name(sp, name)),
                _ => Some(AQueryTerm::Kind(sp, name)),
            }
        }
        _ => {
            tc.next();
            let mut args = Vec::new();
            if tc.at_p(&P::LParen) {
                tc.next();
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
            }
            let nsp = sp.clone();
            Some(AQueryTerm::Call {
                span: sp,
                name: (nsp, w),
                args,
            })
        }
    }
}
