//! Pass one: collect declarations into the one namespace and compute parameter usage.

use super::{CardReg, Compiler, PredReg, SetReg, VerbReg};
use crate::ast::{self, TopItem};
use std::collections::{BTreeMap, BTreeSet};
use story_content::code;

impl Compiler {
    pub(super) fn register(&mut self, items: &[TopItem]) {
        for item in items {
            match item {
                TopItem::Game(g) => {
                    if self.saw_game {
                        let d = g.span.error(code::E232, "duplicate `game` header");
                        self.err(d.with_note(format!(
                            "first declared at {}:{}",
                            self.first_game_span.as_ref().unwrap().file,
                            self.first_game_span.as_ref().unwrap().line
                        )));
                    } else {
                        self.saw_game = true;
                        self.first_game_span = Some(g.span.clone());
                    }
                }
                TopItem::Zones(zs) => {
                    for z in zs {
                        self.claim(&z.name, &z.span, "zone");
                        self.zones.insert(z.name.clone(), z.span.clone());
                    }
                }
                TopItem::Aspects(as_) => {
                    for a in as_ {
                        self.claim_aspect(&a.key, &a.span);
                    }
                }
                TopItem::Legacies(ls) => {
                    for l in ls {
                        self.claim_legacy(&l.key, &l.span);
                    }
                }
                TopItem::Relations(rs) => {
                    for r in rs {
                        if let Some(first) = self.relation_kinds.get(&r.name.1) {
                            let d = r.name.0.error(
                                code::E232,
                                format!("duplicate relation kind `{}`", r.name.1),
                            );
                            self.err(d.with_note(format!(
                                "first declared at {}:{}",
                                first.file, first.line
                            )));
                        } else {
                            self.relation_kinds.insert(r.name.1.clone(), r.span.clone());
                        }
                    }
                }
                TopItem::Cards(cs) => {
                    for c in cs {
                        self.claim(&c.name.1, &c.span, "card");
                        if let Some((_, k)) = &c.kind {
                            self.kinds.insert(k.clone());
                        }
                        self.cards.insert(
                            c.name.1.clone(),
                            CardReg {
                                unique: c.uniq.iter().any(|u| matches!(u, ast::UniqAst::Unique(_))),
                            },
                        );
                    }
                }
                TopItem::Decks(ds) => {
                    for d in ds {
                        self.claim(&d.name.1, &d.span, "deck");
                        self.decks.insert(d.name.1.clone(), d.span.clone());
                    }
                }
                TopItem::Endings(es) => {
                    for e in es {
                        self.claim(&e.name.1, &e.span, "ending");
                        self.endings.insert(e.name.1.clone(), e.span.clone());
                    }
                }
                TopItem::Predicates(ps) => {
                    for p in ps {
                        self.claim(&p.name.1, &p.span, "predicate");
                        let usage = BTreeMap::new();
                        self.preds.insert(
                            p.name.1.clone(),
                            PredReg {
                                params: p.params.clone(),
                                body: p.body.clone(),
                                usage,
                                span: p.span.clone(),
                                cycle_checked: false,
                            },
                        );
                    }
                }
                TopItem::Effectsets(es) => {
                    for e in es {
                        self.claim(&e.name.1, &e.span, "effectset");
                        let usage = BTreeMap::new();
                        self.sets.insert(
                            e.name.1.clone(),
                            SetReg {
                                params: e.params.clone(),
                                body: e.body.clone(),
                                usage,
                                lines: e.body_lines.clone(),
                            },
                        );
                    }
                }
                TopItem::Events(evs) => {
                    for e in evs {
                        self.claim(&e.name.1, &e.span, "event");
                        self.events.insert(e.name.1.clone(), e.span.clone());
                    }
                }
                TopItem::Verbs(vs) => {
                    for v in vs {
                        self.claim(&v.name.1, &v.span, "verb");
                        let mut recipes = BTreeSet::new();
                        for r in &v.recipes {
                            if !recipes.insert(r.name.1.clone()) {
                                let d = r.name.0.error(
                                    code::E232,
                                    format!(
                                        "duplicate recipe id `{}` within verb `{}`",
                                        r.name.1, v.name.1
                                    ),
                                );
                                self.err(d);
                            }
                        }
                        self.verbs.insert(v.name.1.clone(), VerbReg { recipes });
                    }
                }
                TopItem::Include(_) => {}
            }
        }
        // Parameter usage: computed once every declaration is registered, so
        // a pass-through argument inherits the callee's declared typing
        // regardless of declaration order.
        let pred_names: Vec<String> = self.preds.keys().cloned().collect();
        for n in pred_names {
            let usage = self.pred_usage_of(&n, &mut Vec::new());
            if let Some(reg) = self.preds.get_mut(&n) {
                reg.usage = usage;
            }
        }
        let set_names: Vec<String> = self.sets.keys().cloned().collect();
        for n in set_names {
            let usage = self.set_usage_of(&n, &mut Vec::new());
            if let Some(reg) = self.sets.get_mut(&n) {
                reg.usage = usage;
            }
        }
    }
}
