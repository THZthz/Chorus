//! Content model — ts: src/content/content.ts (spec §6). Authored as plain
//! data with string queries/predicates/outcomes; [`compile_content`] parses
//! them once and fails loudly at load time.
//!
//! This module also hosts the deck machinery (FNV id hash, position
//! permutation, the shared draw rule), folded in from the W0 staging module
//! `core/deck_rng.rs` per plan §2.2.

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use crate::core::dice::splitmix64;
use crate::core::dict::Dictionary;
use crate::core::fn_expand::{collect_fn_calls, expand_fn_calls, FnTable, FnTemplate, MAX_FN_DEPTH};
use crate::core::parse::{parse_function_body, parse_outcomes};
use crate::core::predicate::{
    lint_pred, lint_roll_may_bind, lint_trigger_pred, lint_trigger_val_expr_what, lint_val_expr,
    parse_predicate, Pred,
};
use crate::core::query::{parse_query, Query};
use crate::core::r#ref::Ref;
use crate::core::sentence::{Arg, IfArm, Outcome};
use crate::core::types::{Aspects, Attrs};
use crate::js::{
    clone_map_in_js_order, js_trim, ordinary_own_property_keys, static_regex, ChorusError, Result,
};

// ---------------------------------------------------------------------------
// authored (raw) content types — ts: content.ts:13-120
// ---------------------------------------------------------------------------

/// ts: content.ts:13 — `SlotRole`. Q4 (ENG-16): slot roles are NEVER
/// validated, so the port keeps the raw string instead of the `Role` enum —
/// a typo'd role compiles and silently never matches.
pub type SlotRole = String;

/// ts: content.ts:15-21. `max: None` models TS `Infinity` (the open-ended
/// `2..` count spec; ENG-07's Option mapping — slots never serialize).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SlotDef {
    pub role: SlotRole,
    pub min: i64,
    /// TS `Infinity` allowed (§6.1 open-ended); `None` = Infinity.
    pub max: Option<i64>,
    pub query: Option<String>,
    /// default true (§6.2) — resolved at compile
    pub shown: Option<bool>,
}

/// ts: content.ts:23-29
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecipeDef {
    pub id: Option<String>,
    /// undefined/'' → default recipe
    pub when: Option<String>,
    /// gates default hidden (§6.2)
    pub shown: Option<bool>,
    pub hint: Option<String>,
    /// outcome strings, parsed at load
    pub out: Vec<String>,
}

/// ts: content.ts:31-35
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VerbContent {
    pub slots: Vec<SlotDef>,
    /// minutes, consumed automatically on success (§9.12)
    pub duration: i64,
    /// first match wins; a `when`-less recipe is the default
    pub recipes: Vec<RecipeDef>,
}

/// ts: content.ts:40-45 — a clock trigger (§9.29).
#[derive(Debug, Clone, PartialEq)]
pub struct SlotTriggerDef {
    pub id: String,
    /// halfhours-of-day slot 0..47 (`at hour N` in the script is slot 2N)
    pub slot: i64,
    pub once: Option<bool>,
    pub out: Vec<String>,
}

/// ts: content.ts:51-55 — an expiry trigger (`on expire of NAME`, §6.6/§9.32).
#[derive(Debug, Clone, PartialEq)]
pub struct ExpireTriggerDef {
    pub id: String,
    pub on_expire_of: String,
    pub out: Vec<String>,
}

/// ts: content.ts:57 — `TriggerDef = SlotTriggerDef | ExpireTriggerDef`.
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerDef {
    Slot(SlotTriggerDef),
    Expire(ExpireTriggerDef),
}

impl TriggerDef {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            TriggerDef::Slot(t) => &t.id,
            TriggerDef::Expire(t) => &t.id,
        }
    }

    #[must_use]
    pub fn out(&self) -> &[String] {
        match self {
            TriggerDef::Slot(t) => &t.out,
            TriggerDef::Expire(t) => &t.out,
        }
    }
}

/// ts: content.ts:67-70 — the `derive` clause's per-aspect entry.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeriveAspect {
    pub from_object_aspect: String,
    /// TS `minus?: number` — parse always materializes a number (default 0)
    pub minus: i64,
}

/// ts: content.ts:66-71 — `PrototypeDef['derive']`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrototypeDerive {
    /// minted name: `` `${name}_${object.name}` `` — the only TS value is the
    /// literal `'object'`
    pub name_from: Option<String>,
    pub aspects: Option<IndexMap<String, DeriveAspect>>,
}

/// ts: content.ts:61-71 — prototype-parameterized spawn (§6.4, §9.15).
/// Shared by the raw and compiled faces exactly like TS (the compiled
/// `prototypes` map stores the same struct).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PrototypeDef {
    pub kind: String,
    /// §9.38: mints from this prototype may share their name with living
    /// copies. TS keeps it optional; every read is a truthiness test, so a
    /// plain `bool` (default false) is exact.
    pub stackable: bool,
    pub aspects: Option<Aspects>,
    pub attrs: Option<Attrs>,
    pub derive: Option<PrototypeDerive>,
}

/// ts: content.ts:77-80 — a story ending (§9.35). The flavour is an OPEN
/// vocabulary (any lowercase word) — ENG-13: never tighten to an enum.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EndingDef {
    pub title: String,
    pub flavour: Option<String>,
}

/// ts: content.ts:86-90 — a function declaration (§9.40): a parametric
/// outcome MACRO, compiled to templates before verbs/triggers parse.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FunctionDef {
    pub name: String,
    /// [] for a parameterless function
    pub params: Vec<String>,
    /// raw outcome-list source after `->`
    pub body: String,
}

/// ts: content.ts:95-100 — a deck declaration (§9.42).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeckRaw {
    pub id: String,
    /// entries, spec order; repeats are legal (weighting)
    pub spec: Vec<String>,
    /// served forever once the spec is exhausted (no `reset`)
    pub defaultcard: Option<String>,
    /// wrap the permutation when exhausted
    pub reset: bool,
}

/// ts: content.ts:109-120 — the raw content tree. Every optional TS
/// collection becomes `Option`; the maps are IndexMaps (insertion order IS
/// observable: compile-time `Object.entries` walks and `.recipe${i}`).
#[derive(Debug, Clone, Default)]
pub struct ContentRaw {
    /// §9.41: the dice seed (default 0). Meta line `seed: N` (§10).
    pub seed: Option<i64>,
    pub verbs: IndexMap<String, VerbContent>,
    pub triggers: Option<Vec<TriggerDef>>,
    pub prototypes: Option<IndexMap<String, PrototypeDef>>,
    /// prose templates per verb (§5.3)
    pub templates: Option<IndexMap<String, String>>,
    /// the ending table (§9.35)
    pub endings: Option<IndexMap<String, EndingDef>>,
    /// parametric outcome macros (§9.40) — declaration order matters for
    /// error reports only
    pub functions: Option<Vec<FunctionDef>>,
    /// deck declarations (§9.42) — order matters for error reports only
    pub decks: Option<Vec<DeckRaw>>,
}

// ---------------------------------------------------------------------------
// compiled content types — ts: content.ts:122-186
// ---------------------------------------------------------------------------

/// ts: content.ts:102-107 — one resolvable deck entry: the card NAME and the
/// kind its mint carries (prototype kind, or the declared entity line's kind).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeckCard {
    pub name: String,
    pub kind: String,
}

/// ts: content.ts:122-128
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledRecipe {
    pub id: String,
    /// None → default
    pub when: Option<Pred>,
    pub shown: bool,
    pub hint: Option<String>,
    pub out: Vec<Outcome>,
}

/// ts: content.ts:130-136. `max: None` = TS Infinity.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledSlot {
    pub role: SlotRole,
    pub min: i64,
    pub max: Option<i64>,
    pub shown: bool,
    pub query: Option<Query>,
}

/// ts: content.ts:138-144
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledVerb {
    pub name: String,
    pub slots: Vec<CompiledSlot>,
    /// TS `Map<SlotRole, CompiledSlot>` — keyed by the RAW role string (Q4)
    pub slot_by_role: IndexMap<SlotRole, CompiledSlot>,
    pub duration: i64,
    pub recipes: Vec<CompiledRecipe>,
}

/// ts: content.ts:146-149 — flavour resolved (default 'melancholy', §10).
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledEnding {
    pub title: String,
    pub flavour: String,
}

/// ts: content.ts:157-164 — a compiled deck (§9.42): the fixed draw order is
/// a PERMUTATION OVER POSITIONS of the spec (multiset preserved), derived
/// purely from (seed, deck id) at load. It lives on compiled content, never
/// on State: identical at play and fold, consuming NO rng counter (law 5).
#[derive(Debug, Clone)]
pub struct CompiledDeck {
    pub id: String,
    /// the draw order: spec positions shuffled, entries resolved to (name, kind)
    pub order: Vec<DeckCard>,
    /// served forever once the order is exhausted and `reset` is absent
    pub defaultcard: Option<DeckCard>,
    pub reset: bool,
}

/// The compiled trigger forms — ts: content.ts:171-174 (the inline union).
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledTrigger {
    Slot {
        id: String,
        slot: i64,
        once: bool,
        out: Vec<Outcome>,
    },
    Expire {
        id: String,
        on_expire_of: String,
        out: Vec<Outcome>,
    },
}

impl CompiledTrigger {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            CompiledTrigger::Slot { id, .. } | CompiledTrigger::Expire { id, .. } => id,
        }
    }

    /// The compiled outcome list.
    #[must_use]
    pub fn out(&self) -> &[Outcome] {
        match self {
            CompiledTrigger::Slot { out, .. } | CompiledTrigger::Expire { out, .. } => out,
        }
    }
}

/// ts: content.ts:166-186 — `CompiledContent`. All maps IndexMap (ENG-04:
/// compile error order, `.recipe${i}` indices and UI traversal depend on it).
#[derive(Debug, Clone)]
pub struct CompiledContent {
    /// §9.41: every die is `f(seed, counter)`; the run's dice sequence is
    /// fixed at load and replay is counter-based (no re-derivation)
    pub seed: i64,
    pub verbs: IndexMap<String, CompiledVerb>,
    pub triggers: Vec<CompiledTrigger>,
    pub prototypes: IndexMap<String, PrototypeDef>,
    pub templates: IndexMap<String, String>,
    pub endings: IndexMap<String, CompiledEnding>,
    /// §9.38 stackability, derived from `stackable` declarations at load:
    /// declared stackable prototype/entity NAMES, and the KINDS of those
    /// declarations (a `kind:K` mint is stackable when its kind carries one).
    /// Only `.has()` is ever read, so the single ENG-01 build (prototype side
    /// + script entity side, once, here) is behavior-preserving.
    pub stackable_names: IndexSet<String>,
    pub stackable_kinds: IndexSet<String>,
    /// §9.42 the deck table: id → compiled deck (permutation + exhaustion)
    pub decks: IndexMap<String, CompiledDeck>,
}

/// ts: content.ts:194 (`entities: Iterable<{ name: string; kind: string }>`)
/// with ENG-01's sanctioned structural addition: the `stackable` flag rides
/// along so the §9.38 sets build ONCE here instead of `compile_script`
/// mutating the compiled content afterwards (behavior-preserving — Set
/// membership is order- and structure-free).
#[derive(Debug, Clone)]
pub struct DeclaredEntity {
    pub name: String,
    pub kind: String,
    pub stackable: bool,
}

// ---------------------------------------------------------------------------
// compileContent — ts: content.ts:194-294
// ---------------------------------------------------------------------------

/// ts: content.ts:204/208/219/274/276 — the compile-side `Object.entries`
/// iteration points (plan §3.7 / ENG-04 / REACH-04): JS fronts integer-like
/// keys, so every one of these walks goes through
/// [`ordinary_own_property_keys`]. Do NOT reject integer-like key names —
/// reproduce the key order.
fn entries_in_js_order<V>(m: &IndexMap<String, V>) -> Vec<(&str, &V)> {
    let keys = ordinary_own_property_keys(m);
    let mut out = Vec::with_capacity(keys.len());
    for k in &keys {
        // re-borrow both sides from m so the returned refs outlive `keys`
        if let Some((k, v)) = m.get_key_value(k) {
            out.push((k.as_str(), v));
        }
    }
    out
}

/// ts: content.ts:202-206 (prototype side), script.ts:426-428 (entity side)
/// and script.ts:545-552 (the lint's own copy) — the THREE stackable-set
/// build sites folded into ONE builder (ENG-01 / plan W2 "三集合合一"). Set
/// membership is order-independent, so the build order here (entities first,
/// then prototypes, matching the lint's historical order) changes no `has()`
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
        for (name, p) in entries_in_js_order(prototypes) {
            if p.stackable {
                names.insert(name.to_string());
                kinds.insert(p.kind.clone());
            }
        }
    }
    (names, kinds)
}

/// ts: content.ts:194 — compile raw content. `entities` contributes the
/// script's declared entity lines so deck spec entries (§9.42) may name
/// plain entities as well as prototypes; `compile_script` passes them,
/// direct callers without entities pass `&[]` (deck entries validate
/// against prototypes only).
pub fn compile_content(
    raw: &ContentRaw,
    dict: &Dictionary,
    entities: &[DeclaredEntity],
) -> Result<CompiledContent> {
    // §9.40 FIRST, before any verb/trigger body parses: build the function
    // table (content.ts:199) — call sites dispatch against it and expand
    // immediately after their parse, so the guards below walk expanded lists.
    let fns = build_fn_table(raw.functions.as_deref().unwrap_or(&[]), dict)?;
    // §9.38: two derived sets every mint site reads (content.ts:202-206 +
    // the folded entity side, ENG-01)
    let (stackable_names, stackable_kinds) =
        collect_stackable(entities, raw.prototypes.as_ref());
    let mut endings = IndexMap::new();
    for (id, e) in entries_in_js_order(raw.endings.as_ref().unwrap_or(&IndexMap::new())) {
        let re = ending_id_re();
        if !re.is_match(id) {
            return Err(ChorusError::new(format!("ending '{id}' must be UPPER_SNAKE")));
        }
        if e.title.is_empty() || js_trim(&e.title).is_empty() {
            return Err(ChorusError::new(format!("ending {id}: needs a title")));
        }
        let flavour = e.flavour.clone().unwrap_or_else(|| "melancholy".to_string());
        let re = flavour_re();
        if !re.is_match(&flavour) {
            return Err(ChorusError::new(format!(
                "ending {id}: flavour must be a lowercase word, got '{flavour}'"
            )));
        }
        endings.insert(
            id.to_string(),
            CompiledEnding {
                title: e.title.clone(),
                flavour,
            },
        );
    }
    // §9.42: the deck table — spec entries resolve against prototypes +
    // declared entities, then the seed-fixed permutation is derived
    let decks = compile_decks(
        raw.decks.as_deref().unwrap_or(&[]),
        raw.seed.unwrap_or(0),
        dict,
        raw.prototypes.as_ref().unwrap_or(&IndexMap::new()),
        entities,
    )?;
    let mut verbs = IndexMap::new();
    for (name, v) in entries_in_js_order(&raw.verbs) {
        if !dict.is_verb(name) {
            return Err(ChorusError::new(format!(
                "verb '{name}' is not registered in the dictionary"
            )));
        }
        let name_lower = name.to_lowercase();
        let mut slot_by_role = IndexMap::new();
        let mut slots = Vec::with_capacity(v.slots.len());
        for s in &v.slots {
            // ts: content.ts:223 — `s.max < s.min`: TS Infinity is never
            // less than anything, so the check only runs for a finite max
            if let Some(max) = s.max {
                if max < s.min {
                    return Err(ChorusError::new(format!(
                        "verb {name}: slot {} has max < min",
                        s.role
                    )));
                }
            }
            let compiled = CompiledSlot {
                role: s.role.clone(),
                min: s.min,
                max: s.max,
                shown: s.shown.unwrap_or(true),
                // ts: content.ts:224 `s.query ? parseQuery(s.query) : undefined`
                // — JS truthiness: '' is FALSY, so a hand-built ContentRaw with
                // query: '' compiles to NO query (not a parse error); parse-
                // produced queries are never '' (script.ts:384's `\s+(.*)` is
                // absent for a bare count spec), so this is the exact mapping.
                query: match s.query.as_deref() {
                    Some(q) if !q.is_empty() => Some(parse_query(q)?),
                    _ => None,
                },
            };
            slot_by_role.insert(s.role.clone(), compiled.clone());
            slots.push(compiled);
        }
        let mut recipes = Vec::with_capacity(v.recipes.len());
        for (i, r) in v.recipes.iter().enumerate() {
            // QUIRK(Q1/ENG-15): the COMPILE side mints 0-based `.recipe${i}`
            // (content.ts:229; pinned by script-play.test.ts:175
            // `study.recipe0`). The lint mirror mints 1-based — frozen drift.
            // ts: content.ts:229-230 — BOTH the id mint and the when parse
            // read `r.when` JS truthiness: '' is FALSY, so a hand-built
            // ContentRaw with when: '' rides as the DEFAULT recipe (id
            // `${verb}.default`, no parse error); the lint mirror
            // (script.ts:485) reads the same truthiness. parse-produced whens
            // are never '' (script.ts:409's `when\s+(.+)$`).
            let raw_when = r.when.as_deref().filter(|w| !w.is_empty());
            let when = match raw_when {
                Some(w) => Some(parse_predicate(w)?),
                None => None,
            };
            // QUIRK(Q1/ENG-15): the COMPILE side mints 0-based `.recipe${i}`
            // (content.ts:229; pinned by script-play.test.ts:175
            // `study.recipe0`). The lint mirror mints 1-based — frozen drift.
            let id = r
                .id
                .clone()
                .unwrap_or_else(|| mint_recipe_id(&name_lower, raw_when, i, 0));
            let mut out = Vec::new();
            for o in &r.out {
                out.extend(parse_outcome_string(o, dict, &fns)?);
            }
            recipes.push(CompiledRecipe {
                id,
                when,
                shown: r.shown.unwrap_or(false),
                hint: r.hint.clone(),
                out,
            });
        }
        // §9.38 load-time lint: count()/total() args must be declared roles
        // or queries — over every `when` AND every IF condition (content.ts:237-241)
        for r in &recipes {
            let where_ = format!("recipe {}", r.id);
            if let Some(w) = &r.when {
                lint_pred(&where_, w)?;
            }
            for_each_if_condition(&r.out, &mut |w| lint_pred(&where_, w))?;
            lint_dice(&where_, &r.out)?;
        }
        verbs.insert(
            name.to_string(),
            CompiledVerb {
                name: name.to_string(),
                slots,
                slot_by_role,
                duration: v.duration,
                recipes,
            },
        );
    }
    let mut triggers = Vec::new();
    for t in raw.triggers.as_deref().unwrap_or(&[]) {
        // §9.40: expansion (inside parse_outcome_string) has already run —
        // these guards see function-expanded bodies exactly like literal ones
        let mut out = Vec::new();
        for o in t.out() {
            out.extend(parse_outcome_string(o, dict, &fns)?);
        }
        let tid = t.id().to_string();
        // §6.7/§9.34 guards — recursive over IF branch bodies (§9.39)
        for_each_authored_outcome(&out, &mut |o| {
            // §6.7: PROGRESS narrates a processing window; events have none
            if o.op == "PROGRESS" {
                return Err(ChorusError::new(format!(
                    "trigger {tid}: PROGRESS belongs to verb recipes, not events"
                )));
            }
            // §9.34: REFUSE aborts a step (§6.5); events have no step to refuse
            if o.op == "REFUSE" {
                return Err(ChorusError::new(format!(
                    "trigger {tid}: REFUSE belongs to verb recipes — events have no step to refuse"
                )));
            }
            Ok(())
        })?;
        // §9.39: event bindings are empty, so a role-dependent operand in a
        // trigger IF condition would silently read false/0
        for_each_if_condition(&out, &mut |w| {
            lint_pred(&format!("trigger {tid}"), w)?; // the count-typo lint applies here too
            lint_trigger_pred(&format!("trigger {tid}"), w)
        })?;
        // §9.41: trigger ROLL arguments must be role-FREE
        for_each_authored_outcome(&out, &mut |o| {
            if o.op == "ROLL" {
                for a in &o.args {
                    if let Arg::Vexpr { v } = a {
                        lint_trigger_val_expr_what(&format!("trigger {tid}"), v, "ROLL argument")?;
                    }
                }
            }
            Ok(())
        })?;
        lint_dice(&format!("trigger {tid}"), &out)?;
        match t {
            TriggerDef::Expire(e) => triggers.push(CompiledTrigger::Expire {
                id: e.id.clone(),
                on_expire_of: e.on_expire_of.clone(),
                out,
            }),
            TriggerDef::Slot(s) => triggers.push(CompiledTrigger::Slot {
                id: s.id.clone(),
                slot: s.slot,
                once: s.once.unwrap_or(false),
                out,
            }),
        }
    }
    // ts: content.ts:274 — `new Map(Object.entries(raw.prototypes ?? {}))`:
    // JS key order via the JS-order clone
    let prototypes = clone_map_in_js_order(raw.prototypes.as_ref().unwrap_or(&IndexMap::new()));
    let mut templates = IndexMap::new();
    for (name, tpl) in entries_in_js_order(raw.templates.as_ref().unwrap_or(&IndexMap::new())) {
        if !dict.is_verb(name) {
            return Err(ChorusError::new(format!(
                "template '{name}' is not a registered verb — templates configure verbs, same as the verbs: section"
            )));
        }
        templates.insert(name.to_string(), tpl.clone());
    }
    // §9.35: an ENDING outcome must name a declared ending — recipe or
    // trigger, checked once at load. §9.42: the same walk pins DRAW to a
    // declared deck.
    for v in verbs.values() {
        for r in &v.recipes {
            let where_ = format!("recipe {}", r.id);
            check_ending_refs(&where_, &r.out, &endings)?;
            check_deck_refs(&where_, &r.out, &decks)?;
        }
    }
    for t in &triggers {
        let where_ = format!("trigger {}", t.id());
        check_ending_refs(&where_, t.out(), &endings)?;
        check_deck_refs(&where_, t.out(), &decks)?;
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
        decks,
    })
}

/// ts: content.ts:303-311 + reach.ts:205-227 — the ONE authored-outcome
/// traversal both consumers share (REACH-01 must: `for_each_authored_outcome`
/// and the reach analyzer's `walkSites` are hand-mirrored recursions in TS;
/// the port keeps a single implementation so the 'none' verdict's soundness
/// argument — exhaustive enumeration in forEachAuthoredOutcome order — stays
/// structural instead of hopeful).
///
/// The recursion is parameterized by a per-branch CONTEXT (the TS
/// `(path, segs, fallback)` triple on the reach side, `()` for the guards)
/// and a walker-STATE `S` threaded through every callback by `&mut` (the
/// reach side's flags/seg-counter/sites accumulators — one state object, so
/// no closure captures it twice). `arm_ctx`/`else_ctx` derive the child
/// context exactly where TS evaluates them (per arm, left to right, before
/// that arm descends — so the reach consumer's `nextSeg()` allocation order
/// is preserved call for call). The visitor is fallible (the load guards
/// throw); Rust models the throw as `Err`.
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

/// ts: content.ts:303-311 — visit every outcome in an authored list,
/// RECURSING into IF branch bodies (§9.39) — the single entry point for the
/// load-time guards, so nothing escapes by nesting. Ordering with F3:
/// function expansion happens BEFORE these guards run. The visitor is
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

/// ts: content.ts:314-318 — visit every IF condition in an authored list
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

/// ts: content.ts:337-356 — §9.41/§9.42 load lints shared by recipe and
/// trigger outcome lists, all run AFTER function expansion.
fn lint_dice(where_: &str, out: &[Outcome]) -> Result<()> {
    for_each_authored_outcome(out, &mut |o| {
        if o.op == "ROLLED" {
            return Err(ChorusError::new(format!(
                "{where_}: ROLLED is engine-written — author ROLL and the log records the concrete result (§9.41)"
            )));
        }
        if o.op == "DRAWN" {
            return Err(ChorusError::new(format!(
                "{where_}: DRAWN is engine-written — author DRAW and the log records the concrete DRAWN+SPAWN rows (§9.42)"
            )));
        }
        if o.op == "PROGRESS" || o.op == "REFUSE" {
            // PROGRESS carries (dur, string); REFUSE (string) — the FIRST
            // string arg
            if let Some(Arg::Str { v }) = o.args.iter().find(|a| matches!(a, Arg::Str { .. })) {
                if v.contains("{roll}") {
                    return Err(ChorusError::new(format!(
                        "{where_}: {{roll}} is only available in NARRATE strings — {} prose is load-fixed (§9.41)",
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

/// ts: content.ts:358-367 — §9.35: ENDING must name a declared ending;
/// recurses into IF branch bodies (an unknown ending hidden in an untaken
/// branch is still a load error).
fn check_ending_refs(
    where_: &str,
    out: &[Outcome],
    endings: &IndexMap<String, CompiledEnding>,
) -> Result<()> {
    for_each_authored_outcome(out, &mut |o| {
        if o.op != "ENDING" {
            return Ok(());
        }
        let id = match o.args.first() {
            Some(Arg::Ref {
                r#ref: Ref::Name { name, .. },
            }) => Some(name.as_str()),
            _ => None,
        };
        if !id.is_some_and(|id| endings.contains_key(id)) {
            let shown = id.unwrap_or("?");
            return Err(ChorusError::new(format!(
                "{where_}: ENDING '{shown}' is not in the endings table"
            )));
        }
        Ok(())
    })
}

/// ts: content.ts:369-380 — §9.42: a DRAW must name a declared deck (an INTO
/// target needs no check here: it resolves at runtime like every ref
/// position).
fn check_deck_refs(
    where_: &str,
    out: &[Outcome],
    decks: &IndexMap<String, CompiledDeck>,
) -> Result<()> {
    for_each_authored_outcome(out, &mut |o| {
        if o.op != "DRAW" {
            return Ok(());
        }
        let id = match o.args.first() {
            Some(Arg::Ref {
                r#ref: Ref::Name { name, .. },
            }) => Some(name.as_str()),
            _ => None,
        };
        if !id.is_some_and(|id| decks.contains_key(id)) {
            let shown = id.unwrap_or("?");
            return Err(ChorusError::new(format!(
                "{where_}: DRAW '{shown}' is not a declared deck (decks:)"
            )));
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// decks (§9.42): seed-fixed permutations — ts: content.ts:384-477
// ---------------------------------------------------------------------------

/// ts: content.ts:391-425 — compile the deck table. Each spec entry must name
/// a declared prototype or entity (load error naming the deck and the entry
/// otherwise); repeats are legal and weight the draw. The fixed draw order is
/// derived per deck from (seed, deck id) ALONE — a permutation over POSITIONS
/// of the spec, so the multiset is preserved whatever order the shuffle picks.
fn compile_decks(
    raws: &[DeckRaw],
    seed: i64,
    dict: &Dictionary,
    prototypes: &IndexMap<String, PrototypeDef>,
    entities: &[DeclaredEntity],
) -> Result<IndexMap<String, CompiledDeck>> {
    // ts: content.ts:398-399 — `entityKinds` Map: later duplicates overwrite
    // the value (keeping the original insertion slot)
    let mut entity_kinds: IndexMap<&str, &str> = IndexMap::new();
    for e in entities {
        entity_kinds.insert(e.name.as_str(), e.kind.as_str());
    }
    // ts: content.ts:402-408 — entries resolve prototype-first, then entity
    // lines — the same name in both is unambiguous (the prototype is what
    // SPAWN would mint)
    let resolve = |deck: &str, what: &str, name: &str| -> Result<DeckCard> {
        if let Some(proto) = prototypes.get(name) {
            return Ok(DeckCard {
                name: name.to_string(),
                kind: proto.kind.clone(),
            });
        }
        if let Some(kind) = entity_kinds.get(name) {
            return Ok(DeckCard {
                name: name.to_string(),
                kind: (*kind).to_string(),
            });
        }
        Err(ChorusError::new(format!(
            "deck {deck}: {what} '{name}' is neither a declared prototype nor an entity"
        )))
    };
    let mut decks = IndexMap::new();
    for d in raws {
        let re = upper_snake_re();
        if !re.is_match(&d.id) {
            return Err(ChorusError::new(format!("deck '{}' must be UPPER_SNAKE", d.id)));
        }
        if dict.reserved.contains(&d.id) {
            return Err(ChorusError::new(format!(
                "'{}' is reserved and cannot be a deck id",
                d.id
            )));
        }
        if decks.contains_key(&d.id) {
            return Err(ChorusError::new(format!("duplicate deck '{}'", d.id)));
        }
        if d.spec.is_empty() {
            return Err(ChorusError::new(format!("deck {}: the spec is empty", d.id)));
        }
        let mut entries = Vec::with_capacity(d.spec.len());
        for name in &d.spec {
            entries.push(resolve(&d.id, "spec entry", name)?);
        }
        let defaultcard = match &d.defaultcard {
            Some(dc) => Some(resolve(&d.id, "defaultcard", dc)?),
            None => None,
        };
        let order = permute_positions(entries.len(), seed, &d.id)
            .into_iter()
            .map(|p| entries[p].clone())
            .collect();
        decks.insert(
            d.id.clone(),
            CompiledDeck {
                id: d.id.clone(),
                order,
                defaultcard,
                reset: d.reset,
            },
        );
    }
    Ok(decks)
}

/// ts: content.ts:430-437 — FNV-1a 64 over the deck id's UTF-8 bytes
/// (`.as_bytes()` is the same byte sequence TextEncoder produces), folding
/// the id into one full-width 64-bit key part.
#[must_use]
pub fn hash_deck_id(id: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// ts: content.ts:453-461 — Fisher–Yates over positions 0..n-1; position i
/// (descending from n-1 to 1) draws
/// `j = splitmix64(asUintN(seed) ^ hashDeckId(deckId) ^ asUintN(i)) % (i+1)`
/// and swaps. `seed as u64` covers the asUintN two's-complement treatment of
/// negative seeds (the (7, -3, 'NEG') vector pins it); `i as u64` is a no-op
/// asUintN for the non-negative loop index. Uniform up to the negligible
/// `u64 mod (i+1)` bias; pure in (seed, deck id): identical at play and
/// fold, fixed at load, never stored on State.
#[must_use]
pub fn permute_positions(n: usize, seed: i64, deck_id: &str) -> Vec<usize> {
    let mut pos: Vec<usize> = (0..n).collect();
    let hash = hash_deck_id(deck_id);
    let mut i: i64 = n as i64 - 1;
    while i >= 1 {
        let key = (seed as u64) ^ hash ^ (i as u64);
        let j = (splitmix64(key) % (i as u64 + 1)) as usize;
        pos.swap(i as usize, j);
        i -= 1;
    }
    pos
}

/// ts: content.ts:472-478 — serve the card at `cursor`, shared by play (the
/// DRAW op) and fold (the DRAWN row's assertion) so the two can never drift:
/// below the period it is `order[cursor]`; past it, `reset` wraps (periodic
/// with period len(order)) and otherwise `defaultcard` serves forever; with
/// neither, the draw FAILS — a §9.19 step rejection at play (nothing logged,
/// cursor unmoved) and a loud fold error there. Requires a non-empty order
/// (compile_decks rejects an empty spec).
///
/// A negative cursor is unreachable (cursors start at 0 and only advance on
/// the engine's DRAW path); TS would hand back `undefined` from the first
/// branch in that case. Rule 11: the debug_assert documents that
/// unreachability; the release-side `Err` is a guard so the unreachable
/// branch can never become an index panic (TS's `undefined` is not
/// observable here — no caller can produce a negative cursor).
pub fn draw_from(deck: &CompiledDeck, cursor: i64) -> Result<DeckCard> {
    if cursor < 0 {
        debug_assert!(false, "negative deck cursor {cursor} is unreachable");
        return Err(ChorusError::new(format!(
            "internal: negative deck cursor {cursor}"
        )));
    }
    if cursor < deck.order.len() as i64 {
        return Ok(deck.order[cursor as usize].clone());
    }
    if deck.reset {
        return Ok(deck.order[(cursor % deck.order.len() as i64) as usize].clone());
    }
    if let Some(dc) = &deck.defaultcard {
        return Ok(dc.clone());
    }
    Err(ChorusError::new(format!(
        "deck '{}' is exhausted — no 'reset' and no 'defaultcard' (§9.42)",
        deck.id
    ))) // ts: content.ts:476
}

/// ts: content.ts:479-486 — allow several outcomes in one string; usually
/// one per array element. §9.40: with a function table, parse dispatches
/// NAME( calls and the expansion pass runs in the same step — verb/trigger
/// bodies are compiled fully expanded, so every guard (and the engine) sees
/// plain outcomes.
fn parse_outcome_string(src: &str, dict: &Dictionary, fns: &FnTable) -> Result<Vec<Outcome>> {
    let out = parse_outcomes(src, dict, Some(fns))?;
    if !fns.by_name.is_empty() {
        expand_fn_calls(&out, fns)
    } else {
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// functions: the compile-time macro table (§9.40) — ts: content.ts:490-592
// ---------------------------------------------------------------------------

/// ts: content.ts:229 / script.ts:485 — the shared recipe-id mint, one
/// function with two parameterized variants (plan §1.2 Q1 / ENG-15: the
/// bases are FROZEN drift — `base = 0` is the compile side's `.recipe${i}`
/// (pinned by script-play.test.ts:175 `study.recipe0`), `base = 1` is the
/// lint mirror's `.recipe${i + 1}` (script.ts:485; untested but frozen).
/// Outputs are byte-identical to TS on both paths; unifying the bases is a
/// separate issue.
pub(crate) fn mint_recipe_id(verb_lower: &str, when: Option<&str>, i: usize, base: usize) -> String {
    match when {
        Some(_) => format!("{verb_lower}.recipe{}", i + base),
        None => format!("{verb_lower}.default"),
    }
}

/// ts: content.ts:490 — `FN_IDENT` — function names and parameters.
fn fn_ident_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$"))
}

fn ending_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Z][A-Z0-9_]*$"))
}

fn flavour_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[a-z]+$"))
}

fn upper_snake_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Z][A-Z0-9_]*$"))
}

/// ts: content.ts:508-592 — build the function table; functions parse BEFORE
/// any verb/trigger body (§9.40), in three ordered passes:
/// 1. declarations: name/param shape, op-name collisions, IF/ELSE,
///    duplicates. No entity/verb name constraint.
/// 2. bodies: each body parses to a TEMPLATE against the table's names.
/// 3. the call graph: colored DFS rejects recursion naming the cycle; the
///    same graph's depth rejects nesting beyond MAX_FN_DEPTH. Both are load
///    errors even for never-called functions.
fn build_fn_table(defs: &[FunctionDef], dict: &Dictionary) -> Result<FnTable> {
    let mut table = FnTable::default();
    // pass 1 — declarations
    for def in defs {
        if !fn_ident_re().is_match(&def.name) {
            return Err(ChorusError::new(format!(
                "function name '{}' must be an identifier (letters, digits, _)",
                def.name
            )));
        }
        if dict.is_op(&def.name) {
            // ts: content.ts:516-519 — the TS string is one literal split
            // across two source lines; the joined text is byte-exact here
            return Err(ChorusError::new(format!(
                "function '{}' collides with an outcome op — functions and ops share one caller vocabulary; rename the function",
                def.name
            )));
        }
        if def.name == "IF" || def.name == "ELSE" {
            return Err(ChorusError::new(format!(
                "function '{}' cannot be named a conditional keyword — IF/ELSE dispatch before functions in outcome position",
                def.name
            )));
        }
        if table.by_name.contains_key(&def.name) {
            return Err(ChorusError::new(format!("duplicate function '{}'", def.name)));
        }
        for p in &def.params {
            if !fn_ident_re().is_match(p) {
                return Err(ChorusError::new(format!(
                    "function '{}': parameter '{p}' must be an identifier",
                    def.name
                )));
            }
        }
        // ts: content.ts:528-529 — first-occurrence duplicates, deduped
        // preserving first-seen order (the TS `new Set(dups)` iteration)
        let mut dup_set: IndexSet<&str> = IndexSet::new();
        let mut dups: Vec<&str> = Vec::new();
        for (i, p) in def.params.iter().enumerate() {
            if def.params[..i].contains(p) && dup_set.insert(p) {
                dups.push(p);
            }
        }
        if !dups.is_empty() {
            return Err(ChorusError::new(format!(
                "function '{}': duplicate parameter(s) {}",
                def.name,
                dups.join(", ")
            )));
        }
        table.by_name.insert(
            def.name.clone(),
            FnTemplate {
                name: def.name.clone(),
                params: def.params.clone(),
                body: Vec::new(),
            },
        );
    }
    // pass 2 — bodies (nested-call arity checks against the declared params)
    for def in defs {
        // ts: content.ts:536-539 — the body parse is wrapped:
        // `function '${def.name}': ${err.message}`
        let body = match parse_function_body(&def.body, dict, &table, &def.params) {
            Ok(b) => b,
            Err(err) => {
                return Err(ChorusError::new(format!(
                    "function '{}': {}",
                    def.name, err.msg
                )))
            }
        };
        // ts: content.ts:534 `table.byName.get(def.name)!` — pass 1 inserted it
        match table.by_name.get_mut(&def.name) {
            Some(tpl) => tpl.body = body,
            None => unreachable!("pass 1 inserted '{}'", def.name),
        }
    }
    // pass 3 — the call graph: recursion cycles, then the depth cap
    let mut calls_of: IndexMap<String, IndexSet<String>> = IndexMap::new();
    for tpl in table.by_name.values() {
        let mut calls = IndexSet::new();
        collect_fn_calls(&tpl.body, &mut calls);
        for c in &calls {
            // ts: content.ts:547 — messages.md row 76 (pinned at
            // tests/fn_expand_direct.rs `an_undeclared_call_in_a_body_...`)
            if !table.by_name.contains_key(c) {
                return Err(ChorusError::new(format!(
                    "function '{}': call to undeclared function '{c}'",
                    tpl.name
                )));
            }
        }
        calls_of.insert(tpl.name.clone(), calls);
    }
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;
    // ts: content.ts:552-553 — `color`/`stack`
    let mut color: IndexMap<String, u8> = IndexMap::new();
    let mut stack: Vec<String> = Vec::new();
    fn visit(
        name: &str,
        calls_of: &IndexMap<String, IndexSet<String>>,
        color: &mut IndexMap<String, u8>,
        stack: &mut Vec<String>,
    ) -> Result<()> {
        color.insert(name.to_string(), GRAY);
        stack.push(name.to_string());
        if let Some(calls) = calls_of.get(name) {
            for c in calls {
                let col = color.get(c).copied().unwrap_or(WHITE);
                if col == GRAY {
                    // ts: content.ts:560 — `[...stack.slice(stack.indexOf(c)), c]`;
                    // a GRAY node is by construction on the stack
                    let start = match stack.iter().position(|x| x == c) {
                        Some(s) => s,
                        None => unreachable!("GRAY '{c}' is on the DFS stack"),
                    };
                    let mut cycle: Vec<String> = stack[start..].to_vec();
                    cycle.push(c.clone());
                    // messages.md row 74 (pinned at tests/fn_expand_direct.rs
                    // `mutual_recursion_names_the_cycle_...`)
                    return Err(ChorusError::new(format!(
                        "functions recurse: {} — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle",
                        cycle.join(" -> ")
                    )));
                }
                if col == WHITE {
                    visit(c, calls_of, color, stack)?;
                }
            }
        }
        stack.pop();
        color.insert(name.to_string(), BLACK);
        Ok(())
    }
    for name in table.by_name.keys() {
        if color.get(name).copied().unwrap_or(WHITE) == WHITE {
            visit(name, &calls_of, &mut color, &mut stack)?;
        }
    }
    fn depth_of(name: &str, calls_of: &IndexMap<String, IndexSet<String>>, depth: &mut IndexMap<String, u32>) -> u32 {
        if let Some(memo) = depth.get(name) {
            return *memo;
        }
        // safe: the graph is acyclic after the cycle check
        depth.insert(name.to_string(), 1);
        let mut deepest: u32 = 0;
        if let Some(calls) = calls_of.get(name) {
            for c in calls {
                deepest = deepest.max(depth_of(c, calls_of, depth));
            }
        }
        let v = 1 + deepest;
        depth.insert(name.to_string(), v);
        v
    }
    // ts: content.ts:574 — `depth` memo
    let mut depth: IndexMap<String, u32> = IndexMap::new();
    for name in table.by_name.keys() {
        let d = depth_of(name, &calls_of, &mut depth);
        if d > MAX_FN_DEPTH {
            // messages.md row 75 (pinned at tests/fn_expand_direct.rs
            // `a_depth_8_chain_expands_transitively_...`)
            return Err(ChorusError::new(format!(
                "function '{name}' sits {d} calls deep — the nesting cap is {MAX_FN_DEPTH}"
            )));
        }
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deck_machinery_matches_the_w0_vectors_and_the_draw_rules() {
        // the moved-from core/deck_rng vectors stay pinned through the new
        // home (ts: test/decks.test.ts:141-144)
        assert_eq!(permute_positions(4, 0, "RUMOURS"), vec![0, 2, 1, 3]);
        assert_eq!(permute_positions(4, 42, "RUMOURS"), vec![3, 0, 1, 2]);
        assert_eq!(permute_positions(7, 0, "SEVEN"), vec![0, 6, 3, 1, 4, 5, 2]);
        assert_eq!(permute_positions(7, -3, "NEG"), vec![4, 0, 2, 6, 3, 1, 5]);
        let deck = CompiledDeck {
            id: "SINGLES".into(),
            order: vec![DeckCard {
                name: "HEARSAY".into(),
                kind: "idea".into(),
            }],
            defaultcard: None,
            reset: false,
        };
        let err = draw_from(&deck, 1).unwrap_err();
        assert_eq!(
            err.msg,
            "deck 'SINGLES' is exhausted — no 'reset' and no 'defaultcard' (§9.42)"
        );
    }

    #[test]
    fn recipe_id_mint_keeps_both_frozen_bases() {
        // QUIRK(Q1/ENG-15): compile 0-based vs lint 1-based, one function
        assert_eq!(mint_recipe_id("study", Some("x"), 0, 0), "study.recipe0");
        assert_eq!(mint_recipe_id("study", Some("x"), 3, 1), "study.recipe4");
        assert_eq!(mint_recipe_id("study", None, 7, 0), "study.default");
        assert_eq!(mint_recipe_id("study", None, 7, 1), "study.default");
    }

    #[test]
    fn entries_in_js_order_fronts_integer_like_keys() {
        let mut m = IndexMap::new();
        m.insert("container".to_string(), 1);
        m.insert("5".to_string(), 2);
        m.insert("0".to_string(), 3);
        let got: Vec<(&str, i64)> = entries_in_js_order(&m)
            .into_iter()
            .map(|(k, v)| (k, *v))
            .collect();
        assert_eq!(got, [("0", 3), ("5", 2), ("container", 1)]);
    }

    #[test]
    fn empty_string_when_and_query_are_falsy_like_js() {
        // wave-2 must-fix-1 — ts: content.ts:224/230: `s.query ? …` and
        // `r.when ? … : null` read JS truthiness, so a hand-built ContentRaw
        // with when: '' compiles as the DEFAULT recipe (minting
        // `${verb}.default`, no parse error) and query: '' compiles to no
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
                    }],
                    duration: 0,
                    recipes: vec![RecipeDef {
                        id: None,
                        when: Some(String::new()),
                        shown: None,
                        hint: None,
                        out: vec!["NARRATE \"idle.\"".to_string()],
                    }],
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
