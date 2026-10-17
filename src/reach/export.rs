//! The machine-readable exports, both ZERO new analysis over the settled
//! facts:
//!
//! - **`--json`** — the per-card [`CardReport`] as JSON. The report types
//!   carry `serde::Serialize` derives (paths.rs — derives only, no logic),
//!   so the JSON keys are the struct fields in declaration order (target /
//!   verdict / existence / chain / paths / candidates / usable_via / notes,
//!   the text report's own section order) and the enums serialize as their
//!   `as_str` strings. This module adds the envelope the CLI prints: the
//!   text head's meta (file, demand, usable) around the `report`, and —
//!   when the CLI carries `--why` — the blocker table as a `why` array
//!   (each row's `blocker` is [`CardBlocker::as_str`]'s own string, the
//!   very text `render_why` prints). Without `--why` the key is absent,
//!   mirroring the text face where nothing renders. Serialization is
//!   deterministic: struct order, `Vec` order, no maps — two runs over the
//!   same (compiled content, initial state) produce byte-identical JSON.
//!
//! - **`--graph`** — the whole-content mint dependency graph as Graphviz
//!   DOT. Nodes are card names (the post-fixpoint universe ∪ the initial
//!   entities, kind-annotated) plus one owner node per distinct site owner
//!   string (`recipe <id> of VERB` / `trigger <id>`, box-shaped). Edges,
//!   from the facts census/fixpoint/paths already computed: every PROVEN
//!   site filtered through the analyzer's OWN bare-name serving predicate
//!   (`analyze_name`'s) contributes `owner → mints` (SPAWN/REPLACE mints
//!   the site name; a proven DRAW serves a deck name only when it is not
//!   derive-blinded and the freeze model's `deck_cap` — the report side's
//!   value, rewritten after the fixpoint — can reach it, the shared
//!   `served_by_cap`) and `mints → requires` (a recipe-owned site's verb
//!   slot fillers, an expiry trigger's named card); `census.destroyed_by`
//!   contributes dashed `owner → name` kill edges. Determinism is the
//!   contract: node lines sort by node id, edge lines by (src, dst,
//!   label), both under the engine's [`cmp_utf16`] order with exact
//!   duplicates collapsed — two runs over the same inputs render
//!   byte-identical text (pinned by tests/reach/reach_export.rs).

use std::fmt::Write as _;

use indexmap::{IndexMap, IndexSet};
use serde::Serialize;
use serde_json::to_string_pretty;

use crate::content::{CompiledContent, CompiledTrigger};
use crate::core::text::cmp_utf16;
use crate::core::types::State;

use super::census::{draw_blinds_bare, draw_deck, object_always, SiteKind};
use super::fixpoint::SiteBlocker;
use super::paths::{served_by_cap, CardReport};
use super::world::Analyzer;

// ---------------------------------------------------------------------------
// --json — the per-card report envelope
// ---------------------------------------------------------------------------

/// One blocker row of the `why` array (`--why --json`): the `--why` table's
/// row with `CardBlocker::as_str`'s own text — the string `render_why`
/// prints, so the JSON face and the text face carry the same words.
#[derive(Serialize)]
struct BlockerExport<'a> {
    owner: &'a str,
    op: &'static str,
    name: &'a str,
    blocker: String,
}

/// The JSON envelope the CLI prints per card: the text head's meta (file,
/// demand, usable) around the `report`; `why` rides only under `--why`
/// (`skip_serializing_if` — without the flag the key is absent, exactly as
/// the text face renders nothing).
#[derive(Serialize)]
pub struct CardExport<'a> {
    file: &'a str,
    demand: Option<&'a str>,
    usable: bool,
    report: &'a CardReport,
    #[serde(skip_serializing_if = "Option::is_none")]
    why: Option<Vec<BlockerExport<'a>>>,
}

/// Assemble one [`CardExport`] envelope. `blockers` is `Some` only under
/// `--why` (the CLI's `card_blockers` pass — otherwise skipped, matching
/// the text face's zero work).
///
/// The `--why` + `--json` interaction, decided MERGE (not mutex): the
/// blocker table rides inside the envelope as `why` — the merge costs one
/// borrow + one `as_str` per row, and the text face already renders both
/// faces in one run.
#[must_use]
pub fn card_export<'a>(
    file: &'a str,
    demand: Option<&'a str>,
    usable: bool,
    report: &'a CardReport,
    blockers: Option<&'a [SiteBlocker]>,
) -> CardExport<'a> {
    CardExport {
        file,
        demand,
        usable,
        report,
        why: blockers.map(|rows| {
            rows.iter()
                .map(|b| BlockerExport {
                    owner: &b.owner,
                    op: b.op.as_str(),
                    name: &b.name,
                    blocker: b.blocker.as_str(),
                })
                .collect()
        }),
    }
}

/// Render one envelope as the CLI's stdout bytes (pretty JSON + newline).
/// Key order is the struct's declaration order; `to_string_pretty`'s
/// two-space indent is stable serde_json output. Serializing these
/// borrowed report types cannot fail (no maps, no non-string keys) — the
/// panic stays explicit, the reach-scale precedent.
#[must_use]
pub fn render_json(e: &CardExport<'_>) -> String {
    let mut buf =
        to_string_pretty(e).unwrap_or_else(|e| panic!("cannot serialize the card report: {e}"));
    buf.push('\n');
    buf
}

/// Render `--all --json`: the array face, one envelope per card in the
/// UTF-16-sorted name order the text `--all` walks.
#[must_use]
pub fn render_json_array(exports: &[CardExport<'_>]) -> String {
    let mut buf = to_string_pretty(exports)
        .unwrap_or_else(|e| panic!("cannot serialize the card reports: {e}"));
    buf.push('\n');
    buf
}

// ---------------------------------------------------------------------------
// --graph — the whole-content mint dependency graph (Graphviz DOT)
// ---------------------------------------------------------------------------

/// One DOT edge, pre-sorting: (src, dst, label, dashed).
type DotEdge = (String, String, String, bool);

/// DOT quoted-string escaping: backslash first, then the quote. Card names
/// are `[A-Za-z0-9_]` tokens and owners are `recipe <id> of VERB` /
/// `trigger <id>` display strings, so this is defensive.
fn dot_id(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(c),
        }
    }
    out
}

/// The whole-content mint dependency graph over one (content, initial)
/// pair — the `--graph` face. A fresh [`Analyzer`] per call (the world
/// facts are a pure function of the inputs). See the module docs for the
/// node/edge model and the determinism contract.
#[must_use]
pub fn graph_dot(content: &CompiledContent, initial: &State) -> String {
    let analyzer = Analyzer::new(content, initial);
    let facts = analyzer.facts();

    // card name → kind, FIRST occurrence wins: the universe's walk is
    // initial facts ∪ prototypes ∪ deck cards ∪ spawn mints, so an
    // initial entity's kind beats a later same-name mint's; init_names
    // re-asserts the initial face (it is the universe's head anyway).
    let mut kinds: IndexMap<String, String> = IndexMap::new();
    for e in &facts.universe {
        kinds.entry(e.name.clone()).or_insert_with(|| e.kind.clone());
    }
    for (name, e) in &facts.init_names {
        kinds.entry(name.clone()).or_insert_with(|| e.kind.clone());
    }

    // the edge set: exact (src, dst, label) duplicates collapse (a name
    // listed twice in a deck order, two sites of one recipe minting the
    // same name — one edge each)
    let mut edges: IndexSet<DotEdge> = IndexSet::new();

    // every PROVEN site contributes owner → mints and mints → requires —
    // filtered by the analyzer's OWN bare-name serving predicate
    // (`analyze_name`'s): a derive-mint behind an object-mandatory verb
    // never mints the bare name (12320), and a draw serves a name only
    // when it is not derive-blinded and the FREEZE cap can reach it.
    // `proven`'s insertion order is the fixpoint schedule (deterministic),
    // but the sort below makes the walk order unobservable.
    for (&si, _sup) in &facts.proven {
        let site = &facts.census.sites[si];
        // the bare name never mints from a derive-mint behind an
        // object-mandatory verb — the site contributes no edge at all
        // (its `requires` fillers ride the mints it never produces)
        if site.derives && object_always(facts.content, site.verb) {
            continue;
        }
        let owner = site.owner.clone();
        let verb_fillers: Vec<String> = match site.verb {
            Some(vi) => facts
                .fillers
                .get(&vi)
                .map(|fills| fills.iter().cloned().collect())
                .unwrap_or_default(),
            // trigger-owned: an expiry trigger requires its named card;
            // slot/complete triggers carry none (the same split path_for
            // renders)
            None => match facts.trig_idx.get(&site.owner) {
                Some((CompiledTrigger::Expire { on_expire_of, .. }, _)) => {
                    vec![on_expire_of.clone()]
                }
                _ => Vec::new(),
            },
        };
        match site.kind {
            SiteKind::Spawn | SiteKind::Replace => {
                let op = match site.kind {
                    SiteKind::Spawn => "SPAWN",
                    _ => "REPLACE",
                };
                edges.insert((owner.clone(), site.name.clone(), op.to_string(), false));
                for f in &verb_fillers {
                    edges.insert((site.name.clone(), f.clone(), "requires".to_string(), false));
                }
            }
            SiteKind::Draw => {
                // the draw serves a name exactly when the REPORT side's
                // serving predicate holds: not derive-blinded, and
                // `facts.deck_cap` — the freeze model's value, rewritten
                // to 0/1 for trigger-held draws after the fixpoint — can
                // reach it (seed 81401's shape). The site's raw cap is
                // never read here.
                let deck = draw_deck(site);
                let label = format!("DRAW deck {}", deck.id);
                let cap = facts.deck_cap.get(&site.name).copied().unwrap_or(0);
                let mut names: IndexSet<&str> =
                    deck.order.iter().map(|c| c.name.as_str()).collect();
                if let Some(dc) = &deck.defaultcard {
                    names.insert(&dc.name);
                }
                for m in names {
                    if draw_blinds_bare(facts.content, site, m) {
                        continue;
                    }
                    if !served_by_cap(deck, m, cap) {
                        continue;
                    }
                    edges.insert((owner.clone(), m.to_string(), label.clone(), false));
                    for f in &verb_fillers {
                        edges.insert((m.to_string(), f.clone(), "requires".to_string(), false));
                    }
                }
            }
        }
    }

    // the destroyed-by kill edges (dashed): name ← the owners that
    // destroy/replace it
    for (name, owners) in &facts.census.destroyed_by {
        for owner in owners {
            edges.insert((owner.clone(), name.clone(), "destroys".to_string(), true));
        }
    }

    // the node set: every universe ∪ initial card name plus every edge
    // endpoint (owner strings are the only non-card endpoints)
    let mut nodes: IndexSet<String> = kinds.keys().cloned().collect();
    for (src, dst, _, _) in &edges {
        nodes.insert(src.clone());
        nodes.insert(dst.clone());
    }
    let mut sorted_nodes: Vec<String> = nodes.into_iter().collect();
    sorted_nodes.sort_by(|a, b| cmp_utf16(a, b));
    let mut sorted_edges: Vec<&DotEdge> = edges.iter().collect();
    sorted_edges.sort_by(|a, b| {
        cmp_utf16(&a.0, &b.0).then(cmp_utf16(&a.1, &b.1)).then(cmp_utf16(&a.2, &b.2))
    });

    let mut out = String::new();
    out.push_str("digraph reach {\n");
    out.push_str("  rankdir=LR;\n");
    for id in &sorted_nodes {
        let esc = dot_id(id);
        match kinds.get(id) {
            Some(kind) => {
                let _ = writeln!(
                    out,
                    "  \"{esc}\" [label=\"{esc}\\n{}\", shape=ellipse];",
                    dot_id(kind)
                );
            }
            None => {
                let _ = writeln!(out, "  \"{esc}\" [shape=box];");
            }
        }
    }
    for (src, dst, label, dashed) in sorted_edges {
        let style = if *dashed { ", style=dashed" } else { "" };
        let _ = writeln!(
            out,
            "  \"{}\" -> \"{}\" [label=\"{}\"{style}];",
            dot_id(src),
            dot_id(dst),
            dot_id(label)
        );
    }
    out.push_str("}\n");
    out
}
