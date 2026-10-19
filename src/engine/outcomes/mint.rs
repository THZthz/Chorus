//! The mint rules (spec §4.2, §4.4): SPAWN and REPLACE — prototype
//! resolution, `name from object` derivation, stackability and the shared
//! name-pool namespace, and the `aspects=`/`attrs=` materialization pairs
//! the fold replays byte-identically.

use crate::core::jsonw::{canonical_key_order, clone_map_in_key_order};
use crate::core::r#ref::{id_ref, name_ref, Ref};
use crate::core::roles::Role;
use crate::core::sentence::{Arg, IntoTarget, Outcome};
use crate::core::types::{
    add_entity, aspect_get, is_truthy, release_name_entry, AspectVal, Aspects, AttrValue, Attrs,
    Entity,
};
use crate::error::{ChorusError, Result};

use super::super::validate::resolve_ref;
use super::{expand, into_arg, materialize_expires, ref_args, ApplyCtx, ApplyResult};
use crate::core::jsonw::{aspects_from_json, aspects_to_json, attrs_from_json, attrs_to_json};

/// The SPAWN case.
pub(super) fn apply_spawn(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    // the first ref arg must be a NAME ref
    let ref_arg = ref_args(args).into_iter().next();
    let Some(Ref::Name { name: proto_name, kind: ref_kind }) = ref_arg else {
        return Err(ChorusError::new("SPAWN needs a name ref (prototype or kind:NAME)"));
    };
    let proto = ctx.content.prototypes.get(proto_name);
    let kind = match ref_kind {
        Some(k) => Some(k.clone()),
        None => proto.map(|p| p.kind.clone()),
    };
    let Some(kind) = kind else {
        return Err(ChorusError::new(format!(
            "SPAWN {proto_name}: no kind (prototype or kind:NAME required)"
        )));
    };
    let mut name = proto_name.clone();
    // the attrs spread is a canonical key-order point (integer-like keys front)
    let mut aspects: Aspects = match proto.and_then(|p| p.aspects.as_ref()) {
        Some(a) => clone_aspects(a),
        None => Aspects::new(),
    };
    let mut attrs: Attrs = match proto.and_then(|p| p.attrs.as_ref()) {
        Some(a) => clone_map_in_key_order(a),
        None => Attrs::new(),
    };
    // derive (`name from object`, §6.4)
    if let Some(derive) = proto.and_then(|p| p.derive.as_ref()) {
        let obj_id = ctx.bindings.get(&Role::Object).and_then(|v| v.first()).cloned();
        let obj = obj_id.and_then(|id| ctx.state.entities.get(&id));
        if let Some(obj) = obj {
            if derive.name_from.as_deref() == Some("object") {
                name = format!("{name}_{}", obj.name);
            }
            if let Some(rules) = &derive.aspects {
                // the derive.aspects walk is a canonical key-order point —
                // the order is observable (aspects insertion order)
                for key in canonical_key_order(rules) {
                    let rule = &rules[&key];
                    if let Some(base) = aspect_get(obj, &rule.from_object_aspect) {
                        // saturating subtraction at the i64 bounds — the
                        // engine's defined overflow direction
                        aspects.insert(key, AspectVal::Num(base.saturating_sub(rule.minus)));
                    }
                }
            }
        }
    }
    // §4.2: the flag comes from the mint's RESOLVED name+kind declaration
    // (not the authored ref's form — the concrete row always carries the
    // resolved kind, and the fold must recompute the same flag) and applies
    // to whatever name the mint ends up carrying (derived names stack when
    // the prototype is declared stackable)
    let is_stack = ctx.content.mint_is_stackable(proto_name, &kind);
    // §4.2: the static card description — the RESOLVED name's declared
    // `desc` (a derive-renamed mint reads the name it ends up carrying, the
    // same discipline the fold's recompute from the concrete row follows)
    let desc = ctx.content.mint_desc(&name).map(str::to_string);
    // §5.1 + §4.2: a non-stackable mint demands a free name; a stackable
    // mint stacks with its own kind of pool but may not collide with a
    // distinct NON-stackable living name (shared namespace)
    check_mint_name_free(ctx, &name, is_stack, "SPAWN")?;
    // an explicit AS id, else the mint generator
    let as_id = args.iter().find_map(|a| match a {
        Arg::AsId { id } => Some(id.clone()),
        _ => None,
    });
    let id = match as_id {
        Some(id) => id,
        None => (ctx.id_gen)(),
    };
    // container: INTO arg, else the `at` binding,
    // else the first actor's `container` attr (raw value, truthiness-gated)
    let into = into_arg(args);
    let container: Option<AttrValue> = if let Some(into_ref) = into {
        match into_ref {
            IntoTarget::Role { role } => {
                ctx.bindings.get(role).and_then(|v| v.first()).map(|id| AttrValue::Str(id.clone()))
            }
            IntoTarget::Ref(r#ref) => {
                resolve_ref(ctx.state, r#ref)?.map(|e| AttrValue::Str(e.id.clone()))
            }
        }
    } else {
        match ctx.bindings.get(&Role::At).and_then(|v| v.first()).cloned() {
            Some(at_id) => Some(AttrValue::Str(at_id)),
            None => ctx
                .bindings
                .get(&Role::Actors)
                .and_then(|v| v.first())
                .and_then(|id| ctx.state.entities.get(id))
                .and_then(|e| e.attrs.get("container"))
                .cloned(),
        }
    };
    // the container attr is written only when truthy (the frozen truthiness)
    if is_truthy(container.as_ref()) {
        if let Some(c) = &container {
            attrs.insert("container".to_string(), c.clone());
        }
    }
    // fold-time reconstruction: a recorded SPAWN
    // (asId present) may carry materialized aspects/attrs as JSON pairs —
    // prefer them over derive. An explicit attrs= pair is authoritative
    // (§6.6): it already carries the mint's location (container and/or
    // zone), so a mint with no location must not grow one from the replay
    // bindings.
    let aspects_arg = args.iter().find_map(|a| match a {
        Arg::Pair { k, v } if k == "aspects" => Some(v),
        _ => None,
    });
    let attrs_arg = args.iter().find_map(|a| match a {
        Arg::Pair { k, v } if k == "attrs" => Some(v),
        _ => None,
    });
    if let Some(AttrValue::Str(json)) = aspects_arg {
        aspects = aspects_from_json(json)?;
    }
    if let Some(AttrValue::Str(json)) = attrs_arg {
        let materialized = attrs_from_json(json)?;
        // the merge keeps existing keys' slots and appends new ones in the
        // source's order (IndexMap::insert matches);
        // the container survives only if the pair carried one
        let had_container = materialized.contains_key("container");
        for (k, v) in materialized {
            attrs.insert(k, v);
        }
        if !had_container {
            attrs.shift_remove("container"); // removal keeps the other keys' slots (order-preserving)
        }
    }
    // the authored mint-decay modifier (§4.4, §4.2): `SPAWN X expires
    // <duration>` arms the mint at BIRTH with the SET-expires pair — the
    // same key, the same one duration parser, the same checked arithmetic
    // (the shared §6.6 helper) — anchored to the application clock
    // (`ctx.now`: the step clock at the table, the window's START clock
    // inside a completion, §6.7). Applied AFTER the attrs= merge, the
    // modifier is authoritative over a literal in an authored same-step
    // pair; the pair rides the concrete row's attrs= materialization
    // (§6.6), so the fold replays byte-identically with no new log
    // surface — the modifier itself is authored-sugar only and never
    // reaches the log. The DEDICATED arg shape (E12 review) means only the
    // authored modifier position matches: a §10 parameter expanded into a
    // k=v pair position lands as a BARE Str/Dur here and is rejected below,
    // never silently read as (or against) the modifier.
    let expires_arg = args.iter().find_map(|a| match a {
        Arg::Expires { v } => Some(v.clone()),
        _ => None,
    });
    if let Some(v) = &expires_arg {
        materialize_expires(&mut attrs, "expires", v, ctx.now, "SPAWN expires")?;
    }
    if let Some(a) = args.iter().find(|a| matches!(a, Arg::Str { .. } | Arg::Dur { .. })) {
        // authored text cannot produce a bare str/dur in SPAWN args (the
        // modifier is its own shape, pairs are `k=v`), so one of these is
        // always fn-expansion residue: the caller's bound value spliced for
        // a whole pair. Previously this dropped the arg INERTLY — the
        // author's intent silently lost; now it fails loud (§9.2).
        return Err(ChorusError::new(format!(
            "SPAWN: a bare {} arg is not a SPAWN arg shape — a §10 parameter standing in a k=v \
             pair position expands to the caller's whole bound value; author the pair value \
             directly (the expires modifier is its own `expires <duration>` shape, §4.4)",
            a.tag()
        )));
    }
    add_entity(
        ctx.state,
        Entity {
            id: id.clone(),
            kind: kind.clone(),
            name: name.clone(),
            aspects: aspects.clone(),
            attrs: attrs.clone(),
            born_action: Some(ctx.record_id),
            dead_action: None,
            stackable: is_stack,
            desc,
        },
    )?;
    // concrete outcome: everything a fold needs,
    // nothing from the step context
    let mut concrete_args: Vec<Arg> = vec![Arg::Ref { r#ref: name_ref(name, Some(kind)) }];
    if is_truthy(container.as_ref()) {
        if let Some(c) = &container {
            // `id_ref` over the string-cast value
            concrete_args
                .push(Arg::Into { r#ref: IntoTarget::Ref(id_ref(c.to_canonical_string())) });
        }
    }
    concrete_args.push(Arg::AsId { id });
    // The pair STRINGS are built through the canonical key order (the
    // `SET(x,"5",…)` path can put integer-like keys into attrs).
    if !aspects.is_empty() {
        concrete_args.push(Arg::Pair {
            k: "aspects".to_string(),
            v: AttrValue::Str(aspects_to_json(&aspects)),
        });
    }
    if !attrs.is_empty() {
        concrete_args
            .push(Arg::Pair { k: "attrs".to_string(), v: AttrValue::Str(attrs_to_json(&attrs)) });
    }
    Ok(ApplyResult {
        concrete: vec![Outcome::concrete("SPAWN".to_string(), concrete_args)],
        refused: false,
    })
}

/// The REPLACE case: §4.4 REPLACE ref WITH
/// prototype — mint from the prototype into the dying card's CURRENT
/// container, then soft-destroy it. The log carries the concrete SPAWN+
/// DESTROY pair, so the fold replays it exactly (§4.4).
pub(super) fn apply_replace(args: &[Arg], ctx: &mut ApplyCtx) -> Result<ApplyResult> {
    let target_arg = args.first();
    let proto_arg = args.get(1);
    let proto_name = match proto_arg {
        Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => name.clone(),
        _ => {
            return Err(ChorusError::new(
                "REPLACE needs (ref|ROLE) WITH a prototype or kind:NAME ref",
            ));
        }
    };
    // plain prototype mint — REPLACE has no clause, so prototype derivation
    // (`name from object`, §6.4) is unavailable here (§4.4)
    let proto = ctx.content.prototypes.get(&proto_name);
    let kind = match proto_arg {
        Some(Arg::Ref { r#ref: Ref::Name { kind: Some(k), .. } }) => Some(k.clone()),
        _ => proto.map(|p| p.kind.clone()),
    };
    let Some(kind) = kind else {
        return Err(ChorusError::new(format!(
            "REPLACE {proto_name}: no kind (prototype or kind:NAME required)"
        )));
    };
    // §4.2: same computation as SPAWN — the RESOLVED name+kind, so the
    // fold's recompute from the concrete SPAWN row agrees (a stackable
    // successor may share the dying card's name: REPLACE X WITH X succeeds —
    // spawn appends, destroy pops); the successor's desc comes off the same
    // resolved name (REPLACE never derives)
    let is_stack = ctx.content.mint_is_stackable(&proto_name, &kind);
    let desc = ctx.content.mint_desc(&proto_name).map(str::to_string);
    let aspects_tpl: Aspects = match proto.and_then(|p| p.aspects.as_ref()) {
        Some(a) => clone_aspects(a),
        None => Aspects::new(),
    };
    let targets: Vec<String> = match target_arg {
        Some(a) => expand(std::slice::from_ref(a), ctx),
        // unreachable through authored paths (parse enforces the arity);
        // the empty-target error below is the degenerate shape's destination
        // either way
        None => Vec::new(),
    };
    if targets.is_empty() {
        return Err(ChorusError::new("REPLACE: unresolvable ref (nothing to replace)"));
    }
    let mut concrete: Vec<Outcome> = Vec::new();
    for dying_id in targets {
        // spawn happens while the dying card is still alive, so a
        // non-stackable successor's name collides with any living holder —
        // whole-step rejection (§4.4); a stackable successor may share a
        // stackable pool
        check_mint_name_free(ctx, &proto_name, is_stack, "REPLACE")?;
        let id = (ctx.id_gen)();
        let mut attrs: Attrs = match proto.and_then(|p| p.attrs.as_ref()) {
            Some(a) => clone_map_in_key_order(a),
            None => Attrs::new(),
        };
        // container / zone ride off the dying card's CURRENT attrs
        let (container, zone, dying_name) = {
            let Some(dying) = ctx.state.entities.get(&dying_id) else {
                continue; // unreachable under §7's soft destroy
            };
            let container = match dying.attrs.get("container") {
                Some(AttrValue::Str(s)) => Some(s.clone()),
                _ => None,
            };
            // zone-tagged (§10): the grouping has no card, so the tag passes
            // on — as a plain attr pair, NOT as INTO (the fold's SPAWN replay
            // would materialize a container the mint never had)
            let zone = if container.is_none() {
                match dying.attrs.get("zone") {
                    Some(AttrValue::Str(s)) => Some(s.clone()),
                    _ => None,
                }
            } else {
                None
            };
            (container, zone, dying.name.clone())
        };
        if let Some(c) = &container {
            attrs.insert("container".to_string(), AttrValue::Str(c.clone()));
        }
        if let Some(z) = &zone {
            attrs.insert("zone".to_string(), AttrValue::Str(z.clone()));
        }
        add_entity(
            ctx.state,
            Entity {
                id: id.clone(),
                kind: kind.clone(),
                name: proto_name.clone(),
                aspects: clone_aspects(&aspects_tpl),
                attrs: attrs.clone(),
                born_action: Some(ctx.record_id),
                dead_action: None,
                stackable: is_stack,
                desc: desc.clone(),
            },
        )?;
        // soft-destroy AFTER the spawn
        if let Some(d) = ctx.state.entities.get_mut(&dying_id) {
            d.dead_action = Some(ctx.record_id);
        }
        release_name_entry(ctx.state, &dying_name, &dying_id);
        let mut spawn_args: Vec<Arg> =
            vec![Arg::Ref { r#ref: name_ref(proto_name.clone(), Some(kind.clone())) }];
        if let Some(c) = &container {
            spawn_args.push(Arg::Into { r#ref: IntoTarget::Ref(id_ref(c.clone())) });
        }
        spawn_args.push(Arg::AsId { id });
        if !aspects_tpl.is_empty() {
            spawn_args.push(Arg::Pair {
                k: "aspects".to_string(),
                v: AttrValue::Str(aspects_to_json(&aspects_tpl)),
            });
        }
        // always materialize attrs, even when empty (§6.6): the pair pins the
        // mint's location — with no pair the fold would re-add the bound
        // actor's container to a location-less successor
        spawn_args
            .push(Arg::Pair { k: "attrs".to_string(), v: AttrValue::Str(attrs_to_json(&attrs)) });
        concrete.push(Outcome::concrete("SPAWN".to_string(), spawn_args));
        concrete.push(Outcome::concrete(
            "DESTROY".to_string(),
            vec![Arg::Ref { r#ref: id_ref(dying_id.clone()) }],
        ));
    }
    Ok(ApplyResult { concrete, refused: false })
}

/// §5.1 + §4.2: a non-stackable mint demands a free name; a stackable
/// mint stacks with its own kind of pool but may not collide with a
/// distinct NON-stackable living name (shared namespace). `what` names the
/// op for the pinned message ("SPAWN" / "REPLACE").
fn check_mint_name_free(ctx: &ApplyCtx, name: &str, is_stack: bool, what: &str) -> Result<()> {
    if let Some(pool) = ctx.state.by_name.get(name) {
        if !pool.is_empty() {
            let all_stackable =
                pool.iter().all(|pid| ctx.state.entities.get(pid).is_some_and(|e| e.stackable));
            if !is_stack || !all_stackable {
                return Err(ChorusError::new(format!("{what}: name '{name}' already in use")));
            }
        }
    }
    Ok(())
}

/// The aspects clone: the top-level walk AND each sub-map clone are
/// canonical key-order points (the canonical order fronts integer-like keys).
fn clone_aspects(a: &Aspects) -> Aspects {
    let mut out = Aspects::new();
    for k in canonical_key_order(a) {
        if let Some(v) = a.get(&k) {
            out.insert(
                k,
                match v {
                    AspectVal::Num(n) => AspectVal::Num(*n),
                    AspectVal::Sub(m) => AspectVal::Sub(clone_map_in_key_order(m)),
                },
            );
        }
    }
    out
}
