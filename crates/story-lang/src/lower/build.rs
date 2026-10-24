//! Pass two: resolve references and lower each declaration into the validated `Content`.

use super::{Compiler, Env};
use crate::ast;
use crate::span::Span;
use crate::token_check;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use story_content::code;
use story_content::ir::{
    AspectDecl, AspectKey, Book, BootFace, CardDecl, DeckDecl, EndingDecl, Expr, GameHeader, Guard,
    InstanceCap, LegacyDecl, RelationKindDecl, Rule, SeedSpec, SlotDecl, VerbDecl, VerbMode,
    ZoneDecl,
};
use story_core::{CardId, MintPolicy, TriggerRuleId};

impl Compiler {
    pub(super) fn build(&mut self, items: Vec<ast::TopItem>) {
        // Block order is free, but effect defaults (spawn/draw `into`) read
        // the game header; build `game` first. The header carries no
        // declaration index, so the move does not disturb ordering.
        let mut rest = Vec::new();
        let mut games = Vec::new();
        for item in items {
            match item {
                ast::TopItem::Game(g) => games.push(g),
                other => rest.push(other),
            }
        }
        for g in games {
            self.build_game(g);
        }
        for item in rest {
            match item {
                ast::TopItem::Game(_) => {}
                ast::TopItem::Zones(zs) => {
                    for z in zs {
                        if let Some((_, parent)) = &z.parent {
                            if !self.check_zone(parent, &z.span) {
                                continue;
                            }
                        }
                        self.zone_order.push(z.name.clone());
                        let meta = self.meta_map(&z.meta);
                        let decl_index = self.next_index();
                        self.content.zones.insert(
                            z.name.clone(),
                            ZoneDecl {
                                name: z.name,
                                parent: z.parent.map(|p| p.1),
                                meta,
                                decl_index,
                            },
                        );
                    }
                }
                ast::TopItem::Aspects(as_) => {
                    for a in as_ {
                        let meta = self.meta_map(&a.meta);
                        let span = self
                            .aspects
                            .get(&a.key)
                            .map(|(_, sp)| sp.clone())
                            .unwrap_or_else(|| a.span.clone());
                        self.aspects.insert(a.key.clone(), (a.value, span));
                        self.content.aspects.insert(
                            a.key.clone(),
                            AspectDecl {
                                key: a.key.clone(),
                                default: a.value,
                                meta,
                            },
                        );
                    }
                }
                ast::TopItem::Legacies(ls) => {
                    for l in ls {
                        let meta = self.meta_map(&l.meta);
                        self.content.legacies.insert(
                            l.key.clone(),
                            LegacyDecl {
                                key: l.key.clone(),
                                default: l.value,
                                meta,
                            },
                        );
                    }
                }
                ast::TopItem::Relations(rs) => {
                    for r in rs {
                        let meta = self.meta_map(&r.meta);
                        let decl_index = self.next_index();
                        self.content.relation_kinds.insert(
                            r.name.1.clone(),
                            RelationKindDecl {
                                name: r.name.1.clone(),
                                symmetric: r.symmetric.is_some(),
                                exclusive: r.exclusive.is_some(),
                                meta,
                                decl_index,
                            },
                        );
                    }
                }
                ast::TopItem::Cards(cs) => {
                    for c in cs {
                        self.build_card(c);
                    }
                }
                ast::TopItem::Predicates(ps) => {
                    for p in ps {
                        self.check_predicate_decl(&p);
                    }
                }
                ast::TopItem::Decks(ds) => {
                    for d in ds {
                        self.build_deck(d);
                    }
                }
                ast::TopItem::Endings(es) => {
                    for e in es {
                        self.build_ending(e);
                    }
                }
                ast::TopItem::Events(evs) => {
                    for e in evs {
                        self.build_event_book(e);
                    }
                }
                ast::TopItem::Verbs(vs) => {
                    for v in vs {
                        self.build_verb(v);
                    }
                }
                ast::TopItem::Effectsets(es) => {
                    // The expanded database stores no effectset declaration,
                    // so its meta has nowhere to land; the [E236] repeat
                    // check still applies.
                    for e in es {
                        let meta = self.meta_map(&e.meta);
                        let _ = meta;
                    }
                }
                ast::TopItem::Include(_) => {}
            }
        }
    }

    fn build_game(&mut self, g: ast::GameAst) {
        if self.content.game.is_some() {
            return;
        }
        let seed = match &g.seed {
            Some(ast::SeedAst::Fixed((_, n))) => SeedSpec::Fixed(*n),
            Some(ast::SeedAst::Host(_)) | None => SeedSpec::Host,
        };
        let turn_minutes = g.turn.as_ref().map(|t| t.1).unwrap_or(1);
        if turn_minutes <= 0 {
            let sp = g
                .turn
                .as_ref()
                .map(|t| t.0.clone())
                .unwrap_or_else(|| g.span.clone());
            self.err(sp.error(code::E246, "`turn` must be a positive duration"));
        }
        let game = GameHeader {
            title: g.title.1,
            seed,
            turn: turn_minutes,
            home: g.home.clone().map(|h| h.1).unwrap_or_default(),
            meta: self.meta_map(&g.meta),
        };
        self.content.game = Some(game);
    }

    /// Called after the build walk: resolve the default `home` and validate it.
    pub(crate) fn finish(&mut self) {
        if let Some(game) = self.content.game.as_mut() {
            if game.home.is_empty() {
                game.home = self.zone_order.first().cloned().unwrap_or_default();
            } else {
                let home = game.home.clone();
                if !self.zones.contains_key(&home) {
                    let sp = self
                        .first_game_span
                        .clone()
                        .unwrap_or_else(|| Span::new(Arc::from("<content>"), 1, 1));
                    self.err(sp.error(
                        code::E244,
                        format!("unknown zone `{home}` (the game `home`)"),
                    ));
                }
            }
        }
    }

    fn build_card(&mut self, c: ast::CardAst) {
        let meta = self.meta_map(&c.meta);
        let mut decl = CardDecl {
            id: CardId::from(c.name.1.clone()),
            kind: c.kind.clone().map(|k| k.1),
            mint: MintPolicy::SingleCopy,
            aspects: BTreeMap::new(),
            lifetime: None,
            boot: None,
            relates: Vec::new(),
            desc: None,
            mint_alias: None,
            on_complete: Vec::new(),
            on_expire: None,
            meta,
            decl_index: self.next_index(),
        };
        // [E221]: `unique` together with `stackable` or `group`.
        let has_unique = c.uniq.iter().any(|u| matches!(u, ast::UniqAst::Unique(_)));
        let has_other = c
            .uniq
            .iter()
            .any(|u| matches!(u, ast::UniqAst::Stackable(_) | ast::UniqAst::Group(..)));
        if has_unique && has_other {
            self.err(c.span.error(
                code::E221,
                format!(
                    "card `{}` declares `unique` together with `stackable`/`group`; exactly one uniqueness mode is allowed",
                    c.name.1
                ),
            ));
        }
        for u in &c.uniq {
            match u {
                ast::UniqAst::Stackable(_) => decl.mint = MintPolicy::Stackable,
                ast::UniqAst::Unique(_) => decl.mint = MintPolicy::Unique,
                ast::UniqAst::Group(_, (_, g)) => decl.mint = MintPolicy::Group(g.clone()),
            }
        }
        if let Some(life) = c.lifetime {
            if life.1 <= 0 {
                self.err(
                    life.0
                        .error(code::E246, "card lifetimes are positive minute values"),
                );
            } else {
                decl.lifetime = Some(life.1);
            }
        }
        for (sp, key, value) in &c.aspects {
            if self.check_aspect_key(key, sp) {
                decl.aspects.insert(key.clone(), *value);
            }
        }
        if let Some(b) = &c.boot {
            if self.check_zone(&b.zone.1, &b.zone.0) {
                decl.boot = Some(BootFace {
                    zone: b.zone.1.clone(),
                    count: b.count.as_ref().map(|n| n.1).unwrap_or(1),
                });
            }
        }
        // Boot edges: every named object card must exist [E244]; the kind
        // must be declared [E248]. Missing live instances are a boot warning,
        // not a compile error.
        for (kind, objects) in &c.relates {
            if !self.check_relation_kind(&kind.1, &kind.0) {
                continue;
            }
            let mut resolved = Vec::new();
            for (osp, name) in objects {
                if self.check_card_exists(name, osp) {
                    resolved.push(CardId::from(name.clone()));
                }
            }
            if !resolved.is_empty() {
                decl.relates.push((kind.1.clone(), resolved));
            }
        }
        if let Some((sp, text)) = &c.desc {
            token_check::check(self, text, sp, &Env::default());
            decl.desc = Some(text.clone());
        }
        if let Some((sp, alias)) = &c.mint {
            if self.check_card_exists(alias, sp) {
                decl.mint_alias = Some(CardId::from(alias.clone()));
            }
        }
        let mut seen_quals: BTreeSet<Option<AspectKey>> = BTreeSet::new();
        for (sp, qual, rules) in &c.completes {
            let qual_key = qual.as_ref().map(|q| q.1.clone());
            if seen_quals.contains(&qual_key) {
                self.err(sp.error(code::E236, "repeated `on complete` qualifier on one card"));
                continue;
            }
            seen_quals.insert(qual_key.clone());
            if let Some((qsp, q)) = qual {
                self.check_aspect_key(q, qsp);
            }
            let book =
                self.build_book(rules, &format!("{}.on-complete", c.name.1), true, true, &[]);
            decl.on_complete.push((qual_key, book));
        }
        if let Some((_, rules)) = &c.expire {
            let book = self.build_book(rules, &format!("{}.on-expire", c.name.1), false, true, &[]);
            decl.on_expire = Some(book);
        }
        self.content.cards.insert(decl.id.clone(), decl);
    }

    /// Build a trigger book. `situation` marks contexts with a running
    /// situation (recipe and card completion books); `subject` marks books
    /// whose rules address the carrying or dying card.
    pub(crate) fn build_book(
        &mut self,
        rules: &[ast::RuleAst],
        site: &str,
        situation: bool,
        subject: bool,
        slots: &[String],
    ) -> Book {
        let mut book = Book::default();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for r in rules {
            if !seen.insert(r.name.1.clone()) {
                let d = r.name.0.error(
                    code::E232,
                    format!("duplicate rule id `{}` in one book", r.name.1),
                );
                self.err(d);
                continue;
            }
            let mut env = Env {
                subject,
                situation,
                slots: slots.to_vec(),
                ..Env::default()
            };
            let when = r
                .when
                .as_ref()
                .and_then(|(_sp, c)| self.lower_cond(c, &mut env));
            let effects = r
                .effects
                .as_ref()
                .map(|(_, list)| self.lower_effects(list, &mut env))
                .unwrap_or_default();
            book.rules.push(Rule {
                id: TriggerRuleId::from(format!("{site}.{}", r.name.1)),
                priority: r.priority.as_ref().map(|p| p.1).unwrap_or(100),
                once: r.once.is_some(),
                when,
                effects,
                meta: self.meta_map(&r.meta),
                decl_index: self.next_index(),
            });
        }
        book
    }

    fn build_deck(&mut self, d: ast::DeckAst) {
        let meta = self.meta_map(&d.meta);
        let mut entries = Vec::new();
        for (sp, name) in &d.entries {
            if self.check_card_exists(name, sp) {
                entries.push(CardId::from(name.clone()));
            }
        }
        let fallback = d.fallback.as_ref().and_then(|f| {
            if self.check_card_exists(&f.1, &f.0) {
                Some(CardId::from(f.1.clone()))
            } else {
                None
            }
        });
        let decl = DeckDecl {
            id: story_core::DeckId::from(d.name.1.clone()),
            entries,
            fallback,
            wrap: d.wrap,
            meta,
            decl_index: self.next_index(),
        };
        self.content.decks.insert(d.name.1.clone(), decl);
    }

    fn build_ending(&mut self, e: ast::EndingAst) {
        let meta = self.meta_map(&e.meta);
        let text = e.text.as_ref().map(|(sp, t)| {
            token_check::check(self, t, sp, &Env::default());
            t.clone()
        });
        let decl = EndingDecl {
            id: story_core::EndingId::from(e.name.1.clone()),
            title: e.title.1,
            tier: e.tier.1,
            text,
            meta,
            decl_index: self.next_index(),
        };
        self.content.endings.insert(e.name.1.clone(), decl);
    }

    fn build_event_book(&mut self, e: ast::EventBookAst) {
        let book = self.build_book(
            &e.rules,
            &format!("on-event.{}", e.name.1),
            false,
            false,
            &[],
        );
        self.content.events.insert(e.name.1, book);
    }

    fn build_verb(&mut self, v: ast::VerbAst) {
        let meta = self.meta_map(&v.meta);
        let mode = match v.mode.1 {
            ast::ModeAst::Committable => VerbMode::Committable,
            ast::ModeAst::Autonomous => VerbMode::Autonomous,
            ast::ModeAst::Uncommittable => VerbMode::Uncommittable,
        };
        let instances = match &v.instances {
            Some(ast::InstancesAst::Any(_)) => InstanceCap::Any,
            Some(ast::InstancesAst::N(_, n)) => InstanceCap::AtMost(*n),
            None => InstanceCap::AtMost(1),
        };
        let mut decl = VerbDecl {
            id: story_core::VerbId::from(v.name.1.clone()),
            mode,
            instances,
            title: v.title.clone().map(|t| t.1),
            releases: v.releases.as_ref().map(|z| z.1.clone()),
            recipes: BTreeMap::new(),
            recipe_order: Vec::new(),
            meta,
            decl_index: self.next_index(),
        };
        if let Some((_, z)) = &v.releases {
            self.check_zone(z, &v.span);
        }
        let mut built: Vec<(String, Span, i64, bool, Option<Expr>)> = Vec::new();
        for r in &v.recipes {
            let slot_names: Vec<String> = r.slots.iter().map(|s| s.name.1.clone()).collect();
            let (mut craftable, mut chain_only) = match mode {
                VerbMode::Committable => (true, false),
                VerbMode::Autonomous => (false, false),
                VerbMode::Uncommittable => (false, true),
            };
            if r.craftable.is_some() && r.chain_only.is_some() {
                self.err(r.span.error(
                    code::E222,
                    format!(
                        "recipe `{}` declares `craftable` together with `chain-only`",
                        r.name.1
                    ),
                ));
            } else if r.chain_only.is_some() {
                craftable = false;
                chain_only = true;
            } else if r.craftable.is_some() {
                craftable = true;
                chain_only = false;
            }
            let duration = match r.duration {
                Some((_, m)) => m,
                None => {
                    self.err(r.span.error(
                        code::E237,
                        format!(
                            "recipe `{}.{}` has no `duration` line; every recipe declares its own",
                            v.name.1, r.name.1
                        ),
                    ));
                    0
                }
            };
            let mut env = Env {
                situation: true,
                in_recipe: true,
                verb: Some(v.name.1.clone()),
                slots: slot_names.clone(),
                ..Env::default()
            };
            let when = r
                .when
                .as_ref()
                .and_then(|(_, c)| self.lower_cond(c, &mut env));
            let mut guards = Vec::new();
            for g in &r.guards {
                token_check::check(self, &g.prose.1, &g.prose.0, &env);
                if let Some(cond) = self.lower_cond(&g.when, &mut env) {
                    guards.push(Guard {
                        prose: g.prose.1.clone(),
                        when: cond,
                    });
                }
            }
            let mut slots = Vec::new();
            for s in &r.slots {
                if let Some(sd) = self.build_slot(s) {
                    slots.push(sd);
                }
            }
            let effects = r
                .effects
                .as_ref()
                .map(|(_, list)| self.lower_effects(list, &mut env))
                .unwrap_or_default();
            let on_complete = r.complete.as_ref().map(|(_, rules)| {
                self.build_book(
                    rules,
                    &format!("{}.{}.on-complete", v.name.1, r.name.1),
                    true,
                    false,
                    &slot_names,
                )
            });
            for (sp, key) in &r.declares {
                self.check_aspect_key(key, sp);
            }
            let recipe = story_content::ir::RecipeDecl {
                verb: decl.id.clone(),
                name: r.name.1.clone(),
                id: story_core::RecipeId::from(format!("{}.{}", v.name.1, r.name.1)),
                label: r.label.clone().map(|l| l.1),
                priority: r.priority.as_ref().map(|p| p.1).unwrap_or(100),
                craftable,
                chain_only,
                max: r.max.as_ref().map(|m| m.1),
                duration,
                declares: r.declares.iter().map(|(_, k)| k.clone()).collect(),
                when,
                guards,
                slots,
                effects,
                on_complete,
                allow_shadow: r.allow_shadow.is_some(),
                meta: self.meta_map(&r.meta),
                decl_index: self.next_index(),
            };
            built.push((
                r.name.1.clone(),
                r.span.clone(),
                recipe.priority,
                recipe.allow_shadow,
                recipe.when.clone(),
            ));
            decl.recipe_order.push(r.name.1.clone());
            decl.recipes.insert(r.name.1.clone(), recipe);
        }
        // [W110]: two recipes of one verb share a priority and an identical
        // (or missing) `when` — the same input always picks the same recipe,
        // so the second shadows the first. Disjoint `when`s never shadow.
        for i in 1..built.len() {
            let (name_b, sp_b, prio_b, allow_b, when_b) = &built[i];
            if *allow_b {
                continue;
            }
            if let Some((name_a, _, _, _, _)) = built
                .iter()
                .take(i)
                .find(|(_, _, p, _, w)| p == prio_b && when_shadows(w, when_b))
                .cloned()
            {
                let d = sp_b.warning(
                    code::W110,
                    format!(
                        "recipes `{name_a}` and `{name_b}` share priority {prio_b} and an identical `when`; the first declaration wins under the selection law"
                    ),
                );
                self.err(d.with_note(
                    "write `allow shadow` on the shadowed recipe to suppress this warning",
                ));
            }
        }
        self.content.verbs.insert(v.name.1.clone(), decl);
    }

    fn build_slot(&mut self, s: &ast::SlotAst) -> Option<SlotDecl> {
        let (min, max) = match &s.range {
            ast::RangeAst::Exact(_, n) => (*n, Some(*n)),
            ast::RangeAst::Bounded(_, lo, hi) => {
                if lo > hi {
                    self.err(s.span.error(
                        code::E240,
                        format!(
                            "slot `{}` range minimum {lo} exceeds maximum {hi}",
                            s.name.1
                        ),
                    ));
                }
                (*lo, Some(*hi))
            }
            ast::RangeAst::Open(_, lo) => (*lo, None),
        };
        let mut greedy = false;
        let mut consuming = false;
        let mut open = false;
        let mut take = false;
        for (sp, f) in &s.flags {
            match f {
                ast::SlotFlagAst::Greedy => greedy = true,
                ast::SlotFlagAst::Consuming => consuming = true,
                ast::SlotFlagAst::Open => open = true,
                ast::SlotFlagAst::Take => take = true,
            }
            let _ = sp;
        }
        if greedy && max.is_none() {
            self.err(s.span.error(
                code::E241,
                format!(
                    "greedy slot `{}` has no finite maximum; greedy requires a bounded range",
                    s.name.1
                ),
            ));
        }
        let mut env = Env::default();
        let query = self.lower_query(&s.query, &mut env);
        Some(SlotDecl {
            name: story_core::SlotName::from(s.name.1.clone()),
            min,
            max,
            greedy,
            consuming,
            open,
            take,
            query,
            decl_index: self.next_index(),
        })
    }
}

/// [W110] narrowing: a missing `when` is always-true, so it shadows anything;
/// otherwise the two lowered conditions must be syntactically equal.
fn when_shadows(a: &Option<Expr>, b: &Option<Expr>) -> bool {
    match (a, b) {
        (None, _) | (_, None) => true,
        (Some(x), Some(y)) => x == y,
    }
}
