//! Card declarations: the cards block, card fields, and card completion/expire books.

use super::scalar::{count_int, duration_value, signed_int};
use super::{Parser, TC};
use crate::ast::*;
use crate::lexer::{Line, P, Tok};
use story_content::code;

impl<'d> Parser<'d> {
    pub(super) fn parse_cards(&mut self) -> Vec<CardAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut out: Vec<CardAst> = Vec::new();
        self.children(base, |p, l| {
            let kw = l.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "meta" => {
                    if let Some(c) = out.last_mut() {
                        c.meta.push(p.meta_line(&l));
                    } else {
                        p.err_here("meta line before any card");
                    }
                }
                "card" => out.push(p.parse_card(l)),
                _ => p.err_here(format!("expected `card`, found `{kw}`")),
            }
        });
        out
    }

    fn parse_card(&mut self, line: Line) -> CardAst {
        let span = line.first().map(|(s, _)| s.clone()).unwrap();
        let mut tc = TC::new(&line.toks);
        tc.next();
        let name = tc.expect_ident("a card name", self.diags);
        let mut ast = CardAst {
            span: span.clone(),
            name: name.unwrap_or_else(|| (span.clone(), String::new())),
            kind: None,
            uniq: Vec::new(),
            aspects: Vec::new(),
            boot: None,
            relates: Vec::new(),
            desc: None,
            lifetime: None,
            mint: None,
            completes: Vec::new(),
            expire: None,
            meta: Vec::new(),
        };
        let base = line.indent;
        self.children(base, |p, l| {
            let kw = l.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "meta" => ast.meta.push(p.meta_line(&l)),
                "on" => p.parse_card_book(l, &mut ast),
                _ => {
                    let mut tc = TC::new(&l.toks);
                    while !tc.done() {
                        let Some((sp, tok)) = tc.peek().cloned() else {
                            break;
                        };
                        let Tok::Ident(field) = &tok else {
                            p.diags
                                .push(sp.error(code::E201, "expected a card field name"));
                            break;
                        };
                        tc.next();
                        match field.as_str() {
                            "kind" => match tc.expect_ident("a kind tag", p.diags) {
                                Some(k) => ast.kind = Some(k),
                                None => break,
                            },
                            "stackable" => ast.uniq.push(UniqAst::Stackable(sp)),
                            "unique" => ast.uniq.push(UniqAst::Unique(sp)),
                            "group" => match tc.expect_ident("a group name", p.diags) {
                                Some(g) => ast.uniq.push(UniqAst::Group(sp, g)),
                                None => break,
                            },
                            "aspects" => loop {
                                if tc.done() {
                                    break;
                                }
                                if tc.at_p(&P::Comma) {
                                    tc.next();
                                    continue;
                                }
                                let Some(key) = tc.expect_ident("an aspect key", p.diags) else {
                                    break;
                                };
                                let Some((_, value)) =
                                    signed_int(&mut tc, "an aspect value", p.diags)
                                else {
                                    break;
                                };
                                ast.aspects.push((sp.clone(), key.1, value));
                            },
                            "start" => {
                                if tc.eat_ident("in").is_none() {
                                    p.diags
                                        .push(sp.error(code::E201, "expected `start in <zone>`"));
                                    break;
                                }
                                let Some(zone) = tc.expect_ident("a zone name", p.diags) else {
                                    break;
                                };
                                let mut count = None;
                                if tc.eat_ident("count").is_some() {
                                    match count_int(&mut tc, "`count`", p.diags) {
                                        Some((cs, c)) => count = Some((cs, c)),
                                        None => break,
                                    }
                                }
                                ast.boot = Some(BootAst {
                                    span: sp,
                                    zone,
                                    count,
                                });
                            }
                            "relates" => {
                                let Some(kind) = tc.expect_ident("a relation kind", p.diags) else {
                                    break;
                                };
                                let mut objects = Vec::new();
                                loop {
                                    if tc.done() {
                                        break;
                                    }
                                    if tc.at_p(&P::Comma) {
                                        tc.next();
                                        continue;
                                    }
                                    match tc.expect_ident("a card name", p.diags) {
                                        Some(o) => objects.push(o),
                                        None => break,
                                    }
                                }
                                ast.relates.push((kind, objects));
                            }
                            "desc" => match tc.expect_str("a description string", p.diags) {
                                Some(d) => ast.desc = Some(d),
                                None => break,
                            },
                            "lifetime" => match duration_value(&l) {
                                Some(v) => {
                                    ast.lifetime = Some(v);
                                    tc.next();
                                }
                                None => {
                                    p.diags.push(
                                        sp.error(code::E201, "lifetime takes a duration literal"),
                                    );
                                    break;
                                }
                            },
                            "mint" => match tc.expect_ident("an aliased card name", p.diags) {
                                Some(m) => ast.mint = Some(m),
                                None => break,
                            },
                            other => {
                                p.diags.push(
                                    sp.error(code::E201, format!("unknown card field `{other}`")),
                                );
                                break;
                            }
                        }
                    }
                }
            }
        });
        ast
    }

    /// `on complete [<aspect>]` / `on expire` inside a card.
    fn parse_card_book(&mut self, line: Line, card: &mut CardAst) {
        let span = line.first().map(|(s, _)| s.clone()).unwrap();
        let mut tc = TC::new(&line.toks);
        tc.next();
        let kind = tc.expect_ident("`complete` or `expire`", self.diags);
        let base = line.indent;
        match kind.map(|k| k.1) {
            Some(k) if k == "complete" => {
                let mut qualifier = None;
                if tc.eat_p(&P::LBracket).is_some() {
                    qualifier = tc.expect_ident("an aspect qualifier", self.diags);
                    if tc.eat_p(&P::RBracket).is_none() {
                        self.err_here("expected `]` after the aspect qualifier");
                    }
                }
                let rules = self.parse_rules(base);
                card.completes.push((span, qualifier, rules));
            }
            Some(k) if k == "expire" => {
                if card.expire.is_some() {
                    self.diags
                        .push(span.error(code::E236, "repeated `on expire` on one card"));
                }
                let rules = self.parse_rules(base);
                card.expire = Some((span, rules));
            }
            _ => {
                self.err_here("expected `on complete [<aspect>]` or `on expire`");
            }
        }
    }
}
