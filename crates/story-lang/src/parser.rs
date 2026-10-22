//! The recursive-descent line parser (language.md §1–§6). One declaration per
//! block, one fact per line, indentation as nesting: a block's children are
//! the following lines whose first-token column is deeper than the block
//! header's.

use crate::ast::*;
use crate::lexer::{Line, Tok, P};
use crate::span::Span;
use std::sync::Arc;
use story_content::{code, Diagnostic};

fn eof_span() -> Span {
    Span::new(Arc::from("<eof>"), 0, 0)
}

pub struct TC<'a> {
    pub toks: &'a [(Span, Tok)],
    pub i: usize,
}

impl<'a> TC<'a> {
    pub fn new(toks: &'a [(Span, Tok)]) -> Self {
        TC { toks, i: 0 }
    }

    pub fn done(&self) -> bool {
        self.i >= self.toks.len()
    }

    pub fn peek(&self) -> Option<&(Span, Tok)> {
        self.toks.get(self.i)
    }

    pub fn peek_at(&self, off: usize) -> Option<&(Span, Tok)> {
        self.toks.get(self.i + off)
    }

    pub fn next(&mut self) -> Option<(Span, Tok)> {
        let t = self.toks.get(self.i).cloned();
        if t.is_some() {
            self.i += 1;
        }
        t
    }

    pub fn at_p(&self, p: &P) -> bool {
        matches!(self.peek(), Some((_, Tok::P(q))) if q == p)
    }

    pub fn eat_p(&mut self, p: &P) -> Option<Span> {
        if self.at_p(p) {
            self.next().map(|(s, _)| s)
        } else {
            None
        }
    }

    pub fn at_ident(&self, name: &str) -> bool {
        matches!(self.peek(), Some((_, Tok::Ident(s))) if s == name)
    }

    pub fn eat_ident(&mut self, name: &str) -> Option<Span> {
        if self.at_ident(name) {
            self.next().map(|(s, _)| s)
        } else {
            None
        }
    }

    fn last_span(&self) -> Span {
        self.toks
            .last()
            .map(|(s, _)| s.clone())
            .unwrap_or_else(eof_span)
    }

    pub fn expect_ident(
        &mut self,
        what: &str,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<(Span, String)> {
        match self.peek() {
            Some((s, Tok::Ident(n))) => {
                let r = Some((s.clone(), n.clone()));
                self.i += 1;
                r
            }
            _ => {
                let sp = self
                    .peek()
                    .map(|(s, _)| s.clone())
                    .unwrap_or_else(|| self.last_span());
                diags.push(sp.error(code::E201, format!("expected {what}")));
                None
            }
        }
    }

    pub fn expect_int(&mut self, what: &str, diags: &mut Vec<Diagnostic>) -> Option<(Span, i64)> {
        match self.peek() {
            Some((s, Tok::Int(n))) => {
                let r = Some((s.clone(), *n));
                self.i += 1;
                r
            }
            _ => {
                let sp = self
                    .peek()
                    .map(|(s, _)| s.clone())
                    .unwrap_or_else(|| self.last_span());
                diags.push(sp.error(code::E201, format!("expected {what}")));
                None
            }
        }
    }

    pub fn expect_str(
        &mut self,
        what: &str,
        diags: &mut Vec<Diagnostic>,
    ) -> Option<(Span, String)> {
        match self.peek() {
            Some((s, Tok::Str(v))) => {
                let r = Some((s.clone(), v.clone()));
                self.i += 1;
                r
            }
            _ => {
                let sp = self
                    .peek()
                    .map(|(s, _)| s.clone())
                    .unwrap_or_else(|| self.last_span());
                diags.push(sp.error(code::E201, format!("expected {what}")));
                None
            }
        }
    }
}

pub struct Parser<'d> {
    pub file: Arc<str>,
    pub lines: Vec<Line>,
    pub pos: usize,
    pub diags: &'d mut Vec<Diagnostic>,
}

impl<'d> Parser<'d> {
    pub fn new(file: Arc<str>, lines: Vec<Line>, diags: &'d mut Vec<Diagnostic>) -> Self {
        Parser {
            file,
            lines,
            pos: 0,
            diags,
        }
    }

    pub fn parse_file(&mut self) -> Vec<TopItem> {
        let mut items = Vec::new();
        while let Some(line) = self.peek_line() {
            if line.indent != 1 {
                let sp = line.first().map(|(s, _)| s.clone()).unwrap();
                self.diags
                    .push(sp.error(code::E201, "top-level declarations start at column 1"));
                self.pos += 1;
                continue;
            }
            let kw = line.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "game" => items.push(TopItem::Game(self.parse_game())),
                "zones" => items.push(TopItem::Zones(self.parse_zones())),
                "aspects" => items.push(TopItem::Aspects(self.parse_aspects())),
                "predicates" => items.push(TopItem::Predicates(self.parse_predicates())),
                "cards" => items.push(TopItem::Cards(self.parse_cards())),
                "decks" => items.push(TopItem::Decks(self.parse_decks())),
                "legacies" => items.push(TopItem::Legacies(self.parse_legacies())),
                "endings" => items.push(TopItem::Endings(self.parse_endings())),
                "verbs" => items.push(TopItem::Verbs(self.parse_verbs())),
                "effectset" => {
                    let line = self.next_line().unwrap();
                    items.push(TopItem::Effectsets(vec![self.parse_effectset_decl(line)]));
                }
                "on" => items.push(TopItem::Events(self.parse_events())),
                "include" => {
                    let line = self.next_line().unwrap();
                    let mut tc = TC::new(&line.toks);
                    tc.next();
                    let path = tc.expect_str("an include path string", self.diags);
                    if let Some((sp, _)) = tc.peek() {
                        self.diags.push(
                            sp.clone()
                                .error(code::E201, "unexpected tokens after include path"),
                        );
                    }
                    if let Some(p) = path {
                        items.push(TopItem::Include(IncludeAst {
                            span: p.0.clone(),
                            path: p,
                        }));
                    }
                }
                other => {
                    let sp = line.first().map(|(s, _)| s.clone()).unwrap();
                    self.diags.push(sp.error(
                        code::E201,
                        format!("unknown top-level declaration `{other}`"),
                    ));
                    self.pos += 1;
                }
            }
        }
        items
    }

    fn peek_line(&self) -> Option<&Line> {
        self.lines.get(self.pos)
    }

    /// Clone the block of lines deeper than `base` without consuming them.
    fn peek_deeper(&self, base: u32) -> Vec<Line> {
        let mut out = Vec::new();
        let mut i = self.pos;
        while let Some(l) = self.lines.get(i) {
            if l.indent <= base {
                break;
            }
            out.push(l.clone());
            i += 1;
        }
        out
    }

    fn next_line(&mut self) -> Option<Line> {
        let l = self.lines.get(self.pos).cloned();
        if l.is_some() {
            self.pos += 1;
        }
        l
    }

    fn err_here(&mut self, msg: impl Into<String>) {
        if let Some(line) = self.peek_line() {
            let sp = line.first().map(|(s, _)| s.clone()).unwrap();
            self.diags.push(sp.error(code::E201, msg));
        }
    }

    /// Child lines of a header at `indent`, dispatched one by one; the loop
    /// ends at the first line at or above the header's column.
    fn children(&mut self, indent: u32, mut f: impl FnMut(&mut Self, Line)) {
        loop {
            let deeper = match self.peek_line() {
                Some(l) => l.indent > indent,
                None => false,
            };
            if !deeper {
                return;
            }
            let line = self.next_line().unwrap();
            f(self, line);
        }
    }

    fn meta_line(&self, line: &Line) -> MetaLine {
        let mut tc = TC::new(&line.toks);
        tc.next();
        let key = match tc.peek() {
            Some((_, Tok::Ident(k))) => k.clone(),
            _ => String::new(),
        };
        tc.next();
        let value = match tc.peek() {
            Some((_, Tok::Str(v))) => v.clone(),
            _ => String::new(),
        };
        MetaLine {
            span: line.first().map(|(s, _)| s.clone()).unwrap(),
            key,
            value,
        }
    }

    // -- top-level blocks ---------------------------------------------------

    fn parse_game(&mut self) -> GameAst {
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

    fn parse_zones(&mut self) -> Vec<ZoneAst> {
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

    fn parse_aspects(&mut self) -> Vec<AspectAst> {
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

    fn parse_predicates(&mut self) -> Vec<PredAst> {
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

    fn parse_cards(&mut self) -> Vec<CardAst> {
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

    fn parse_rules(&mut self, book_indent: u32) -> Vec<RuleAst> {
        let mut rules: Vec<RuleAst> = Vec::new();
        loop {
            let deeper = match self.peek_line() {
                Some(l) => l.indent > book_indent,
                None => false,
            };
            if !deeper {
                return rules;
            }
            let line = self.next_line().unwrap();
            if line.first_ident() == Some("meta") {
                if let Some(r) = rules.last_mut() {
                    r.meta.push(self.meta_line(&line));
                } else {
                    self.err_here("meta line before any rule");
                }
                continue;
            }
            if line.first_ident() != Some("rule") {
                self.err_here("expected a `rule` declaration");
                continue;
            }
            let span = line.first().map(|(s, _)| s.clone()).unwrap();
            let rule_indent = line.indent;
            let mut tc = TC::new(&line.toks);
            tc.next();
            let Some(name) = tc.expect_ident("a rule name", self.diags) else {
                continue;
            };
            let mut priority = None;
            let mut once = None;
            let mut when = None;
            while !tc.done() {
                if tc.at_ident("priority") {
                    tc.next();
                    match signed_int(&mut tc, "a priority", self.diags) {
                        Some(v) => priority = Some(v),
                        None => break,
                    }
                } else if tc.at_ident("once") {
                    once = tc.eat_ident("once");
                } else if tc.at_ident("when") {
                    tc.next();
                    match parse_cond(&mut tc, self.diags) {
                        Some(c) => when = Some((c.span().clone(), c)),
                        None => break,
                    }
                } else {
                    let (sp, t) = tc.peek().cloned().unwrap();
                    self.diags
                        .push(sp.error(code::E201, format!("unexpected `{t:?}` in rule line")));
                    tc.next();
                }
            }
            let mut effects = None;
            let mut rule_meta = Vec::new();
            loop {
                let deeper = match self.peek_line() {
                    Some(l) => l.indent > rule_indent,
                    None => false,
                };
                if !deeper {
                    break;
                }
                let child = self.next_line().unwrap();
                if child.first_ident() == Some("meta") {
                    rule_meta.push(self.meta_line(&child));
                } else if child.first_ident() == Some("effects") {
                    let list = self.parse_effect_lines(child.indent);
                    effects = Some((child.first().unwrap().0.clone(), list));
                } else {
                    let sp = child.first().map(|(s, _)| s.clone()).unwrap();
                    self.diags
                        .push(sp.error(code::E201, "expected `effects` or `meta` under the rule"));
                }
            }
            rules.push(RuleAst {
                span,
                name,
                priority,
                once,
                when,
                effects,
                meta: rule_meta,
            });
        }
    }

    fn parse_decks(&mut self) -> Vec<DeckAst> {
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

    fn parse_legacies(&mut self) -> Vec<LegacyAst> {
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

    fn parse_endings(&mut self) -> Vec<EndingAst> {
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

    fn parse_verbs(&mut self) -> Vec<VerbAst> {
        let line = self.next_line().unwrap();
        let base = line.indent;
        let mut out: Vec<VerbAst> = Vec::new();
        self.children(base, |p, l| {
            let kw = l.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "meta" => {
                    if let Some(v) = out.last_mut() {
                        v.meta.push(p.meta_line(&l));
                    } else {
                        p.err_here("meta line before any verb");
                    }
                }
                "verb" => out.push(p.parse_verb(l)),
                _ => p.err_here(format!("expected `verb`, found `{kw}`")),
            }
        });
        out
    }

    fn parse_verb(&mut self, line: Line) -> VerbAst {
        let span = line.first().map(|(s, _)| s.clone()).unwrap();
        let mut tc = TC::new(&line.toks);
        tc.next();
        let name = tc.expect_ident("a verb name", self.diags);
        let mode: Option<(Span, ModeAst)> = match tc.peek() {
            Some((s, Tok::Ident(m))) => {
                let m = m.clone();
                let mode = match m.as_str() {
                    "committable" => Some(ModeAst::Committable),
                    "autonomous" => Some(ModeAst::Autonomous),
                    "uncommittable" => Some(ModeAst::Uncommittable),
                    other => {
                        self.diags.push(
                            s.clone().error(
                                code::E201,
                                format!("`{other}` is not a verb mode; use committable, autonomous, or uncommittable"),
                            ),
                        );
                        None
                    }
                };
                mode.map(|m| (s.clone(), m))
            }
            _ => None,
        };
        if mode.is_some() {
            tc.next();
        } else {
            self.err_here("a verb declares exactly one mode");
        }
        let mut instances = None;
        if tc.eat_ident("instances").is_some() {
            match tc.peek() {
                Some((s, Tok::Ident(w))) if w == "any" => {
                    instances = Some(InstancesAst::Any(s.clone()));
                    tc.next();
                }
                Some((s, Tok::Int(_))) | Some((s, Tok::P(P::Minus))) => {
                    let spn = s.clone();
                    match count_int(&mut tc, "`instances`", self.diags) {
                        Some((_, c)) => instances = Some(InstancesAst::N(spn, c)),
                        None => {}
                    }
                    tc.next();
                }
                _ => self.err_here("instances takes a number or `any`"),
            }
        }
        let mut ast = VerbAst {
            span,
            name: name.unwrap_or_else(|| (line.first().unwrap().0.clone(), String::new())),
            mode: mode.unwrap_or((line.first().unwrap().0.clone(), ModeAst::Committable)),
            instances,
            title: None,
            releases: None,
            recipes: Vec::new(),
            meta: Vec::new(),
        };
        let base = line.indent;
        self.children(base, |p, l| {
            let kw = l.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "meta" => ast.meta.push(p.meta_line(&l)),
                "title" => {
                    let mut tc = TC::new(&l.toks);
                    tc.next();
                    if let Some(t) = tc.expect_str("a title string", p.diags) {
                        ast.title = Some(t);
                    }
                }
                "releases" => {
                    let mut tc = TC::new(&l.toks);
                    tc.next();
                    if let Some(z) = tc.expect_ident("a zone name", p.diags) {
                        ast.releases = Some(z);
                    }
                }
                "recipe" => {
                    let r = p.parse_recipe(l);
                    ast.recipes.push(r);
                }
                other => p.err_here(format!("unknown verb field `{other}`")),
            }
        });
        ast
    }

    fn parse_recipe(&mut self, line: Line) -> RecipeAst {
        let span = line.first().map(|(s, _)| s.clone()).unwrap();
        let mut tc = TC::new(&line.toks);
        tc.next();
        let name = tc.expect_ident("a recipe name", self.diags);
        let mut ast = RecipeAst {
            span: span.clone(),
            name: name.unwrap_or_else(|| (span.clone(), String::new())),
            label: None,
            priority: None,
            craftable: None,
            chain_only: None,
            max: None,
            duration: None,
            declares: Vec::new(),
            when: None,
            guards: Vec::new(),
            allow_shadow: None,
            slots: Vec::new(),
            effects: None,
            complete: None,
            meta: Vec::new(),
        };
        let base = line.indent;
        self.children(base, |p, l| {
            let kw = l.first_ident().unwrap_or("").to_string();
            match kw.as_str() {
                "meta" => ast.meta.push(p.meta_line(&l)),
                "slots" => p.parse_slot_lines(l, &mut ast),
                "effects" => {
                    let list = p.parse_effect_lines(l.indent);
                    ast.effects = Some((l.first().unwrap().0.clone(), list));
                }
                "on" => {
                    let mut tc = TC::new(&l.toks);
                    tc.next();
                    if tc.eat_ident("complete").is_some() {
                        if let Some((sp, _)) = tc.peek() {
                            p.diags.push(sp.clone().error(
                                code::E201,
                                "a recipe completion book is unqualified; aspect qualifiers belong to cards",
                            ));
                        }
                        let rules = p.parse_rules(l.indent);
                        ast.complete = Some((l.first().unwrap().0.clone(), rules));
                    } else {
                        p.err_here("expected `on complete` inside a recipe");
                    }
                }
                _ => {
                    let mut tc = TC::new(&l.toks);
                    while !tc.done() {
                        let Some((sp, tok)) = tc.peek().cloned() else {
                            break;
                        };
                        let Tok::Ident(field) = &tok else {
                            p.diags.push(sp.error(code::E201, "expected a recipe field"));
                            break;
                        };
                        tc.next();
                        match field.as_str() {
                            "label" => match tc.expect_str("a label string", p.diags) {
                                Some(t) => ast.label = Some(t),
                                None => break,
                            },
                            "priority" => match signed_int(&mut tc, "a priority", p.diags) {
                                Some(v) => ast.priority = Some(v),
                                None => break,
                            },
                            "craftable" => ast.craftable = Some(sp),
                            "chain" => {
                                if tc.eat_p(&P::Minus).is_some() && tc.eat_ident("only").is_some()
                                {
                                    ast.chain_only = Some(sp);
                                } else {
                                    p.diags.push(sp.error(code::E201, "expected `chain-only`"));
                                }
                            }
                            "max" => match signed_int(&mut tc, "a max count", p.diags) {
                                Some((msp, n)) => {
                                    if n <= 0 {
                                        p.diags
                                            .push(msp.error(code::E246, "`max` must be a positive count"));
                                    } else {
                                        ast.max = Some((sp, n as u32));
                                    }
                                }
                                None => break,
                            },
                            "duration" => match duration_value(&l) {
                                Some(v) => {
                                    ast.duration = Some(v);
                                    tc.next();
                                }
                                None => {
                                    p.diags.push(
                                        sp.error(
                                            code::E201,
                                            "duration takes a duration literal or a non-negative integer",
                                        ),
                                    );
                                    break;
                                }
                            },
                            "declares" => loop {
                                if tc.done() {
                                    break;
                                }
                                if tc.at_p(&P::Comma) {
                                    tc.next();
                                    continue;
                                }
                                match tc.expect_ident("an aspect key", p.diags) {
                                    Some(a) => ast.declares.push(a),
                                    None => break,
                                }
                            },
                            "when" => match parse_cond(&mut tc, p.diags) {
                                Some(c) => ast.when = Some((c.span().clone(), c)),
                                None => break,
                            },
                            "guard" => {
                                let Some(prose) = tc.expect_str("guard prose", p.diags) else {
                                    break;
                                };
                                if tc.eat_ident("when").is_none() {
                                    p.diags.push(
                                        prose.0.error(
                                            code::E201,
                                            "a guard is `guard \"<prose>\" when <condition>`",
                                        ),
                                    );
                                    break;
                                }
                                if let Some(c) = parse_cond(&mut tc, p.diags) {
                                    ast.guards.push(GuardAst {
                                        span: sp,
                                        prose,
                                        when: c,
                                    });
                                }
                            }
                            "allow" => {
                                if tc.eat_ident("shadow").is_some() {
                                    ast.allow_shadow = Some(sp);
                                } else {
                                    p.diags
                                        .push(sp.error(code::E201, "expected `allow shadow`"));
                                }
                            }
                            other => {
                                p.diags.push(
                                    sp.error(
                                        code::E201,
                                        format!("unknown recipe field `{other}`"),
                                    ),
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

    fn parse_slot_lines(&mut self, line: Line, recipe: &mut RecipeAst) {
        let base = line.indent;
        self.children(base, |p, l| {
            if l.first_ident() == Some("meta") {
                if let Some(s) = recipe.slots.last_mut() {
                    s.meta.push(p.meta_line(&l));
                } else {
                    p.err_here("meta line before any slot");
                }
                return;
            }
            let span = l.first().map(|(s, _)| s.clone()).unwrap();
            let mut tc = TC::new(&l.toks);
            let Some(name) = tc.expect_ident("a slot name", p.diags) else {
                return;
            };
            if tc.eat_p(&P::Colon).is_none() {
                p.diags
                    .push(name.0.error(code::E201, "expected `:` and a slot range"));
                return;
            }
            let Some((rs, lo)) = tc.expect_int("a slot range minimum", p.diags) else {
                return;
            };
            let range = if tc.eat_p(&P::DotDot).is_some() {
                match tc.peek() {
                    Some((_, Tok::Int(hi))) => {
                        let hi = *hi as u32;
                        tc.next();
                        RangeAst::Bounded(rs, lo as u32, hi)
                    }
                    _ => RangeAst::Open(rs, lo as u32),
                }
            } else {
                RangeAst::Exact(rs, lo as u32)
            };
            let mut flags = Vec::new();
            loop {
                let flag = match tc.peek() {
                    Some((s, Tok::Ident(w))) => match w.as_str() {
                        "greedy" => Some((s.clone(), SlotFlagAst::Greedy)),
                        "consuming" => Some((s.clone(), SlotFlagAst::Consuming)),
                        "open" => Some((s.clone(), SlotFlagAst::Open)),
                        "take" => Some((s.clone(), SlotFlagAst::Take)),
                        _ => None,
                    },
                    _ => None,
                };
                match flag {
                    Some((s, f)) => {
                        flags.push((s, f));
                        tc.next();
                    }
                    None => break,
                }
            }
            let query = parse_query_terms(&mut tc, p.diags);
            recipe.slots.push(SlotAst {
                span,
                name,
                range,
                flags,
                query,
                meta: Vec::new(),
            });
        });
    }

    /// One top-level `effectset <name>(<params>)` declaration whose children
    /// are its `effects` body and `meta` lines (language.md §6).
    fn parse_effectset_decl(&mut self, line: Line) -> EffectsetAst {
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

    fn parse_events(&mut self) -> Vec<EventBookAst> {
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

    // -- effects ------------------------------------------------------------

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
                    match count_expr(&mut tc, &span, "`count`", self.diags) {
                        Some(c) => count = Some(c),
                        None => return None,
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
                    match count_expr(&mut tc, &span, "`count`", self.diags) {
                        Some(c) => count = Some(c),
                        None => return None,
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
                    self.diags.push(
                        span.error(code::E201, "continue is `continue via <recipe>`"),
                    );
                    return None;
                }
                let recipe = self.parse_recipe_spelling(&mut tc)?;
                Some(AEffect::Continue { span, recipe })
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

fn duration_value(line: &Line) -> Option<(Span, i64)> {
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
fn signed_int(tc: &mut TC, what: &str, diags: &mut Vec<Diagnostic>) -> Option<(Span, i64)> {
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
fn count_int(tc: &mut TC, what: &str, diags: &mut Vec<Diagnostic>) -> Option<(Span, u32)> {
    let (sp, n) = signed_int(tc, what, diags)?;
    let c = count_arg(&sp, n, what, diags)?;
    Some((sp, c))
}

/// An effect count: a full integer expression with the roll-operand grammar,
/// evaluated at effect time (language.md §4.3). Effects are line-oriented, so
/// the expression ends at end-of-line; a following clause keyword (`into`,
/// `as`, `to`, `via`) is no operator and stops the arithmetic parse on its
/// own.
fn count_expr(tc: &mut TC, at: &Span, what: &str, diags: &mut Vec<Diagnostic>) -> Option<AExpr> {
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

fn parse_set_op(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<(Span, ASetOp)> {
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

fn is_cmp_p(p: &P) -> bool {
    matches!(p, P::Ge | P::Le | P::Gt | P::Lt | P::EqEq | P::Ne)
}

fn cmp_op(p: &P) -> ACmpOp {
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

fn bin_op(p: &P) -> Option<ABinOp> {
    match p {
        P::Plus => Some(ABinOp::Add),
        P::Minus => Some(ABinOp::Sub),
        P::Star => Some(ABinOp::Mul),
        P::Slash => Some(ABinOp::Div),
        _ => None,
    }
}

fn is_name_path(tc: &TC) -> bool {
    matches!(tc.peek(), Some((_, Tok::Ident(_))))
        && tc
            .peek_at(1)
            .is_some_and(|(_, t)| matches!(t, Tok::P(P::Dot)))
        && tc
            .peek_at(2)
            .is_some_and(|(_, t)| matches!(t, Tok::Ident(_)))
}

fn starts_scope(tc: &TC) -> bool {
    match tc.peek() {
        Some((_, Tok::Ident(w))) => match w.as_str() {
            "world" | "interior" | "table" | "anywhere" | "bound" => true,
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
fn starts_query_term(tc: &TC) -> bool {
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
        other => {
            diags.push(s.error(code::E201, format!("`{other}` is not a scope")));
            None
        }
    }
}

/// Space-conjoined query terms (language.md §3). The list ends at end-of-line,
/// `)`, a stop-clause keyword, or a bare integer (a trailing count).
fn parse_query_terms(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Vec<AQueryTerm> {
    let mut out = Vec::new();
    loop {
        match tc.peek() {
            None | Some((_, Tok::Int(_))) | Some((_, Tok::P(P::RParen))) => return out,
            Some((_, Tok::Ident(w))) => {
                if matches!(w.as_str(), "count" | "all" | "to" | "into") {
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
fn parse_set_query_terms(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Vec<AQueryTerm> {
    let mut out = Vec::new();
    loop {
        match tc.peek() {
            None | Some((_, Tok::Int(_))) | Some((_, Tok::P(P::RParen))) => return out,
            Some((_, Tok::Ident(w))) => {
                if matches!(w.as_str(), "count" | "all" | "to" | "into") {
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
        if !matches!(w.as_str(), "count" | "total" | "alive" | "clock" | "legacy")
            && tc
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
fn parse_arith_term(tc: &mut TC, diags: &mut Vec<Diagnostic>) -> Option<AExpr> {
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
