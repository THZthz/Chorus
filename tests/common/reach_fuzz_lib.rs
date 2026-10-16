//! Differential-fuzz machinery: gen_scenario + the
//! LCG + the frozen action selector. The selector and its caller discipline
//! are the frozen surface: they must stay
//! bit-identical draw for draw — that identity is the
//! precondition for reproducing every recorded fuzz run at all.
//!
//! `gen_scenario` keeps the frozen rng draw order (every short-circuit and
//! evaluation-order choice is deliberate — see the inline draw-order notes),
//! because recorded runs are only reproducible while scenario construction
//! is draw-for-draw identical.
//!
//! (Shared test lib discipline: the `allow` lives in this one place.)
#![allow(dead_code)]

use indexmap::IndexMap;

use chorus::content::content::{
    compile_content, CompiledContent, CompiledSlot, ContentRaw, DeckRaw, DeclaredEntity,
    ExpireTriggerDef, PrototypeDef, PrototypeDerive, RecipeDef, SlotDef, SlotTriggerDef,
    TriggerDef, VerbContent,
};
use chorus::core::clock::{abs_min, clock_from_abs_min, format_clock, next_step_clock};
use chorus::core::dict::{core_dictionary, Dictionary, CORE_VERBS};
use chorus::core::parse::parse_sentence;
use chorus::core::query::match_query;
use chorus::core::r#ref::{id_ref, name_ref, Ref};
use chorus::core::sentence::{empty_clause, Clause};
use chorus::core::types::{add_entity, new_state, AspectVal, Entity, State};
#[allow(unused_imports)]
// ActionRecord only flows through drive_fuzz_seed (unused in the #[path]-mounted fuzz bin)
use chorus::engine::engine::{ActionRecord, Step};
use chorus::error::Result;
use chorus::lcg::Lcg;

use super::settle::settle_step;

// ---------- deterministic RNG (the frozen LCG discipline) ----------

/// `Lcg::new` XORs 0x9e3779b9 into the seed; `next_f64`
/// supplies exactly one uniform draw per call.
#[must_use]
pub fn rng(seed: u32) -> Lcg {
    Lcg::new(seed)
}

/// One uniform draw, floored into `0..n`.
fn int(r: &mut Lcg, n: usize) -> usize {
    r.next_int(n)
}

/// Index draw, then clone the string out (the draw order is what is frozen,
/// not the borrow).
fn pick_str(r: &mut Lcg, arr: &[&str]) -> String {
    arr[int(r, arr.len())].to_string()
}

/// Index draw over owned Strings, then clone.
fn pick_of(r: &mut Lcg, arr: &[String]) -> String {
    arr[int(r, arr.len())].clone()
}

/// Sample `n` distinct entries: each draw removes one pool element.
fn sample_distinct(r: &mut Lcg, arr: &[String], n: usize) -> Vec<String> {
    let mut pool = arr.to_vec();
    let mut out: Vec<String> = Vec::new();
    while out.len() < n && !pool.is_empty() {
        let i = int(r, pool.len());
        out.push(pool.remove(i));
    }
    out
}

// ---------- scenario generator ----------

/// The literal pools.
const ASPECTS: [&str; 2] = ["a", "b"];
const NAME_POOL: [&str; 5] = ["LAMP", "ROPE", "LEDGER", "CRATE", "VIAL"];
const KIND_POOL: [&str; 5] = ["item", "text", "idea", "place", "resource"];

fn str_vec(vs: &[&str]) -> Vec<String> {
    vs.iter().map(|s| (*s).to_string()).collect()
}

/// CORE_VERBS minus WAIT.
fn non_wait_verbs() -> Vec<String> {
    CORE_VERBS
        .iter()
        .filter(|v| **v != "WAIT")
        .map(|v| (*v).to_string())
        .collect()
}

/// KIND_POOL minus place.
fn non_char_kinds() -> Vec<String> {
    KIND_POOL
        .iter()
        .filter(|k| **k != "place")
        .map(|k| (*k).to_string())
        .collect()
}

/// The analyzer-facing probe list rides along; its draws continue on
/// the SAME rng after the content is built, so the order below is part of the
/// frozen draw order.
pub struct Probe {
    pub mode: ProbeMode,
    pub q: String,
}
pub enum ProbeMode {
    Name,
    Demand,
}

/// One generated scenario: the raw content, its compile inputs, the initial
/// state, and the probe list.
pub struct Scenario {
    pub seed: i64,
    pub raw: ContentRaw,
    pub initial: State,
    /// the initial entities as name/kind pairs — compile_content's deck-entry
    /// validation takes them explicitly
    pub entity_kinds: Vec<DeclaredEntity>,
    pub probes: Vec<Probe>,
}

/// Fuzz entities are minted at action 0 by convention (born_action Some(0)),
/// unlike the test-fixture worlds' None.
fn ent(name: &str, kind: &str, aspects: IndexMap<String, i64>) -> Entity {
    Entity {
        id: format!("s_{}", name.to_lowercase()),
        kind: kind.to_string(),
        name: name.to_string(),
        aspects: aspects
            .into_iter()
            .map(|(k, v)| (k, AspectVal::Num(v)))
            .collect(),
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
    }
}

/// The scenario's mutable build context — the rng handle plus the local
/// tables every generator step draws against, threaded explicitly.
struct Gen<'a> {
    r: &'a mut Lcg,
    others: Vec<String>,
    proto_names: Vec<String>,
    has_house: bool,
    has_deck: bool,
}

impl Gen<'_> {
    /// One draw in 0..4.
    fn aspect_val(&mut self) -> i64 {
        int(self.r, 4) as i64
    }

    /// The outcome-op generator. Every branch guard's draw happens EXACTLY
    /// where the guard evaluates it (short-circuits included) — the draw
    /// order is frozen.
    fn op(&mut self, allow_refuse: bool) -> String {
        let roll = self.r.next_f64();
        if roll < 0.22 {
            let pn = pick_of(self.r, &self.proto_names);
            let kind_override = if self.r.next_f64() < 0.2 {
                format!("{}:", pick_of(self.r, &non_char_kinds()))
            } else {
                String::new()
            };
            // short-circuit: no draw when has_house is false
            let into = if self.has_house && self.r.next_f64() < 0.15 {
                " INTO HOUSE".to_string()
            } else {
                String::new()
            };
            return format!("SPAWN {kind_override}{pn}{into}");
        }
        if roll < 0.32 && !self.others.is_empty() {
            let a = pick_of(self.r, &self.others);
            let b = pick_of(self.r, &self.proto_names);
            return format!("REPLACE {a} WITH {b}");
        }
        // the deck check draws nothing; a deckless world
        // falls through to the SET branch
        if roll < 0.42 && self.has_deck {
            return "DRAW D1".to_string();
        }
        if roll < 0.56 {
            let tgt = pick_str(self.r, &["ACTORS", "OBJECT", "ANA", "BERTHA"]);
            // the "expires" condition draw ALWAYS happens (it precedes the
            // key pick) even when the key is not added
            let has_expires = self.r.next_f64() < 0.1;
            let mut keys = vec![
                "aspect:a".to_string(),
                "aspect:b".to_string(),
                "coin".to_string(),
                "state.sealed".to_string(),
            ];
            if has_expires {
                keys.push("expires".to_string());
            }
            let key = pick_of(self.r, &keys);
            let val = if key == "expires" {
                pick_str(self.r, &["30m", "1halfhour"])
            } else if self.r.next_f64() < 0.6 {
                format!("+{}", 1 + int(self.r, 2) as i64)
            } else {
                int(self.r, 4).to_string()
            };
            return format!("SET({tgt}, {key}, {val})");
        }
        if roll < 0.62 && !self.others.is_empty() {
            let t = pick_of(self.r, &self.others);
            return format!("DESTROY {t}");
        }
        if roll < 0.70 && self.has_house {
            return "MOVE ANA INTO HOUSE".to_string();
        }
        if roll < 0.78 {
            let mut xs = self.others.clone();
            xs.extend(self.proto_names.iter().cloned());
            xs.push("ANA".to_string());
            xs.push("GHOST".to_string());
            let x = pick_of(self.r, &xs);
            // the 0.3 draw always happens; the length check is the
            // (free) right operand of the &&
            let nested = self.r.next_f64() < 0.3 && self.proto_names.len() >= 2;
            let inner = if nested {
                let g = pick_of(self.r, &self.proto_names);
                let p = pick_of(self.r, &self.proto_names);
                format!("IF exists({g}) -> (SPAWN {p}) ELSE -> (NARRATE \"nested\")")
            } else {
                let p = pick_of(self.r, &self.proto_names);
                format!("SPAWN {p}")
            };
            return format!("IF exists({x}) -> ({inner})");
        }
        if roll < 0.90 {
            let dice = if self.r.next_f64() < 0.3 { 2 } else { 1 };
            // the template draws left to right — the bound, then the mint
            let bound = 2 + int(self.r, 4 * dice) as i64;
            let p = pick_of(self.r, &self.proto_names);
            return format!(
                "ROLL({dice}, 6), IF roll >= {bound} -> (SPAWN {p}) ELSE -> (NARRATE \"low\")"
            );
        }
        if roll < 0.94 && allow_refuse {
            return "REFUSE(\"no\")".to_string();
        }
        "NARRATE \"beat\"".to_string()
    }

    /// The `when` candidates. All four are built EAGERLY — every template
    /// interpolation draws BEFORE the final pick selects one:
    /// t1 one int, t2 one pool pick, t3 none, t4 one proto-name pick.
    fn gen_when(&mut self) -> String {
        let t1 = format!("actors.aspect.a >= {}", 1 + int(self.r, 3) as i64);
        let mut pool = self.others.clone();
        pool.extend(self.proto_names.iter().cloned());
        pool.push("ANA".to_string());
        pool.push("GHOST".to_string());
        let inner = pick_of(self.r, &pool);
        let t2 = format!("exists({inner})");
        let t3 = "count(name:ANA) >= 1".to_string();
        let named_pool: Vec<String> = if self.proto_names.is_empty() {
            vec!["P1".to_string()]
        } else {
            self.proto_names.clone()
        };
        let named = pick_of(self.r, &named_pool);
        let t4 = format!("count(name:{named}) >= 1");
        let choices: [&str; 4] = [&t1, &t2, &t3, &t4];
        pick_str(self.r, &choices)
    }
}

/// The scenario generator — the frozen draw order described in the module
/// header.
pub fn gen_scenario(seed: u32) -> Scenario {
    let mut r = rng(seed);
    let non_wait = non_wait_verbs();
    // the count draw happens BEFORE sample_distinct's internal draws
    let n_verbs = 2 + int(&mut r, 3); // 2–4
    let verbs = sample_distinct(&mut r, &non_wait, n_verbs);

    // initial entities — two characters (ANA the player + maybe BERTHA), 1–3
    // pool cards, maybe a place
    let mut entities: Vec<Entity> = vec![ent("ANA", "character", IndexMap::new())];
    let n_others = 1 + int(&mut r, 3);
    let others = sample_distinct(&mut r, &str_vec(&NAME_POOL), n_others);
    for n in &others {
        // draw order: kind pick, then aspect a, then aspect b
        let kind = pick_of(&mut r, &non_char_kinds());
        let a = int(&mut r, 4) as i64;
        let b = int(&mut r, 3) as i64;
        let mut aspects = IndexMap::new();
        aspects.insert("a".to_string(), a);
        aspects.insert("b".to_string(), b);
        entities.push(ent(n, &kind, aspects));
    }
    let has_bertha = r.next_f64() < 0.5;
    if has_bertha {
        entities.push(ent("BERTHA", "character", IndexMap::new()));
    }
    let has_house = r.next_f64() < 0.6;
    if has_house {
        entities.push(ent("HOUSE", "place", IndexMap::new()));
    }

    // prototypes — static aspects, stackable, derive mints (up to one)
    let n_protos = 1 + int(&mut r, 3);
    let proto_names: Vec<String> = ["P1", "P2", "P3"][..n_protos]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    let mut prototypes: IndexMap<String, PrototypeDef> = IndexMap::new();
    let mut derive_done = false;
    for pn in &proto_names {
        let kind = pick_of(&mut r, &non_char_kinds());
        let mut p = PrototypeDef {
            kind,
            ..PrototypeDef::default()
        };
        // key draw, then value draw
        if r.next_f64() < 0.6 {
            let key = pick_str(&mut r, &ASPECTS);
            let v = int(&mut r, 3) as i64;
            let mut aspects = IndexMap::new();
            aspects.insert(key, AspectVal::Num(v));
            p.aspects = Some(aspects);
        }
        if r.next_f64() < 0.25 {
            p.stackable = true;
        }
        // short-circuit: no draw once derive is done
        if !derive_done && r.next_f64() < 0.25 {
            p.derive = Some(PrototypeDerive {
                name_from: Some("object".to_string()),
                aspects: None,
            });
            derive_done = true;
        }
        prototypes.insert(pn.clone(), p);
    }

    // deck — repeats weight the spec; defaultcard/reset combinations.
    // The 0.6 deck draw ALWAYS happens (left operand of the &&).
    let mut deck_names: Vec<String> = proto_names.clone();
    deck_names.extend(others.iter().cloned());
    let deck_roll = r.next_f64();
    let deck: Option<DeckRaw> = if deck_roll < 0.6 && deck_names.len() >= 2 {
        let n_spec = 2 + int(&mut r, 2);
        let mut spec: Vec<String> = Vec::new();
        for _ in 0..n_spec {
            spec.push(pick_of(&mut r, &deck_names));
        }
        // draw order — the defaultcard condition draw, then the reset
        let defaultcard = if r.next_f64() < 0.4 {
            Some(pick_of(&mut r, &deck_names))
        } else {
            None
        };
        let reset = r.next_f64() < 0.3;
        Some(DeckRaw {
            id: "D1".to_string(),
            spec,
            defaultcard,
            reset,
        })
    } else {
        None
    };

    // verbs × recipes
    let mut ctx = Gen {
        r: &mut r,
        others: others.clone(),
        proto_names: proto_names.clone(),
        has_house,
        has_deck: deck.is_some(),
    };

    let mut verbs_raw: IndexMap<String, VerbContent> = IndexMap::new();
    verbs_raw.insert(
        "WAIT".to_string(),
        VerbContent {
            slots: vec![SlotDef {
                role: "actors".to_string(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".to_string()),
                shown: None,
            }],
            duration: 30,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: vec![],
            }],
        },
    );
    for v in &verbs {
        let mut slots: Vec<SlotDef> = vec![SlotDef {
            role: "actors".to_string(),
            min: 1,
            max: Some(1),
            query: Some("kind:character".to_string()),
            shown: None,
        }];
        let n_slots = int(ctx.r, 3); // 0–2 non-actor slots
        for _ in 0..n_slots {
            let role = pick_str(ctx.r, &["object", "with", "with"]);
            let mut query = String::new();
            if ctx.r.next_f64() < 0.5 {
                query.push_str(&format!(" kind:{}", pick_of(ctx.r, &non_char_kinds())));
            }
            if ctx.r.next_f64() < 0.4 {
                let a = pick_str(ctx.r, &ASPECTS);
                let bound = 1 + int(ctx.r, 3) as i64;
                query.push_str(&format!(" aspect:{a}>={bound}"));
            }
            let query = query.trim().to_string();
            // short-circuit on the role check (min:2 slots are "with"-only)
            let double = role == "with" && ctx.r.next_f64() < 0.15;
            slots.push(SlotDef {
                role,
                min: if double { 2 } else { 1 },
                max: Some(if double { 2 } else { 1 }),
                query: if query.is_empty() { None } else { Some(query) },
                shown: None,
            });
        }
        let n_recipes = 1 + int(ctx.r, 3);
        let mut recipes: Vec<RecipeDef> = Vec::new();
        for i in 0..n_recipes {
            let when = if ctx.r.next_f64() < 0.4 {
                Some(ctx.gen_when())
            } else {
                None
            };
            let n_outs = 1 + int(ctx.r, 4);
            let mut outs: Vec<String> = Vec::new();
            for _ in 0..n_outs {
                outs.push(ctx.op(true));
            }
            recipes.push(RecipeDef {
                id: Some(format!("{}.r{}", v.to_lowercase(), i)),
                when,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: outs,
            });
        }
        let duration: i64 = pick_str(ctx.r, &["0", "30", "60"])
            .parse()
            .expect("duration literal");
        verbs_raw.insert(
            v.clone(),
            VerbContent {
                slots,
                duration,
                recipes,
            },
        );
    }

    // triggers
    let n_triggers = 1 + int(ctx.r, 3);
    let mut triggers: Vec<TriggerDef> = Vec::new();
    for i in 0..n_triggers {
        let n_out = 1 + int(ctx.r, 2);
        let mut out: Vec<String> = Vec::new();
        for _ in 0..n_out {
            out.push(ctx.op(false)); // events cannot REFUSE (§6.7)
        }
        let id = format!("t{i}");
        if ctx.r.next_f64() < 0.25 {
            let mut pool = vec!["ANA".to_string()];
            pool.extend(ctx.others.iter().cloned());
            let target = pick_of(ctx.r, &pool);
            triggers.push(TriggerDef::Expire(ExpireTriggerDef {
                id,
                on_expire_of: target,
                out,
            }));
        } else {
            let slot = int(ctx.r, 48) as i64;
            let once = ctx.r.next_f64() < 0.4;
            triggers.push(TriggerDef::Slot(SlotTriggerDef {
                id,
                slot,
                once: Some(once),
                out,
            }));
        }
    }

    let raw_seed = int(ctx.r, 1000) as i64;
    drop(ctx); // release the &mut r borrow for the probe draws below

    let mut initial = new_state(0);
    for e in entities.iter().cloned() {
        add_entity(&mut initial, e).expect("gen_scenario entities never collide (unique names)");
    }

    // probes — every name that could ever exist, derived shapes, and demands;
    // the seen-list preserves first-occurrence order
    let mut probes: Vec<Probe> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    let push_unique = |seen: &mut Vec<String>, name: String| {
        if !seen.contains(&name) {
            seen.push(name.clone());
        }
    };
    push_unique(&mut seen, "ANA".to_string());
    push_unique(&mut seen, "BERTHA".to_string());
    for n in &others {
        push_unique(&mut seen, n.clone());
    }
    for n in &proto_names {
        push_unique(&mut seen, n.clone());
    }
    if let Some(d) = &deck {
        for s in &d.spec {
            push_unique(&mut seen, s.clone());
        }
        if let Some(dc) = &d.defaultcard {
            push_unique(&mut seen, dc.clone());
        }
    }
    for name in seen {
        probes.push(Probe {
            mode: ProbeMode::Name,
            q: name,
        });
    }
    // first derive-bearing prototype, in insertion order
    let derive_proto = proto_names
        .iter()
        .find(|pn| prototypes.get(*pn).is_some_and(|p| p.derive.is_some()))
        .cloned();
    if let Some(dp) = derive_proto {
        for n in &others {
            probes.push(Probe {
                mode: ProbeMode::Name,
                q: format!("{dp}_{n}"),
            });
        }
    }
    probes.push(Probe {
        mode: ProbeMode::Name,
        q: "GHOST".to_string(),
    });
    probes.push(Probe {
        mode: ProbeMode::Demand,
        q: format!("aspect:a>={}", 1 + int(&mut r, 3) as i64),
    });
    probes.push(Probe {
        mode: ProbeMode::Demand,
        q: format!("aspect:b>={}", 1 + int(&mut r, 2) as i64),
    });
    probes.push(Probe {
        mode: ProbeMode::Demand,
        q: format!("kind:{}", pick_str(&mut r, &KIND_POOL)),
    });
    let mut name_pool = others.clone();
    name_pool.extend(proto_names.iter().cloned());
    name_pool.push("ANA".to_string());
    probes.push(Probe {
        mode: ProbeMode::Demand,
        q: format!("name:{}", pick_of(&mut r, &name_pool)),
    });
    let kind = pick_of(&mut r, &non_char_kinds());
    probes.push(Probe {
        mode: ProbeMode::Demand,
        q: format!("kind:{kind} aspect:a>={}", 1 + int(&mut r, 2) as i64),
    });

    let raw = ContentRaw {
        seed: Some(raw_seed),
        verbs: verbs_raw,
        prototypes: Some(prototypes),
        decks: deck.map(|d| vec![d]),
        // an EMPTY trigger list is absent (None), never an empty vec
        triggers: if triggers.is_empty() {
            None
        } else {
            Some(triggers)
        },
        ..ContentRaw::default()
    };

    Scenario {
        seed: seed as i64,
        raw,
        initial,
        entity_kinds: entities
            .iter()
            .map(|e| DeclaredEntity {
                name: e.name.clone(),
                kind: e.kind.clone(),
                stackable: e.stackable,
            })
            .collect(),
        probes,
    }
}

// ---------- the frozen action selector (must stay bit-identical to the
// ---------- frozen caller discipline — that identity is the precondition
// ---------- for reproducing recorded runs) --------------------------------

/// the frozen action selector — ONE uniform draw over the candidate list
/// (never empty: the waits are always offered); draw-for-draw identical to
/// the frozen caller discipline.
#[must_use]
pub fn choose_step<'a>(r: &mut Lcg, steps: &'a [Step]) -> &'a Step {
    let i = r.next_int(steps.len());
    &steps[i]
}

// ---------- the exploration candidate space -------------------------------

/// All k-element subsets of `arr`, in combinatorial order.
fn k_subsets<'a>(arr: &[&'a Entity], k: usize) -> Vec<Vec<&'a Entity>> {
    fn walk<'a>(
        arr: &[&'a Entity],
        k: usize,
        start: usize,
        cur: &mut Vec<&'a Entity>,
        out: &mut Vec<Vec<&'a Entity>>,
    ) {
        if cur.len() == k {
            out.push(cur.clone());
            return;
        }
        for i in start..arr.len() {
            cur.push(arr[i]);
            walk(arr, k, i + 1, cur, out);
            cur.pop();
        }
    }
    let mut out = Vec::new();
    walk(arr, k, 0, &mut Vec::new(), &mut out);
    out
}

/// The bounded candidate space.
pub fn candidates(content: &CompiledContent, dict: &Dictionary, s: &State) -> Vec<Step> {
    let mut out: Vec<Step> = Vec::new();
    let mut min_fills: Vec<Clause> = Vec::new();
    let alive: Vec<&Entity> = s
        .entities
        .values()
        .filter(|e| e.dead_action.is_none())
        .collect();
    let clock = next_step_clock(s.now);
    // waits — advance the clock so slot triggers, expiries and completions
    // strike
    for ahead in [0usize, 1, 2, 12, 24, 48] {
        let begin = if ahead == 0 {
            clock
        } else {
            clock_from_abs_min(abs_min(clock) + ahead as i64 * 30)
        };
        let parsed = parse_sentence(&format!("{} @ ANA WAIT", format_clock(begin)), dict)
            .expect("the WAIT probe parses");
        out.push(Step {
            start_min: None,
            clock: Some(parsed.clock.unwrap_or(begin)),
            clauses: parsed.clauses,
            t_real: None,
        });
    }
    let wait_clause = parse_sentence(&format!("{} @ ANA WAIT", format_clock(clock)), dict)
        .expect("the WAIT probe parses")
        .clauses;
    let id_of = |name: &str| -> String {
        alive
            .iter()
            .find(|e| e.name == name)
            .map(|e| e.id.clone())
            .unwrap_or_else(|| name.to_string())
    };
    for verb in content.verbs.values() {
        if verb.name == "WAIT" {
            continue;
        }
        // the engine validates ONE slot per role: the LAST entry per role wins
        // — eff_slots keeps the last defs in verb.slots order
        let mut eff_slots: Vec<&CompiledSlot> = Vec::new();
        {
            let mut by_role: IndexMap<&str, usize> = IndexMap::new();
            for (i, slot) in verb.slots.iter().enumerate() {
                by_role.insert(slot.role.as_str(), i); // last def wins
            }
            for (i, slot) in verb.slots.iter().enumerate() {
                if by_role.get(slot.role.as_str()).copied() == Some(i) {
                    eff_slots.push(slot);
                }
            }
        }
        let mut per_role: Vec<(String, Vec<Vec<Ref>>)> = Vec::new();
        let mut viable = true;
        for slot in &eff_slots {
            let pool: Vec<&Entity> = alive
                .iter()
                .copied()
                .filter(|e| match slot.query.as_ref() {
                    Some(q) => match_query(e, q),
                    None => true,
                })
                .collect();
            // candidate arity is capped at 2 — an unbounded slot max counts
            // as 2 here, never more
            let max_k: usize = match slot.max {
                None => 2,
                Some(m) => std::cmp::min(m.max(0) as usize, 2),
            };
            let mut opts: Vec<Vec<Ref>> = Vec::new();
            let mut k = slot.min.max(0) as usize;
            while k <= max_k {
                for set in k_subsets(&pool, k) {
                    // a binding taking TWO copies of one name must pin them by
                    // #id
                    let dup = {
                        let mut names: Vec<&str> = set.iter().map(|e| e.name.as_str()).collect();
                        names.sort_unstable();
                        names.windows(2).any(|w| w[0] == w[1])
                    };
                    opts.push(
                        set.iter()
                            .map(|e| {
                                if dup {
                                    id_ref(e.id.clone())
                                } else {
                                    name_ref(e.name.clone(), None)
                                }
                            })
                            .collect(),
                    );
                }
                k += 1;
            }
            if slot.min > 0 && opts.is_empty() {
                viable = false;
                break;
            }
            per_role.push((
                slot.role.clone(),
                if opts.is_empty() {
                    vec![Vec::new()]
                } else {
                    opts
                },
            ));
        }
        if !viable {
            continue;
        }
        // cross-role identity: one entity cannot fill two roles of a clause
        let ref_key = |r: &Ref| -> String {
            match r {
                Ref::Id { id } => id.clone(),
                Ref::Name { name, .. } => id_of(name),
                Ref::Param { .. } => unreachable!("param refs never reach candidates (§9.40)"),
            }
        };
        let mut combos: Vec<IndexMap<String, Vec<Ref>>> = vec![IndexMap::new()];
        for (role, opts) in &per_role {
            let mut next: Vec<IndexMap<String, Vec<Ref>>> = Vec::new();
            'outer: for base in &combos {
                for opt in opts.iter().take(6) {
                    let used: Vec<String> = base.values().flatten().map(&ref_key).collect();
                    if opt.iter().map(&ref_key).any(|id| used.contains(&id)) {
                        continue;
                    }
                    let mut m = base.clone();
                    m.insert(role.clone(), opt.clone());
                    next.push(m);
                    if next.len() >= 8 {
                        break 'outer;
                    }
                }
            }
            combos = next;
        }
        for combo in &combos {
            let mut clause = empty_clause(vec![]);
            clause.verb = verb.name.clone();
            for (role, refs) in combo {
                let first = refs.first().cloned();
                match role.as_str() {
                    "actors" => clause.actors = refs.clone(),
                    "object" => clause.objects = refs.clone(),
                    "with" => clause.with = refs.clone(),
                    "at" => clause.at = first,
                    "about" => clause.about = first,
                    "to" => clause.to = first,
                    _ => {}
                }
            }
            out.push(Step {
                start_min: None,
                clock: Some(clock),
                clauses: vec![clause.clone()],
                t_real: None,
            });
            min_fills.push(clause);
        }
    }
    // multi-clause steps — per-clause recipe provenance, summed durations, and
    // cross-clause locks
    let mut i = 0usize;
    while i < min_fills.len() && i < 3 {
        let mut clauses = vec![min_fills[i].clone()];
        clauses.extend(wait_clause.iter().cloned());
        out.push(Step {
            start_min: None,
            clock: Some(clock),
            clauses,
            t_real: None,
        });
        let mut j = i + 1;
        while j < min_fills.len() && j < 3 {
            out.push(Step {
                start_min: None,
                clock: Some(clock),
                clauses: vec![min_fills[i].clone(), min_fills[j].clone()],
                t_real: None,
            });
            j += 1;
        }
        i += 1;
    }
    out
}

// ---------- exhaustive bounded forward exploration (the ground truth) ------
// The explorer + the differential driver (run_fuzz_seed) sit beside the
// generator so the fuzz bin and the 100-seed sweep share ONE implementation
// (run_fuzz_seed is a pure function of its seed — no module state
// anywhere below).

use std::collections::VecDeque;

use indexmap::IndexSet;

use chorus::core::query::parse_query;
use chorus::core::types::AttrValue;
use chorus::reach::{Analyzer, CardOptions};

/// Cumulative exploration bookkeeping.
pub struct Forward {
    pub ever_names: IndexSet<String>,
    /// keyed by the `name|kind|canon(aspects)|canon(attrs)` descriptor string
    pub ever_desc: IndexMap<String, Entity>,
    pub states_seen: usize,
    /// pushes dropped because an equal-signature state was already queued —
    /// pure diagnostics (budget tuning)
    pub deduped: usize,
    pub capped: bool,
}

/// JSON text for one aspect leaf value (a number or a sub-map here).
/// LOCAL key only (sig / ever_desc keys): the sub-map arm joins `k:v` rather
/// than the canonical `k=` form, but sub-maps arise solely from Q13 dotted
/// aspect keys, which the scenario generator never mints — the equivalence
/// relation is unaffected on the reachable domain.
fn json_of_aspect(v: &AspectVal) -> String {
    match v {
        AspectVal::Num(n) => n.to_string(),
        AspectVal::Sub(m) => {
            let parts: Vec<String> = m.iter().map(|(k, v)| format!("{k}:{v}")).collect();
            format!("{{{}}}", parts.join(","))
        }
    }
}

/// Aspect pairs joined `{k=v,…}` with the keys in default-sort (=
/// UTF-16 code unit) order.
fn canon_pairs<'a>(pairs: impl Iterator<Item = (&'a String, &'a AspectVal)>) -> String {
    let mut keys: Vec<(&String, &AspectVal)> = pairs.collect();
    keys.sort_by(|a, b| chorus::core::text::cmp_utf16(a.0, b.0));
    let parts: Vec<String> = keys
        .into_iter()
        .map(|(k, v)| format!("{k}={}", json_of_aspect(v)))
        .collect();
    format!("{{{}}}", parts.join(","))
}

/// The attrs twin of `canon_pairs` (attr values are strings, numbers, or
/// bools).
fn canon_attrs<'a>(pairs: impl Iterator<Item = (&'a String, &'a AttrValue)>) -> String {
    let mut keys: Vec<(&String, &AttrValue)> = pairs.collect();
    keys.sort_by(|a, b| chorus::core::text::cmp_utf16(a.0, b.0));
    let parts: Vec<String> = keys
        .into_iter()
        .map(|(k, v)| {
            let json = match v {
                AttrValue::Str(s) => serde_json::to_string(s).expect("a string serializes"),
                AttrValue::Num(n) => n.to_string(),
                AttrValue::Bool(b) => b.to_string(),
            };
            format!("{k}={json}")
        })
        .collect();
    format!("{{{}}}", parts.join(","))
}

/// Drop `container` from signatures and descriptors (it is derivable from
/// relations).
fn scrubbed(e: &Entity) -> impl Iterator<Item = (&String, &AttrValue)> {
    e.attrs.iter().filter(|(k, _)| k.as_str() != "container")
}

/// The state signature: the SAME fields, the SAME default-sort discipline.
/// Array elements are pre-joined with ',' and sorted as strings (the
/// default string sort — the sort input is exactly this unquoted join, not
/// the quoted JSON form). The text itself is a LOCAL dedup key (its exact
/// bytes are never replayed from recorded runs); what must be stable is WHICH
/// states collide, i.e. the equivalence relation. Every interpolatable value
/// in the fuzz domain (fixed name/kind/aspect/attr pools, verbs, recipe ids,
/// integer fields) is comma/bracket-free, so both encodings separate the same
/// state pairs.
#[must_use]
pub fn sig(s: &State) -> String {
    let name_of = |id: &str| -> String {
        s.entities
            .values()
            .find(|e| e.id == id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| id.to_string())
    };
    let e: Vec<String> = s
        .entities
        .values()
        .filter(|e| e.dead_action.is_none())
        .map(|e| {
            [
                e.name.clone(),
                e.kind.clone(),
                canon_pairs(e.aspects.iter()),
                canon_attrs(scrubbed(e)),
            ]
            .join(",")
        })
        .collect();
    let mut e = e;
    e.sort_by(|a, b| chorus::core::text::cmp_utf16(a, b));
    // relations are ids — map through names like locked windows (a generated
    // scenario never RELATEs today, but a signature that ignores a State
    // field is a latent subtree loss)
    let r: Vec<String> = s
        .relations
        .values()
        .filter(|rel| rel.dead_action.is_none())
        .map(|rel| {
            [
                name_of(&rel.a),
                name_of(&rel.b),
                rel.type_.clone(),
                rel.value.to_string(),
            ]
            .join(",")
        })
        .collect();
    let mut r = r;
    r.sort_by(|a, b| chorus::core::text::cmp_utf16(a, b));
    let mut f: Vec<String> = s.fired.iter().cloned().collect();
    f.sort_by(|a, b| chorus::core::text::cmp_utf16(a, b));
    let mut d: Vec<String> = s.decks.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    d.sort_by(|a, b| chorus::core::text::cmp_utf16(a, b));
    // a window's completion applies ITS recipe's outcomes anchored to w.start
    // (SET expires!) over ITS role bindings — verb+end+locked alone would
    // collapse worlds whose completions mint different cards. beginId/clause
    // stay OUT: they are exploration-order artifacts, not world facts.
    let w: Vec<String> = s
        .windows
        .values()
        .map(|win| {
            let mut refs: Vec<String> = win
                .refs
                .iter()
                .flat_map(|(role, list)| {
                    list.iter().map(move |rref| {
                        let who = match rref {
                            Ref::Name { name, .. } => name.clone(),
                            Ref::Id { id } => name_of(id),
                            Ref::Param { name, .. } => format!("param:{name}"),
                        };
                        format!("{}:{}", role.as_str(), who)
                    })
                })
                .collect();
            refs.sort_by(|a, b| chorus::core::text::cmp_utf16(a, b));
            let mut locked: Vec<String> = win.locked.iter().map(|id| name_of(id)).collect();
            locked.sort_by(|a, b| chorus::core::text::cmp_utf16(a, b));
            [
                win.verb.clone(),
                win.start.to_string(),
                win.end.to_string(),
                win.recipe.clone(),
                refs.join(","),
                locked.join(","),
            ]
            .join(",")
        })
        .collect();
    let mut w = w;
    w.sort_by(|a, b| chorus::core::text::cmp_utf16(a, b));
    format!(
        "{{now={},g={},ended={},e=[{}],r=[{}],f=[{}],d=[{}],w=[{}]}}",
        s.now,
        s.rng,
        s.ended
            .as_ref()
            .map(|en| en.id.clone())
            .unwrap_or_else(|| "null".to_string()),
        e.join(","),
        r.join(","),
        f.join(","),
        d.join(","),
        w.join(","),
    )
}

/// Record every living descriptor once.
fn record(s: &State, ever_names: &mut IndexSet<String>, ever_desc: &mut IndexMap<String, Entity>) {
    for e in s.entities.values() {
        if e.dead_action.is_some() {
            continue;
        }
        ever_names.insert(e.name.clone());
        // attrs belong in the key: a later SET(state.sealed/expires) on the
        // same living card is a NEW demand-descriptor, not a re-sight of the
        // old one
        let key = format!(
            "{}|{}|{}|{}",
            e.name,
            e.kind,
            canon_pairs(e.aspects.iter()),
            canon_attrs(scrubbed(e))
        );
        if !ever_desc.contains_key(&key) {
            let mut scrubbed_entity = e.clone();
            scrubbed_entity.attrs = e
                .attrs
                .iter()
                .filter(|(k, _)| k.as_str() != "container")
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            ever_desc.insert(key, scrubbed_entity);
        }
    }
}

/// BFS over settle_step with push-time dedup.
pub fn explore(
    content: &CompiledContent,
    dict: &Dictionary,
    initial: &State,
    max_steps: usize,
    max_states: usize,
) -> Result<Forward> {
    let mut ever_names: IndexSet<String> = IndexSet::new();
    let mut ever_desc: IndexMap<String, Entity> = IndexMap::new();
    let mut visited: IndexSet<String> = IndexSet::new();
    let mut states_seen = 0usize;
    let mut deduped = 0usize;
    let mut capped = false;
    // i64: the settle_step first-record id domain
    let mut id_counter = 0i64;
    // dedupe at PUSH time: the old pop-time check counted every duplicate pop
    // against max_states AND let the queue grow with them — the budget bought
    // re-sights instead of new worlds. BFS pop order makes the first
    // (shallowest) push the keeper; equal-signature states record identical
    // descriptors.
    let mut queue: VecDeque<(State, usize)> = VecDeque::new();
    {
        let key = sig(initial);
        if visited.insert(key) {
            record(initial, &mut ever_names, &mut ever_desc);
            queue.push_back((initial.clone(), 0));
        } else {
            deduped += 1;
        }
    }
    while let Some((s, steps)) = queue.pop_front() {
        if states_seen >= max_states {
            capped = true;
            break;
        }
        states_seen += 1;
        if steps >= max_steps || s.ended.is_some() {
            continue;
        }
        for step in candidates(content, dict, &s) {
            // the first-record id reads the counter BEFORE the id generator
            // closure ever runs; the closure increments per minted id.
            let first = 1 + id_counter;
            let mut gen = || {
                let out = format!("f{id_counter}");
                id_counter += 1;
                out
            };
            // settle consumes the state; the rejected branch rides the
            // rollback clone back (the caller's `s` is left untouched)
            let res = settle_step(content, dict, s.clone(), step, first, Some(&mut gen))?;
            if res.rejected.is_some() {
                continue;
            }
            let key = sig(&res.state);
            if visited.contains(&key) {
                deduped += 1;
                continue;
            }
            visited.insert(key);
            record(&res.state, &mut ever_names, &mut ever_desc);
            queue.push_back((res.state, steps + 1));
        }
    }
    Ok(Forward {
        ever_names,
        ever_desc,
        states_seen,
        deduped,
        capped,
    })
}

// ---------- the differential property ----------

/// The outcome of one differential-fuzz seed.
pub struct FuzzOutcome {
    pub seed: u32,
    pub failures: Vec<String>,
    pub inconclusive: Vec<String>,
    pub states_seen: usize,
    pub deduped: usize,
    pub probes: usize,
}

/// The differential driver: a pure function of its seed.
pub fn run_fuzz_seed(seed: u32, max_steps: usize, max_states: usize) -> Result<FuzzOutcome> {
    let sc = gen_scenario(seed);
    let dict = core_dictionary();
    let content = compile_content(&sc.raw, &dict, &sc.entity_kinds)?;
    let fw = explore(&content, &dict, &sc.initial, max_steps, max_states)?;
    let mut failures: Vec<String> = Vec::new();
    let mut inconclusive: Vec<String> = Vec::new();
    // at most one deep exploration per seed — every failing probe would
    // otherwise re-run the identical (content, initial, bounds) search
    let mut deep: Option<Forward> = None;
    // one Analyzer across the probes — the facts for this (content, initial)
    // pair are built once, not once per probe
    let analyzer = Analyzer::new(&content, &sc.initial);
    for p in &sc.probes {
        let rep = match p.mode {
            ProbeMode::Name => analyzer.analyze_card(&p.q, CardOptions::default())?,
            ProbeMode::Demand => analyzer.analyze_card(
                "",
                CardOptions {
                    usable: false,
                    demand: Some(p.q.clone()),
                },
            )?,
        };
        // path-export invariants: a proven verdict MUST come with at least one
        // exported path, and (name mode) every exported root must mint the
        // probed name — the enumeration and the verdict must never disagree
        if rep.existence.as_str() == "proven" && rep.paths.is_empty() {
            failures.push(format!(
                "seed {seed}: '{}' proven obtainable but the path export is empty",
                p.q
            ));
        }
        if matches!(p.mode, ProbeMode::Name) {
            for root in &rep.paths {
                if root.mints != p.q {
                    failures.push(format!(
                        "seed {seed}: exported path mints '{}' but the probe was '{}'",
                        root.mints, p.q
                    ));
                }
            }
        }
        let truth = match p.mode {
            ProbeMode::Name => fw.ever_names.contains(&p.q),
            ProbeMode::Demand => {
                let q = parse_query(&p.q)?;
                fw.ever_desc.values().any(|e| match_query(e, &q))
            }
        };
        if rep.verdict.as_str() == "unobtainable" && truth {
            failures.push(format!(
                "seed {seed}: '{}' judged UNOBTAINABLE but appears in play (verdict {}/{})",
                p.q,
                rep.verdict.as_str(),
                rep.existence.as_str()
            ));
        }
        if rep.verdict.as_str() == "obtainable" && !truth {
            // second phase — a deeper exhaustive oracle. The analyzer's claims
            // are existential over UNBOUNDED play (repeatable verbs, recurring
            // triggers), so a shallow miss can be horizon, not error.
            if deep.is_none() {
                deep = Some(explore(
                    &content,
                    &dict,
                    &sc.initial,
                    max_steps + 7,
                    max_states * 20,
                )?);
            }
            let deep_fw = deep.as_ref().expect("the deep exploration was just seeded");
            let truth_deep = match p.mode {
                ProbeMode::Name => deep_fw.ever_names.contains(&p.q),
                ProbeMode::Demand => {
                    let q = parse_query(&p.q)?;
                    deep_fw.ever_desc.values().any(|e| match_query(e, &q))
                }
            };
            if truth_deep {
                // analyzer vindicated at depth — base horizon was the limit
            } else if deep_fw.capped {
                inconclusive.push(format!(
                    "seed {seed}: '{}' obtainable but the deep exploration hit its state cap",
                    p.q
                ));
            } else {
                failures.push(format!(
                    "seed {seed}: '{}' judged OBTAINABLE but never appears in the exhaustive {}-step exploration (chain: {})",
                    p.q,
                    max_steps + 7,
                    rep.chain.first().map(String::as_str).unwrap_or("?"),
                ));
            }
        }
    }
    Ok(FuzzOutcome {
        seed,
        failures,
        inconclusive,
        states_seen: fw.states_seen,
        deduped: fw.deduped,
        probes: sc.probes.len(),
    })
}

// ---------- the frozen caller discipline ----------

/// the outcome of one fuzz driver run — the frozen loop shape: K applied
/// settle_step calls or the attempt cap; a soft-locked world legitimately lands
/// short of K.
pub struct FuzzRun {
    /// a pristine clone of sc.initial — the fold-check base for the harness
    pub base: State,
    pub records: Vec<ActionRecord>,
    pub final_state: State,
    pub applied: usize,
    pub attempts: usize,
}

/// THE FROZEN CALLER DISCIPLINE: one selector
/// instance per seed seeded rng(seed); per attempt draw once over the full
/// candidate list; a rejected settle_step consumes the draw and retries; K=25
/// applied steps or the 400-attempt cap.
pub fn drive_fuzz_seed(seed: u32, k: usize, attempt_cap: usize) -> Result<FuzzRun> {
    let sc = gen_scenario(seed);
    let dict = core_dictionary();
    let content = compile_content(&sc.raw, &dict, &sc.entity_kinds)?;
    let mut sel = rng(seed); // the selector LCG — fresh per seed
    let base = sc.initial.clone();
    let mut state = sc.initial;
    let mut records: Vec<ActionRecord> = Vec::new();
    let mut id: i64 = 1;
    let mut applied = 0usize;
    let mut attempts = 0usize;
    while applied < k && attempts < attempt_cap {
        attempts += 1;
        let steps = candidates(&content, &dict, &state);
        let step = choose_step(&mut sel, &steps).clone();
        // settle consumes the state; the rejected branch rides the rollback
        // clone back (the rejected attempt leaves `state` untouched)
        let res = settle_step(&content, &dict, state, step, id, None)?;
        state = res.state;
        if res.rejected.is_some() {
            continue;
        }
        let n = res.records.len() as i64;
        records.extend(res.records);
        id += n;
        applied += 1;
    }
    Ok(FuzzRun {
        base,
        records,
        final_state: state,
        applied,
        attempts,
    })
}
