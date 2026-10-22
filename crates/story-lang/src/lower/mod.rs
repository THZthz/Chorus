//! The two-pass compiler: pass one collects declarations into the one
//! namespace (duplicates reported at the second site, both named); pass two
//! resolves references, inlines predicates and effectsets, and lowers the AST
//! into the validated `Content` (language.md §1, spec.md §5).

pub mod effect;
pub mod expr;

use crate::ast::{self, AEffect, MetaLine, TopItem};
use crate::includes;
use crate::lexer;
use crate::parser::Parser;
use crate::span::Span;
use crate::token_check;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;
use story_content::code;
use story_content::ir::{
    AspectDecl, AspectKey, Book, BootFace, CardDecl, DeckDecl, EndingDecl, Expr, GameHeader, Guard,
    InstanceCap, LegacyDecl, LegacyKey, Meta, RelationKindDecl, Rule, SeedSpec, SlotDecl, VerbDecl,
    VerbMode, ZoneDecl,
};
use story_content::{Content, Diagnostic};
use story_core::{CardId, MintPolicy, TriggerRuleId};

pub struct Output {
    pub content: Option<Content>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn compile_file(path: &Path) -> Output {
    let mut diags = Vec::new();
    let items = includes::load(path, &mut diags);
    run(items, diags)
}

pub fn compile_source(file: &str, src: &str) -> Output {
    let file: Arc<str> = Arc::from(file);
    let mut diags = Vec::new();
    let lines = lexer::lex(&file, src, &mut diags);
    let items = {
        let mut p = Parser::new(file, lines, &mut diags);
        p.parse_file()
    };
    run(items, diags)
}

fn run(items: Vec<TopItem>, mut diags: Vec<Diagnostic>) -> Output {
    let mut c = Compiler::new();
    c.register(&items);
    c.build(items);
    c.finish();
    diags.append(&mut c.diags);
    let mut has_errors = diags.iter().any(|d| d.is_error());
    if c.content.game.is_none() && !c.saw_game {
        diags.push(Diagnostic::error(
            code::E201,
            "<content>",
            1,
            1,
            "missing `game` header",
        ));
        has_errors = true;
    }
    Output {
        content: if has_errors { None } else { Some(c.content) },
        diagnostics: diags,
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CardReg {
    pub unique: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct PredReg {
    pub params: Vec<(Span, String)>,
    pub body: Option<ast::ACond>,
    /// (used as card reference, used as integer expression) per parameter.
    pub usage: BTreeMap<String, (bool, bool)>,
    pub span: Span,
    pub cycle_checked: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct SetReg {
    pub params: Vec<(Span, String)>,
    pub body: Option<(Span, Vec<AEffect>)>,
    pub usage: BTreeMap<String, (bool, bool)>,
    pub lines: Vec<crate::lexer::Line>,
}

#[derive(Clone, Debug)]
pub(crate) struct VerbReg {
    pub recipes: BTreeSet<String>,
}

/// The inline environment threaded through lowering: predicate and effectset
/// parameters, roll bindings and spawn handles visible so far, and the
/// context kind of the effect list.
#[derive(Clone, Default)]
pub(crate) struct Env {
    pub params: Vec<(String, ParamVal)>,
    pub rolls: BTreeSet<String>,
    pub handles: BTreeSet<String>,
    pub subject: bool,
    pub slots: Vec<String>,
    /// True in situation contexts (recipe and card completion books): spawn
    /// and draw default to the interior; elsewhere to `home`.
    pub situation: bool,
    /// True while lowering a recipe's own `effects` block — the only context
    /// where `continue via` is legal ([E247]); book rules clear it.
    pub in_recipe: bool,
    /// The verb whose recipe is being lowered; local recipe references in
    /// `continue via` resolve against it.
    pub verb: Option<String>,
    pub pred_depth: u32,
    pub es_depth: u32,
    pub es_stack: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) enum ParamVal {
    Card(CardId),
    Int(story_content::ir::Expr),
    /// The candidate card of a query-position predicate.
    Subject,
}

pub(crate) struct Compiler {
    pub diags: Vec<Diagnostic>,
    dedupe: HashSet<(u32, String, u32, u32, String)>,
    /// The one global id namespace (cards, verbs, decks, zones, endings,
    /// predicates, event names, effectsets).
    pub ns: BTreeMap<String, Span>,
    pub aspects: BTreeMap<AspectKey, (i64, Span)>,
    pub legacies: BTreeMap<LegacyKey, Span>,
    pub kinds: BTreeSet<String>,
    /// The relation-kind registry's own namespace (disjoint from card `kind`
    /// tags); duplicates are [E232] at the second site.
    pub relation_kinds: BTreeMap<String, Span>,
    pub cards: BTreeMap<String, CardReg>,
    pub zones: BTreeMap<String, Span>,
    pub decks: BTreeMap<String, Span>,
    pub endings: BTreeMap<String, Span>,
    pub events: BTreeMap<String, Span>,
    pub verbs: BTreeMap<String, VerbReg>,
    pub preds: BTreeMap<String, PredReg>,
    pub sets: BTreeMap<String, SetReg>,
    pub content: Content,
    pub saw_game: bool,
    pub first_game_span: Option<Span>,
    zone_order: Vec<String>,
    counter: u32,
    /// Per-predicate inlining stack, for [E230].
    pred_stack: Vec<String>,
    /// Memoized parameter-usage maps for the pass-through-aware analysis.
    pred_usage_memo: BTreeMap<String, BTreeMap<String, (bool, bool)>>,
    set_usage_memo: BTreeMap<String, BTreeMap<String, (bool, bool)>>,
}

impl Compiler {
    fn new() -> Self {
        Compiler {
            diags: Vec::new(),
            dedupe: HashSet::new(),
            ns: BTreeMap::new(),
            aspects: BTreeMap::new(),
            legacies: BTreeMap::new(),
            kinds: BTreeSet::new(),
            relation_kinds: BTreeMap::new(),
            cards: BTreeMap::new(),
            zones: BTreeMap::new(),
            decks: BTreeMap::new(),
            endings: BTreeMap::new(),
            events: BTreeMap::new(),
            verbs: BTreeMap::new(),
            preds: BTreeMap::new(),
            sets: BTreeMap::new(),
            content: Content::default(),
            saw_game: false,
            first_game_span: None,
            zone_order: Vec::new(),
            counter: 0,
            pred_stack: Vec::new(),
            pred_usage_memo: BTreeMap::new(),
            set_usage_memo: BTreeMap::new(),
        }
    }

    pub(crate) fn err(&mut self, d: Diagnostic) {
        let key = (d.number, d.file.clone(), d.line, d.col, d.message.clone());
        if self.dedupe.insert(key) {
            self.diags.push(d);
        }
    }

    pub(crate) fn next_index(&mut self) -> u32 {
        let i = self.counter;
        self.counter += 1;
        i
    }

    /// Claim an id in the global namespace; a second claim is [E232] reported
    /// at the second site with both named.
    pub(crate) fn claim(&mut self, name: &str, span: &Span, what: &str) {
        if let Some(first) = self.ns.get(name) {
            let d = span.error(
                code::E232,
                format!("duplicate id `{name}` in the one namespace ({what})"),
            );
            self.err(d.with_note(format!("first declared at {}:{}", first.file, first.line)));
        } else {
            self.ns.insert(name.to_string(), span.clone());
        }
    }

    pub(crate) fn claim_aspect(&mut self, key: &str, span: &Span) {
        if let Some((_, first)) = self.aspects.get(key) {
            let d = span.error(code::E232, format!("duplicate aspect key `{key}`"));
            self.err(d.with_note(format!("first declared at {}:{}", first.file, first.line)));
        } else {
            self.aspects.insert(key.to_string(), (0, span.clone()));
        }
    }

    pub(crate) fn claim_legacy(&mut self, key: &str, span: &Span) {
        if let Some(first) = self.legacies.get(key) {
            let d = span.error(code::E232, format!("duplicate legacy key `{key}`"));
            self.err(d.with_note(format!("first declared at {}:{}", first.file, first.line)));
        } else {
            self.legacies.insert(key.to_string(), span.clone());
        }
    }

    /// Fold meta lines into a map; a repeated key on one declaration is [E236].
    pub(crate) fn meta_map(&mut self, lines: &[MetaLine]) -> Meta {
        let mut m = Meta::new();
        for l in lines {
            if m.contains_key(&l.key) {
                self.err(l.span.error(
                    code::E236,
                    format!("meta key `{}` repeats on one declaration", l.key),
                ));
            } else {
                m.insert(l.key.clone(), l.value.clone());
            }
        }
        m
    }

    pub(crate) fn nearest_aspect(&self, key: &str) -> Option<(String, Span)> {
        let mut best: Option<(usize, String, Span)> = None;
        for (k, (_, sp)) in &self.aspects {
            let d = edit_distance(k, key);
            if best.as_ref().is_none_or(|(bd, _, _)| d < *bd) {
                best = Some((d, k.clone(), sp.clone()));
            }
        }
        best.and_then(|(d, k, sp)| if d <= 3 { Some((k, sp)) } else { None })
    }

    pub(crate) fn check_aspect_key(&mut self, key: &str, sp: &Span) -> bool {
        if self.aspects.contains_key(key) {
            true
        } else {
            let mut d = sp.error(
                code::E203,
                format!(
                    "unknown aspect key `{key}` (read in a query, condition, predicate, or effect)"
                ),
            );
            if let Some((near, nsp)) = self.nearest_aspect(key) {
                d = d.with_note(format!(
                    "nearest declared key is `{near}` (declared at {}:{})",
                    nsp.file, nsp.line
                ));
            }
            self.err(d);
            false
        }
    }

    pub(crate) fn check_kind(&mut self, kind: &str, sp: &Span) -> bool {
        if self.kinds.contains(kind) {
            true
        } else {
            self.err(
                sp.error(
                    code::E203,
                    format!("unknown card kind `{kind}` (the kind registry is collected from card `kind` lines)"),
                ),
            );
            false
        }
    }

    /// A relation kind read or written must be declared in a `relations`
    /// block [E248]. Kinds form their own registry, disjoint from card
    /// `kind` tags.
    pub(crate) fn check_relation_kind(&mut self, kind: &str, sp: &Span) -> bool {
        if self.relation_kinds.contains_key(kind) {
            true
        } else {
            self.err(sp.error(
                code::E248,
                format!(
                    "relation kind `{kind}` is read or written but is not declared (the relations registry)"
                ),
            ));
            false
        }
    }

    pub(crate) fn check_legacy_key(&mut self, key: &str, sp: &Span) -> bool {
        if self.legacies.contains_key(key) {
            true
        } else {
            self.err(sp.error(
                code::E204,
                format!("unknown legacy key `{key}` (read or written)"),
            ));
            false
        }
    }

    pub(crate) fn check_card_exists(&mut self, name: &str, sp: &Span) -> bool {
        if self.cards.contains_key(name) {
            true
        } else {
            self.err(sp.error(code::E244, format!("unknown card `{name}`")));
            false
        }
    }

    pub(crate) fn check_card_unique(&mut self, name: &str, sp: &Span) -> bool {
        if !self.check_card_exists(name, sp) {
            return false;
        }
        if self.cards.get(name).is_some_and(|r| r.unique) {
            true
        } else {
            self.err(
                sp.error(
                    code::E239,
                    format!("name path addresses `{name}`, which is not `unique`; only unique cards are addressable by name"),
                ),
            );
            false
        }
    }

    pub(crate) fn check_zone(&mut self, name: &str, sp: &Span) -> bool {
        if self.zones.contains_key(name) {
            true
        } else {
            self.err(sp.error(code::E244, format!("unknown zone `{name}`")));
            false
        }
    }

    // -- pass 1: collect declarations ---------------------------------------

    fn register(&mut self, items: &[TopItem]) {
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

    // -- pass 2: resolve and lower ------------------------------------------

    fn build(&mut self, items: Vec<TopItem>) {
        // Block order is free, but effect defaults (spawn/draw `into`) read
        // the game header; build `game` first. The header carries no
        // declaration index, so the move does not disturb ordering.
        let mut rest = Vec::new();
        let mut games = Vec::new();
        for item in items {
            match item {
                TopItem::Game(g) => games.push(g),
                other => rest.push(other),
            }
        }
        for g in games {
            self.build_game(g);
        }
        for item in rest {
            match item {
                TopItem::Game(_) => {}
                TopItem::Zones(zs) => {
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
                TopItem::Aspects(as_) => {
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
                TopItem::Legacies(ls) => {
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
                TopItem::Relations(rs) => {
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
                TopItem::Cards(cs) => {
                    for c in cs {
                        self.build_card(c);
                    }
                }
                TopItem::Predicates(ps) => {
                    for p in ps {
                        self.check_predicate_decl(&p);
                    }
                }
                TopItem::Decks(ds) => {
                    for d in ds {
                        self.build_deck(d);
                    }
                }
                TopItem::Endings(es) => {
                    for e in es {
                        self.build_ending(e);
                    }
                }
                TopItem::Events(evs) => {
                    for e in evs {
                        self.build_event_book(e);
                    }
                }
                TopItem::Verbs(vs) => {
                    for v in vs {
                        self.build_verb(v);
                    }
                }
                TopItem::Effectsets(es) => {
                    // The expanded database stores no effectset declaration,
                    // so its meta has nowhere to land; the [E236] repeat
                    // check still applies.
                    for e in es {
                        let meta = self.meta_map(&e.meta);
                        let _ = meta;
                    }
                }
                TopItem::Include(_) => {}
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
            if r.craftable.is_some() {
                craftable = true;
            }
            if r.chain_only.is_some() {
                chain_only = true;
            }
            if craftable && chain_only {
                self.err(r.span.error(
                    code::E222,
                    format!(
                        "recipe `{}` declares `craftable` together with `chain-only`",
                        r.name.1
                    ),
                ));
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

    /// Declaration-time predicate checks: recursion [E230], key registration
    /// (every aspect, legacy, kind, and card name the body reads must be
    /// declared — language.md §2.5).
    fn check_predicate_decl(&mut self, p: &ast::PredAst) {
        let mut stack = vec![p.name.1.clone()];
        let mut visiting: BTreeSet<String> = BTreeSet::new();
        self.pred_cycle_dfs(&p.name.1, &mut visiting, &mut stack);
        if let Some(body) = &p.body {
            let params: HashSet<String> = p.params.iter().map(|(_, n)| n.clone()).collect();
            check_cond_names(self, body, &params);
        }
    }

    fn pred_cycle_dfs(
        &mut self,
        name: &str,
        visiting: &mut BTreeSet<String>,
        stack: &mut Vec<String>,
    ) {
        if visiting.contains(name) {
            if let Some(reg) = self.preds.get(name) {
                if !reg.cycle_checked {
                    let sp = reg.span.clone();
                    let chain = stack.join(" -> ");
                    self.err(sp.error(
                        code::E230,
                        format!("predicate `{name}` expansion reaches itself ({chain})"),
                    ));
                    if let Some(reg) = self.preds.get_mut(name) {
                        reg.cycle_checked = true;
                    }
                }
            }
            return;
        }
        // Do not re-descend into predicates already proven acyclic.
        let body_calls = match self.preds.get(name) {
            Some(reg) if !reg.cycle_checked => reg
                .body
                .as_ref()
                .map(|b| {
                    let mut calls = Vec::new();
                    collect_pred_calls(b, &mut calls);
                    calls
                })
                .unwrap_or_default(),
            _ => return,
        };
        visiting.insert(name.to_string());
        stack.push(name.to_string());
        for call in body_calls {
            if self.preds.contains_key(&call) {
                self.pred_cycle_dfs(&call, visiting, stack);
            }
        }
        stack.pop();
        visiting.remove(name);
        if let Some(reg) = self.preds.get_mut(name) {
            reg.cycle_checked = true;
        }
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

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=b.len() {
        dp[0][j] = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[a.len()][b.len()]
}

// ---------------------------------------------------------------------------
// Parameter usage analysis (language.md §2.5, §6: parameters are integer
// expressions or card references). A bare parameter passed as a call argument
// inherits the callee's declared typing for that parameter, so forwarding
// chains classify correctly; the callee's flags are resolved on demand with a
// cycle guard (a cycle is [E230]'s business).
// ---------------------------------------------------------------------------

type Usage = BTreeMap<String, (bool, bool)>;

impl Compiler {
    /// The parameter-usage flags of one predicate, memoized.
    fn pred_usage_of(&mut self, name: &str, stack: &mut Vec<String>) -> Usage {
        if let Some(m) = self.pred_usage_memo.get(name) {
            return m.clone();
        }
        if stack.iter().any(|s| s == name) {
            return BTreeMap::new();
        }
        let Some(reg) = self.preds.get(name) else {
            return BTreeMap::new();
        };
        let params: HashSet<String> = reg.params.iter().map(|(_, n)| n.clone()).collect();
        let body = reg.body.clone();
        stack.push(name.to_string());
        let mut usage: Usage = params.iter().map(|n| (n.clone(), (false, false))).collect();
        if let Some(body) = body {
            self.usage_cond(&body, &params, &mut usage, stack);
        }
        stack.pop();
        self.pred_usage_memo.insert(name.to_string(), usage.clone());
        usage
    }

    /// The parameter-usage flags of one effectset, memoized.
    fn set_usage_of(&mut self, name: &str, stack: &mut Vec<String>) -> Usage {
        if let Some(m) = self.set_usage_memo.get(name) {
            return m.clone();
        }
        if stack.iter().any(|s| s == name) {
            return BTreeMap::new();
        }
        let Some(reg) = self.sets.get(name) else {
            return BTreeMap::new();
        };
        let params: HashSet<String> = reg.params.iter().map(|(_, n)| n.clone()).collect();
        let body = reg.body.clone();
        stack.push(name.to_string());
        let mut usage: Usage = params.iter().map(|n| (n.clone(), (false, false))).collect();
        if let Some((_, body)) = body {
            for eff in &body {
                self.usage_effect(eff, &params, &mut usage, stack);
            }
        }
        stack.pop();
        self.set_usage_memo.insert(name.to_string(), usage.clone());
        usage
    }

    /// Is the k-th argument of a call to predicate `callee` a card reference?
    fn pred_arg_is_card(&mut self, callee: &str, k: usize, stack: &mut Vec<String>) -> bool {
        let usage = self.pred_usage_of(callee, stack);
        self.preds
            .get(callee)
            .and_then(|r| r.params.get(k))
            .and_then(|(_, pn)| usage.get(pn))
            .is_some_and(|(c, _)| *c)
    }

    /// Is the k-th argument of a call to effectset `callee` a card reference?
    fn set_arg_is_card(&mut self, callee: &str, k: usize, stack: &mut Vec<String>) -> bool {
        let usage = self.set_usage_of(callee, stack);
        self.sets
            .get(callee)
            .and_then(|r| r.params.get(k))
            .and_then(|(_, pn)| usage.get(pn))
            .is_some_and(|(c, _)| *c)
    }

    fn usage_cond(
        &mut self,
        c: &ast::ACond,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match c {
            ast::ACond::Or(l, r) | ast::ACond::And(l, r) => {
                self.usage_cond(l, params, usage, stack);
                self.usage_cond(r, params, usage, stack);
            }
            ast::ACond::Not(l) => self.usage_cond(l, params, usage, stack),
            ast::ACond::Rel { from, to, .. } => {
                self.usage_card_ref(from, params, usage, stack);
                self.usage_card_ref(to, params, usage, stack);
            }
            ast::ACond::Cmp { l, r, .. } => {
                self.usage_expr(l, params, usage, stack);
                self.usage_expr(r, params, usage, stack);
            }
            ast::ACond::Call { name, args, .. } => {
                if let Some((card, int)) = usage.get_mut(&name.1) {
                    *int = true;
                    let _ = card;
                }
                for (k, a) in args.iter().enumerate() {
                    self.usage_pred_arg(a, &name.1, k, params, usage, stack);
                }
            }
            ast::ACond::Name(_, (sp, n)) => {
                if let Some((card, int)) = usage.get_mut(n) {
                    *int = true;
                    let _ = card;
                }
                let _ = sp;
            }
        }
    }

    /// One call argument: a bare parameter forwarded to a card-typed callee
    /// parameter is a card use; anything else is an integer expression.
    fn usage_pred_arg(
        &mut self,
        a: &ast::AExpr,
        callee: &str,
        k: usize,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        if let ast::AExpr::Ident(_, n) = a {
            if params.contains(n) && self.pred_arg_is_card(callee, k, stack) {
                if let Some((card, _)) = usage.get_mut(n) {
                    *card = true;
                }
                return;
            }
        }
        self.usage_expr(a, params, usage, stack);
    }

    fn usage_expr(
        &mut self,
        e: &ast::AExpr,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match e {
            ast::AExpr::Int(..) | ast::AExpr::Dur(..) | ast::AExpr::Clock(_) => {}
            ast::AExpr::NamePath { card, .. } => {
                if let Some((card_u, _)) = usage.get_mut(&card.1) {
                    *card_u = true;
                }
            }
            ast::AExpr::Ident(_, n) => {
                if let Some((_, int_u)) = usage.get_mut(n) {
                    *int_u = true;
                }
            }
            ast::AExpr::Legacy { .. } => {}
            ast::AExpr::Count { scope, terms, .. } => {
                self.usage_scope(scope, params, usage, stack);
                for t in terms {
                    self.usage_term(t, params, usage, stack);
                }
            }
            ast::AExpr::Total { scope, .. } => {
                self.usage_scope(scope, params, usage, stack);
            }
            ast::AExpr::Alive { card, .. } => {
                if let Some((card_u, _)) = usage.get_mut(&card.1) {
                    *card_u = true;
                }
            }
            ast::AExpr::Rel { from, to, .. } => {
                self.usage_card_ref(from, params, usage, stack);
                self.usage_card_ref(to, params, usage, stack);
            }
            ast::AExpr::Neg(_, inner) => self.usage_expr(inner, params, usage, stack),
            ast::AExpr::Bin { l, r, .. } => {
                self.usage_expr(l, params, usage, stack);
                self.usage_expr(r, params, usage, stack);
            }
        }
    }

    fn usage_term(
        &mut self,
        t: &ast::AQueryTerm,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match t {
            ast::AQueryTerm::Aspect { .. }
            | ast::AQueryTerm::Name(..)
            | ast::AQueryTerm::Kind(..) => {}
            ast::AQueryTerm::Not(inner) => self.usage_term(inner, params, usage, stack),
            ast::AQueryTerm::Call { name, args, .. } => {
                if let Some((_, int_u)) = usage.get_mut(&name.1) {
                    *int_u = true;
                }
                for (k, a) in args.iter().enumerate() {
                    self.usage_pred_arg(a, &name.1, k, params, usage, stack);
                }
            }
        }
    }

    fn usage_effect(
        &mut self,
        e: &ast::AEffect,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match e {
            ast::AEffect::Spawn { card, .. } => {
                if let Some((card_u, _)) = usage.get_mut(&card.1) {
                    *card_u = true;
                }
            }
            ast::AEffect::Destroy { target, .. }
            | ast::AEffect::Consume { target, .. }
            | ast::AEffect::Move { target, .. } => self.usage_target(target, params, usage, stack),
            ast::AEffect::Relate { from, to, .. } | ast::AEffect::Unrelate { from, to, .. } => {
                self.usage_target(from, params, usage, stack);
                self.usage_target(to, params, usage, stack);
            }
            ast::AEffect::Swap {
                target, to, into, ..
            } => {
                self.usage_target(target, params, usage, stack);
                if let Some((card_u, _)) = usage.get_mut(&to.1) {
                    *card_u = true;
                }
                if let Some(ast::AInto::Id(_, id)) = into {
                    if let Some((card_u, _)) = usage.get_mut(id) {
                        *card_u = true;
                    }
                }
            }
            ast::AEffect::Set { target, expr, .. } => {
                self.usage_target(target, params, usage, stack);
                self.usage_expr(expr, params, usage, stack);
            }
            ast::AEffect::Legacy { expr, .. } => self.usage_expr(expr, params, usage, stack),
            ast::AEffect::Narrate { .. } => {}
            ast::AEffect::Roll {
                count,
                sides,
                bonus,
                ..
            } => {
                self.usage_expr(count, params, usage, stack);
                self.usage_expr(sides, params, usage, stack);
                if let Some(b) = bonus {
                    self.usage_expr(b, params, usage, stack);
                }
            }
            ast::AEffect::Draw { .. } => {}
            ast::AEffect::Start { .. } => {}
            ast::AEffect::Continue { .. } => {}
            ast::AEffect::Signal { .. } | ast::AEffect::Ending { .. } => {}
            ast::AEffect::If {
                cond, then, els, ..
            } => {
                self.usage_cond(cond, params, usage, stack);
                for e in then {
                    self.usage_effect(e, params, usage, stack);
                }
                for e in els {
                    self.usage_effect(e, params, usage, stack);
                }
            }
            ast::AEffect::Call { name, args, .. } => {
                if let Some((_, int_u)) = usage.get_mut(&name.1) {
                    *int_u = true;
                }
                for (k, a) in args.iter().enumerate() {
                    // A single-ident argument forwards the parameter; its
                    // typing follows the callee effectset's parameter.
                    if let [(sp, crate::lexer::Tok::Ident(w))] = a.toks.as_slice() {
                        if params.contains(w) {
                            if self.set_arg_is_card(&name.1, k, stack) {
                                if let Some((card, _)) = usage.get_mut(w) {
                                    *card = true;
                                }
                            } else if let Some((_, int_u)) = usage.get_mut(w) {
                                *int_u = true;
                            }
                        }
                        let _ = sp;
                        continue;
                    }
                    for (sp, t) in &a.toks {
                        if let crate::lexer::Tok::Ident(w) = t {
                            if let Some((_, int_u)) = usage.get_mut(w) {
                                *int_u = true;
                            }
                        }
                        let _ = sp;
                    }
                }
            }
        }
    }

    fn usage_target(
        &mut self,
        t: &ast::ATarget,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match t {
            ast::ATarget::Name(_, (sp, n)) => {
                if let Some((card_u, _)) = usage.get_mut(n) {
                    *card_u = true;
                }
                let _ = sp;
            }
            ast::ATarget::NamePath { card, .. } => {
                if let Some((card_u, _)) = usage.get_mut(&card.1) {
                    *card_u = true;
                }
            }
            ast::ATarget::Slot(..) => {}
            ast::ATarget::Query { terms, .. } => {
                for t in terms {
                    self.usage_term(t, params, usage, stack);
                }
            }
            ast::ATarget::Bare { terms, .. } => {
                for t in terms {
                    self.usage_term(t, params, usage, stack);
                }
            }
        }
    }

    /// A relation endpoint's parameter use: a bare parameter name in
    /// card-ref position is a card use.
    fn usage_card_ref(
        &mut self,
        r: &ast::ACardRef,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        match r {
            ast::ACardRef::Name(_, (sp, n)) => {
                if let Some((card_u, _)) = usage.get_mut(n) {
                    *card_u = true;
                }
                let _ = sp;
            }
            ast::ACardRef::Subject(_) => {}
            ast::ACardRef::First { scope, terms, .. } => {
                self.usage_scope(scope, params, usage, stack);
                for t in terms {
                    self.usage_term(t, params, usage, stack);
                }
            }
        }
    }

    /// A query scope's parameter use: only `related:<kind> of <card-ref>`
    /// carries a card reference; the other scopes name zones, slots, or the
    /// whole world.
    fn usage_scope(
        &mut self,
        s: &ast::AScope,
        params: &HashSet<String>,
        usage: &mut Usage,
        stack: &mut Vec<String>,
    ) {
        if let ast::AScope::Related { of, .. } = s {
            self.usage_card_ref(of, params, usage, stack);
        }
    }
}

fn collect_pred_calls(c: &ast::ACond, out: &mut Vec<String>) {
    match c {
        ast::ACond::Or(l, r) | ast::ACond::And(l, r) => {
            collect_pred_calls(l, out);
            collect_pred_calls(r, out);
        }
        ast::ACond::Not(l) => collect_pred_calls(l, out),
        ast::ACond::Rel { from, to, .. } => {
            collect_ref_calls(from, out);
            collect_ref_calls(to, out);
        }
        ast::ACond::Cmp { l, r, .. } => {
            collect_expr_calls(l, out);
            collect_expr_calls(r, out);
        }
        ast::ACond::Call { name, args, .. } => {
            out.push(name.1.clone());
            for a in args {
                collect_expr_calls(a, out);
            }
        }
        ast::ACond::Name(_, (sp, n)) => {
            out.push(n.clone());
            let _ = sp;
        }
    }
}

fn collect_expr_calls(e: &ast::AExpr, out: &mut Vec<String>) {
    match e {
        ast::AExpr::Count { terms, .. } => {
            for t in terms {
                if let ast::AQueryTerm::Call { name, args, .. } = t {
                    out.push(name.1.clone());
                    for a in args {
                        collect_expr_calls(a, out);
                    }
                }
            }
        }
        ast::AExpr::Rel { from, to, .. } => {
            collect_ref_calls(from, out);
            collect_ref_calls(to, out);
        }
        ast::AExpr::Neg(_, inner) => collect_expr_calls(inner, out),
        ast::AExpr::Bin { l, r, .. } => {
            collect_expr_calls(l, out);
            collect_expr_calls(r, out);
        }
        _ => {}
    }
}

fn collect_ref_calls(r: &ast::ACardRef, out: &mut Vec<String>) {
    match r {
        ast::ACardRef::Name(..) | ast::ACardRef::Subject(_) => {}
        ast::ACardRef::First { terms, .. } => {
            for t in terms {
                if let ast::AQueryTerm::Call { name, args, .. } = t {
                    out.push(name.1.clone());
                    for a in args {
                        collect_expr_calls(a, out);
                    }
                }
            }
        }
    }
}

/// Declaration-time name checks inside a predicate body: every aspect key,
/// legacy key, kind, card name, and relation kind the body reads must be
/// declared.
fn check_cond_names(c: &mut Compiler, cond: &ast::ACond, params: &HashSet<String>) {
    match cond {
        ast::ACond::Or(l, r) | ast::ACond::And(l, r) => {
            check_cond_names(c, l, params);
            check_cond_names(c, r, params);
        }
        ast::ACond::Not(l) => check_cond_names(c, l, params),
        ast::ACond::Cmp { l, r, .. } => {
            check_expr_names(c, l, params);
            check_expr_names(c, r, params);
        }
        ast::ACond::Rel { kind, from, to, .. } => {
            c.check_relation_kind(&kind.1, &kind.0);
            check_card_ref_names(c, from, params);
            check_card_ref_names(c, to, params);
        }
        ast::ACond::Call { name, args, .. } => {
            if !params.contains(&name.1) && !c.preds.contains_key(&name.1) {
                c.err(
                    name.0
                        .error(code::E244, format!("unknown predicate `{}`", name.1)),
                );
            }
            for a in args {
                check_expr_names(c, a, params);
            }
        }
        ast::ACond::Name(_, (sp, n)) => {
            if !params.contains(n) {
                c.err(sp.error(code::E244, format!("unknown predicate `{n}`")));
            }
        }
    }
}

/// Declaration-time card-ref checks: a bare name must be a declared `unique`
/// card unless it is a visible parameter.
fn check_card_ref_names(c: &mut Compiler, r: &ast::ACardRef, params: &HashSet<String>) {
    match r {
        ast::ACardRef::Name(_, (sp, n)) => {
            if !params.contains(n) {
                c.check_card_unique(n, sp);
            }
        }
        ast::ACardRef::Subject(_) => {}
        ast::ACardRef::First { scope, terms, .. } => {
            check_scope_names(c, scope, params);
            for t in terms {
                check_term_names(c, t, params);
            }
        }
    }
}

fn check_expr_names(c: &mut Compiler, e: &ast::AExpr, params: &HashSet<String>) {
    match e {
        ast::AExpr::NamePath { card, aspect, .. } => {
            if !params.contains(&card.1) {
                c.check_card_unique(&card.1, &card.0);
            }
            c.check_aspect_key(&aspect.1, &aspect.0);
        }
        ast::AExpr::Ident(_, n) => {
            if !params.contains(n) {
                // Bare identifiers resolve to roll bindings at call sites;
                // at declaration they cannot be checked further.
                let _ = n;
            }
        }
        ast::AExpr::Legacy { key, .. } => {
            c.check_legacy_key(&key.1, &key.0);
        }
        ast::AExpr::Count { scope, terms, .. } => {
            check_scope_names(c, scope, params);
            for t in terms {
                check_term_names(c, t, params);
            }
        }
        ast::AExpr::Total { scope, key, .. } => {
            check_scope_names(c, scope, params);
            c.check_aspect_key(&key.1, &key.0);
        }
        ast::AExpr::Alive { card, .. } => {
            if !params.contains(&card.1) {
                c.check_card_unique(&card.1, &card.0);
            }
        }
        ast::AExpr::Rel { kind, from, to, .. } => {
            c.check_relation_kind(&kind.1, &kind.0);
            check_card_ref_names(c, from, params);
            check_card_ref_names(c, to, params);
        }
        ast::AExpr::Neg(_, inner) => check_expr_names(c, inner, params),
        ast::AExpr::Bin { l, r, .. } => {
            check_expr_names(c, l, params);
            check_expr_names(c, r, params);
        }
        ast::AExpr::Int(..) | ast::AExpr::Dur(..) | ast::AExpr::Clock(_) => {}
    }
}

fn check_scope_names(c: &mut Compiler, s: &ast::AScope, params: &HashSet<String>) {
    match s {
        ast::AScope::Zone(_, (sp, z)) => {
            c.check_zone(z, sp);
        }
        ast::AScope::Related { kind, of, .. } => {
            c.check_relation_kind(&kind.1, &kind.0);
            check_card_ref_names(c, of, params);
        }
        _ => {}
    }
}

fn check_term_names(c: &mut Compiler, t: &ast::AQueryTerm, params: &HashSet<String>) {
    match t {
        ast::AQueryTerm::Aspect { key, .. } => {
            c.check_aspect_key(&key.1, &key.0);
        }
        ast::AQueryTerm::Name(_, (sp, n)) => {
            if !params.contains(n) {
                c.check_card_exists(n, sp);
            }
        }
        ast::AQueryTerm::Kind(_, (sp, k)) => {
            c.check_kind(k, sp);
        }
        ast::AQueryTerm::Not(inner) => check_term_names(c, inner, params),
        ast::AQueryTerm::Call { name, args, .. } => {
            if !params.contains(&name.1) && !c.preds.contains_key(&name.1) {
                c.err(
                    name.0
                        .error(code::E244, format!("unknown predicate `{}`", name.1)),
                );
            }
            for a in args {
                check_expr_names(c, a, params);
            }
        }
    }
}
