//! Verb and recipe declarations: the verbs block, verb fields, recipes, and slots.

use super::cond::parse_cond;
use super::query::parse_query_terms;
use super::scalar::{count_int, duration_value, signed_int};
use super::{Parser, TC};
use crate::ast::*;
use crate::lexer::{Line, P, Tok};
use crate::span::Span;
use story_content::code;

impl<'d> Parser<'d> {
    pub(super) fn parse_verbs(&mut self) -> Vec<VerbAst> {
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
                    if let Some((_, c)) = count_int(&mut tc, "`instances`", self.diags) {
                        instances = Some(InstancesAst::N(spn, c))
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
}
