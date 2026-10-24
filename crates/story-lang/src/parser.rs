//! The recursive-descent line parser (language.md §1–§6). One declaration per
//! block, one fact per line, indentation as nesting: a block's children are
//! the following lines whose first-token column is deeper than the block
//! header's.

mod arith;
mod cards;
mod cond;
mod decls;
mod effects;
mod query;
mod scalar;
mod verbs;

pub use arith::parse_arith;
pub use cond::parse_cond;
pub use query::parse_scope;

use crate::ast::*;
use crate::lexer::{Line, Tok, P};
use crate::span::Span;
use std::sync::Arc;
use story_content::{code, Diagnostic};

use scalar::signed_int;

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
                "relations" => items.push(TopItem::Relations(self.parse_relations())),
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
}
