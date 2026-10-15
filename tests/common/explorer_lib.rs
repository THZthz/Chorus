//! Shared machinery for the nightjar explorer tests — the compiled
//! Nightjar Flyer content, the candidate generator (sampled for
//! the walk, exhaustive for the sweep), and the state sources both passes play
//! against. No test framework here — scripts and tests import this alike.
//!
//! Modeling note: the world bundle (`dict`, `content`, `base`,
//! `meta`, `player`) is [`ExplorerWorld`], built fresh per call —
//! `compile_script` is pure, so the values are identical; there is no
//! process-global state.

//! (Shared test lib: every helper is the common discipline, not per-file
//! dead code — each mounting test crate uses a different subset, exactly like
//! harness.rs. The `allow` lives in this one place.)
#![allow(dead_code)]

use indexmap::IndexMap;

use chorus::content::content::{CompiledContent, CompiledSlot};
use chorus::content::script::{compile_script, ScriptMeta};
use chorus::core::clock::{abs_min, clock_from_abs_min, format_clock, next_step_clock, Clock};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::query::match_query;
use chorus::core::r#ref::{name_ref, Ref};
use chorus::core::roles::{Role, ROLES};
use chorus::core::sentence::{empty_clause, Clause};
use chorus::core::types::{clone_state, Entity, State};
use chorus::engine::engine::{ActionRecord, Step};
use chorus::engine::fold::fold;
use chorus::js::{ChorusError, Lcg, Result};
use chorus::session::lenient::{parse_player_input, LenientCtx, ParsedInput};

use super::settle::settle_step;

/// The script text is DATA: embedded by path, never precompiled.
pub const SCRIPT_TEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/the-nightjar-flyer.txt"
));

/// The world bundle: the dictionary, the compiled script's
/// `content`/`base`/`meta`, and the player name. `meta` rides
/// along for the explorer tests that assert on it (title/goal/start).
pub struct ExplorerWorld {
    pub dict: Dictionary,
    pub content: CompiledContent,
    pub base: State,
    #[allow(dead_code)] // consumed by the explorer tests
    pub meta: ScriptMeta,
    /// the player name — `meta.player`, defaulting to 'MARCH' when unset
    pub player: String,
}

/// Built fresh per call (see module doc).
pub fn explorer_world() -> ExplorerWorld {
    let dict = core_dictionary();
    let compiled = compile_script(SCRIPT_TEXT, &dict).expect("the nightjar flyer compiles");
    let player = compiled
        .meta
        .player
        .clone()
        .unwrap_or_else(|| "MARCH".to_string());
    ExplorerWorld {
        dict,
        content: compiled.content,
        base: compiled.state,
        meta: compiled.meta,
        player,
    }
}

// ------------------------ candidate generation -----------------------------

/// A candidate action: a display tag plus the step it plays.
#[derive(Debug, Clone)]
pub struct Cand {
    pub tag: String,
    pub step: Step,
}

/// 'sampled' = the bounded space the random walk
/// picks from; 'full' = the sweep's exhaustive space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    Sampled,
    Full,
}

/// roles whose fill is a list (the rest are single-ref and cannot over-fill)
const LIST_ROLES: [&str; 3] = ["actors", "object", "with"];

/// The role's ref list — the single-ref
/// roles yield the one ref or null. The JSON image (null / array / object)
/// is the canonical wire form.
fn role_refs(c: &Clause, role: Role) -> serde_json::Value {
    match role {
        Role::Actors => serde_json::to_value(&c.actors).unwrap_or(serde_json::Value::Null),
        Role::Object => serde_json::to_value(&c.objects).unwrap_or(serde_json::Value::Null),
        Role::With => serde_json::to_value(&c.with).unwrap_or(serde_json::Value::Null),
        Role::At => serde_json::to_value(&c.at).unwrap_or(serde_json::Value::Null),
        Role::About => serde_json::to_value(&c.about).unwrap_or(serde_json::Value::Null),
        Role::To => serde_json::to_value(&c.to).unwrap_or(serde_json::Value::Null),
    }
}

/// The clause-set key: the verb plus every role's ref-list JSON, `|`-joined.
#[must_use]
pub fn clause_key(c: &Clause) -> String {
    let mut parts: Vec<String> = vec![c.verb.clone()];
    for r in ROLES {
        parts.push(role_refs(c, r).to_string());
    }
    parts.join("|")
}

/// every k-subset of arr with min ≤ k ≤ max, in
/// pool order (deterministic). `max: None` models JS `Infinity` (the recursion
/// then only stops when the pool is exhausted).
fn subsets(arr: &[&Entity], min: i64, max: Option<i64>) -> Vec<Vec<Ref>> {
    let mut out: Vec<Vec<Ref>> = Vec::new();
    walk(arr, min, max, 0, &mut Vec::new(), &mut out);
    out
}

fn walk(
    arr: &[&Entity],
    min: i64,
    max: Option<i64>,
    start: usize,
    cur: &mut Vec<Ref>,
    out: &mut Vec<Vec<Ref>>,
) {
    if cur.len() as i64 >= min {
        out.push(cur.clone());
    }
    if max.is_some_and(|m| cur.len() as i64 >= m) {
        return;
    }
    for i in start..arr.len() {
        cur.push(name_ref(arr[i].name.clone(), None));
        walk(arr, min, max, i + 1, cur, out);
        cur.pop();
    }
}

/// the keyed dedupe: first tag for a clause-set
/// key wins, insertion order preserved.
fn push_tag(out: &mut IndexMap<String, Cand>, clock: Clock, tag: &str, clauses: Vec<Clause>) {
    let key = clauses
        .iter()
        .map(clause_key)
        .collect::<Vec<_>>()
        .join(";");
    out.entry(key).or_insert_with(|| Cand {
        tag: tag.to_string(),
        step: Step {
            start_min: None,
            clock: Some(clock),
            clauses,
            t_real: None,
        },
    });
}

/// Assemble a clause from a per-role fill map.
fn build_clause(verb_name: &str, over: &IndexMap<String, Vec<Ref>>) -> Clause {
    let mut c = empty_clause(vec![]);
    c.verb = verb_name.to_string();
    c.actors = over.get("actors").cloned().unwrap_or_default();
    c.objects = over.get("object").cloned().unwrap_or_default();
    c.with = over.get("with").cloned().unwrap_or_default();
    c.at = over.get("at").and_then(|v| v.first().cloned());
    c.about = over.get("about").and_then(|v| v.first().cloned());
    c.to = over.get("to").and_then(|v| v.first().cloned());
    c
}

/// the small-verb cartesian product, enumerated first-role-outermost (deterministic order).
fn product_rec(
    i: usize,
    roles: &[Role],
    fills: &IndexMap<String, Vec<Vec<Ref>>>,
    idx: &mut IndexMap<Role, usize>,
    verb_name: &str,
    out: &mut IndexMap<String, Cand>,
    clock: Clock,
) {
    if i == roles.len() {
        let mut over: IndexMap<String, Vec<Ref>> = IndexMap::new();
        for r in roles {
            // idx always has r
            // by the time this depth runs (set by the loop one level up);
            // a missing fill is an empty vec
            let v = match (fills.get(r.as_str()), idx.get(r)) {
                (Some(list), Some(&k)) => list.get(k).cloned().unwrap_or_default(),
                _ => Vec::new(),
            };
            over.insert(r.as_str().to_string(), v);
        }
        push_tag(
            out,
            clock,
            &format!("{verb_name} product"),
            vec![build_clause(verb_name, &over)],
        );
        return;
    }
    let r = roles[i];
    let Some(list) = fills.get(r.as_str()) else {
        return;
    };
    for k in 0..list.len() {
        idx.insert(r, k);
        product_rec(i + 1, roles, fills, idx, verb_name, out, clock);
    }
}

/// The candidate generator. (`dict` is an explicit parameter rather than a
/// captured global.)
pub fn candidates_for(
    content: &CompiledContent,
    dict: &Dictionary,
    state: &State,
    player: &str,
    depth: Depth,
) -> Vec<Cand> {
    let full = depth == Depth::Full;
    let clock = next_step_clock(state.now); // the front door's rule (§9.28)
    let alive: Vec<&Entity> = state
        .entities
        .values()
        .filter(|e| e.dead_action.is_none())
        .collect();
    let chars: Vec<&Entity> = alive
        .iter()
        .copied()
        .filter(|e| e.kind == "character")
        .collect();
    let pool = |slot: &CompiledSlot| -> Vec<&Entity> {
        match &slot.query {
            Some(q) => alive
                .iter()
                .copied()
                .filter(|e| match_query(e, q))
                .collect(),
            None => alive.clone(),
        }
    };

    let mut out: IndexMap<String, Cand> = IndexMap::new();
    let mut min_fill_clauses: IndexMap<String, Clause> = IndexMap::new();

    for verb in content.verbs.values() {
        // options per declared role: empty (when allowed), singles, and
        // shaped extras.
        let mut fills: IndexMap<String, Vec<Vec<Ref>>> = IndexMap::new();
        for slot in &verb.slots {
            let mut opts: Vec<Vec<Ref>> = Vec::new();
            let candidates: Vec<&Entity> =
                if slot.role == "actors" && !chars.is_empty() {
                    chars.clone()
                } else {
                    pool(slot)
                };
            if full {
                // the whole cardinality space (§6.1): every k-subset in
                // [min, max] …
                for set in subsets(&candidates, slot.min, slot.max) {
                    opts.push(set);
                }
                // … then the boundary: one card beyond the declared max
                // (an unbounded max — Infinity — fails both comparisons,
                // hence Option)
                if let Some(m) = slot.max {
                    if candidates.len() as i64 > m && m > 0 {
                        opts.push(
                            candidates
                                .iter()
                                .take((m + 1) as usize)
                                .map(|e| name_ref(e.name.clone(), None))
                                .collect(),
                        );
                    }
                }
            } else {
                if slot.min == 0 {
                    opts.push(vec![]);
                }
                for e in candidates.iter().take(4) {
                    opts.push(vec![name_ref(e.name.clone(), None)]);
                }
                // an actors slot max >= 2 — unbounded (None) passes
                let actors_roomy = verb
                    .slot_by_role
                    .get("actors")
                    .is_some_and(|s| s.max.is_none_or(|m| m >= 2));
                if slot.role == "actors" && actors_roomy && chars.len() >= 2 {
                    opts.push(
                        chars
                            .iter()
                            .take(2)
                            .map(|e| name_ref(e.name.clone(), None))
                            .collect(),
                    );
                }
                if slot.role == "with" && candidates.len() >= 2 {
                    opts.push(
                        candidates
                            .iter()
                            .take(std::cmp::min(3, candidates.len()))
                            .map(|e| name_ref(e.name.clone(), None))
                            .collect(),
                    );
                }
            }
            fills.insert(slot.role.clone(), opts);
        }

        // baseline valid fill: the first option covering each slot's minimum.
        let mut min_fill: IndexMap<String, Vec<Ref>> = IndexMap::new();
        let mut viable = true;
        for role in ROLES {
            match verb.slot_by_role.get(role.as_str()) {
                None => {
                    min_fill.insert(role.as_str().to_string(), Vec::new());
                }
                Some(slot) => {
                    let cover: Vec<&Vec<Ref>> = fills
                        .get(role.as_str())
                        .map(|opts| {
                            opts.iter()
                                .filter(|o| o.len() as i64 >= slot.min)
                                .collect()
                        })
                        .unwrap_or_default();
                    if cover.is_empty() {
                        viable = false;
                        break;
                    }
                    min_fill.insert(role.as_str().to_string(), cover[0].clone());
                }
            }
        }
        if !viable {
            continue;
        }

        let mk = |over: &IndexMap<String, Vec<Ref>>, tag: &str, out: &mut IndexMap<String, Cand>| {
            push_tag(out, clock, tag, vec![build_clause(&verb.name, over)]);
        };

        mk(&min_fill, &format!("{} min-fill", verb.name), &mut out);
        if full {
            min_fill_clauses.insert(verb.name.clone(), build_clause(&verb.name, &min_fill));
        }

        for role in ROLES {
            let Some(slot) = verb.slot_by_role.get(role.as_str()) else {
                continue;
            };
            if let Some(opts) = fills.get(role.as_str()) {
                for opt in opts {
                    let mut over = min_fill.clone();
                    over.insert(role.as_str().to_string(), opt.clone());
                    mk(
                        &over,
                        &format!("{} {}={}", verb.name, role.as_str(), opt.len()),
                        &mut out,
                    );
                }
            }
            // deliberate slot-mismatch probe(s): card(s) that must not fit.
            if let Some(q) = &slot.query {
                let wrong: Vec<&Entity> =
                    alive.iter().copied().filter(|e| !match_query(e, q)).collect();
                let wrong: Vec<&Entity> = if full {
                    wrong
                } else {
                    wrong.into_iter().take(1).collect()
                };
                for w in wrong {
                    let mut over = min_fill.clone();
                    over.insert(
                        role.as_str().to_string(),
                        vec![name_ref(w.name.clone(), None)],
                    );
                    mk(
                        &over,
                        &format!("{} {}=wrong-kind", verb.name, role.as_str()),
                        &mut out,
                    );
                }
            }
            // §6.7: a role takes DISTINCT cards — the same card twice in one
            // role (full sweep only).
            if full && LIST_ROLES.contains(&role.as_str()) {
                let first = min_fill.get(role.as_str()).and_then(|v| v.first()).cloned();
                if let Some(first) = first {
                    let mut over = min_fill.clone();
                    over.insert(role.as_str().to_string(), vec![first.clone(), first]);
                    mk(
                        &over,
                        &format!("{} {}=dupe", verb.name, role.as_str()),
                        &mut out,
                    );
                }
            }
        }

        if full {
            // §6.7: distinct cards ACROSS roles too — force each pair of
            // LIST_ROLES to share the same card and let the engine rule.
            for (i, a) in LIST_ROLES.into_iter().enumerate() {
                for b in &LIST_ROLES[i + 1..] {
                    let Some(ref_a) = min_fill.get(a).and_then(|v| v.first()).cloned() else {
                        continue;
                    };
                    if !verb.slot_by_role.contains_key(a)
                        || !verb.slot_by_role.contains_key(*b)
                    {
                        continue;
                    }
                    let mut over = min_fill.clone();
                    over.insert(b.to_string(), vec![ref_a]);
                    mk(
                        &over,
                        &format!("{} {}/{}=shared", verb.name, a, b),
                        &mut out,
                    );
                }
            }
            // small verbs: the full cartesian product of role options.
            let roles_with_opts: Vec<Role> = ROLES
                .iter()
                .copied()
                .filter(|r| {
                    fills
                        .get(r.as_str())
                        .is_some_and(|opts| !opts.is_empty())
                })
                .collect();
            let mut product: usize = 1;
            for r in &roles_with_opts {
                if let Some(opts) = fills.get(r.as_str()) {
                    product *= opts.len();
                }
            }
            if product <= 3000 && roles_with_opts.len() >= 2 {
                let mut idx: IndexMap<Role, usize> = IndexMap::new();
                product_rec(0, &roles_with_opts, &fills, &mut idx, &verb.name, &mut out, clock);
            }
        }

        // witness × topic matrix for verbs that interrogate (to × about).
        let to_opts = fills.get("to").cloned().unwrap_or_default();
        let about_opts = fills.get("about").cloned().unwrap_or_default();
        if !to_opts.is_empty() && !about_opts.is_empty() {
            let cap = if full { usize::MAX } else { 4 };
            for t in to_opts.iter().take(cap) {
                for a in about_opts.iter().take(cap) {
                    let mut over = min_fill.clone();
                    over.insert("to".to_string(), t.clone());
                    over.insert("about".to_string(), a.clone());
                    mk(&over, &format!("{} to×about", verb.name), &mut out);
                }
            }
        }
    }

    // multi-clause and clock-discipline probes.
    let wait_clauses = parse_sentence(
        &format!("{} @ {} WAIT", format_clock(clock), player),
        dict,
    )
    .expect("the WAIT probe sentence parses")
    .clauses;
    let mut doubled = wait_clauses.clone();
    doubled.extend(wait_clauses.iter().cloned());
    push_tag(&mut out, clock, "multi-clause", doubled);
    if full {
        // every verb against WAIT (both orders) and every verb pair — summed
        // durations (§6.6), per-clause recipe provenance, and role-ref
        // re-resolution against the draft over multi-clause steps
        let names: Vec<String> = min_fill_clauses.keys().cloned().collect();
        for name in &names {
            let c = &min_fill_clauses[name];
            let mut then_wait = vec![c.clone()];
            then_wait.extend(wait_clauses.iter().cloned());
            push_tag(&mut out, clock, &format!("{name} then wait"), then_wait);
            let mut wait_then = wait_clauses.clone();
            wait_then.push(c.clone());
            push_tag(&mut out, clock, &format!("wait then {name}"), wait_then);
            for other in &names {
                if other == name {
                    continue;
                }
                push_tag(
                    &mut out,
                    clock,
                    &format!("{name} then {other}"),
                    vec![c.clone(), min_fill_clauses[other].clone()],
                );
            }
        }
    }
    if state.now >= 60 {
        let past = clock_from_abs_min(abs_min(clock) - 60);
        let rewind = parse_sentence(
            &format!("{} @ {} WAIT", format_clock(past), player),
            dict,
        )
        .expect("the clock-rewind probe sentence parses")
        .clauses;
        push_tag(&mut out, clock, "clock rewind", rewind);
    }
    out.into_values().collect()
}

// ------------------------ state sources ------------------------------------

/// The canonical win path through the Nightjar Flyer (nine commands).
pub const WIN_PATH: [&str; 9] = [
    "wait",
    "explore the first cabin",
    "read the solicitor letter",
    "explore the firebox",
    "read the vane ledger",
    "talk with the spiritualist", // meet Seraphine — the name is learned by talking
    "seraphine read the wax cylinder",
    "seraphine read the telegram",
    "use on the inquest with the embezzlement the stolen voice the vienna warning",
];

/// Parses a player-ish line; throws when a win-path line stops parsing.
fn parse_playerish(world: &ExplorerWorld, line: &str, state: &State) -> Result<Step> {
    match parse_player_input(
        line,
        &LenientCtx {
            dict: &world.dict,
            content: &world.content,
            state,
            player: Some(&world.player),
        },
    ) {
        ParsedInput::Ok { step, .. } => Ok(step),
        ParsedInput::Fail { error, .. } => Err(ChorusError::new(format!(
            "win path line not parseable: '{line}': {error}"
        ))),
    }
}

/// The labeled per-step state snapshots plus the whole record list.
pub type PathStates = (Vec<(String, State)>, Vec<ActionRecord>);

/// play the win path on primitives, snapshotting a fresh clone after each step
/// (each step settles — its window completes, the way the pre-v0.5 engine
/// committed effects at once).
pub fn win_path_states(world: &ExplorerWorld) -> Result<PathStates> {
    let mut state = clone_state(&world.base);
    let mut states = vec![("start".to_string(), clone_state(&state))];
    let mut records: Vec<ActionRecord> = Vec::new();
    let mut id: i64 = 1;
    for line in WIN_PATH {
        let step = parse_playerish(world, line, &state)?;
        let r = settle_step(&world.content, &world.dict, state, step, id, None)?;
        if let Some(rejected) = r.rejected {
            return Err(ChorusError::new(format!(
                "win path broke at '{line}': {rejected}"
            )));
        }
        let n = r.records.len();
        records.extend(r.records);
        id += n as i64;
        state = r.state;
        states.push((format!("after {line}"), clone_state(&state)));
    }
    Ok((states, records))
}

/// diverse states beyond the win path: a short deterministic sampled walk
/// (no verification — the walk pass owns that) harvested every 20 applied
/// steps, up to ten states.
pub fn sampled_walk_states(world: &ExplorerWorld) -> Result<Vec<(String, State)>> {
    // 'NIGH' — same LCG discipline as the walk pass (the RAW state,
    // no XOR — js::Lcg::from_raw_state)
    let mut lcg = Lcg::from_raw_state(0x4e49_4748);
    let mut state = clone_state(&world.base);
    let mut records: Vec<ActionRecord> = Vec::new();
    let mut states: Vec<(String, State)> = Vec::new();
    let mut turn_lengths: Vec<usize> = vec![0]; // ending-undo truncates to the previous turn (v0.5)
    let mut id: i64 = 1;
    let mut executed = 0usize;
    let mut i = 0usize;
    while i < 500 && states.len() < 10 {
        i += 1;
        let cands = candidates_for(
            &world.content,
            &world.dict,
            &state,
            &world.player,
            Depth::Sampled,
        );
        let benign: Vec<&Cand> = cands
            .iter()
            .filter(|c| !c.tag.contains("wrong-kind") && c.tag != "clock rewind")
            .collect();
        let pick_from: Vec<&Cand> =
            if lcg.next_f64() < 0.2 || benign.is_empty() {
                cands.iter().collect()
            } else {
                benign
            };
        // multi-clause is always pushed, so the pick space is never empty
        debug_assert!(
            !pick_from.is_empty(),
            "candidates_for always offers at least the multi-clause probe"
        );
        let pick = ((lcg.next_f64() * pick_from.len() as f64).floor() as usize)
            % pick_from.len();
        let cand = &pick_from[pick];
        let r = settle_step(
            &world.content,
            &world.dict,
            state,
            cand.step.clone(),
            id,
            None,
        )?;
        // any rejection returns the untouched input state (settle rolls back
        // whole), so the assignment is exact on every branch
        state = r.state;
        if r.rejected.is_some() {
            continue;
        }
        turn_lengths.push(records.len());
        let n = r.records.len();
        records.extend(r.records);
        id += n as i64;
        // §9.35 ending-awareness, as in the walk pass: undo through the
        // ending step so the harvest keeps exercising real play and reaches
        // later hours
        if state.ended.is_some() {
            let len = turn_lengths
                .pop()
                .expect("turn_lengths never empties (seeded [0])");
            records.truncate(len);
            state = fold(
                &records,
                &clone_state(&world.base),
                &world.dict,
                &world.content,
            )?;
            continue;
        }
        executed += 1;
        // harvest every 20 applied steps
        if executed.is_multiple_of(20) {
            states.push((format!("walk {executed}"), clone_state(&state)));
        }
    }
    Ok(states)
}
