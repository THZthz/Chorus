//! Boot: state construction, declared zones, legacy folding, deck
//! permutations, boot mints, boot relations, and the t=0 checkpoint
//! (kernel-api.md §1, spec.md §4.8).

use super::checkpoint;
use super::Run;
use crate::effects::EffectCtx;
use crate::{DiagExt, Engine};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use story_content::ir::{Book, SeedSpec};
use story_content::runtime::{DiagKind, RuntimeDiagnostic};
use story_content::Content;
use story_core::rng::splitmix64;
use story_core::{CardId, DeckId, EntityId, State, Zone, ZoneId, ZoneKind};

pub(crate) fn boot_tables(
    content: Arc<Content>,
    registry: crate::EffectRegistry,
    seed: u64,
    legacy_in: Option<BTreeMap<String, i64>>,
) -> (Engine, Vec<RuntimeDiagnostic>) {
    let mut diags: Vec<RuntimeDiagnostic> = Vec::new();
    let (turn, home_name) = content
        .game
        .as_ref()
        .map(|g| (g.turn, g.home.clone()))
        .unwrap_or((1, String::new()));

    // Seed handling: a content-fixed seed must match the boot argument.
    if let Some(SeedSpec::Fixed(n)) = content.game.as_ref().map(|g| g.seed.clone()) {
        if n != seed {
            diags.push(DiagKind::SeedMismatch.in_context(
                "boot",
                format!("content fixes seed {n} but boot received {seed}"),
            ));
        }
    }

    // Every referenced host op must be registered (`UnregisteredOp`).
    let mut ops = BTreeSet::new();
    collect_ops(&content, &mut ops);
    for op in &ops {
        if !registry.contains_op(op) {
            diags.push(DiagKind::UnregisteredOp.in_context(
                "boot",
                format!("content references op `{op}` which no host registered"),
            ));
        }
    }

    let mut state = State::default();
    state.next_entity = 1;
    state.next_situation = 1;
    state.next_zone = 1;
    state.rng_counter = 0;

    // Declared zones, in declaration order (Table: public, bindable).
    let mut zone_ids: BTreeMap<String, ZoneId> = BTreeMap::new();
    let mut zone_order: BTreeMap<ZoneId, u32> = BTreeMap::new();
    let mut zone_parent: BTreeMap<ZoneId, Option<ZoneId>> = BTreeMap::new();
    let mut zones_sorted: Vec<&story_content::ir::ZoneDecl> = content.zones.values().collect();
    zones_sorted.sort_by_key(|z| z.decl_index);
    for z in &zones_sorted {
        let id = ZoneId(state.next_zone);
        state.next_zone += 1;
        state.zones.insert(
            id,
            Zone {
                id,
                kind: ZoneKind::Table,
                public: true,
                bindable: true,
            },
        );
        zone_ids.insert(z.name.clone(), id);
        zone_order.insert(id, z.decl_index);
        zone_parent.insert(id, None);
    }
    for z in content.zones.values() {
        if let Some(p) = &z.parent {
            if let (Some(&child), Some(&parent)) = (zone_ids.get(&z.name), zone_ids.get(p)) {
                zone_parent.insert(child, Some(parent));
            }
        }
    }
    let home = zone_ids.get(&home_name).copied().unwrap_or_else(|| {
        // The compiler validates `home`; the fallback keeps boot total.
        zone_ids.values().copied().next().unwrap_or(ZoneId(0))
    });

    // Legacy: declared defaults, then the host's incoming values folded over.
    for l in content.legacies.values() {
        state.legacy.insert(l.key.clone(), l.default);
    }
    if let Some(incoming) = legacy_in {
        for (k, v) in incoming {
            if !state.legacy.contains_key(&k) {
                diags.push(DiagKind::UnresolvableRef.warning(format!(
                    "host legacy key `{k}` is not declared in content; folded as {v}"
                )));
            }
            state.legacy.insert(k, v);
        }
    }

    // Deck cursors and the seeded permutations. The seed participates in the
    // permutation setup; the run RNG counter is untouched by it.
    let mut deck_orders: BTreeMap<DeckId, Vec<CardId>> = BTreeMap::new();
    for (name, decl) in &content.decks {
        state.decks.insert(decl.id.clone(), 0);
        let mut perm: Vec<CardId> = decl.entries.clone();
        let mut ctr = seed ^ story_core::fingerprint::fingerprint(name);
        for i in (1..perm.len()).rev() {
            ctr = ctr.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let j = (splitmix64(ctr) % (i as u64 + 1)) as usize;
            perm.swap(i, j);
        }
        deck_orders.insert(decl.id.clone(), perm);
        if decl.fallback.is_none() && !decl.wrap {
            diags.push(DiagKind::DeckExhausted.warning(format!(
                "deck `{name}` declares neither fallback nor wrap; a draw past its entries will fail its resolution"
            )));
        }
    }

    let verb_recipes = Arc::new(crate::select::build_verb_recipes(&content));

    let engine = Engine {
        registry,
        state,
        seed,
        zone_ids,
        zone_order,
        zone_parent,
        home,
        turn,
        deck_orders,
        content,
        verb_recipes,
    };

    (engine, diags)
}

pub(crate) fn boot(
    content: Arc<Content>,
    registry: crate::EffectRegistry,
    seed: u64,
    legacy_in: Option<BTreeMap<String, i64>>,
) -> (Engine, Vec<RuntimeDiagnostic>) {
    let (mut engine, mut diags) = boot_tables(content, registry, seed, legacy_in);

    // Arm boot instances from the cards' own declarations (mint aliasing
    // never applies at boot), lifetimes armed at t=0. Aspects, kind, and the
    // uniqueness policy all read the own declaration; the `mint <alias>` body
    // applies only to mints (spec.md §4.5).
    let mut card_ids: Vec<(u32, story_core::CardId, story_content::ir::BootFace)> = Vec::new();
    for decl in engine.content.cards.values() {
        if let Some(face) = decl.boot.clone() {
            card_ids.push((decl.decl_index, decl.id.clone(), face));
        }
    }
    card_ids.sort_by_key(|(idx, _, _)| *idx);
    let fctx = EffectCtx {
        sit: None,
        now: 0,
        roll: None,
        handles: BTreeMap::new(),
        continued: None,
    };
    let mut ev = Vec::new();
    for (_, id, face) in card_ids {
        let Some(&z) = engine.zone_ids.get(&face.zone) else {
            continue;
        };
        let Some(own) = engine.content.card(&id).cloned() else {
            continue;
        };
        for _ in 0..face.count {
            if engine.mint_collision(&own.mint, &id) {
                diags.push(DiagKind::NameInUse.warning(format!(
                    "boot instance of `{id}` skipped: a live copy blocks the mint"
                )));
                continue;
            }
            let _ = engine.mint_body(&own, &id, z, &fctx, &mut ev);
        }
    }

    // Boot `relates` edges (spec.md §4.9): after all boot mints, before the
    // t=0 checkpoint, in card declaration order. Subjects are every live
    // entity of the declaring card, objects every live entity of each named
    // card, both in mint order; a named card with no live instance warns and
    // skips. Boot emits no events.
    let mut related_cards: Vec<(u32, story_core::CardId, story_content::ir::CardDecl)> = engine
        .content
        .cards
        .values()
        .filter(|c| !c.relates.is_empty())
        .map(|c| (c.decl_index, c.id.clone(), c.clone()))
        .collect();
    related_cards.sort_by_key(|(idx, _, _)| *idx);
    for (_, id, decl) in related_cards {
        let subjects: Vec<EntityId> = engine
            .state
            .entities
            .values()
            .filter(|e| e.name == id.as_str())
            .map(|e| e.id)
            .collect();
        for (kind, object_names) in &decl.relates {
            let (symmetric, exclusive) = engine
                .content
                .relation_kind(kind)
                .map(|k| (k.symmetric, k.exclusive))
                .unwrap_or((false, false));
            for object_name in object_names {
                let objects: Vec<EntityId> = engine
                    .state
                    .entities
                    .values()
                    .filter(|e| e.name == object_name.as_str())
                    .map(|e| e.id)
                    .collect();
                if objects.is_empty() {
                    diags.push(DiagKind::UnresolvableRef.warning(format!(
                        "boot `relates {kind} {object_name}` on card `{id}` names no live instance; the edge is skipped"
                    )));
                    continue;
                }
                for a in &subjects {
                    for b in &objects {
                        if a == b {
                            continue;
                        }
                        if exclusive {
                            let victims: Vec<story_core::RemovedEdge> = if symmetric {
                                story_core::edges_touching(&engine.state.relations, kind, *a, *b)
                            } else {
                                story_core::edges_into(&engine.state.relations, kind, *b)
                            }
                            .into_iter()
                            .filter(|(f, _, t)| {
                                !(*f == *a && *t == *b) && !(symmetric && *f == *b && *t == *a)
                            })
                            .collect();
                            for (f, k, t) in victims {
                                story_core::unrelate_edge(&mut engine.state.relations, f, &k, t);
                            }
                        }
                        story_core::relate_edge(
                            &mut engine.state.relations,
                            *a,
                            kind,
                            *b,
                            symmetric,
                        );
                    }
                }
            }
        }
    }

    // One autonomous checkpoint at t=0; its events are dropped by design of
    // the boot signature (opens are silent).
    let mut run = Run::new(engine.state.clone());
    let _ = checkpoint(&mut engine, &mut ev, &mut run);

    (engine, diags)
}

fn collect_ops(content: &Content, ops: &mut BTreeSet<String>) {
    fn walk_book(b: &Book, ops: &mut BTreeSet<String>) {
        for r in &b.rules {
            walk_effects(&r.effects, ops);
        }
    }
    fn walk_effects(e: &[story_content::ir::EffectNode], ops: &mut BTreeSet<String>) {
        for n in e {
            walk_node(n, ops);
        }
    }
    fn walk_node(n: &story_content::ir::EffectNode, ops: &mut BTreeSet<String>) {
        match n {
            story_content::ir::EffectNode::Op { name, .. } => {
                ops.insert(name.clone());
            }
            story_content::ir::EffectNode::Branch { then, els, .. } => {
                walk_effects(then, ops);
                walk_effects(els, ops);
            }
            _ => {}
        }
    }
    for v in content.verbs.values() {
        for r in v.recipes.values() {
            walk_effects(&r.effects, ops);
            if let Some(b) = &r.on_complete {
                walk_book(b, ops);
            }
        }
    }
    for c in content.cards.values() {
        for (_, b) in &c.on_complete {
            walk_book(b, ops);
        }
        if let Some(b) = &c.on_expire {
            walk_book(b, ops);
        }
    }
    for b in content.events.values() {
        walk_book(b, ops);
    }
}
