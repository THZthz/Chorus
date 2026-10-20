//! The content compiler: [`compile_content`] turns a [`ContentRaw`] into a
//! [`CompiledContent`] — parsing every query/predicate/outcome body once,
//! running the §10 call-table expansion (functions AND script ops) and the
//! load-time lints
//! (§6.3 dice, §4.4 ref checks), so nothing malformed reaches the engine.

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use super::deck::compile_decks;
use super::functions::{build_call_tables, parse_outcome_string};
use super::model::*;
use crate::core::dict::Dictionary;
use crate::core::fn_expand::FnTable;
use crate::core::jsonw::{canonical_key_order, clone_map_in_key_order};
use crate::core::predicate::{
    lint_pred, lint_roll_may_bind, lint_trigger_pred, lint_trigger_val_expr_what, lint_val_expr,
    parse_predicate, Pred,
};
use crate::core::query::parse_query;
use crate::core::r#ref::Ref;
use crate::core::regex_util::static_regex;
use crate::core::sentence::{Arg, IfArm, Outcome};
use crate::core::text::dsl_trim;
use crate::error::{ChorusError, Result};

// ---------------------------------------------------------------------------
// compile_content
// ---------------------------------------------------------------------------

/// The compile-side key-order walks: the canonical order fronts integer-like
/// keys, so every one of these goes through
/// [`entries_in_key_order`]. Integer-like key names are legal —
/// reproduce the order, do not reject them.
fn entries_in_key_order<V>(m: &IndexMap<String, V>) -> Vec<(&str, &V)> {
    let keys = canonical_key_order(m);
    let mut out = Vec::with_capacity(keys.len());
    for k in &keys {
        // re-borrow both sides from m so the returned refs outlive `keys`
        if let Some((k, v)) = m.get_key_value(k) {
            out.push((k.as_str(), v));
        }
    }
    out
}

/// The stackable-set builder: the compile side, the script entity side,
/// and the lint's own copy all share ONE builder. Set
/// membership is order-independent, so the build order here (entities first,
/// then prototypes) changes no membership
/// result anywhere.
pub(crate) fn collect_stackable(
    entities: &[DeclaredEntity],
    prototypes: Option<&IndexMap<String, PrototypeDef>>,
) -> (IndexSet<String>, IndexSet<String>) {
    let mut names = IndexSet::new();
    let mut kinds = IndexSet::new();
    for e in entities {
        if e.stackable {
            names.insert(e.name.clone());
            kinds.insert(e.kind.clone());
        }
    }
    if let Some(prototypes) = prototypes {
        for (name, p) in entries_in_key_order(prototypes) {
            if p.stackable {
                names.insert(name.to_string());
                kinds.insert(p.kind.clone());
            }
        }
    }
    (names, kinds)
}

/// The §4.2 unique-set builder: the prototypes carrying `unique` contribute
/// their NAMES. Uniqueness attaches to the name (a derive-renamed mint
/// escapes it; kind-mates are unaffected), so there is no kind set beside
/// it. Entity lines reject `unique` at parse, so prototypes are the only
/// source. Set membership is order-independent, so the walk order here
/// changes no result anywhere.
pub(crate) fn collect_uniques(
    prototypes: Option<&IndexMap<String, PrototypeDef>>,
) -> IndexSet<String> {
    let mut names = IndexSet::new();
    if let Some(prototypes) = prototypes {
        for (name, p) in entries_in_key_order(prototypes) {
            if p.unique {
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// The §4.2 unique-GROUP table: the prototypes carrying `uniquegroup <NAME>`
/// contribute (GROUP → member NAMES) in first-member declaration order.
/// Membership attaches to the name (a derive-renamed mint escapes it, the
/// `unique` sets' discipline); a name can carry at most one membership (the
/// grammar rejects a second `uniquegroup` keyword on one line, and
/// prototype names are unique), so the group sets are pairwise disjoint and
/// the build order here is unobservable.
pub(crate) fn collect_unique_groups(
    prototypes: Option<&IndexMap<String, PrototypeDef>>,
) -> IndexMap<String, IndexSet<String>> {
    let mut groups: IndexMap<String, IndexSet<String>> = IndexMap::new();
    if let Some(prototypes) = prototypes {
        for (name, p) in entries_in_key_order(prototypes) {
            if let Some(group) = &p.unique_group {
                groups.entry(group.clone()).or_default().insert(name.to_string());
            }
        }
    }
    groups
}

/// The §4.2 description table: the declared entity and prototype names
/// carrying a `desc "…"`. Keyed by NAME (an entity/prototype name collision
/// is a load error, so the keys are unique); insertion order is
/// unobservable — only `mint_desc` membership is ever read, and the build
/// order here (entities first, then prototypes, mirroring
/// [`collect_stackable`]) changes no result anywhere.
fn collect_descs(
    entities: &[DeclaredEntity],
    prototypes: Option<&IndexMap<String, PrototypeDef>>,
) -> IndexMap<String, String> {
    let mut descs = IndexMap::new();
    for e in entities {
        if let Some(d) = &e.desc {
            descs.insert(e.name.clone(), d.clone());
        }
    }
    if let Some(prototypes) = prototypes {
        for (name, p) in entries_in_key_order(prototypes) {
            if let Some(d) = &p.desc {
                descs.insert(name.to_string(), d.clone());
            }
        }
    }
    descs
}

/// Compile raw content. `entities` contributes the
/// script's declared entity lines so deck spec entries (§4.4) may name
/// plain entities as well as prototypes; `compile_script` passes them,
/// direct callers without entities pass `&[]` (deck entries validate
/// against prototypes only).
pub fn compile_content(
    raw: &ContentRaw,
    dict: &Dictionary,
    entities: &[DeclaredEntity],
) -> Result<CompiledContent> {
    // §10 FIRST, before any verb/trigger body parses: build the combined
    // call table (functions AND script ops) — call sites dispatch
    // against it and expand
    // immediately after their parse, so the guards below walk expanded lists.
    let fns = build_call_tables(raw, dict)?;
    // §4.2: two derived sets every mint site reads
    let (stackable_names, stackable_kinds) = collect_stackable(entities, raw.prototypes.as_ref());
    // §4.2: …and the unique names beside them
    let unique_names = collect_uniques(raw.prototypes.as_ref());
    // §4.2: …and the unique groups (GROUP → member names) with their
    // one order-independent load check: a group id that IS a card name —
    // a declared entity or prototype — would make the manifested sets
    // ambiguous (a manifested_uniques entry and a manifested_groups entry
    // would share one string with different meanings). Entity/prototype
    // declarations are all in hand here, so the check is order-independent
    // however the sections were listed.
    let unique_groups = collect_unique_groups(raw.prototypes.as_ref());
    for group in unique_groups.keys() {
        let entity_hit = entities.iter().any(|e| &e.name == group);
        let proto_hit = raw.prototypes.as_ref().is_some_and(|ps| ps.contains_key(group));
        if entity_hit || proto_hit {
            return Err(ChorusError::new(format!(
                "uniquegroup group id '{group}' is also a declared entity/prototype name — a group id must not be a card name (§4.2)"
            )));
        }
    }
    // §4.2: …and the description table they mint from
    let descs = collect_descs(entities, raw.prototypes.as_ref());
    let endings = compile_endings(raw.endings.as_ref())?;
    // §4.4: the deck table — spec entries resolve against prototypes +
    // declared entities, then the seed-fixed permutation is derived
    let decks = compile_decks(
        raw.decks.as_deref().unwrap_or(&[]),
        raw.seed.unwrap_or(0),
        dict,
        raw.prototypes.as_ref().unwrap_or(&IndexMap::new()),
        entities,
    )?;
    let mut verbs = IndexMap::new();
    for (name, v) in entries_in_key_order(&raw.verbs) {
        let compiled = compile_verb(name, v, dict, &fns)?;
        verbs.insert(name.to_string(), compiled);
    }
    // §6.7: the global recipe-id table
    let recipe_verbs = build_recipe_verbs(&verbs)?;
    let mut triggers = Vec::new();
    for t in raw.triggers.as_deref().unwrap_or(&[]) {
        triggers.push(compile_trigger(t, dict, &fns, &recipe_verbs)?);
    }
    // canonical key order via the key-order clone
    let prototypes = clone_map_in_key_order(raw.prototypes.as_ref().unwrap_or(&IndexMap::new()));
    let mut templates = IndexMap::new();
    for (name, tpl) in entries_in_key_order(raw.templates.as_ref().unwrap_or(&IndexMap::new())) {
        if !dict.is_verb(name) {
            return Err(ChorusError::new(format!(
                "template '{name}' is not a registered verb — templates configure verbs, same as the verbs: section"
            )));
        }
        templates.insert(name.to_string(), tpl.clone());
    }
    // §4.4: an ENDING outcome must name a declared ending — recipe or
    // trigger, checked once at load. §4.4: the same walk pins DRAW to a
    // declared deck. §6.7: a RUN candidate must name a compiled recipe —
    // the GLOBAL table, recipe bodies and completion triggers alike —
    // checked after every verb is compiled so a candidate may name any
    // verb's recipe, position notwithstanding.
    for v in verbs.values() {
        for r in &v.recipes {
            let where_ = format!("recipe {}", r.id);
            check_named_ref("ENDING", &where_, &r.out, &endings, |where_, shown| {
                format!("{where_}: ENDING '{shown}' is not in the endings table")
            })?;
            check_named_ref("DRAW", &where_, &r.out, &decks, |where_, shown| {
                format!("{where_}: DRAW '{shown}' is not a declared deck (decks:)")
            })?;
            check_run_scope("RUN", &where_, &r.out, &recipe_verbs)?;
            check_run_scope("FORK", &where_, &r.out, &recipe_verbs)?;
        }
    }
    for t in &triggers {
        let where_ = format!("trigger {}", t.id());
        check_named_ref("ENDING", &where_, t.out(), &endings, |where_, shown| {
            format!("{where_}: ENDING '{shown}' is not in the endings table")
        })?;
        check_named_ref("DRAW", &where_, t.out(), &decks, |where_, shown| {
            format!("{where_}: DRAW '{shown}' is not a declared deck (decks:)")
        })?;
    }
    Ok(CompiledContent {
        seed: raw.seed.unwrap_or(0),
        verbs,
        triggers,
        prototypes,
        templates,
        endings,
        stackable_names,
        stackable_kinds,
        unique_names,
        unique_groups,
        descs,
        decks,
        recipe_verbs,
    })
}

/// Compile the ending table (§4.4): UPPER_SNAKE ids, a non-blank title,
/// flavour resolved (default 'melancholy', §10).
fn compile_endings(
    raws: Option<&IndexMap<String, EndingDef>>,
) -> Result<IndexMap<String, CompiledEnding>> {
    let mut endings = IndexMap::new();
    for (id, e) in entries_in_key_order(raws.unwrap_or(&IndexMap::new())) {
        let re = upper_snake_re();
        if !re.is_match(id) {
            return Err(ChorusError::new(format!("ending '{id}' must be UPPER_SNAKE")));
        }
        if e.title.is_empty() || dsl_trim(&e.title).is_empty() {
            return Err(ChorusError::new(format!("ending {id}: needs a title")));
        }
        let flavour = e.flavour.clone().unwrap_or_else(|| "melancholy".to_string());
        let re = flavour_re();
        if !re.is_match(&flavour) {
            return Err(ChorusError::new(format!(
                "ending {id}: flavour must be a lowercase word, got '{flavour}'"
            )));
        }
        endings.insert(id.to_string(), CompiledEnding { title: e.title.clone(), flavour });
    }
    Ok(endings)
}

/// Compile one verb: the slots (max ≥ min, the frozen ''-query truthiness),
/// the recipes (the frozen ''-when truthiness, the §6.4 `max N ≥ 1` guard,
/// the one shared id mint), then the §4.5 recipe-side lint over every `when`
/// AND every IF condition.
fn compile_verb(
    name: &str,
    v: &VerbContent,
    dict: &Dictionary,
    fns: &FnTable,
) -> Result<CompiledVerb> {
    if !dict.is_verb(name) {
        return Err(ChorusError::new(format!("verb '{name}' is not registered in the dictionary")));
    }
    let name_lower = name.to_lowercase();
    let mut slot_by_role = IndexMap::new();
    let mut slots = Vec::with_capacity(v.slots.len());
    // §6.9: an ambient verb declares NO player-facing slots — the walk binds
    // nothing (empty bindings) and a commit refuses, so a slot would be dead
    // weight the hold could never reach either (the held slot included: the
    // deferral mark is a held-slot shape, and an ambient verb never defers)
    if v.ambient {
        if let Some(s) = v.slots.first() {
            return Err(ChorusError::new(format!(
                "verb {name}: an ambient verb declares no slots — the §6.9 walk opens its windows over empty bindings (got slot '{}')",
                s.role
            )));
        }
    }
    for s in &v.slots {
        // `s.max < s.min`: Infinity is never
        // less than anything, so the check only runs for a finite max
        if let Some(max) = s.max {
            if max < s.min {
                return Err(ChorusError::new(format!(
                    "verb {name}: slot {} has max < min",
                    s.role
                )));
            }
        }
        // §6.1: `greedy`/`consuming` are held-only flags — on any other role
        // they would compile a slot whose faces (the begin pull, the
        // completion burn) have no home.
        if s.role != "held" {
            for (flag, set) in [("greedy", s.greedy), ("consuming", s.consuming)] {
                if set {
                    return Err(ChorusError::new(format!(
                        "verb {name}: '{flag}' is a held-only flag — slot {} cannot take it (§6.1)",
                        s.role
                    )));
                }
            }
        }
        // §6.1: the held slot is a real slot class now — min stays 0 (a hold
        // is optional per card) and the max must leave room for one binding
        // (a finite max >= 1, or the unbounded `0..`). `held: 1 …`,
        // `held: 1.. …` and `held: 0..0` fail loud instead of compiling a
        // slot the hold step could never legally fill.
        if s.role == "held" && (s.min != 0 || s.max == Some(0)) {
            return Err(ChorusError::new(format!(
                "verb {name}: the held slot takes '0..N' with N >= 1, or the unbounded '0..' — a hold is optional per card and the slot must have room for one (§6.1), got '{}..{}'",
                s.min,
                s.max.map_or("∞".to_string(), |m| m.to_string())
            )));
        }
        // §6.1: a greedy slot pulls fitting cards at window begin — pulling
        // needs a rule (a query) and a bound (a finite max; an unbounded
        // greedy vacuums the table).
        if s.role == "held" && s.greedy {
            if s.query.as_deref().is_none_or(|q| q.is_empty()) {
                return Err(ChorusError::new(format!(
                    "verb {name}: the greedy held slot needs a query — pulling \"any card\" is not a rule (§6.1)"
                )));
            }
            if s.max.is_none() {
                return Err(ChorusError::new(format!(
                    "verb {name}: the greedy held slot needs a finite max — an unbounded pull vacuums the table (§6.1)"
                )));
            }
        }
        let compiled = CompiledSlot {
            role: s.role.clone(),
            min: s.min,
            max: s.max,
            shown: s.shown.unwrap_or(true),
            greedy: s.greedy,
            consuming: s.consuming,
            // the frozen truthiness of an empty string:
            // '' is FALSY, so a hand-built ContentRaw with
            // query: '' compiles to NO query (not a parse error); parse-
            // produced queries are never '' (the script grammar's
            // `query\s+(.*)` capture is absent for a bare count spec),
            // so this is the exact mapping.
            query: match s.query.as_deref() {
                Some(q) if !q.is_empty() => Some(parse_query(q, dict)?),
                _ => None,
            },
        };
        slot_by_role.insert(s.role.clone(), compiled.clone());
        slots.push(compiled);
    }
    let mut recipes = Vec::with_capacity(v.recipes.len());
    for (i, r) in v.recipes.iter().enumerate() {
        // BOTH the id mint and the when parse read `r.when` through the frozen truthiness:
        // '' is FALSY, so a hand-built
        // ContentRaw with when: '' rides as the DEFAULT recipe (id
        // `<verb>.default`, no parse error); the lint mirror
        // reads the same truthiness. parse-produced whens
        // are never '' (the script grammar's `when\s+(.+)` requires
        // content).
        let raw_when = r.when.as_deref().filter(|w| !w.is_empty());
        let when = match raw_when {
            Some(w) => Some(parse_predicate(w, dict)?),
            None => None,
        };
        // id-less recipes mint the 0-based `.recipe<i>` (§6.4) — the
        // one mint, shared verbatim by the lint mirror (Q1 unified)
        let id = r.id.clone().unwrap_or_else(|| mint_recipe_id(&name_lower, raw_when, i));
        // §6.4: N ≥ 1 — a `max 0` (or negative) can never fire, so it
        // fails loudly at load instead (the script parser checks its own
        // token; this guards the code-level ContentRaw face, like the
        // slot max<min check above)
        if let Some(m) = r.max {
            if m < 1 {
                return Err(ChorusError::new(format!("verb {name}: recipe {id} has max < 1")));
            }
        }
        let mut out = Vec::new();
        for o in &r.out {
            out.extend(parse_outcome_string(o, dict, fns)?);
        }
        // §6.9: a top-level REFUSE in an ambient recipe is a load error —
        // its home is the §6.5 table, which an ambient open never sees; the
        // gate is `when` (the walk opens nothing when no recipe matches). A
        // REFUSE inside an IF branch keeps the §6.7 window-refuse, exactly
        // like every completion (only the top level is banned).
        if v.ambient && out.iter().any(|o| o.op == "REFUSE") {
            return Err(ChorusError::new(format!(
                "verb {name}: recipe {id}: a top-level REFUSE has no step to refuse on an ambient verb — gate with `when` (the §6.9 walk opens nothing when no recipe matches)"
            )));
        }
        recipes.push(CompiledRecipe {
            id,
            when,
            shown: r.shown.unwrap_or(false),
            hint: r.hint.clone(),
            max: r.max,
            aspects: r.aspects.clone(),
            dur: r.dur,
            out,
        });
    }
    // §4.5 load-time lint: count()/total() args must be declared roles
    // or queries — over every `when` AND every IF condition
    for r in &recipes {
        let where_ = format!("recipe {}", r.id);
        if let Some(w) = &r.when {
            lint_pred(&where_, w)?;
        }
        for_each_if_condition(&r.out, &mut |w| lint_pred(&where_, w))?;
        lint_dice(&where_, &r.out)?;
    }
    Ok(CompiledVerb {
        name: name.to_string(),
        slots,
        slot_by_role,
        duration: v.duration,
        recipes,
        ambient: v.ambient,
    })
}

/// The global recipe-id table: recipe id → owning verb name. RUN/FORK
/// candidates resolve against it in EVERY home — recipe bodies and trigger
/// bodies alike — so the build is where a cross-verb collision fails loud.
/// Same-verb duplicates within one verb remain first-match selection (§6.4).
fn build_recipe_verbs(verbs: &IndexMap<String, CompiledVerb>) -> Result<IndexMap<String, String>> {
    let mut recipe_verbs: IndexMap<String, String> = IndexMap::new();
    for v in verbs.values() {
        for r in &v.recipes {
            match recipe_verbs.get(&r.id) {
                Some(prev) if prev != &v.name => {
                    return Err(ChorusError::new(format!(
                        "recipe id '{}' is declared by both verb {prev} and verb {} — chain-op resolution needs globally unique recipe ids (§6.7)",
                        r.id, v.name
                    )));
                }
                _ => {
                    recipe_verbs.insert(r.id.clone(), v.name.clone());
                }
            }
        }
    }
    Ok(recipe_verbs)
}

/// Compile one trigger: the body parses and expands (§10), then the guards —
/// completion triggers (§6.7) run under a different role-free-lint regime
/// than the event kinds (§6.6), the dice lint applies everywhere, and every
/// kind's chain candidates resolve against the global recipe table.
fn compile_trigger(
    t: &TriggerDef,
    dict: &Dictionary,
    fns: &FnTable,
    recipe_verbs: &IndexMap<String, String>,
) -> Result<CompiledTrigger> {
    // §10: expansion (inside parse_outcome_string) has already run —
    // these guards see function-expanded bodies exactly like literal ones
    let mut out = Vec::new();
    for o in t.out() {
        out.extend(parse_outcome_string(o, dict, fns)?);
    }
    let tid = t.id().to_string();
    // completion triggers (§6.7) run under a different guard regime than
    // the event kinds: the role-free lint does not run on them — the
    // window's bindings are live there. REFUSE stays banned in every home
    // (§6.6); the chain ops are legal in ALL three kinds since the §6.6
    // event-chain ruling (their candidates resolve against the global
    // table below).
    let is_complete = matches!(t, TriggerDef::Complete(_));
    // §6.7/§6.6 guards — recursive over IF branch bodies (§6.8)
    for_each_authored_outcome(&out, &mut |o| {
        // §6.7: PROGRESS narrates a processing window; events have none
        if o.op == "PROGRESS" {
            return Err(ChorusError::new(if is_complete {
                format!(
                    "trigger {tid}: PROGRESS belongs to the completing recipe — completion triggers fire after the outcome list (§6.7)"
                )
            } else {
                format!("trigger {tid}: PROGRESS belongs to verb recipes, not events")
            }));
        }
        // §6.6: REFUSE aborts a step (§6.5); events have no step to refuse
        // — and a completion trigger cannot refuse the completion it
        // fires inside of either (§6.7 keeps the ban in all homes)
        if o.op == "REFUSE" {
            return Err(ChorusError::new(if is_complete {
                format!(
                    "trigger {tid}: REFUSE belongs to verb recipes — a completion trigger cannot refuse the completion it fires in (§6.6, §6.7)"
                )
            } else {
                format!(
                    "trigger {tid}: REFUSE belongs to verb recipes — events have no step to refuse"
                )
            }));
        }
        // §6.6/§6.7: ADVANCE stretches the advance pass a window
        // completion rides (the completion ctx's extra_advance_min feeds
        // the pass target); the event kinds own no pass, so ADVANCE in
        // one is a load error. A completion trigger fires inside a pass
        // and keeps ADVANCE.
        if o.op == "ADVANCE" && !is_complete {
            return Err(ChorusError::new(format!(
                "trigger {tid}: ADVANCE belongs to verb recipes — events own no advance pass to stretch"
            )));
        }
        Ok(())
    })?;
    // §6.3: event bindings are empty, so a role-dependent operand in a
    // trigger IF condition would silently read false/0. Completion-trigger
    // bodies see the WINDOW's re-resolved bindings, so the role-free lint
    // does NOT run on them — only the recipe-side count()/total() lint.
    for_each_if_condition(&out, &mut |w| {
        lint_pred(&format!("trigger {tid}"), w)?; // the count-typo lint applies here too
        if !is_complete {
            lint_trigger_pred(&format!("trigger {tid}"), w)?;
        }
        Ok(())
    })?;
    // §4.4: trigger ROLL arguments must be role-FREE — except in
    // completion triggers, whose window bindings are live (the dice lint
    // in lint_dice still applies)
    for_each_authored_outcome(&out, &mut |o| {
        if o.op == "ROLL" && !is_complete {
            for a in &o.args {
                if let Arg::Vexpr { v } = a {
                    lint_trigger_val_expr_what(&format!("trigger {tid}"), v, "ROLL argument")?;
                }
            }
        }
        Ok(())
    })?;
    lint_dice(&format!("trigger {tid}"), &out)?;
    // §6.7/§6.6: a trigger's chain candidates resolve against the GLOBAL
    // recipe table in all three kinds (cross-verb legal; a collision already
    // failed loud in the table build above) — an unknown id is a load error,
    // IF branches included (the §4.4 walk). Event-trigger bodies see the
    // empty-bindings vocabulary: their follow-ups lock nothing, and a
    // candidate `when:` reading roles never matches (§6.6).
    check_run_scope("RUN", &format!("trigger {tid}"), &out, recipe_verbs)?;
    check_run_scope("FORK", &format!("trigger {tid}"), &out, recipe_verbs)?;
    match t {
        TriggerDef::Expire(e) => Ok(CompiledTrigger::Expire {
            id: e.id.clone(),
            on_expire_of: e.on_expire_of.clone(),
            out,
        }),
        TriggerDef::Slot(s) => Ok(CompiledTrigger::Slot {
            id: s.id.clone(),
            slot: s.slot,
            once: s.once.unwrap_or(false),
            out,
        }),
        TriggerDef::Complete(c) => Ok(CompiledTrigger::Complete {
            id: c.id.clone(),
            home: c.home.clone(),
            aspect: c.aspect.clone(),
            out,
        }),
    }
}

/// The ONE authored-outcome
/// traversal both consumers share (the load guards and the reach analyzer):
/// a single implementation keeps the reach 'none' verdict's soundness
/// argument — exhaustive enumeration in this order —
/// structural instead of hopeful.
///
/// The recursion is parameterized by a per-branch CONTEXT (the reach side's
/// `(path, segs, fallback)` triple, `()` for the guards)
/// and a walker-STATE `S` threaded through every callback by `&mut` (the
/// reach side's flags/seg-counter/sites accumulators — one state object, so
/// no closure captures it twice). `arm_ctx`/`else_ctx` derive the child
/// context per arm, left to right, BEFORE that arm descends — so the reach
/// consumer's allocation order is preserved call for call. The visitor is
/// fallible (the load guards throw); Rust models the throw as `Err`.
pub fn walk_outcomes<S, C: Clone>(
    out: &[Outcome],
    ctx: &C,
    state: &mut S,
    visit: &mut dyn FnMut(&mut S, &Outcome, &C) -> Result<()>,
    arm_ctx: &mut dyn FnMut(&mut S, &IfArm, &C) -> C,
    else_ctx: &mut dyn FnMut(&mut S, &C) -> C,
) -> Result<()> {
    for o in out {
        visit(state, o, ctx)?;
        if o.op == "IF" {
            if let Some(arms) = &o.arms {
                for arm in arms {
                    let child = arm_ctx(state, arm, ctx);
                    walk_outcomes(&arm.then, &child, state, visit, arm_ctx, else_ctx)?;
                }
            }
            if let Some(fallback) = &o.fallback {
                let child = else_ctx(state, ctx);
                walk_outcomes(fallback, &child, state, visit, arm_ctx, else_ctx)?;
            }
        }
    }
    Ok(())
}

/// Visit every outcome in an authored list,
/// RECURSING into IF branch bodies (§6.8) — the single entry point for the
/// load-time guards, so nothing escapes by nesting. Function expansion
/// happens BEFORE these guards run. The visitor is
/// fallible (the guards throw); Rust models the throw as `Err`.
///
/// The context-free face of [`walk_outcomes`] — the guard consumers carry no
/// per-branch state, so their context and walker state are both `()`.
pub fn for_each_authored_outcome(
    out: &[Outcome],
    visit: &mut dyn FnMut(&Outcome) -> Result<()>,
) -> Result<()> {
    walk_outcomes(
        out,
        &(),
        &mut (),
        &mut |_state, o, ()| visit(o),
        &mut |_state, _arm, ()| (),
        &mut |_state, ()| (),
    )
}

/// Visit every outcome in an authored list, RECURSING into IF branch bodies
/// (§6.8) — the INFALLIBLE face of [`for_each_authored_outcome`], for the
/// read-only scans (the lint's) whose visitor cannot fail.
pub fn for_each_authored_outcome_infallible(out: &[Outcome], visit: &mut dyn FnMut(&Outcome)) {
    let _ = for_each_authored_outcome(out, &mut |o| {
        visit(o);
        Ok(())
    });
}

/// Visit every IF condition in an authored list
/// (branch bodies included).
fn for_each_if_condition(
    out: &[Outcome],
    visit: &mut dyn FnMut(&Pred) -> Result<()>,
) -> Result<()> {
    for_each_authored_outcome(out, &mut |o| {
        if o.op == "IF" {
            if let Some(arms) = &o.arms {
                for arm in arms {
                    visit(&arm.when)?;
                }
            }
        }
        Ok(())
    })
}

/// §4.4 load lints shared by recipe and
/// trigger outcome lists, all run AFTER function expansion.
fn lint_dice(where_: &str, out: &[Outcome]) -> Result<()> {
    for_each_authored_outcome(out, &mut |o| {
        if o.op == "ROLLED" {
            return Err(ChorusError::new(format!(
                "{where_}: ROLLED is engine-written — author ROLL and the log records the concrete result (§4.4)"
            )));
        }
        if o.op == "DRAWN" {
            return Err(ChorusError::new(format!(
                "{where_}: DRAWN is engine-written — author DRAW and the log records the concrete DRAWN+SPAWN rows (§4.4)"
            )));
        }
        if o.op == "PROGRESS" || o.op == "REFUSE" {
            // PROGRESS carries (dur, string); REFUSE (string) — the FIRST
            // string arg
            if let Some(Arg::Str { v }) = o.args.iter().find(|a| matches!(a, Arg::Str { .. })) {
                if v.contains("{roll}") {
                    return Err(ChorusError::new(format!(
                        "{where_}: {{roll}} is only available in NARRATE strings — {} prose is load-fixed (§4.4)",
                        o.op
                    )));
                }
            }
        }
        if o.op == "ROLL" {
            for a in &o.args {
                if let Arg::Vexpr { v } = a {
                    lint_val_expr(where_, v)?;
                }
            }
        }
        Ok(())
    })?;
    lint_roll_may_bind(where_, out)
}

/// §4.4: a named-ref outcome must name a declared entry in `table` —
/// `ENDING` a declared ending, `DRAW` a declared deck; recurses into IF
/// branch bodies (an unknown id hidden in an untaken branch is still a load
/// error). An INTO target needs no check here: it resolves at runtime like
/// every ref position. Each op's error wording is pinned — `unknown_msg`
/// renders it from `(where_, shown)`.
fn check_named_ref<V>(
    op: &str,
    where_: &str,
    out: &[Outcome],
    table: &IndexMap<String, V>,
    unknown_msg: fn(where_: &str, shown: &str) -> String,
) -> Result<()> {
    for_each_authored_outcome(out, &mut |o| {
        if o.op != op {
            return Ok(());
        }
        let id = match o.args.first() {
            Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) => Some(name.as_str()),
            _ => None,
        };
        if !id.is_some_and(|id| table.contains_key(id)) {
            let shown = id.unwrap_or("?");
            return Err(ChorusError::new(unknown_msg(where_, shown)));
        }
        Ok(())
    })
}

/// The chain-candidate walk shared by every chain-op load check: every
/// candidate must be a quoted-string ref, IF branch bodies included (the
/// §4.4 walk). The arg-shape error is byte-pinned; `check` receives one
/// candidate id and renders each site's membership error.
fn check_run_candidates(
    op: &str,
    where_: &str,
    out: &[Outcome],
    check: &mut dyn FnMut(&str) -> Result<()>,
) -> Result<()> {
    for_each_authored_outcome(out, &mut |o| {
        if o.op != op {
            return Ok(());
        }
        for a in &o.args {
            let id = match a {
                Arg::Ref { r#ref: Ref::Name { name, .. } } => name.as_str(),
                _ => {
                    return Err(ChorusError::new(format!(
                        "{where_}: {op} takes quoted-string recipe ids: {op} \"id\" [, \"id\" …]"
                    )))
                }
            };
            check(id)?;
        }
        Ok(())
    })
}

/// §6.7: a chain op's candidates must each name a compiled recipe — the
/// GLOBAL id table, in every home (recipe bodies and trigger bodies alike;
/// a cross-verb collision already failed loud in the table build), IF
/// branch bodies included (the §4.4 walk; function expansion happened
/// before this guard, so expanded bodies check like literal ones). The
/// runtime walk resolves candidates against this same table, so loaded
/// content never hits a chain candidate it cannot resolve.
fn check_run_scope(
    op: &str,
    where_: &str,
    out: &[Outcome],
    recipe_verbs: &IndexMap<String, String>,
) -> Result<()> {
    check_run_candidates(op, where_, out, &mut |id| {
        if !recipe_verbs.contains_key(id) {
            return Err(ChorusError::new(format!(
                "{where_}: {op} '{id}' does not name a compiled recipe — candidates resolve against the global recipe table (§6.7)"
            )));
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// shared lexeme regexes + the recipe-id mint
// ---------------------------------------------------------------------------

/// `/^[A-Za-z_][A-Za-z0-9_]*$/` — the identifier lexeme: `FN_IDENT` (§10
/// function names and parameters) and the script lint's name-lexeme test
/// share one pattern.
pub(crate) fn ident_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$"))
}

/// `/^[A-Z][A-Z0-9_]*$/` — the UPPER_SNAKE id test (ending ids here, deck
/// ids in the deck machinery).
pub(crate) fn upper_snake_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Z][A-Z0-9_]*$"))
}

fn flavour_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[a-z]+$"))
}

/// The ONE id mint for id-less recipes (§6.4): `<verb>.recipe<i>`, 0-based,
/// or `<verb>.default` — the compile side and the lint mirror share it
/// verbatim (Q1 unified: the mirror's old 1-based base is retired; pinned
/// by the script-play fixture's `study.recipe0`).
pub(crate) fn mint_recipe_id(verb_lower: &str, when: Option<&str>, i: usize) -> String {
    match when {
        Some(_) => format!("{verb_lower}.recipe{i}"),
        None => format!("{verb_lower}.default"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_id_mint_is_zero_based() {
        // Q1 unified: the one mint, 0-based (the lint mirror's old 1-based
        // base is retired)
        assert_eq!(mint_recipe_id("study", Some("x"), 0), "study.recipe0");
        assert_eq!(mint_recipe_id("study", Some("x"), 3), "study.recipe3");
        assert_eq!(mint_recipe_id("study", None, 7), "study.default");
    }

    #[test]
    fn entries_in_key_order_fronts_integer_like_keys() {
        let mut m = IndexMap::new();
        m.insert("container".to_string(), 1);
        m.insert("5".to_string(), 2);
        m.insert("0".to_string(), 3);
        let got: Vec<(&str, i64)> =
            entries_in_key_order(&m).into_iter().map(|(k, v)| (k, *v)).collect();
        assert_eq!(got, [("0", 3), ("5", 2), ("container", 1)]);
    }

    #[test]
    fn empty_string_when_and_query_are_falsy() {
        // the empty-string `query`/`when` checks read the frozen truthiness, so a
        // hand-built ContentRaw
        // with when: '' compiles as the DEFAULT recipe (minting
        // `<verb>.default`, no parse error) and query: '' compiles to no
        // query. (Unreachable through parse_script — its `when\s+(.+)` and
        // slot `(.*)` shapes never yield '' — exactly why only a hand-built
        // raw can probe it.)
        let dict = crate::core::dict::core_dictionary();
        let raw = ContentRaw {
            verbs: [(
                "WAIT".to_string(),
                VerbContent {
                    slots: vec![SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some(String::new()),
                        shown: None,
                        greedy: false,
                        consuming: false,
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: Some(String::new()),
                        shown: None,
                        hint: None,
                        max: None,
                        aspects: Vec::new(),
                        dur: None,
                        out: vec!["NARRATE \"idle.\"".to_string()],
                    }],
                    ambient: false,
                },
            )]
            .into_iter()
            .collect(),
            ..ContentRaw::default()
        };
        let compiled = compile_content(&raw, &dict, &[]).unwrap();
        let verb = compiled.verbs.get("WAIT").unwrap();
        assert!(verb.slots[0].query.is_none()); // '' query → no query, not an error
        let recipe = &verb.recipes[0];
        assert!(recipe.when.is_none()); // '' when → the DEFAULT recipe
        assert_eq!(recipe.id, "wait.default"); // the falsy when mints .default
    }
}
