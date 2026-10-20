//! Differential-fuzz machinery: gen_scenario + the
//! LCG + the frozen action selector. The selector (`choose_step`) and its
//! caller discipline (`drive_fuzz_seed`) remain the frozen surface: they
//! must stay bit-identical draw for draw.
//!
//! `gen_scenario`'s draw order is deliberate (every short-circuit and
//! evaluation-order choice carries an inline draw-order note) but VERSIONED,
//! not frozen: the v2 scenario space added completion triggers, RUN
//! candidates and recipe `aspects` bands, re-banding several thresholds and
//! reordering the trigger-kind roll ahead of the body draws; v3 added the
//! two aspect-scoped world forms to the `when` candidate table; v4 added a
//! closed (strict `<`) aspect-count form to the same table; v5 widened the
//! recipe-body chain-candidate table from the same verb's built prefix to
//! the GLOBAL build prefix (cross-verb chains, §6.7); v6 split the chain
//! band into RUN [0.22, 0.27) and FORK [0.27, 0.32) (the FORK fan-out, §6.7);
//! v7 turned the chain band ON in slot/expire trigger bodies (the §6.6
//! event-chain channel); v8 added the recipe-line `dur` band (NEW draw,
//! p=0.15, one ladder literal, after the aspects gate); v9 added the §6.3
//! sum scopes to the `when` table (`total(bound, aspect:a) >= n`,
//! `total(table, aspect:b) >= n`); v10 added the §6.9 ambient draw
//! (p=0.15, FIRST draw of the verb block: an ambient verb declares no
//! slots, draws its duration from the nonzero pool only, short-circuits
//! the recipe `dur` band and the REFUSE band off); v11 added the §4.2
//! `unique` draw to the prototype loop (p=0.15, AFTER the stackable draw,
//! one roll per prototype always — the roll happens even when `stackable`
//! already landed, and wins only on a non-stackable prototype: the grammar
//! rejects `unique` + `stackable` on one line); v12 added the §6.1 held
//! draw to the verb block (p=0.15, AFTER the non-actor slots: a range from
//! {0..1, 0..2, 0..3}, a query from the aspect pool, the greedy/consuming
//! flags drawn after — the held verb defers selection and the campaign's
//! step enumeration gained HOLD steps, `all_candidates`). Every recorded
//! fuzz run taken before v12 is therefore NOT reproducible
//! seed-for-seed; reproduce against the matching generator version. What
//! stays pinned is that the generator is a pure function of its seed — the
//! same seed builds the same scenario, draw for draw, under ONE version.
//!
//! (Shared test lib discipline: the `allow` lives in this one place.)
#![allow(dead_code)]

use indexmap::IndexMap;

use crate::content::{
    compile_content, CompiledContent, CompiledSlot, CompleteTriggerDef, ContentRaw, DeckRaw,
    DeclaredEntity, ExpireTriggerDef, PrototypeDef, PrototypeDerive, RecipeDef, SlotDef,
    SlotTriggerDef, TriggerDef, VerbContent,
};
use crate::core::clock::{
    abs_min, clock_from_abs_min, format_clock, next_step_clock, parse_duration, MIN_PER_TICK,
};
use crate::core::dict::{core_dictionary, Dictionary, CORE_VERBS};
use crate::core::parse::parse_sentence;
use crate::core::query::match_query;
use crate::core::r#ref::{id_ref, name_ref, Ref};
use crate::core::sentence::{empty_clause, Clause};
use crate::core::types::{add_entity, new_state, AspectVal, Entity, State};
#[allow(unused_imports)]
// ActionRecord only flows through drive_fuzz_seed (unused by the fuzz bin)
use crate::engine::{execute_hold, ActionRecord, Step};
use crate::error::Result;
use crate::lcg::Lcg;

use crate::engine::settle::settle_step;

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
/// The §6.4 recipe `dur` ladder (v8): instant (0m) plus the two sub-halfhour
/// ticks — small windows a verb's own `duration:` pool (0/30/60) never
/// produces.
const DUR_LADDER: [&str; 3] = ["0m", "10m", "20m"];

fn str_vec(vs: &[&str]) -> Vec<String> {
    vs.iter().map(|s| (*s).to_string()).collect()
}

/// Script-style verb lexemes the generator declares beside the core set:
/// sampled from the alphabet exactly like a core verb, authored by
/// the same verb loop, and registered into the scenario's dictionary with
/// the loader's register-if-unknown discipline.
const SCRIPT_VERBS: [&str; 3] = ["INTERROGATE", "COMMUNE", "DISTILL"];

/// The generated-action alphabet: CORE_VERBS ∪ the declarable script verbs
/// (SCRIPT_VERBS), minus WAIT as today. v0.7.6 widened the pool from the
/// bare core twelve; the draw order stays frozen — same seed, same scenario,
/// draw for draw — only the pool contents grew.
fn action_alphabet() -> Vec<String> {
    CORE_VERBS
        .iter()
        .chain(SCRIPT_VERBS.iter())
        .filter(|v| **v != "WAIT")
        .map(|v| (*v).to_string())
        .collect()
}

/// KIND_POOL minus place.
fn non_char_kinds() -> Vec<String> {
    KIND_POOL.iter().filter(|k| **k != "place").map(|k| (*k).to_string()).collect()
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
/// state, and the probe list. The dictionary is the scenario's OWN
/// load-effective dictionary — the core seed plus every script verb the
/// sample declared (§4.3) — so every consumer compiles, explores and
/// folds against the vocabulary the content actually uses.
pub struct Scenario {
    pub seed: i64,
    pub raw: ContentRaw,
    pub dict: Dictionary,
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
        aspects: aspects.into_iter().map(|(k, v)| (k, AspectVal::Num(v))).collect(),
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
        desc: None,
    }
}

/// The scenario's mutable build context — the rng handle plus the local
/// tables every generator step draws against, threaded explicitly.
/// `run_targets` is the §6.7 chain-candidate table for the op currently
/// being built: inside the recipe loop it holds the recipes built SO FAR
/// across ALL verbs (the growing build-order prefix — a recipe-body RUN
/// chain lowers the build index every hop, so it is acyclic by
/// construction), and for trigger bodies the global duration>0 table (see
/// the trigger loop).
struct Gen<'a> {
    r: &'a mut Lcg,
    others: Vec<String>,
    proto_names: Vec<String>,
    has_house: bool,
    has_deck: bool,
    run_targets: Vec<String>,
}

impl Gen<'_> {
    /// One draw in 0..4.
    fn aspect_val(&mut self) -> i64 {
        int(self.r, 4) as i64
    }

    /// The outcome-op generator (v2 bands, re-banded since). The chain
    /// band's insertion re-banded every later threshold — the widths below,
    /// with the funding called out; the pre-v2 recorded streams do not line
    /// up against this table, and the v6 split re-versioned the streams
    /// again:
    ///
    /// | op      | v6 band          | v2–v5 band       |
    /// |---------|------------------|------------------|
    /// | SPAWN   | [0.00, 0.22)     | [0.00, 0.22)     |
    /// | RUN     | [0.22, 0.27)     | [0.22, 0.32)     |
    /// | FORK    | [0.27, 0.32) NEW | —                |
    /// | REPLACE | [0.32, 0.42)     | [0.32, 0.42)     |
    /// | DRAW    | [0.42, 0.52)     | [0.42, 0.52)     |
    /// | SET     | [0.52, 0.64)     | [0.52, 0.64)     |
    /// | DESTROY | [0.64, 0.70)     | [0.64, 0.70)     |
    /// | MOVE    | [0.70, 0.78)     | [0.70, 0.78)     |
    /// | IF      | [0.78, 0.86)     | [0.78, 0.86)     |
    /// | ROLL    | [0.86, 0.94)     | [0.86, 0.94)     |
    /// | REFUSE  | [0.94, 0.98)     | [0.94, 0.98)     |
    /// | NARRATE | [0.98, 1.00]     | [0.98, 1.00]     |
    ///
    /// The 0.10 chain band funds both chain ops (RUN 0.05 + FORK 0.05;
    /// every other op keeps its exact v2 width). `allow_chain` gates both
    /// bands: verb recipes and completion-trigger bodies are the chain-legal
    /// homes — a slot/expire body's chain op is a load error (§6.7,
    /// compile.rs's event guard) — and `allow_refuse` keeps doing duty for
    /// the REFUSE band. Every branch guard's draw happens EXACTLY where the
    /// guard evaluates it (short-circuits included) — the draw order is
    /// deliberate and the generator is a pure function of its seed.
    fn op(&mut self, allow_refuse: bool, allow_chain: bool) -> String {
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
        // RUN (§6.7): the candidates live in run_targets — the recipes built
        // so far across ALL verbs inside the recipe loop, the global
        // duration>0 table for trigger bodies. Falls through DRAW-FREE when
        // the home is not chain-legal or no candidate exists yet (the
        // empty-table check is the (free) right operand of the &&).
        if roll < 0.27 && allow_chain && !self.run_targets.is_empty() {
            let target = pick_of(self.r, &self.run_targets);
            return format!("RUN \"{target}\"");
        }
        // FORK (§6.7): the fan-out — the same target tables, the same
        // legality gate, one band over.
        if roll < 0.32 && allow_chain && !self.run_targets.is_empty() {
            let target = pick_of(self.r, &self.run_targets);
            return format!("FORK \"{target}\"");
        }
        if roll < 0.42 && !self.others.is_empty() {
            let a = pick_of(self.r, &self.others);
            let b = pick_of(self.r, &self.proto_names);
            return format!("REPLACE {a} WITH {b}");
        }
        // the deck check draws nothing; a deckless world
        // falls through to the SET branch
        if roll < 0.52 && self.has_deck {
            return "DRAW D1".to_string();
        }
        if roll < 0.64 {
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
                pick_str(self.r, &["30m", "1h"])
            } else if self.r.next_f64() < 0.6 {
                format!("+{}", 1 + int(self.r, 2) as i64)
            } else {
                int(self.r, 4).to_string()
            };
            return format!("SET({tgt}, {key}, {val})");
        }
        if roll < 0.70 && !self.others.is_empty() {
            let t = pick_of(self.r, &self.others);
            return format!("DESTROY {t}");
        }
        if roll < 0.78 && self.has_house {
            return "MOVE ANA INTO HOUSE".to_string();
        }
        if roll < 0.86 {
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
        if roll < 0.94 {
            let dice = if self.r.next_f64() < 0.3 { 2 } else { 1 };
            // the template draws left to right — the bound, then the mint
            let bound = 2 + int(self.r, 4 * dice) as i64;
            let p = pick_of(self.r, &self.proto_names);
            return format!(
                "ROLL({dice}, 6), IF roll >= {bound} -> (SPAWN {p}) ELSE -> (NARRATE \"low\")"
            );
        }
        if roll < 0.98 && allow_refuse {
            return "REFUSE(\"no\")".to_string();
        }
        "NARRATE \"beat\"".to_string()
    }

    /// The `when` candidates. All nine are built EAGERLY — every template
    /// interpolation draws BEFORE the final pick selects one:
    /// t1 one int, t2 one pool pick, t3 none, t4 one proto-name pick,
    /// ta one int, tb one int, tc one int, td one int, te one int.
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
        // the aspect-scoped world forms (v3): an exists over the aspect
        // pool and a pool-count form — the analyzer's world-static gates
        // under the exact match and the unbounded-pool Unknown
        let ta = format!("exists(aspect:a>={})", 1 + int(self.r, 2) as i64);
        let tb = format!("count(aspect:b>=1) >= {}", 1 + int(self.r, 2) as i64);
        // the closed face (v4): a strict comparison the pool's floor can
        // already violate — over an unbounded pool the analyzer must
        // decide it False (never may-select Unknown), and this form makes
        // that face visible to the differential campaign
        let tc = format!("count(aspect:b>=1) < {}", 1 + int(self.r, 3) as i64);
        // the predicate sum scopes (v9, §6.3): the window-bound sum over
        // the step's bindings and the world-table sum over every living
        // card — the analyzer's window-model and world-sum intervals under
        // the differential campaign
        let td = format!("total(bound, aspect:a) >= {}", 1 + int(self.r, 3) as i64);
        let te = format!("total(table, aspect:b) >= {}", 1 + int(self.r, 2) as i64);
        let choices: [&str; 9] = [&t1, &t2, &t3, &t4, &ta, &tb, &tc, &td, &te];
        pick_str(self.r, &choices)
    }
}

/// The scenario generator — the frozen draw order described in the module
/// header.
pub fn gen_scenario(seed: u32) -> Scenario {
    let mut r = rng(seed);
    let alphabet = action_alphabet();
    // the count draw happens BEFORE sample_distinct's internal draws
    let n_verbs = 2 + int(&mut r, 3); // 2–4
    let verbs = sample_distinct(&mut r, &alphabet, n_verbs);
    // the scenario's dictionary: the core seed plus every non-core lexeme
    // the sample declared — register-if-unknown, the script loader's
    // discipline (§4.3), so a sampled core verb skips and a declared one
    // joins both verbs and reserved
    let mut dict = core_dictionary();
    for v in verbs.iter().filter(|v| SCRIPT_VERBS.contains(&v.as_str())) {
        // SCRIPT_VERBS are UPPER_SNAKE literals naming no reserved word —
        // registration cannot fail; a failure would be a defect in the
        // literal table (the core_dictionary seed's own argument)
        if let Err(e) = dict.register_verb(v) {
            unreachable!("scenario verb '{v}' failed to register: {e}");
        }
    }

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
    let proto_names: Vec<String> =
        ["P1", "P2", "P3"][..n_protos].iter().map(|s| (*s).to_string()).collect();
    let mut prototypes: IndexMap<String, PrototypeDef> = IndexMap::new();
    let mut derive_done = false;
    for pn in &proto_names {
        let kind = pick_of(&mut r, &non_char_kinds());
        let mut p = PrototypeDef { kind, ..PrototypeDef::default() };
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
        // §4.2 (v11 NEW draw, p=0.15, AFTER the stackable draw): the
        // prototype goes unique — one roll per prototype ALWAYS (the draw
        // order stays draw-for-draw stable), landing only on a non-stackable
        // prototype (`unique` + `stackable` on one line is a load error).
        // Unique prototypes feed the deck spec pool like any other name, so
        // unique entries ride the deck bands for free.
        if r.next_f64() < 0.15 && !p.stackable {
            p.unique = true;
        }
        // short-circuit: no draw once derive is done
        if !derive_done && r.next_f64() < 0.25 {
            p.derive =
                Some(PrototypeDerive { name_from: Some("object".to_string()), aspects: None });
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
        let defaultcard =
            if r.next_f64() < 0.4 { Some(pick_of(&mut r, &deck_names)) } else { None };
        let reset = r.next_f64() < 0.3;
        Some(DeckRaw { id: "D1".to_string(), spec, defaultcard, reset })
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
        run_targets: Vec::new(),
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
                greedy: false,
                consuming: false,
            }],
            duration: 30,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                dur: None,
                out: vec![],
            }],
            ambient: false,
        },
    );
    // the global §6.7 trigger-side chain-candidate table — every recipe id
    // of the duration>0 verbs, collected as the build loop finishes each verb
    let mut global_run_targets: Vec<String> = Vec::new();
    // §6.7 recipe-body chain candidates: the recipes built SO FAR across ALL
    // verbs — the growing prefix of the build loop, threaded exactly like a
    // per-verb prefix (strictly earlier in build order, so a recipe-body RUN
    // chain lowers the build index every hop and cannot cycle)
    let mut recipe_prefix: Vec<String> = Vec::new();
    for v in &verbs {
        // §6.9 (v10 NEW draw, p=0.15, FIRST draw of the verb block): the verb
        // goes ambient — it declares NO slots (the slot draws below skip
        // entirely, a documented short-circuit) and its duration draws from
        // the nonzero pool only: an ambient recipe with effective duration 0
        // self-chains inside one advance pass (open → complete → the
        // §6.9 check re-opens) and trips the engine's §6.7 livelock
        // budget, a hard ChorusError that would abort the oracle run instead
        // of reporting a differential — the same filter discipline as the
        // trigger-side duration>0 table. The §6.4 recipe `dur` band below
        // short-circuits OFF for ambient verbs for the same reason (the
        // ladder's `0m` literal is lethal there).
        let ambient = ctx.r.next_f64() < 0.15;
        let mut slots: Vec<SlotDef> = Vec::new();
        if !ambient {
            slots.push(SlotDef {
                role: "actors".to_string(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".to_string()),
                shown: None,
                greedy: false,
                consuming: false,
            });
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
                    greedy: false,
                    consuming: false,
                });
            }
            // §6.1 (v12 NEW draw, p=0.15, AFTER the non-actor slots): the
            // verb declares a HELD slot — the §6.4 deferral rides it. Range
            // from {0..1, 0..2, 0..3} (the drawn max), query from the aspect
            // pool (a threshold the initial entities — aspect a 0..3, b
            // 0..2 — can meet), then the two flags, greedy then consuming,
            // each p=0.25 — both land only on this held role (the load
            // rejects them on any other), and the greedy requirements (a
            // query, a finite max) hold by construction.
            if ctx.r.next_f64() < 0.15 {
                let max = 1 + int(ctx.r, 3) as i64;
                let a = pick_str(ctx.r, &ASPECTS);
                let bound = 1 + int(ctx.r, 2) as i64;
                let greedy = ctx.r.next_f64() < 0.25;
                let consuming = ctx.r.next_f64() < 0.25;
                slots.push(SlotDef {
                    role: "held".to_string(),
                    min: 0,
                    max: Some(max),
                    query: Some(format!("aspect:{a}>={bound}")),
                    shown: None,
                    greedy,
                    consuming,
                });
            }
        }
        let n_recipes = 1 + int(ctx.r, 3);
        let mut recipes: Vec<RecipeDef> = Vec::new();
        for i in 0..n_recipes {
            let when = if ctx.r.next_f64() < 0.4 { Some(ctx.gen_when()) } else { None };
            // §6.4 recipe-line `aspects` presence list (NEW draw, p=0.15,
            // single name) — the emitter gate the §6.7 `on complete
            // <aspect>` trigger reads on the completed recipe. Draw order:
            // when gate → aspects gate → dur band → n_outs.
            let aspects =
                if ctx.r.next_f64() < 0.15 { vec![pick_str(ctx.r, &ASPECTS)] } else { Vec::new() };
            // §6.4 recipe-line `dur` override (v8 NEW draw, p=0.15, one
            // ladder literal) — a minority of recipes complete under a
            // window of their own, in place of the verb's `duration:`:
            // `0m` (an instant window on a slow verb), `10m`, `20m`. Draw
            // order: when gate → aspects gate → dur band → n_outs.
            // §6.9: the band short-circuits OFF for an ambient verb — a
            // zero-duration ambient recipe self-chains into the livelock
            // budget (see the ambient draw above).
            let dur = if !ambient && ctx.r.next_f64() < 0.15 {
                Some(
                    parse_duration(&pick_str(ctx.r, &DUR_LADDER))
                        .unwrap_or_else(|e| panic!("dur literal: {e}")),
                )
            } else {
                None
            };
            // §6.7 chain candidates: the recipes built SO FAR across ALL
            // verbs — strictly earlier in build order only, so a recipe-body
            // RUN chain lowers the build index every hop and cannot cycle
            // (the very first recipe sees an empty table and cannot carry
            // RUN). This threading is the minimal-perturbation option: the
            // chain band draws its target from the prefix the build loop
            // already produced, with no post-hoc pass over the finished
            // verbs and no extra draws outside op's own band.
            ctx.run_targets = recipe_prefix.clone();
            let n_outs = 1 + int(ctx.r, 4);
            let mut outs: Vec<String> = Vec::new();
            for _ in 0..n_outs {
                // §6.9: an ambient recipe cannot REFUSE at the top level
                // (a load error — the gate is `when`), so the REFUSE band
                // is off for one; the chain band stays ON (RUN/FORK is the
                // self-chaining mechanism the Time-loop shape is built from).
                outs.push(ctx.op(!ambient, true));
            }
            recipes.push(RecipeDef {
                id: Some(format!("{}.r{}", v.to_lowercase(), i)),
                when,
                shown: None,
                hint: None,
                max: None,
                aspects,
                dur,
                out: outs,
            });
            recipe_prefix.push(format!("{}.r{}", v.to_lowercase(), i));
        }
        let duration: i64 = if ambient {
            pick_str(ctx.r, &["30", "60"])
        } else {
            pick_str(ctx.r, &["0", "30", "60"])
        }
        .parse()
        .unwrap_or_else(|e| panic!("duration literal: {e}"));
        // §6.7/§6.6 trigger-side chain candidates: only recipes whose
        // EFFECTIVE duration is > 0 join the global table — the recipe's
        // own `dur` overrides the verb's `duration:` (§6.4), so a `dur 0m`
        // recipe of a duration-30 verb is a zero-duration window exactly
        // like a whole zero-duration verb. A trigger chain onto a
        // zero-effective-duration recipe whose windows lock the trigger's
        // home card
        // re-fires the gate forever INSIDE one advance pass and trips the
        // engine's §6.7 livelock budget (a hard ChorusError, which would
        // abort the oracle run instead of reporting a differential) — the
        // effective-duration>0 filter bounds every trigger-fed chain to one
        // hop per advance, far under the 1024-completion budget.
        global_run_targets.extend(
            recipes.iter().filter(|r| r.dur.unwrap_or(duration) > 0).filter_map(|r| r.id.clone()),
        );
        verbs_raw.insert(v.clone(), VerbContent { slots, duration, recipes, ambient });
    }
    // from here the chain-legal bodies are the completion triggers; the table
    // switches to the global duration>0 candidates (the §6.7 global recipe
    // resolution)
    ctx.run_targets = global_run_targets;

    // triggers (v2: the KIND roll now precedes the body draws — it decides
    // the body's RUN legality and the complete form's home/aspect draws;
    // v1 drew the body first and banded kind as <0.25 expire / else slot).
    // Kind bands: Complete [0.00, 0.25) NEW · Expire [0.25, 0.45) (was
    // [0.00, 0.25)) · Slot [0.45, 1.00].
    let n_triggers = 1 + int(ctx.r, 3);
    let mut triggers: Vec<TriggerDef> = Vec::new();
    for i in 0..n_triggers {
        let kind = ctx.r.next_f64();
        let id = format!("t{i}");
        if kind < 0.25 {
            // §6.7 completion trigger (`on complete [<aspect>]`): home is a
            // pool/proto NAME — the gate matches a locked card's CURRENT
            // name, which is how a prototype's trigger reaches its mints.
            // The aspect gate draws 0.5 (draw order: home pick → aspect
            // gate → aspect pick → n_out → ops); the body is drawn
            // chain-legal (RUN and FORK), REFUSE stays banned (§6.6/§6.7).
            let mut pool = ctx.others.clone();
            pool.extend(ctx.proto_names.iter().cloned());
            let home = pick_of(ctx.r, &pool);
            let aspect =
                if ctx.r.next_f64() < 0.5 { Some(pick_str(ctx.r, &ASPECTS)) } else { None };
            let n_out = 1 + int(ctx.r, 2);
            let mut out: Vec<String> = Vec::new();
            for _ in 0..n_out {
                out.push(ctx.op(false, true));
            }
            triggers.push(TriggerDef::Complete(CompleteTriggerDef { id, home, aspect, out }));
        } else {
            let n_out = 1 + int(ctx.r, 2);
            let mut out: Vec<String> = Vec::new();
            for _ in 0..n_out {
                // events cannot REFUSE (§6.7); the chain band is ON since
                // v7 (the §6.6 event-chain channel — the global duration>0
                // table)
                out.push(ctx.op(false, true));
            }
            if kind < 0.45 {
                let mut pool = vec!["ANA".to_string()];
                pool.extend(ctx.others.iter().cloned());
                let target = pick_of(ctx.r, &pool);
                triggers.push(TriggerDef::Expire(ExpireTriggerDef {
                    id,
                    on_expire_of: target,
                    out,
                }));
            } else {
                let slot = int(ctx.r, 144) as i64;
                let once = ctx.r.next_f64() < 0.4;
                triggers.push(TriggerDef::Slot(SlotTriggerDef { id, slot, once: Some(once), out }));
            }
        }
    }

    let raw_seed = int(ctx.r, 1000) as i64;
    drop(ctx); // release the &mut r borrow for the probe draws below

    let mut initial = new_state(0);
    for e in entities.iter().cloned() {
        add_entity(&mut initial, e)
            .unwrap_or_else(|e| panic!("gen_scenario entities never collide (unique names): {e}"));
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
        probes.push(Probe { mode: ProbeMode::Name, q: name });
    }
    // first derive-bearing prototype, in insertion order
    let derive_proto = proto_names
        .iter()
        .find(|pn| prototypes.get(*pn).is_some_and(|p| p.derive.is_some()))
        .cloned();
    if let Some(dp) = derive_proto {
        for n in &others {
            probes.push(Probe { mode: ProbeMode::Name, q: format!("{dp}_{n}") });
        }
    }
    probes.push(Probe { mode: ProbeMode::Name, q: "GHOST".to_string() });
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
        triggers: if triggers.is_empty() { None } else { Some(triggers) },
        ..ContentRaw::default()
    };

    Scenario {
        seed: seed as i64,
        raw,
        dict,
        initial,
        entity_kinds: entities
            .iter()
            .map(|e| DeclaredEntity {
                name: e.name.clone(),
                kind: e.kind.clone(),
                stackable: e.stackable,
                desc: None,
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

/// One enumerated candidate of the differential campaign (v12): a committed
/// step, or a `hold <card>` against an open deferred held-capable station —
/// the campaign's step enumeration gained the holds (§6.1).
#[derive(Clone)]
pub enum Candidate {
    Step(Step),
    Hold { verb: String, held: Ref },
}

/// the same one-uniform-draw shape as [`choose_step`], over the widened v12
/// list.
#[must_use]
pub fn choose_candidate<'a>(r: &mut Lcg, cands: &'a [Candidate]) -> &'a Candidate {
    let i = r.next_int(cands.len());
    &cands[i]
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
    let alive: Vec<&Entity> = s.entities.values().filter(|e| e.dead_action.is_none()).collect();
    let clock = next_step_clock(s.now);
    // waits — advance the clock so slot triggers, expiries and completions
    // strike
    for ahead in [0usize, 1, 3, 6, 12, 36, 72, 144] {
        let begin = if ahead == 0 {
            clock
        } else {
            clock_from_abs_min(abs_min(clock) + ahead as i64 * MIN_PER_TICK)
        };
        let parsed = parse_sentence(&format!("{} @ ANA WAIT", format_clock(begin)), dict)
            .unwrap_or_else(|e| panic!("the WAIT probe parses: {e}"));
        out.push(Step {
            start_min: None,
            clock: Some(parsed.clock.unwrap_or(begin)),
            clauses: parsed.clauses,
            t_real: None,
        });
    }
    let wait_clause = parse_sentence(&format!("{} @ ANA WAIT", format_clock(clock)), dict)
        .unwrap_or_else(|e| panic!("the WAIT probe parses: {e}"))
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
            per_role
                .push((slot.role.clone(), if opts.is_empty() { vec![Vec::new()] } else { opts }));
        }
        if !viable {
            continue;
        }
        // cross-role identity: one entity cannot fill two roles of a clause
        let ref_key = |r: &Ref| -> String {
            match r {
                Ref::Id { id } => id.clone(),
                Ref::Name { name, .. } => id_of(name),
                Ref::Param { .. } => unreachable!("param refs never reach candidates (§10)"),
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
        out.push(Step { start_min: None, clock: Some(clock), clauses, t_real: None });
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

/// The per-window cap on enumerated holds (v12): the explorer stays finite
/// — at most this many fitting cards per open deferred station, in birth
/// order.
const HOLD_CANDIDATE_CAP: i64 = 4;

/// The v12 candidate space: [`candidates`] plus the HOLD steps — per open
/// DEFERRED held-capable window (the empty recipe entry; a pinned window
/// refuses holds, §6.7) whose held slot is under its declared max, one hold
/// per fitting card. The pull/hold validity the engine runs filters the
/// rest: a candidate the engine rejects is skipped by the driver, exactly
/// like a rejected step.
pub fn all_candidates(content: &CompiledContent, dict: &Dictionary, s: &State) -> Vec<Candidate> {
    let mut out: Vec<Candidate> =
        candidates(content, dict, s).into_iter().map(Candidate::Step).collect();
    for w in s.windows.values() {
        if !w.recipe.is_empty() {
            continue;
        }
        let Some(slot) = content.verbs.get(&w.verb).and_then(|v| v.slot_by_role.get("held")) else {
            continue;
        };
        let taken = w.refs.get(&crate::core::roles::Role::Held).map_or(0, Vec::len) as i64;
        let room = match slot.max {
            None => HOLD_CANDIDATE_CAP,
            Some(max) => (max - taken).min(HOLD_CANDIDATE_CAP),
        };
        let mut n: i64 = 0;
        for e in s.entities.values() {
            if n >= room {
                break;
            }
            if e.dead_action.is_some() {
                continue;
            }
            if let Some(q) = &slot.query {
                if !match_query(e, q) {
                    continue;
                }
            }
            // the §6.7 busy rule: a lock held by THIS window is the
            // cross-role case; any other window's lock skips the card
            let busy = s
                .windows
                .iter()
                .any(|(k, w2)| k != &w.key && w2.locked.iter().any(|id| id == &e.id));
            if busy {
                continue;
            }
            n += 1;
            out.push(Candidate::Hold { verb: w.verb.clone(), held: id_ref(e.id.clone()) });
        }
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

use crate::core::query::parse_query;
use crate::core::types::AttrValue;
use crate::reach::{Analyzer, CardOptions};

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
    keys.sort_by(|a, b| crate::core::text::cmp_utf16(a.0, b.0));
    let parts: Vec<String> =
        keys.into_iter().map(|(k, v)| format!("{k}={}", json_of_aspect(v))).collect();
    format!("{{{}}}", parts.join(","))
}

/// The attrs twin of `canon_pairs` (attr values are strings, numbers, or
/// bools).
fn canon_attrs<'a>(pairs: impl Iterator<Item = (&'a String, &'a AttrValue)>) -> String {
    let mut keys: Vec<(&String, &AttrValue)> = pairs.collect();
    keys.sort_by(|a, b| crate::core::text::cmp_utf16(a.0, b.0));
    let parts: Vec<String> =
        keys.into_iter()
            .map(|(k, v)| {
                let json = match v {
                    AttrValue::Str(s) => serde_json::to_string(s)
                        .unwrap_or_else(|e| panic!("a string serializes: {e}")),
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
    e.sort_by(|a, b| crate::core::text::cmp_utf16(a, b));
    // relations are ids — map through names like locked windows (a generated
    // scenario never RELATEs today, but a signature that ignores a State
    // field is a latent subtree loss)
    let r: Vec<String> = s
        .relations
        .values()
        .filter(|rel| rel.dead_action.is_none())
        .map(|rel| {
            [name_of(&rel.a), name_of(&rel.b), rel.type_.clone(), rel.value.to_string()].join(",")
        })
        .collect();
    let mut r = r;
    r.sort_by(|a, b| crate::core::text::cmp_utf16(a, b));
    let mut f: Vec<String> = s.fired.iter().cloned().collect();
    f.sort_by(|a, b| crate::core::text::cmp_utf16(a, b));
    let mut d: Vec<String> = s.decks.iter().map(|(k, v)| format!("{k}:{v}")).collect();
    d.sort_by(|a, b| crate::core::text::cmp_utf16(a, b));
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
            refs.sort_by(|a, b| crate::core::text::cmp_utf16(a, b));
            let mut locked: Vec<String> = win.locked.iter().map(|id| name_of(id)).collect();
            locked.sort_by(|a, b| crate::core::text::cmp_utf16(a, b));
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
    w.sort_by(|a, b| crate::core::text::cmp_utf16(a, b));
    format!(
        "{{now={},g={},ended={},e=[{}],r=[{}],f=[{}],d=[{}],w=[{}]}}",
        s.now,
        s.rng,
        s.ended.as_ref().map(|en| en.id.clone()).unwrap_or_else(|| "null".to_string()),
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
    // dedupe at PUSH time — the max_states budget buys new worlds, never
    // re-sights. BFS pop order makes the first (shallowest) push the
    // keeper; equal-signature states record identical descriptors.
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
        for cand in all_candidates(content, dict, &s) {
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
            let res = match cand {
                Candidate::Step(step) => {
                    settle_step(content, dict, s.clone(), step, first, Some(&mut gen))?
                }
                Candidate::Hold { verb, held } => {
                    execute_hold(content, dict, s.clone(), &verb, held, first)?
                }
            };
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
    Ok(Forward { ever_names, ever_desc, states_seen, deduped, capped })
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
    // the scenario's own load-effective dictionary (field move — the other
    // `sc` fields stay in use below)
    let dict = sc.dict;
    let content = compile_content(&sc.raw, &dict, &sc.entity_kinds)?;
    let fw = explore(&content, &dict, &sc.initial, max_steps, max_states)?;
    let mut failures: Vec<String> = Vec::new();
    let mut inconclusive: Vec<String> = Vec::new();
    // at most one deep exploration per seed — every failing probe would
    // otherwise re-run the identical (content, initial, bounds) search
    let mut deep: Option<Forward> = None;
    // one Analyzer across the probes — the facts for this (content, initial)
    // pair are built once, not once per probe
    let analyzer = Analyzer::new(&content, &sc.initial, &dict);
    for p in &sc.probes {
        let rep = match p.mode {
            ProbeMode::Name => analyzer.analyze_card(&p.q, CardOptions::default())?,
            ProbeMode::Demand => analyzer
                .analyze_card("", CardOptions { usable: false, demand: Some(p.q.clone()) })?,
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
                let q = parse_query(&p.q, &dict)?;
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
                deep = Some(explore(&content, &dict, &sc.initial, max_steps + 7, max_states * 20)?);
            }
            let deep_fw = deep
                .as_ref()
                .unwrap_or_else(|| unreachable!("the deep exploration was just seeded"));
            let truth_deep = match p.mode {
                ProbeMode::Name => deep_fw.ever_names.contains(&p.q),
                ProbeMode::Demand => {
                    let q = parse_query(&p.q, &dict)?;
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
    // the scenario's own load-effective dictionary (field move — the other
    // `sc` fields stay in use below)
    let dict = sc.dict;
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
        let cands = all_candidates(&content, &dict, &state);
        let cand = choose_candidate(&mut sel, &cands).clone();
        // settle consumes the state; the rejected branch rides the rollback
        // clone back (the rejected attempt leaves `state` untouched)
        let res = match cand {
            Candidate::Step(step) => settle_step(&content, &dict, state, step, id, None)?,
            Candidate::Hold { verb, held } => {
                execute_hold(&content, &dict, state, &verb, held, id)?
            }
        };
        state = res.state;
        if res.rejected.is_some() {
            continue;
        }
        let n = res.records.len() as i64;
        records.extend(res.records);
        id += n;
        applied += 1;
    }
    Ok(FuzzRun { base, records, final_state: state, applied, attempts })
}
