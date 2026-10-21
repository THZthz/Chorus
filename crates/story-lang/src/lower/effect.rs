//! Effect lowering (language.md §4): the closed vocabulary into
//! `EffectNode`, target resolution, spawn-handle and roll-binding tracking
//! ([E238], [E239]), and effectset expansion ([E233]–[E235]).

use super::expr::INLINE_DEPTH_CAP;
use super::{Compiler, Env};
use crate::ast::{self, ATarget, ArgAst};
use crate::lexer::{Line, Tok};
use crate::parser::parse_arith;
use crate::parser::{Parser, TC};
use crate::span::Span;
use story_content::code;
use story_content::ir::{self, DestroyCount, EffectNode, Expr, Into as IntoZone, Target};
use story_core::{CardId, DeckId, EndingId, RecipeId, SlotName, VerbId};

impl Compiler {
    pub(crate) fn lower_effects(
        &mut self,
        items: &[ast::AEffect],
        env: &mut Env,
    ) -> Vec<EffectNode> {
        let mut out = Vec::new();
        for e in items {
            match e {
                ast::AEffect::Call { span, name, args } => {
                    if let Some(nodes) = self.expand_effectset(span, name, args, env) {
                        out.extend(nodes);
                    }
                }
                other => {
                    if let Some(n) = self.lower_effect(other, env) {
                        out.push(n);
                    }
                }
            }
        }
        out
    }

    fn default_into(&self, env: &Env) -> IntoZone {
        if env.situation {
            IntoZone::Interior
        } else {
            IntoZone::Zone(self.home_zone())
        }
    }

    pub(crate) fn home_zone(&self) -> String {
        self.content
            .game
            .as_ref()
            .map(|g| g.home.clone())
            .unwrap_or_default()
    }

    fn lower_effect(&mut self, e: &ast::AEffect, env: &mut Env) -> Option<EffectNode> {
        match e {
            ast::AEffect::Spawn {
                card,
                count,
                into,
                handle,
                ..
            } => {
                if !self.check_card_exists(&card.1, &card.0) {
                    return None;
                }
                let into = match into {
                    Some(i) => self.lower_into(i)?,
                    None => self.default_into(env),
                };
                if let Some((hsp, h)) = handle {
                    env.handles.insert(h.clone());
                    let _ = hsp;
                }
                Some(EffectNode::Spawn {
                    prototype: CardId::from(card.1.clone()),
                    count: count
                        .as_ref()
                        .map(|c| Expr::Lit(c.1 as i64))
                        .unwrap_or(Expr::Lit(1)),
                    into,
                    handle: handle.as_ref().map(|h| h.1.clone()),
                })
            }
            ast::AEffect::Destroy { target, count, .. } => {
                let target = self.lower_target(target, env)?;
                let count = match count {
                    Some((_, ast::DestroyCountAst::All)) => DestroyCount::All,
                    Some((_, ast::DestroyCountAst::N(n))) => {
                        DestroyCount::AtMost(Expr::Lit(*n as i64))
                    }
                    None => DestroyCount::All,
                };
                Some(EffectNode::Destroy { target, count })
            }
            ast::AEffect::Consume { target, count, .. } => {
                let target = self.lower_target(target, env)?;
                Some(EffectNode::Consume {
                    target,
                    count: count
                        .as_ref()
                        .map(|c| Expr::Lit(c.1 as i64))
                        .unwrap_or(Expr::Lit(1)),
                })
            }
            ast::AEffect::Swap {
                target,
                count,
                to,
                into,
                ..
            } => {
                let target = self.lower_target(target, env)?;
                if !self.check_card_exists(&to.1, &to.0) {
                    return None;
                }
                let into = match into {
                    Some(i) => Some(self.lower_into(i)?),
                    None => None,
                };
                Some(EffectNode::Swap {
                    target,
                    count: count
                        .as_ref()
                        .map(|c| Expr::Lit(c.1 as i64))
                        .unwrap_or(Expr::Lit(1)),
                    to: CardId::from(to.1.clone()),
                    into,
                })
            }
            ast::AEffect::Move { target, into, .. } => {
                let target = self.lower_target(target, env)?;
                let into = self.lower_into(into)?;
                Some(EffectNode::Move { target, into })
            }
            ast::AEffect::Set {
                target,
                key,
                op,
                expr,
                unclamped,
                ..
            } => {
                let target = self.lower_target(target, env)?;
                if !self.check_aspect_key(&key.1, &key.0) {
                    return None;
                }
                let expr = self.lower_arith(expr, env)?;
                Some(EffectNode::Set {
                    target,
                    key: key.1.clone(),
                    op: set_op(op.1),
                    expr,
                    unclamped: unclamped.is_some(),
                })
            }
            ast::AEffect::Legacy { key, op, expr, .. } => {
                if !self.check_legacy_key(&key.1, &key.0) {
                    return None;
                }
                let expr = self.lower_arith(expr, env)?;
                Some(EffectNode::Legacy {
                    key: key.1.clone(),
                    op: set_op(op.1),
                    expr,
                })
            }
            ast::AEffect::Narrate { text, .. } => {
                crate::token_check::check(self, &text.1, &text.0, env);
                Some(EffectNode::Narrate {
                    template: text.1.clone(),
                })
            }
            ast::AEffect::Roll {
                count,
                sides,
                bonus,
                bind,
                ..
            } => {
                let count = self.lower_arith(count, env)?;
                let sides = self.lower_arith(sides, env)?;
                let bonus = match bonus {
                    Some(b) => self.lower_arith(b, env)?,
                    None => Expr::Lit(0),
                };
                env.rolls.insert(bind.1.clone());
                Some(EffectNode::Roll {
                    count,
                    sides,
                    bonus,
                    bind: bind.1.clone(),
                })
            }
            ast::AEffect::Draw {
                deck, count, into, ..
            } => {
                if !self.decks.contains_key(&deck.1) {
                    self.err(
                        deck.0
                            .error(code::E244, format!("unknown deck `{}`", deck.1)),
                    );
                    return None;
                }
                let into = match into {
                    Some(i) => Some(self.lower_into(i)?),
                    None => Some(self.default_into(env)),
                };
                Some(EffectNode::Draw {
                    deck: DeckId::from(deck.1.clone()),
                    count: count
                        .as_ref()
                        .map(|c| Expr::Lit(c.1 as i64))
                        .unwrap_or(Expr::Lit(1)),
                    into,
                })
            }
            ast::AEffect::Start {
                verb, via, count, ..
            } => {
                if !self.verbs.contains_key(&verb.1) {
                    self.err(
                        verb.0
                            .error(code::E244, format!("unknown verb `{}`", verb.1)),
                    );
                    return None;
                }
                let via = match via {
                    Some((sp, r)) => match self.resolve_via(&verb.1, r, sp) {
                        Some(id) => Some(id),
                        None => return None,
                    },
                    None => None,
                };
                Some(EffectNode::Start {
                    verb: VerbId::from(verb.1.clone()),
                    via,
                    count: count
                        .as_ref()
                        .map(|c| Expr::Lit(c.1 as i64))
                        .unwrap_or(Expr::Lit(1)),
                })
            }
            ast::AEffect::Signal { event, .. } => {
                if !self.events.contains_key(&event.1) {
                    self.err(event.0.error(
                        code::E243,
                        format!(
                            "no `on event` book declares `{}`; signal is an event book's only ignition",
                            event.1
                        ),
                    ));
                    return None;
                }
                Some(EffectNode::Signal {
                    event: event.1.clone(),
                })
            }
            ast::AEffect::Ending { id, .. } => {
                if !self.endings.contains_key(&id.1) {
                    self.err(id.0.error(code::E244, format!("unknown ending `{}`", id.1)));
                    return None;
                }
                Some(EffectNode::Ending {
                    id: EndingId::from(id.1.clone()),
                })
            }
            ast::AEffect::If {
                cond, then, els, ..
            } => {
                let cond = self.lower_cond(cond, env)?;
                let then = self.lower_effects(then, env);
                let els = self.lower_effects(els, env);
                Some(EffectNode::Branch { cond, then, els })
            }
            ast::AEffect::Call { .. } => unreachable!("handled by lower_effects"),
        }
    }

    fn resolve_via(&mut self, verb: &str, recipe: &str, sp: &Span) -> Option<RecipeId> {
        let local_ok = self
            .verbs
            .get(verb)
            .is_some_and(|v| v.recipes.contains(recipe));
        if local_ok {
            return Some(RecipeId::from(format!("{verb}.{recipe}")));
        }
        if let Some((v2, r2)) = recipe.split_once('.') {
            if v2 == verb && self.verbs.get(v2).is_some_and(|v| v.recipes.contains(r2)) {
                return Some(RecipeId::from(recipe.to_string()));
            }
        }
        self.err(sp.error(
            code::E244,
            format!("unknown recipe `{recipe}` within verb `{verb}`"),
        ));
        None
    }

    fn lower_into(&mut self, i: &ast::AInto) -> Option<IntoZone> {
        match i {
            ast::AInto::Interior(_) => Some(IntoZone::Interior),
            ast::AInto::Id(sp, id) => {
                if self.zones.contains_key(id) {
                    Some(IntoZone::Zone(id.clone()))
                } else if self.cards.contains_key(id) {
                    Some(IntoZone::Carrier(CardId::from(id.clone())))
                } else {
                    self.err(sp.error(
                        code::E244,
                        format!("`{id}` is neither a declared zone nor a declared card"),
                    ));
                    None
                }
            }
        }
    }

    fn lower_target(&mut self, t: &ATarget, env: &mut Env) -> Option<Target> {
        match t {
            ATarget::Name(sp, (nsp, n)) => {
                if env.handles.contains(n) {
                    return Some(Target::Handle(n.clone()));
                }
                if self.cards.contains_key(n) {
                    if self.check_card_unique(n, nsp) {
                        return Some(Target::Name {
                            card: CardId::from(n.clone()),
                            aspect: None,
                        });
                    }
                    return None;
                }
                // Not a declared card: the only reading is a spawn handle.
                self.err(
                    nsp.error(
                        code::E238,
                        format!(
                            "handle target `{n}` has no enclosing `spawn ... as {n}` in this effect list"
                        ),
                    ),
                );
                let _ = sp;
                None
            }
            ATarget::NamePath { card, aspect, .. } => {
                if env.subject && card.1 == "subject" {
                    if !self.check_aspect_key(&aspect.1, &aspect.0) {
                        return None;
                    }
                    return Some(Target::Handle("subject".to_string()));
                }
                if !self.check_card_unique(&card.1, &card.0) {
                    return None;
                }
                Some(Target::Name {
                    card: CardId::from(card.1.clone()),
                    aspect: Some(aspect.1.clone()),
                })
            }
            ATarget::Slot(_, (_, n)) => Some(Target::Slot(SlotName::from(n.clone()))),
            ATarget::Query { scope, terms, .. } => {
                let scope = self.lower_scope(scope)?;
                let terms = self.lower_query(terms, env);
                Some(Target::Query { scope, terms })
            }
            ATarget::Bare { terms, .. } => {
                let terms = self.lower_query(terms, env);
                Some(Target::BareQuery(terms))
            }
        }
    }

    /// Compile-time effectset expansion (language.md §6): the body splices in
    /// with each parameter bound to the argument, checked at expansion; a
    /// body that reaches itself or exceeds depth 16 is [E235].
    fn expand_effectset(
        &mut self,
        span: &Span,
        name: &(Span, String),
        args: &[ArgAst],
        env: &mut Env,
    ) -> Option<Vec<EffectNode>> {
        let n = &name.1;
        let reg = match self.sets.get(n) {
            Some(r) => r.clone(),
            None => {
                self.err(name.0.error(
                    code::E233,
                    format!("unknown effectset `{n}` (no macro of that name is declared)"),
                ));
                return None;
            }
        };
        if env.es_stack.contains(n) {
            self.err(span.error(
                code::E235,
                format!("effectset `{n}` expansion reaches itself"),
            ));
            return None;
        }
        if env.es_depth >= INLINE_DEPTH_CAP {
            self.err(span.error(
                code::E235,
                format!("effectset expansion exceeds the depth cap of {INLINE_DEPTH_CAP}"),
            ));
            return None;
        }
        if reg.body.is_none() {
            return None;
        }
        if args.len() != reg.params.len() {
            self.err(span.error(
                code::E234,
                format!(
                    "effectset `{n}` expects {} argument(s), got {}",
                    reg.params.len(),
                    args.len()
                ),
            ));
            return None;
        }
        // The body splices in with each parameter replaced by the argument's
        // token stream, and the result parses as ordinary effects; the
        // declaration-time AST only fixes the parameter typing.
        let mut lines = reg.lines.clone();
        for ((psp, pname), arg) in reg.params.iter().zip(args) {
            let (card_use, int_use) = reg.usage.get(pname).copied().unwrap_or((false, false));
            if card_use && int_use {
                self.err(
                    psp.error(
                        code::E245,
                        format!(
                            "parameter `{pname}` of `{n}` is used both as a card reference and as an integer expression"
                        ),
                    ),
                );
                return None;
            }
            let splice: Vec<(crate::span::Span, Tok)> = if card_use {
                match arg.toks.as_slice() {
                    [(sp, Tok::Ident(w))] => {
                        if !self.check_card_unique(w, sp) {
                            return None;
                        }
                        arg.toks.clone()
                    }
                    _ => {
                        self.err(arg.span.error(
                            code::E245,
                            format!("argument for `{pname}` of `{n}` must be a card name"),
                        ));
                        return None;
                    }
                }
            } else if int_use {
                let mut tc = TC::new(&arg.toks);
                match parse_arith(&mut tc, &mut self.diags, false) {
                    Some(a) => {
                        if self.lower_arith(&a, env).is_none() {
                            return None;
                        }
                        arg.toks.clone()
                    }
                    None => return None,
                }
            } else {
                vec![(psp.clone(), Tok::Int(0))]
            };
            substitute_param(&mut lines, pname, &splice);
        }
        let base_indent = lines
            .first()
            .map(|l| l.indent.saturating_sub(1))
            .unwrap_or(0);
        let expanded = {
            let mut sub = Parser::new(span.file.clone(), lines, &mut self.diags);
            sub.parse_effect_lines(base_indent)
        };
        env.es_depth += 1;
        env.es_stack.push(n.clone());
        let out = self.lower_effects(&expanded, env);
        env.es_stack.pop();
        env.es_depth -= 1;
        Some(out)
    }
}

fn substitute_param(lines: &mut [Line], param: &str, with: &[(crate::span::Span, Tok)]) {
    for line in lines {
        let mut new_toks = Vec::with_capacity(line.toks.len());
        for (sp, t) in &line.toks {
            match t {
                Tok::Ident(w) if w == param => new_toks.extend(with.iter().cloned()),
                other => new_toks.push((sp.clone(), other.clone())),
            }
        }
        line.toks = new_toks;
    }
}

fn set_op(o: ast::ASetOp) -> ir::SetOp {
    match o {
        ast::ASetOp::Assign => ir::SetOp::Assign,
        ast::ASetOp::AddAssign => ir::SetOp::Add,
        ast::ASetOp::SubAssign => ir::SetOp::Sub,
    }
}
