//! The `DesignAuditor` (story-analyze spec §7.3): semantic recipe shadowing
//! proved over a free-atom model, unreachable content, and reachable
//! runtime faults with witnesses.

use std::collections::{BTreeMap, BTreeSet};

use story_content::ir::{Book, CmpOp, EffectNode, Expr, RecipeDecl, Rule};
use story_content::Content;
use story_core::{CardId, DeckId, EndingId, RecipeId, VerbId};
use story_engine::Event;

use crate::causal::{Plausibility, Site};
use crate::search::{Analyzer, ExploredGraph, ExploreConfig};
use crate::transitions::WitnessPath;

// ---------------------------------------------------------------------------
// Findings
// ---------------------------------------------------------------------------

/// A shadowing finding (§7.3): the later-scanned item never wins selection
/// because the earlier-scanned one always matches or refuses first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shadowing {
    /// The shadowing item (scanned first).
    pub shadower: String,
    /// The shadowed item (never selected where the shadower matches or its
    /// guard refuses).
    pub shadowed: String,
    /// The enclosing verb, for recipes.
    pub verb: Option<VerbId>,
    /// `once`/`max` suppression: the shadower is spent after its first
    /// firing, so the shadow is conditional (§7.3).
    pub conditional: bool,
}

/// The evidence class of an unreachable-content finding (§7.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Evidence {
    /// Empty producer cone: definitive.
    StaticallyProved,
    /// Not found within the exploration bounds: conditional on
    /// `max_states`/`max_time`.
    NotWithinBounds,
}

/// An unreachable-content finding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unreachable {
    pub kind: &'static str,
    pub id: String,
    pub evidence: Evidence,
}

/// A reachable runtime fault with its witness (§7.3).
#[derive(Clone, Debug)]
pub struct FaultWitness {
    pub event: FaultEvent,
    pub witness: WitnessPath,
}

/// The fault kinds the auditor flags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FaultEvent {
    ResolutionFailed { reason: String },
    StartSkipped { reason: String },
}

/// The audit report (§7.3).
#[derive(Clone, Debug, Default)]
pub struct AuditReport {
    pub shadowing: Vec<Shadowing>,
    pub unreachable: Vec<Unreachable>,
    pub faults: Vec<FaultWitness>,
    /// How many states the audit's exploration reached: the size the
    /// not-within-bounds findings are conditional on.
    pub explored_nodes: usize,
    /// Whether the exploration hit its bounds: the bounds-conditional
    /// findings are then incomplete (states past the bounds were never
    /// visited).
    pub truncated: bool,
}

// ---------------------------------------------------------------------------
// The auditor
// ---------------------------------------------------------------------------

/// Audit compiled content: static shadowing proofs (Tier 1 only, no state),
/// unreachable content (Tier 1 cones plus the explored graph), and
/// reachable faults (Tier 2 witnesses).
pub fn audit(analyzer: &mut Analyzer, explore: &ExploreConfig) -> AuditReport {
    let shadowing = shadowing_findings(analyzer.content());
    let graph = analyzer.explore(explore);
    let unreachable = {
        let _span = tracing::info_span!("audit_unreachable").entered();
        unreachable_findings(analyzer, &graph)
    };
    let faults = {
        let _span = tracing::info_span!("audit_faults").entered();
        fault_findings(analyzer, &graph)
    };
    AuditReport {
        shadowing,
        unreachable,
        faults,
        explored_nodes: graph.nodes.len(),
        truncated: graph.truncated,
    }
}

// ---------------------------------------------------------------------------
// Shadowing (§7.3)
// ---------------------------------------------------------------------------

/// Every shadowing finding over recipe pairs and trigger rule pairs.
pub fn shadowing_findings(content: &Content) -> Vec<Shadowing> {
    let mut out = Vec::new();
    let mut verbs: Vec<&story_content::ir::VerbDecl> = content.verbs.values().collect();
    verbs.sort_by_key(|v| v.decl_index);
    for verb in verbs {
        let mut recipes: Vec<&RecipeDecl> = verb
            .recipe_order
            .iter()
            .filter_map(|n| content.recipe(&format!("{}.{}", verb.id, n)))
            .collect();
        recipes.sort_by_key(|r| (r.priority, r.decl_index));
        for (i, r1) in recipes.iter().enumerate() {
            for r2 in &recipes[i + 1..] {
                if let Some(find) = recipe_shadowing(r1, r2) {
                    out.push(Shadowing {
                        shadower: r1.name.clone(),
                        shadowed: r2.name.clone(),
                        verb: Some(verb.id.clone()),
                        conditional: find,
                    });
                }
            }
        }
    }
    // Trigger rule pairs inside every book (rules carry no guards).
    let mut books: Vec<(&Book, String)> = Vec::new();
    for verb in content.verbs.values() {
        for rec in verb.recipes.values() {
            if let Some(b) = &rec.on_complete {
                books.push((b, rec.id.to_string()));
            }
        }
    }
    for card in content.cards.values() {
        for (q, b) in &card.on_complete {
            let label = match q {
                Some(k) => format!("{} on {k}", card.id),
                None => format!("{} on complete", card.id),
            };
            books.push((b, label));
        }
        if let Some(b) = &card.on_expire {
            books.push((b, format!("{} on expire", card.id)));
        }
    }
    for (name, b) in &content.events {
        books.push((b, format!("event {name}")));
    }
    for (book, label) in books {
        let mut rules: Vec<&Rule> = book.rules.iter().collect();
        rules.sort_by_key(|r| (r.priority, r.decl_index));
        for (i, a) in rules.iter().enumerate() {
            for b in &rules[i + 1..] {
                // The obligation: wherever `when(b)` holds, `a` fires first
                // (its `when` holds) — unsatisfiability of
                // `when(b) ∧ ¬when(a)` over the free-atom model.
                if implies(a.when.as_ref(), b.when.as_ref()) {
                    out.push(Shadowing {
                        shadower: format!("{label}#{}", a.id),
                        shadowed: format!("{label}#{}", b.id),
                        verb: None,
                        // `once` suppresses like a max cap: a once-shadowing
                        // rule is spent after its first firing (§7.3).
                        conditional: a.once,
                    });
                }
            }
        }
    }
    out
}

/// The recipe-pair check (§7.3): shadowing is suppressed when either recipe
/// declares `allow_shadow`. Two proofs apply. The *matches-everywhere*
/// proof is unsatisfiability of `when(R2) ∧ ¬when(R1)` over the free-atom
/// model — a capped shadower cannot use it (the cap spends it). The
/// *refuses-everywhere* proof is unsatisfiability of
/// `when(R2) ∧ when(R1) ∧ eligibility(R1)` where the eligibility atoms are
/// real interval atoms: the run counter below the `max` cap, and every slot
/// minimum as a count interval over the public pool (an over-approximation
/// of the bindable candidacy, so the conjunct holds in every state where R1
/// is genuinely selectable). Guards need no modeling: a guard refusal at R1
/// stops the scan exactly like a match — a shadow all the same.
fn recipe_shadowing(r1: &RecipeDecl, r2: &RecipeDecl) -> Option<bool> {
    if r1.allow_shadow || r2.allow_shadow {
        return None;
    }
    let when1 = r1.when.clone().unwrap_or(Expr::Bool(true));
    let when2 = r2.when.clone().unwrap_or(Expr::Bool(true));
    if r1.max.is_none() && implies(Some(&when2), Some(&when1)) {
        return Some(false);
    }
    // The eligibility atoms of the refusal proof (§7.3).
    let mut atoms: Vec<Expr> = Vec::new();
    if let Some(m) = r1.max {
        // The run counter below the cap, as an interval atom.
        atoms.push(Expr::Cmp {
            op: CmpOp::Lt,
            left: Box::new(Expr::Counter),
            right: Box::new(Expr::Lit(i64::from(m))),
        });
    }
    for sd in &r1.slots {
        // Slot-minimum satisfiability, expressible as a count interval.
        if sd.min >= 1 && !sd.query.is_empty() {
            atoms.push(Expr::Cmp {
                op: CmpOp::Ge,
                left: Box::new(Expr::Count {
                    scope: story_content::ir::Scope::World,
                    query: sd.query.clone(),
                }),
                right: Box::new(Expr::Lit(i64::from(sd.min))),
            });
        }
    }
    if atoms.is_empty() {
        return None;
    }
    let mut formula = Expr::Logic {
        op: story_content::ir::LogicOp::And,
        left: Box::new(when2),
        right: Box::new(when1),
    };
    for atom in atoms {
        formula = Expr::Logic {
            op: story_content::ir::LogicOp::And,
            left: Box::new(formula),
            right: Box::new(atom),
        };
    }
    if !satisfiable(&formula) {
        // The shadower is spent (or otherwise ineligible) wherever R2
        // matches: the shadow is permanent there, not conditional.
        return Some(false);
    }
    None
}

// ---------------------------------------------------------------------------
// The free-atom model (§7.3)
// ---------------------------------------------------------------------------

/// A comparison atom: `(variable, op, operand)`. The variable names a
/// distinct atomic predicate (aspect read, aggregate, `clock`, `legacy`,
/// run counter); numeric variables carry the interval constraints of their
/// active comparisons. `implies` answers: is `when2 ∧ ¬when1` unsatisfiable
/// over the model? Free-atom unsatisfiability implies real-state
/// unsatisfiability, so a proved implication is sound (§7.3).
pub fn implies(when2: Option<&Expr>, when1: Option<&Expr>) -> bool {
    let w2 = when2.cloned().unwrap_or(Expr::Bool(true));
    let w1 = when1.cloned().unwrap_or(Expr::Bool(true));
    // The formula: w2 ∧ ¬w1. Satisfiable when some boolean assignment of
    // its comparison atoms has mutually consistent intervals.
    let formula = Expr::Logic {
        op: story_content::ir::LogicOp::And,
        left: Box::new(w2),
        right: Box::new(Expr::Not(Box::new(w1))),
    };
    !satisfiable(&formula)
}

/// One comparison under the interval model.
#[derive(Clone, Debug, PartialEq, Eq)]
struct CmpAtom {
    variable: String,
    op: CmpOp,
    operand: String, // a variable name or a literal `=<n>`
}

/// The numeric variables of an expression (one variable per distinct atom,
/// §7.3): aspect reads, count/total/exists/attr aggregates, `clock`,
/// `legacy`, the run counter. Alive/rel tests and everything opaque
/// normalize to free booleans (no constraints).
fn atom_of(e: &Expr) -> String {
    match e {
        Expr::NamePath { card, key } => format!("aspect:{card}.{key}"),
        Expr::Subject(key) => format!("subject:{key}"),
        Expr::Legacy(key) => format!("legacy:{key}"),
        Expr::Clock => "clock".to_string(),
        Expr::Counter => "counter".to_string(),
        Expr::Roll => "roll".to_string(),
        Expr::Attr { scope, query, key } => format!("attr:{scope:?}:{query:?}:{key}"),
        Expr::Aspect { scope, query, .. } => format!("total:{scope:?}:{query:?}"),
        Expr::Count { scope, query } => format!("count:{scope:?}:{query:?}"),
        Expr::Exists { scope, query } => format!("exists:{scope:?}:{query:?}"),
        Expr::Lit(n) => format!("={n}"),
        _ => String::new(),
    }
}

/// The comparison atoms of an expression (the boolean leaves).
fn cmp_atoms(e: &Expr, out: &mut Vec<CmpAtom>) {
    match e {
        Expr::Cmp { op, left, right } => {
            let l = atom_of(left);
            let r = atom_of(right);
            if !l.is_empty() && !r.is_empty() {
                out.push(CmpAtom {
                    variable: l,
                    op: *op,
                    operand: r,
                });
            }
        }
        Expr::Arith { .. } => {}
        Expr::Logic { left, right, .. } => {
            cmp_atoms(left, out);
            cmp_atoms(right, out);
        }
        Expr::Not(inner) => cmp_atoms(inner, out),
        _ => {}
    }
}

/// The boolean formula over comparison atoms, in negation normal form.
enum Formula {
    /// A comparison atom held.
    Held(usize),
    /// A comparison atom refuted (its flipped comparison constrains).
    Refuted(usize),
    And(Box<Formula>, Box<Formula>),
    Or(Box<Formula>, Box<Formula>),
    True,
}

fn formula_of(e: &Expr, atoms: &mut Vec<CmpAtom>) -> Formula {
    match e {
        Expr::Logic {
            op: story_content::ir::LogicOp::And,
            left,
            right,
        } => Formula::And(
            Box::new(formula_of(left, atoms)),
            Box::new(formula_of(right, atoms)),
        ),
        Expr::Logic {
            op: story_content::ir::LogicOp::Or,
            left,
            right,
        } => Formula::Or(
            Box::new(formula_of(left, atoms)),
            Box::new(formula_of(right, atoms)),
        ),
        Expr::Not(inner) => not_of(inner, atoms),
        Expr::Bool(b) => {
            if *b {
                Formula::True
            } else {
                // A refuted tautology: unsatisfiable in the model.
                Formula::Refuted(usize::MAX)
            }
        }
        _ => {
            let mut leaves = Vec::new();
            cmp_atoms(e, &mut leaves);
            if leaves.is_empty() {
                // Opaque leaf (an aspect read as a boolean, alive, rel,
                // ...): a free atom — no interval constraints.
                atoms.push(CmpAtom {
                    variable: format!("free#{}", atoms.len()),
                    op: CmpOp::Ge,
                    operand: "=1".to_string(),
                });
                Formula::Held(atoms.len() - 1)
            } else {
                // A leaf's comparison chain is conjunctive in the model;
                // anything disjunctive inside the operands only weakens
                // the model, never a proof.
                let mut f = Formula::True;
                for l in leaves {
                    atoms.push(l);
                    let a = Formula::Held(atoms.len() - 1);
                    f = Formula::And(Box::new(f), Box::new(a));
                }
                f
            }
        }
    }
}

fn not_of(e: &Expr, atoms: &mut Vec<CmpAtom>) -> Formula {
    match e {
        Expr::Not(inner) => formula_of(inner, atoms),
        Expr::Bool(b) => {
            if *b {
                // A refuted tautology: unsatisfiable in the model.
                Formula::Refuted(usize::MAX)
            } else {
                Formula::True
            }
        }
        Expr::Logic {
            op: story_content::ir::LogicOp::And,
            left,
            right,
        } => Formula::Or(
            Box::new(not_of(left, atoms)),
            Box::new(not_of(right, atoms)),
        ),
        Expr::Logic {
            op: story_content::ir::LogicOp::Or,
            left,
            right,
        } => Formula::And(
            Box::new(not_of(left, atoms)),
            Box::new(not_of(right, atoms)),
        ),
        _ => {
            let mut leaves = Vec::new();
            cmp_atoms(e, &mut leaves);
            if leaves.is_empty() {
                atoms.push(CmpAtom {
                    variable: format!("free#{}", atoms.len()),
                    op: CmpOp::Ge,
                    operand: "=1".to_string(),
                });
                Formula::Refuted(atoms.len() - 1)
            } else {
                let mut f = Formula::True;
                for l in leaves {
                    atoms.push(l);
                    let a = Formula::Refuted(atoms.len() - 1);
                    f = Formula::And(Box::new(f), Box::new(a));
                }
                f
            }
        }
    }
}

/// Path-consistency satisfiability over the interval model: conjunctions
/// thread the accumulated intervals, disjunctions branch, held/refuted
/// atoms constrain their variable (refuted flips the comparison; a
/// disequality constrains nothing — a sound relaxation). The model is a
/// relaxation of the real states — variables are independent reals and
/// distinct variables never interact — so unsatisfiability here implies
/// unsatisfiability in every real state (§7.3).
fn satisfiable(e: &Expr) -> bool {
    let mut atoms: Vec<CmpAtom> = Vec::new();
    let formula = formula_of(e, &mut atoms);
    if atoms.len() > 24 {
        // Beyond the assignment budget the model cannot decide:
        // satisfiable (no proof), the sound direction for shadowing.
        return true;
    }
    walk(&formula, &atoms, &mut BTreeMap::new())
}

fn walk(
    f: &Formula,
    atoms: &[CmpAtom],
    intervals: &mut BTreeMap<String, (i64, i64)>,
) -> bool {
    match f {
        Formula::True => true,
        Formula::Held(i) => {
            if *i == usize::MAX {
                return false; // the unsatisfiable marker of a refuted tautology
            }
            constrain(intervals, &atoms[*i], false)
        }
        Formula::Refuted(i) => {
            if *i == usize::MAX {
                return false;
            }
            constrain(intervals, &atoms[*i], true)
        }
        Formula::And(a, b) => walk(a, atoms, intervals) && walk(b, atoms, intervals),
        Formula::Or(a, b) => {
            let mut left = intervals.clone();
            walk(a, atoms, &mut left) || walk(b, atoms, intervals)
        }
    }
}

/// The flipped comparison of a refuted atom.
fn flip_op(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Ge => CmpOp::Lt,
        CmpOp::Gt => CmpOp::Le,
        CmpOp::Le => CmpOp::Gt,
        CmpOp::Lt => CmpOp::Ge,
        CmpOp::Eq => CmpOp::Ne,
        CmpOp::Ne => CmpOp::Eq,
    }
}

/// Apply one comparison's interval constraint; `flip` negates the atom.
/// Returns false on an empty interval. Only `variable <op> literal`
/// constrains; `variable <op> variable` stays free (a conservative
/// relaxation).
fn constrain(
    intervals: &mut BTreeMap<String, (i64, i64)>,
    atom: &CmpAtom,
    flip: bool,
) -> bool {
    let Some(value) = atom.operand.strip_prefix('=') else {
        return true;
    };
    let Ok(v) = value.parse::<i64>() else {
        return true;
    };
    let op = if flip { flip_op(atom.op) } else { atom.op };
    let (lo, hi) = intervals.entry(atom.variable.clone()).or_insert((i64::MIN, i64::MAX));
    match op {
        CmpOp::Ge => {
            if v > *lo {
                *lo = v;
            }
        }
        CmpOp::Gt => {
            let b = v.saturating_add(1);
            if b > *lo {
                *lo = b;
            }
        }
        CmpOp::Le => {
            if v < *hi {
                *hi = v;
            }
        }
        CmpOp::Lt => {
            let b = v.saturating_sub(1);
            if b < *hi {
                *hi = b;
            }
        }
        CmpOp::Eq => {
            if v > *lo {
                *lo = v;
            }
            if v < *hi {
                *hi = v;
            }
        }
        // A disequality refines no interval (sound relaxation).
        CmpOp::Ne => {}
    }
    lo <= hi
}

// ---------------------------------------------------------------------------
// Unreachable content (§7.3)
// ---------------------------------------------------------------------------

fn unreachable_findings(analyzer: &Analyzer, graph: &ExploredGraph) -> Vec<Unreachable> {
    let content = analyzer.content();
    let causal = analyzer.causal();
    let mut out = Vec::new();

    // Explored occurrences.
    let mut live_cards: BTreeSet<CardId> = BTreeSet::new();
    let mut opened_recipes: BTreeSet<RecipeId> = BTreeSet::new();
    let mut touched_decks: BTreeSet<DeckId> = BTreeSet::new();
    let mut reached_endings: BTreeSet<EndingId> = BTreeSet::new();
    let start = &graph.nodes[graph.start].state;
    for node in &graph.nodes {
        for ent in node.state.entities.values() {
            live_cards.insert(ent.prototype.clone());
        }
        for sit in node.state.situations.values() {
            opened_recipes.insert(sit.recipe.clone());
        }
        for (deck, cursor) in &node.state.decks {
            if start.decks.get(deck) != Some(cursor) {
                touched_decks.insert(deck.clone());
            }
        }
        if let Some(e) = &node.state.ended {
            reached_endings.insert(e.clone());
        }
    }
    for e in &graph.edges {
        if let Some(r) = &e.record.recipe {
            opened_recipes.insert(r.clone());
        }
    }

    let statically = |fact: &crate::causal::Fact| {
        causal.plausibility(fact) == Plausibility::StaticallyImpossible
    };

    for verb in content.verbs.values() {
        for rec in verb.recipes.values() {
            if opened_recipes.contains(&rec.id) {
                continue;
            }
            let evidence = if statically(&crate::causal::Fact::RecipeCanOpen(rec.id.clone())) {
                Evidence::StaticallyProved
            } else {
                Evidence::NotWithinBounds
            };
            out.push(Unreachable {
                kind: "recipe",
                id: rec.id.to_string(),
                evidence,
            });
        }
    }
    for card in content.cards.keys() {
        if !live_cards.contains(card) {
            let evidence = if statically(&crate::causal::Fact::CardLive(card.clone())) {
                Evidence::StaticallyProved
            } else {
                Evidence::NotWithinBounds
            };
            out.push(Unreachable {
                kind: "card",
                id: card.to_string(),
                evidence,
            });
        }
    }
    for deck in content.decks.keys() {
        let deck = story_core::DeckId::from(deck.as_str());
        if !touched_decks.contains(&deck) {
            // Static evidence: no draw anywhere references the deck.
            let referenced = {
                use std::cell::Cell;
                let found = Cell::new(false);
                crate::causal::visit_content_effects(content, &mut |n| {
                    if matches!(n, EffectNode::Draw { deck: d, .. } if *d == deck) {
                        found.set(true);
                    }
                });
                found.get()
            };
            out.push(Unreachable {
                kind: "deck",
                id: deck.to_string(),
                evidence: if !referenced {
                    Evidence::StaticallyProved
                } else {
                    Evidence::NotWithinBounds
                },
            });
        }
    }
    for ending in content.endings.keys() {
        let ending = story_core::EndingId::from(ending.as_str());
        if !reached_endings.contains(&ending) {
            let evidence = if statically(&crate::causal::Fact::EndingReached(ending.clone())) {
                Evidence::StaticallyProved
            } else {
                Evidence::NotWithinBounds
            };
            out.push(Unreachable {
                kind: "ending",
                id: ending.to_string(),
                evidence,
            });
        }
    }
    // Books: a book's ignition conditions are its hyperedge requirements;
    // a required fact with no producer cone means the book never fires
    // (static). Otherwise the finding is bounds-conditional (Tier 1 only:
    // book firings leave no explored trace of their own).
    let mut sites: BTreeSet<(String, String)> = BTreeSet::new();
    for edge in causal.edges() {
        let pair = match &edge.site {
            Site::CardBook { card, qualifier } => (card, qualifier.as_ref().map(|q| format!("complete:{q}")).unwrap_or_else(|| "complete".to_string()).to_string()),
            Site::CardExpire(card) => (card, "expire".to_string()),
            _ => continue,
        };
        {
            let (card, label) = pair;
            let impossible = edge
                .requires
                .iter()
                .any(|r| statically(r) && !matches!(r, crate::causal::Fact::RecipeCanComplete(_)));
            if impossible {
                sites.insert((card.to_string(), label));
            }
        }
    }
    for (card, label) in sites {
        out.push(Unreachable {
            kind: "book",
            id: format!("{card} {label}"),
            evidence: Evidence::StaticallyProved,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Reachable runtime faults (§7.3)
// ---------------------------------------------------------------------------

fn fault_findings(analyzer: &Analyzer, graph: &ExploredGraph) -> Vec<FaultWitness> {
    let mut out = Vec::new();
    for (i, e) in graph.edges.iter().enumerate() {
        for ev in &e.record.events {
            let fault = match ev {
                Event::ResolutionFailed { reason, .. } => FaultEvent::ResolutionFailed {
                    reason: reason.clone(),
                },
                Event::StartSkipped { reason, .. } => FaultEvent::StartSkipped {
                    reason: format!("{reason:?}"),
                },
                _ => continue,
            };
            if let Some(witness) = witness_through(analyzer, graph, e.from, i) {
                out.push(FaultWitness {
                    event: fault,
                    witness,
                });
            }
            break;
        }
    }
    out
}

/// The witness to a faulting edge: the BFS-tree path to its source node,
/// then the edge itself. The oracle verifies the whole path (§7.3: every
/// reported witness replays); a fault witness asserts no goal.
fn witness_through(
    analyzer: &Analyzer,
    graph: &ExploredGraph,
    from: usize,
    edge: usize,
) -> Option<WitnessPath> {
    let mut witness = graph.witness_to(from, analyzer.start())?;
    let record = graph.edges[edge].record.clone();
    witness.records.push(record);
    witness.actions = witness.records.iter().map(|r| r.actions()).sum();
    witness.depth = witness.records.len();
    // Adversarial witnesses carry no cumulative probability (§6); the
    // other modes re-fold the exact branch products.
    witness.probability = if matches!(graph.rng_mode, crate::query::RngMode::Adversarial) {
        None
    } else {
        Some(witness.records.iter().map(|r| r.probability).product::<f64>())
    };
    match analyzer
        .replayer()
        .replay(&witness, None, None)
    {
        Ok(_) => Some(witness),
        Err(_) => None,
    }
}
