//! Effect-line parsing: the effect vocabulary, targets, and `set` spellings.

use super::arith::{parse_arith, parse_arith_term};
use super::cond::parse_cond;
use super::query::{
    is_name_path, parse_query_terms, parse_scope, parse_set_query_terms, starts_query_term,
    starts_scope,
};
use super::scalar::{count_expr, parse_set_op};
use super::{Parser, TC};
use crate::ast::*;
use crate::lexer::{Line, P, Tok};
use crate::span::Span;
use story_content::code;

impl<'d> Parser<'d> {
    pub(crate) fn parse_effect_lines(&mut self, base_indent: u32) -> Vec<AEffect> {
        let mut out = Vec::new();
        loop {
            let deeper = match self.peek_line() {
                Some(l) => l.indent > base_indent,
                None => false,
            };
            if !deeper {
                return out;
            }
            let line = self.next_line().unwrap();
            if let Some(e) = self.parse_effect_line(line) {
                out.push(e);
            }
        }
    }

    fn parse_effect_line(&mut self, line: Line) -> Option<AEffect> {
        let span = line.first().map(|(s, _)| s.clone()).unwrap();
        let kw = line.first_ident().unwrap_or("").to_string();
        let mut tc = TC::new(&line.toks);
        tc.next();
        match kw.as_str() {
            "spawn" => {
                let card = tc.expect_ident("a card name", self.diags)?;
                let mut count = None;
                let mut into = None;
                let mut handle = None;
                while !tc.done() {
                    if tc.at_ident("count") {
                        tc.next();
                        match count_expr(&mut tc, &span, "`count`", self.diags) {
                            Some(c) => count = Some(c),
                            None => break,
                        }
                    } else if tc.at_ident("into") {
                        tc.next();
                        into = Some(self.parse_into(&mut tc)?);
                    } else if tc.at_ident("as") {
                        tc.next();
                        match tc.expect_ident("a handle name", self.diags) {
                            Some(h) => handle = Some(h),
                            None => break,
                        }
                    } else {
                        let (sp, t) = tc.peek().cloned().unwrap();
                        self.diags
                            .push(sp.error(code::E201, format!("unexpected `{t:?}` after spawn")));
                        tc.next();
                    }
                }
                Some(AEffect::Spawn {
                    span,
                    card,
                    count,
                    into,
                    handle,
                })
            }
            "destroy" => {
                let target = self.parse_target(&mut tc)?;
                let mut count = None;
                while !tc.done() {
                    if tc.at_ident("all") {
                        tc.next();
                        count = Some((span.clone(), DestroyCountAst::All));
                    } else if tc.at_ident("count") {
                        tc.next();
                        match count_expr(&mut tc, &span, "`count`", self.diags) {
                            Some(c) => count = Some((span.clone(), DestroyCountAst::N(c))),
                            None => break,
                        }
                    } else {
                        let (sp, t) = tc.peek().cloned().unwrap();
                        self.diags.push(
                            sp.error(code::E201, format!("unexpected `{t:?}` after destroy")),
                        );
                        tc.next();
                    }
                }
                Some(AEffect::Destroy {
                    span,
                    target,
                    count,
                })
            }
            "consume" => {
                let target = self.parse_target(&mut tc)?;
                // The count is whatever follows the target on the line: a
                // full integer expression (language.md §4.3).
                let mut count = None;
                if !tc.done() {
                    {
                        let c = count_expr(&mut tc, &span, "`count`", self.diags)?;
                        count = Some(c)
                    }
                }
                Some(AEffect::Consume {
                    span,
                    target,
                    count,
                })
            }
            "swap" => {
                let target = self.parse_target(&mut tc)?;
                let mut count = None;
                if tc.at_ident("count") {
                    tc.next();
                    {
                        let c = count_expr(&mut tc, &span, "`count`", self.diags)?;
                        count = Some(c)
                    }
                }
                if tc.eat_ident("to").is_none() {
                    self.diags.push(span.error(
                        code::E201,
                        "swap is `swap <target> [count <n>] to <card> [into <id>]`",
                    ));
                    return None;
                }
                let to = tc.expect_ident("a replacement card name", self.diags)?;
                let mut into = None;
                if tc.at_ident("into") {
                    tc.next();
                    into = Some(self.parse_into(&mut tc)?);
                }
                Some(AEffect::Swap {
                    span,
                    target,
                    count,
                    to,
                    into,
                })
            }
            "move" => {
                let target = self.parse_target(&mut tc)?;
                if tc.eat_ident("into").is_none() {
                    self.diags.push(
                        span.error(code::E201, "move is `move <target> into (interior | <id>)`"),
                    );
                    return None;
                }
                let into = self.parse_into(&mut tc)?;
                Some(AEffect::Move { span, target, into })
            }
            "set" => self.parse_set(span, &mut tc),
            "legacy" => {
                let key = tc.expect_ident("a legacy key", self.diags)?;
                let op = parse_set_op(&mut tc, self.diags)?;
                let expr = parse_arith(&mut tc, self.diags, false)?;
                Some(AEffect::Legacy {
                    span,
                    key,
                    op,
                    expr,
                })
            }
            "narrate" => {
                let text = tc.expect_str("narration prose", self.diags)?;
                Some(AEffect::Narrate { span, text })
            }
            "roll" => {
                let count = parse_arith(&mut tc, self.diags, true)?;
                let sides = match tc.peek() {
                    Some((_, Tok::Ident(d))) if is_dice_ident(d) => {
                        let d = d.clone();
                        tc.next();
                        if d == "d" {
                            parse_arith_term(&mut tc, self.diags)?
                        } else {
                            let n = d[1..].parse::<i64>().unwrap_or(0);
                            AExpr::Int(span.clone(), n)
                        }
                    }
                    _ => {
                        self.diags.push(span.error(
                            code::E201,
                            "roll is `roll <count>d<sides> [+ <bonus>] as <name>`",
                        ));
                        return None;
                    }
                };
                let mut bonus = None;
                if tc.at_p(&P::Plus) {
                    tc.next();
                    bonus = Some(parse_arith(&mut tc, self.diags, false)?);
                }
                if tc.eat_ident("as").is_none() {
                    self.diags
                        .push(span.error(code::E201, "roll binds its result with `as <name>`"));
                    return None;
                }
                let bind = tc.expect_ident("a roll binding name", self.diags)?;
                Some(AEffect::Roll {
                    span,
                    count,
                    sides,
                    bonus,
                    bind,
                })
            }
            "draw" => {
                if tc.eat_ident("from").is_none() {
                    self.diags.push(span.error(
                        code::E201,
                        "draw is `draw from <deck> [count <n>] [into (interior | <id>)]`",
                    ));
                    return None;
                }
                let deck = tc.expect_ident("a deck name", self.diags)?;
                let mut count = None;
                let mut into = None;
                while !tc.done() {
                    if tc.at_ident("count") {
                        tc.next();
                        match count_expr(&mut tc, &span, "`count`", self.diags) {
                            Some(c) => count = Some(c),
                            None => break,
                        }
                    } else if tc.at_ident("into") {
                        tc.next();
                        into = Some(self.parse_into(&mut tc)?);
                    } else {
                        let (sp, t) = tc.peek().cloned().unwrap();
                        self.diags
                            .push(sp.error(code::E201, format!("unexpected `{t:?}` after draw")));
                        tc.next();
                    }
                }
                Some(AEffect::Draw {
                    span,
                    deck,
                    count,
                    into,
                })
            }
            "start" => {
                let verb = tc.expect_ident("a verb name", self.diags)?;
                let mut via = None;
                let mut count = None;
                while !tc.done() {
                    if tc.at_ident("via") {
                        tc.next();
                        match tc.expect_ident("a recipe name", self.diags) {
                            Some(v) => via = Some(v),
                            None => break,
                        }
                    } else if tc.at_ident("count") {
                        tc.next();
                        match count_expr(&mut tc, &span, "`count`", self.diags) {
                            Some(c) => count = Some(c),
                            None => break,
                        }
                    } else {
                        let (sp, t) = tc.peek().cloned().unwrap();
                        self.diags
                            .push(sp.error(code::E201, format!("unexpected `{t:?}` after start")));
                        tc.next();
                    }
                }
                Some(AEffect::Start {
                    span,
                    verb,
                    via,
                    count,
                })
            }
            "signal" => {
                let event = tc.expect_ident("an event name", self.diags)?;
                Some(AEffect::Signal { span, event })
            }
            "continue" => {
                if tc.eat_ident("via").is_none() {
                    self.diags
                        .push(span.error(code::E201, "continue is `continue via <recipe>`"));
                    return None;
                }
                let recipe = self.parse_recipe_spelling(&mut tc)?;
                Some(AEffect::Continue { span, recipe })
            }
            "relate" => {
                let from = self.parse_target(&mut tc)?;
                if tc.eat_ident("to").is_none() {
                    self.diags.push(span.error(
                        code::E201,
                        "relate is `relate <target> to <target> as <kind>`",
                    ));
                    return None;
                }
                let to = self.parse_target(&mut tc)?;
                if tc.eat_ident("as").is_none() {
                    self.diags.push(span.error(
                        code::E201,
                        "relate is `relate <target> to <target> as <kind>`",
                    ));
                    return None;
                }
                let kind = tc.expect_ident("a relation kind", self.diags)?;
                Some(AEffect::Relate {
                    span,
                    from,
                    to,
                    kind,
                })
            }
            "unrelate" => {
                let from = self.parse_target(&mut tc)?;
                if tc.eat_ident("from").is_none() {
                    self.diags.push(span.error(
                        code::E201,
                        "unrelate is `unrelate <target> from <target> [as <kind>]`",
                    ));
                    return None;
                }
                let to = self.parse_target(&mut tc)?;
                let mut kind = None;
                if tc.at_ident("as") {
                    tc.next();
                    kind = tc.expect_ident("a relation kind", self.diags);
                    kind.as_ref()?;
                }
                Some(AEffect::Unrelate {
                    span,
                    from,
                    to,
                    kind,
                })
            }
            "ending" => {
                let id = tc.expect_ident("an ending id", self.diags)?;
                Some(AEffect::Ending { span, id })
            }
            "if" => {
                let cond = parse_cond(&mut tc, self.diags)?;
                let then = self.parse_effect_lines(line.indent);
                let mut els = Vec::new();
                let is_else = match self.peek_line() {
                    Some(l) => l.indent == line.indent && l.first_ident() == Some("else"),
                    None => false,
                };
                if is_else {
                    let else_line = self.next_line().unwrap();
                    els = self.parse_effect_lines(else_line.indent);
                }
                Some(AEffect::If {
                    span,
                    cond,
                    then,
                    els,
                })
            }
            other => {
                // effectset call: `<name>(<args>)`
                if tc.at_p(&P::LParen) {
                    let lp = tc.eat_p(&P::LParen).unwrap();
                    let mut args: Vec<ArgAst> = Vec::new();
                    let mut cur: Toks = Vec::new();
                    let mut saw_comma = false;
                    let mut depth = 1usize;
                    while let Some((sp, t)) = tc.next() {
                        match &t {
                            Tok::P(P::LParen) => {
                                depth += 1;
                                cur.push((sp, t));
                            }
                            Tok::P(P::RParen) => {
                                depth -= 1;
                                if depth == 0 {
                                    break;
                                }
                                cur.push((sp, t));
                            }
                            Tok::P(P::Comma) if depth == 1 => {
                                saw_comma = true;
                                args.push(ArgAst {
                                    span: sp,
                                    toks: std::mem::take(&mut cur),
                                });
                            }
                            _ => cur.push((sp, t)),
                        }
                    }
                    if depth != 0 {
                        self.diags
                            .push(lp.error(code::E201, "unclosed `(` in effectset call"));
                        return None;
                    }
                    // `f()` passes zero arguments; only a non-empty group or a
                    // trailing comma contributes a final argument.
                    if !cur.is_empty() || saw_comma {
                        args.push(ArgAst {
                            span: span.clone(),
                            toks: cur,
                        });
                    }
                    return Some(AEffect::Call {
                        span,
                        name: (lp, other.to_string()),
                        args,
                    });
                }
                self.diags
                    .push(span.error(code::E201, format!("unknown effect `{other}`")));
                None
            }
        }
    }

    /// A recipe reference: a local name, or the global `<verb>.<recipe>`
    /// spelling (language.md §1). Returns the joined spelling.
    fn parse_recipe_spelling(&mut self, tc: &mut TC) -> Option<(Span, String)> {
        let (sp, name) = tc.expect_ident("a recipe name", self.diags)?;
        if tc.at_p(&P::Dot) {
            tc.next();
            let (_, rest) = tc.expect_ident("a recipe name after the dot", self.diags)?;
            return Some((sp, format!("{}.{}", name, rest)));
        }
        Some((sp, name))
    }

    fn parse_into(&mut self, tc: &mut TC) -> Option<AInto> {
        let (s, t) = tc.peek().cloned()?;
        match t {
            Tok::Ident(ref w) if w == "interior" => {
                tc.next();
                Some(AInto::Interior(s))
            }
            Tok::Ident(id) => {
                tc.next();
                Some(AInto::Id(s, id))
            }
            _ => {
                let sp = tc.peek().map(|(s, _)| s.clone());
                if let Some(sp) = sp {
                    self.diags
                        .push(sp.error(code::E201, "expected `interior` or an id after `into`"));
                }
                None
            }
        }
    }

    /// The shared target grammar (language.md §4.2). A bare identifier is a
    /// handle or unique-card name, resolved at lowering.
    fn parse_target(&mut self, tc: &mut TC) -> Option<ATarget> {
        let (sp, t) = tc.peek().cloned()?;
        let Tok::Ident(w) = &t else {
            self.diags.push(sp.error(code::E201, "expected a target"));
            return None;
        };
        let w = w.clone();
        if w == "slot"
            && tc
                .peek_at(1)
                .is_some_and(|(_, t)| matches!(t, Tok::P(P::Colon)))
        {
            tc.next();
            tc.next();
            let name = tc.expect_ident("a slot name", self.diags)?;
            return Some(ATarget::Slot(sp, name));
        }
        if starts_scope(tc) {
            let scope = parse_scope(tc, self.diags)?;
            let terms = parse_query_terms(tc, self.diags);
            let span = scope.span().clone();
            return Some(ATarget::Query { span, scope, terms });
        }
        if is_name_path(tc) {
            let card = (sp.clone(), w);
            tc.next();
            tc.next();
            let aspect = tc.expect_ident("an aspect key", self.diags)?;
            return Some(ATarget::NamePath {
                span: sp,
                card,
                aspect,
            });
        }
        if starts_query_term(tc) {
            let terms = parse_query_terms(tc, self.diags);
            let span = terms.first().map(|t| t.span().clone()).unwrap_or(sp);
            return Some(ATarget::Bare { span, terms });
        }
        tc.next();
        let nsp = sp.clone();
        Some(ATarget::Name(sp, (nsp, w)))
    }

    /// `set` has two spellings: the name path `set <card>.<aspect> (op) <expr>`
    /// and the multi-target `set <target> aspect:<key> (op) <expr>`.
    fn parse_set(&mut self, span: Span, tc: &mut TC) -> Option<AEffect> {
        let (target, key) = if is_name_path(tc) {
            let (csp, t) = tc.next()?;
            let Tok::Ident(card) = t else { unreachable!() };
            tc.next(); // dot
            let aspect = tc.expect_ident("an aspect key", self.diags)?;
            (
                ATarget::NamePath {
                    span: csp.clone(),
                    card: (csp, card),
                    aspect: aspect.clone(),
                },
                aspect,
            )
        } else {
            let target = self.parse_set_target(tc)?;
            if tc.at_ident("aspect")
                && tc
                    .peek_at(1)
                    .is_some_and(|(_, t)| matches!(t, Tok::P(P::Colon)))
            {
                tc.next();
                tc.next();
                let key = tc.expect_ident("an aspect key", self.diags)?;
                (target, key)
            } else {
                let sp = target.span().clone();
                self.diags.push(sp.error(
                    code::E201,
                    "a multi-target set names its key as `aspect:<key>`",
                ));
                return None;
            }
        };
        let op = parse_set_op(tc, self.diags)?;
        let expr = parse_arith(tc, self.diags, false)?;
        let mut unclamped = None;
        if tc.eat_ident("unclamped").is_some() {
            unclamped = Some(span.clone());
        }
        Some(AEffect::Set {
            span,
            target,
            key,
            op,
            expr,
            unclamped,
        })
    }

    /// Multi-target parse for `set`: slot:, scoped query, bare query, or
    /// handle. Query terms end at the `aspect:<key>` that the op follows.
    fn parse_set_target(&mut self, tc: &mut TC) -> Option<ATarget> {
        let (sp, t) = tc.peek().cloned()?;
        let Tok::Ident(w) = &t else {
            self.diags
                .push(sp.error(code::E201, "expected a set target"));
            return None;
        };
        let w = w.clone();
        if w == "slot"
            && tc
                .peek_at(1)
                .is_some_and(|(_, t)| matches!(t, Tok::P(P::Colon)))
        {
            tc.next();
            tc.next();
            let name = tc.expect_ident("a slot name", self.diags)?;
            return Some(ATarget::Slot(sp, name));
        }
        if starts_scope(tc) {
            let scope = parse_scope(tc, self.diags)?;
            let terms = parse_set_query_terms(tc, self.diags);
            let span = scope.span().clone();
            return Some(ATarget::Query { span, scope, terms });
        }
        if starts_query_term(tc) {
            let terms = parse_set_query_terms(tc, self.diags);
            let span = terms.first().map(|t| t.span().clone()).unwrap_or(sp);
            return Some(ATarget::Bare { span, terms });
        }
        tc.next();
        let nsp = sp.clone();
        Some(ATarget::Name(sp, (nsp, w)))
    }
}
