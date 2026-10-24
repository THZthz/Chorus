//! Top-level block declarations: game, zones, aspects, relations, predicates, legacies, decks, endings, effectsets, and event books.

use super::cond::parse_cond;
use super::scalar::{duration_value, signed_int};
use super::{Parser, TC};
use crate::ast::*;
use crate::lexer::{Line, P, Tok};
use story_content::code;

impl<'d> Parser<'d> {
    pub(super) fn parse_game(&mut self) -> GameAst {
        let line = self.next_line().unwrap();
        let header_span = line.first().map(|(s, _)| s.clone()).unwrap();
        let mut tc = TC::new(&line.toks);
        tc.next();
        let title = tc.expect_str("a quoted game title", self.diags);
        let mut ast = GameAst {
            span: header_span,
            title: title.unwrap_or_else(|| (line.first().unwrap().0.clone(), String::new())),
            seed: None,
            turn: None,
            home: None,
            meta: Vec::new(),
        };
        let base = line.indent;
        self.children(base, |p, l| {
            let kw = l.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "seed" => {
                    let mut tc = TC::new(&l.toks);
                    tc.next();
                    match tc.peek() {
                        Some((s, Tok::Int(n))) => {
                            ast.seed = Some(SeedAst::Fixed((s.clone(), *n as u64)));
                            tc.next();
                        }
                        Some((s, Tok::Ident(w))) if w == "host" => {
                            ast.seed = Some(SeedAst::Host(s.clone()));
                            tc.next();
                        }
                        _ => p.err_here("seed takes a number or `host`"),
                    }
                }
                "turn" => match duration_value(&l) {
                    Some(v) => ast.turn = Some(v),
                    None => p.err_here("turn takes a duration literal"),
                },
                "home" => {
                    let mut tc = TC::new(&l.toks);
                    tc.next();
                    if let Some(z) = tc.expect_ident("a zone name", p.diags) {
                        ast.home = Some(z);
                    }
                }
                "meta" => ast.meta.push(p.meta_line(&l)),
                _ => p.err_here(format!("unknown game field `{kw}`")),
            }
        });
        ast
    }

    pub(super) fn parse_zones(&mut self) -> Vec<ZoneAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut zones: Vec<ZoneAst> = Vec::new();
        self.children(base, |p, l| {
            if l.first_ident() == Some("meta") {
                if let Some(z) = zones.last_mut() {
                    z.meta.push(p.meta_line(&l));
                } else {
                    p.err_here("meta line before any zone");
                }
                return;
            }
            let mut tc = TC::new(&l.toks);
            let span = l.first().map(|(s, _)| s.clone()).unwrap();
            let Some(name) = tc.expect_ident("a zone name", p.diags) else {
                return;
            };
            let mut parent = None;
            if tc.eat_ident("in").is_some() {
                parent = tc.expect_ident("a parent zone name", p.diags);
            }
            zones.push(ZoneAst {
                span,
                name: name.1,
                parent,
                meta: Vec::new(),
            });
        });
        zones
    }

    pub(super) fn parse_aspects(&mut self) -> Vec<AspectAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut out: Vec<AspectAst> = Vec::new();
        self.children(base, |p, l| {
            if l.first_ident() == Some("meta") {
                if let Some(a) = out.last_mut() {
                    a.meta.push(p.meta_line(&l));
                } else {
                    p.err_here("meta line before any aspect");
                }
                return;
            }
            let mut tc = TC::new(&l.toks);
            let span = l.first().map(|(s, _)| s.clone()).unwrap();
            let Some(key) = tc.expect_ident("an aspect key", p.diags) else {
                return;
            };
            if tc.eat_p(&P::Colon).is_none() {
                p.diags.push(
                    key.0
                        .error(code::E201, "expected `:` and a default integer value"),
                );
                return;
            }
            if let Some((_, v)) = signed_int(&mut tc, "a default integer value", p.diags) {
                out.push(AspectAst {
                    span,
                    key: key.1,
                    value: v,
                    meta: Vec::new(),
                });
            }
        });
        out
    }

    /// The `relations` registry (language.md §2): one kind per line, flags
    /// order-free (`symmetric`, `exclusive`; directed is the default and has
    /// no keyword).
    pub(super) fn parse_relations(&mut self) -> Vec<RelationKindAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut out: Vec<RelationKindAst> = Vec::new();
        self.children(base, |p, l| {
            if l.first_ident() == Some("meta") {
                if let Some(k) = out.last_mut() {
                    k.meta.push(p.meta_line(&l));
                } else {
                    p.err_here("meta line before any relation kind");
                }
                return;
            }
            let mut tc = TC::new(&l.toks);
            let span = l.first().map(|(s, _)| s.clone()).unwrap();
            let Some(name) = tc.expect_ident("a relation kind name", p.diags) else {
                return;
            };
            let mut symmetric = None;
            let mut exclusive = None;
            while !tc.done() {
                let Some((sp, tok)) = tc.peek().cloned() else {
                    break;
                };
                let Tok::Ident(flag) = &tok else {
                    p.diags.push(sp.error(
                        code::E201,
                        format!("unexpected `{tok:?}` after the relation kind name"),
                    ));
                    tc.next();
                    continue;
                };
                match flag.as_str() {
                    "symmetric" => {
                        if symmetric.is_some() {
                            p.diags.push(sp.error(
                                code::E236,
                                "repeated `symmetric` flag on one relation kind",
                            ));
                        }
                        symmetric = Some(sp);
                    }
                    "exclusive" => {
                        if exclusive.is_some() {
                            p.diags.push(sp.error(
                                code::E236,
                                "repeated `exclusive` flag on one relation kind",
                            ));
                        }
                        exclusive = Some(sp);
                    }
                    other => p.diags.push(sp.error(
                        code::E201,
                        format!("unknown relation flag `{other}` (use `symmetric` or `exclusive`)"),
                    )),
                }
                tc.next();
            }
            out.push(RelationKindAst {
                span,
                name,
                symmetric,
                exclusive,
                meta: Vec::new(),
            });
        });
        out
    }

    pub(super) fn parse_predicates(&mut self) -> Vec<PredAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut out: Vec<PredAst> = Vec::new();
        self.children(base, |p, l| {
            if l.first_ident() == Some("meta") {
                if let Some(a) = out.last_mut() {
                    a.meta.push(p.meta_line(&l));
                } else {
                    p.err_here("meta line before any predicate");
                }
                return;
            }
            let mut tc = TC::new(&l.toks);
            let span = l.first().map(|(s, _)| s.clone()).unwrap();
            if tc.eat_ident("predicate").is_none() {
                p.err_here("expected a `predicate` declaration");
                return;
            }
            let Some(name) = tc.expect_ident("a predicate name", p.diags) else {
                return;
            };
            let mut params = Vec::new();
            if tc.eat_p(&P::LParen).is_some() {
                loop {
                    if tc.done() || tc.eat_p(&P::RParen).is_some() {
                        break;
                    }
                    let Some(prm) = tc.expect_ident("a parameter name", p.diags) else {
                        break;
                    };
                    params.push(prm);
                    if tc.eat_p(&P::Comma).is_none() {
                        tc.eat_p(&P::RParen);
                        break;
                    }
                }
            }
            let body = if tc.eat_p(&P::Eq).is_some() {
                parse_cond(&mut tc, p.diags)
            } else {
                p.diags
                    .push(name.0.error(code::E201, "expected `= <condition>`"));
                None
            };
            out.push(PredAst {
                span,
                name,
                params,
                body,
                meta: Vec::new(),
            });
        });
        out
    }

    pub(super) fn parse_decks(&mut self) -> Vec<DeckAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut out: Vec<DeckAst> = Vec::new();
        self.children(base, |p, l| {
            let kw = l.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "meta" => {
                    if let Some(d) = out.last_mut() {
                        d.meta.push(p.meta_line(&l));
                    } else {
                        p.err_here("meta line before any deck");
                    }
                }
                "deck" => {
                    let span = l.first().map(|(s, _)| s.clone()).unwrap();
                    let mut tc = TC::new(&l.toks);
                    tc.next();
                    let Some(name) = tc.expect_ident("a deck name", p.diags) else {
                        return;
                    };
                    let mut ast = DeckAst {
                        span,
                        name,
                        entries: Vec::new(),
                        fallback: None,
                        wrap: false,
                        meta: Vec::new(),
                    };
                    let dbase = l.indent;
                    p.children(dbase, |p, c| {
                        let ck = c.first_ident().unwrap_or("").to_string();
                        match ck.as_str() {
                            "meta" => ast.meta.push(p.meta_line(&c)),
                            "entries" => {
                                let mut tc = TC::new(&c.toks);
                                tc.next();
                                while !tc.done() {
                                    if tc.at_p(&P::Comma) {
                                        tc.next();
                                        continue;
                                    }
                                    match tc.expect_ident("a card name", p.diags) {
                                        Some(e) => ast.entries.push(e),
                                        None => break,
                                    }
                                }
                            }
                            "fallback" => {
                                let mut tc = TC::new(&c.toks);
                                tc.next();
                                if let Some(f) = tc.expect_ident("a card name", p.diags) {
                                    ast.fallback = Some(f);
                                }
                            }
                            "wrap" => ast.wrap = true,
                            other => p.err_here(format!("unknown deck field `{other}`")),
                        }
                    });
                    out.push(ast);
                }
                _ => p.err_here(format!("expected `deck`, found `{kw}`")),
            }
        });
        out
    }

    pub(super) fn parse_legacies(&mut self) -> Vec<LegacyAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut out: Vec<LegacyAst> = Vec::new();
        self.children(base, |p, l| {
            if l.first_ident() == Some("meta") {
                if let Some(a) = out.last_mut() {
                    a.meta.push(p.meta_line(&l));
                } else {
                    p.err_here("meta line before any legacy key");
                }
                return;
            }
            let mut tc = TC::new(&l.toks);
            let span = l.first().map(|(s, _)| s.clone()).unwrap();
            let Some(key) = tc.expect_ident("a legacy key", p.diags) else {
                return;
            };
            if tc.eat_p(&P::Colon).is_none() {
                p.diags.push(
                    key.0
                        .error(code::E201, "expected `:` and a default integer value"),
                );
                return;
            }
            if let Some((_, v)) = signed_int(&mut tc, "a default integer value", p.diags) {
                out.push(LegacyAst {
                    span,
                    key: key.1,
                    value: v,
                    meta: Vec::new(),
                });
            }
        });
        out
    }

    pub(super) fn parse_endings(&mut self) -> Vec<EndingAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut out: Vec<EndingAst> = Vec::new();
        self.children(base, |p, l| {
            let kw = l.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "meta" => {
                    if let Some(e) = out.last_mut() {
                        e.meta.push(p.meta_line(&l));
                    } else {
                        p.err_here("meta line before any ending");
                    }
                }
                "ending" => {
                    let span = l.first().map(|(s, _)| s.clone()).unwrap();
                    let mut tc = TC::new(&l.toks);
                    tc.next();
                    let Some(name) = tc.expect_ident("an ending id", p.diags) else {
                        return;
                    };
                    let Some(title) = tc.expect_str("a display title", p.diags) else {
                        return;
                    };
                    let Some(tier) = tc.expect_ident("a tier label", p.diags) else {
                        return;
                    };
                    let mut ast = EndingAst {
                        span,
                        name,
                        title,
                        tier,
                        text: None,
                        meta: Vec::new(),
                    };
                    let ebase = l.indent;
                    p.children(ebase, |p, c| {
                        let ck = c.first_ident().unwrap_or("").to_string();
                        match ck.as_str() {
                            "meta" => ast.meta.push(p.meta_line(&c)),
                            "text" => {
                                let mut tc = TC::new(&c.toks);
                                tc.next();
                                if let Some(t) = tc.expect_str("ending text", p.diags) {
                                    ast.text = Some(t);
                                }
                            }
                            other => p.err_here(format!("unknown ending field `{other}`")),
                        }
                    });
                    out.push(ast);
                }
                _ => p.err_here(format!("expected `ending`, found `{kw}`")),
            }
        });
        out
    }

    /// One top-level `effectset <name>(<params>)` declaration whose children
    /// are its `effects` body and `meta` lines (language.md §6).
    pub(super) fn parse_effectset_decl(&mut self, line: Line) -> EffectsetAst {
        let span = line.first().map(|(s, _)| s.clone()).unwrap();
        let mut tc = TC::new(&line.toks);
        tc.next();
        let name = tc.expect_ident("an effectset name", self.diags);
        let mut params = Vec::new();
        if tc.eat_p(&P::LParen).is_some() {
            loop {
                if tc.done() || tc.eat_p(&P::RParen).is_some() {
                    break;
                }
                let Some(prm) = tc.expect_ident("a parameter name", self.diags) else {
                    break;
                };
                params.push(prm);
                if tc.eat_p(&P::Comma).is_none() {
                    tc.eat_p(&P::RParen);
                    break;
                }
            }
        }
        let mut ast = EffectsetAst {
            span: span.clone(),
            name: name.unwrap_or_else(|| (span, String::new())),
            params,
            body: None,
            body_lines: Vec::new(),
            meta: Vec::new(),
        };
        let base = line.indent;
        loop {
            let deeper = match self.peek_line() {
                Some(l) => l.indent > base,
                None => false,
            };
            if !deeper {
                break;
            }
            let child = self.next_line().unwrap();
            match child.first_ident() {
                Some("meta") => ast.meta.push(self.meta_line(&child)),
                Some("effects") => {
                    let ebase = child.indent;
                    ast.body_lines = self.peek_deeper(ebase);
                    let list = self.parse_effect_lines(ebase);
                    ast.body = Some((child.first().unwrap().0.clone(), list));
                }
                _ => {
                    let sp = child.first().map(|(s, _)| s.clone()).unwrap();
                    self.diags.push(sp.error(
                        code::E201,
                        "expected `effects` or `meta` under the effectset",
                    ));
                }
            }
        }
        ast
    }

    pub(super) fn parse_events(&mut self) -> Vec<EventBookAst> {
        let line = self.next_line().unwrap();
        let span = line.first().map(|(s, _)| s.clone()).unwrap();
        let mut tc = TC::new(&line.toks);
        tc.next();
        if tc.eat_ident("event").is_none() {
            self.diags
                .push(span.error(code::E201, "top-level books are `on event <name>`"));
        }
        let Some(name) = tc.expect_ident("an event name", self.diags) else {
            return Vec::new();
        };
        let rules = self.parse_rules(line.indent);
        vec![EventBookAst {
            span,
            name,
            rules,
            meta: Vec::new(),
        }]
    }
}
