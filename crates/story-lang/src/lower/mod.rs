//! The two-pass compiler: pass one collects declarations into the one
//! namespace (duplicates reported at the second site, both named); pass two
//! resolves references, inlines predicates and effectsets, and lowers the AST
//! into the validated `Content` (language.md §1, spec.md §5).

pub mod effect;
pub mod expr;

mod build;
mod collect;
mod names;
mod usage;

use crate::ast::{self, AEffect, MetaLine, TopItem};
use crate::includes;
use crate::lexer;
use crate::parser::Parser;
use crate::span::Span;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;
use story_content::code;
use story_content::ir::{AspectKey, LegacyKey, Meta};
use story_content::{Content, Diagnostic};
use story_core::CardId;

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
