//! Lenient player input → canonical [`Step`].
//! The player types English-ish text; the engine assigns the clock, resolves
//! names fuzzily, and suggests fixes (spec §11):
//!   "read the forgetting"                          → ANA READ THE_FORGETTING
//!   "ana, bertha talk about clue at dining hall"   → multi-actor + ABOUT + AT
//!   "0:20 @ ANA WAIT"                              → canonical passthrough
//!
//! QUIRK(Q8): the frozen whitespace class includes `\uFEFF` and excludes `\u{85}`;
//! Rust's `\s` (White_Space) is exactly the reverse. The four
//! `\s` regex sites are therefore spelled with
//! [`DSL_WS_CLASS`], never the crate's class; `/^\d+:\d+$/` has no
//! `\s`, and digits are ASCII `[0-9]`.

use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;

use crate::content::CompiledContent;
use crate::core::clock::{
    abs_min, clock_from_abs_min, parse_clock, Clock, MIN_PER_TICK, TICKS_PER_DAY,
};
use crate::core::dict::Dictionary;
use crate::core::num_parse::parse_authored_int;
use crate::core::parse::parse_sentence;
use crate::core::query::match_query;
use crate::core::r#ref::{id_ref, name_ref, Ref};
use crate::core::regex_util::static_regex;
use crate::core::text::{dsl_trim, DSL_WS_CLASS};
use crate::core::types::{Entity, State};
use crate::engine::Step;

use super::render::{display_name, pretty};

/// The parse context: dictionaries, content, state and the default actor.
pub struct LenientCtx<'a> {
    pub dict: &'a Dictionary,
    pub content: &'a CompiledContent,
    pub state: &'a State,
    /// default actor when the player omits one
    pub player: Option<&'a str>,
}

/// Success carries the parsed [`Step`]; failure
/// carries the error message, a suggestion pool (typeable display forms —
/// `the vane ledger`, `read the manifest`) and an optional hint line (why a
/// name did not resolve: gone from play, not yet on the table).
/// [`ParsedInput::is_ok`] discriminates the two variants.
#[derive(Debug, Clone)]
pub enum ParsedInput {
    Ok { step: Step },
    Fail { error: String, suggestions: Vec<String>, hint: Option<String> },
}

impl ParsedInput {
    /// The `parsed.ok` test.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, ParsedInput::Ok { .. })
    }
}

fn fail(error: impl Into<String>, suggestions: Vec<String>) -> ParsedInput {
    ParsedInput::Fail { error: error.into(), suggestions, hint: None }
}

/// Articles dropped during normalization.
const ARTICLES: [&str; 7] = ["the", "a", "an", "my", "her", "his", "their"];

/// Role keywords that route phrases.
const ROLE_KEYWORDS: [&str; 4] = ["at", "with", "about", "to"];

/// Longest-name-first scan (§11): a role phrase resolves a name of up to
/// four tokens.
const MAX_NAME_WORDS: usize = 4;

// The five role groups a clause can hold; `index()` addresses the fixed-size
// slot arrays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Group {
    Object,
    At,
    With,
    About,
    To,
}

impl Group {
    fn index(self) -> usize {
        match self {
            Group::Object => 0,
            Group::At => 1,
            Group::With => 2,
            Group::About => 3,
            Group::To => 4,
        }
    }

    /// The slot role this group fills (§6.1's role names).
    fn role(self) -> &'static str {
        match self {
            Group::Object => "object",
            Group::At => "at",
            Group::With => "with",
            Group::About => "about",
            Group::To => "to",
        }
    }

    /// A role keyword routes the current group; `object` is
    /// the starting group and is never a keyword.
    fn from_keyword(low: &str) -> Option<Group> {
        match low {
            "at" => Some(Group::At),
            "with" => Some(Group::With),
            "about" => Some(Group::About),
            "to" => Some(Group::To),
            _ => None,
        }
    }
}

// `/^\d+:\d+\s*@/` — the canonical escape hatch;
// `\s` → DSL_WS_CLASS, `\d` → ASCII [0-9].
fn passthrough_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(&format!("^[0-9]+:[0-9]+{DSL_WS_CLASS}@")))
}

// `split(/[\s,;]+)/`: the DSL_WS_CLASS
// extended with the two literal separators inside ONE bracket class.
fn token_split_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        // DSL_WS_CLASS is "[…]" — splice its inner run into the union class
        let inner = &DSL_WS_CLASS[1..DSL_WS_CLASS.len() - 1];
        Regex::new(&format!("[{inner},;]+"))
    })
}

// `/^\d+:\d+$/` (no `\s` on this line).
fn clock_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new("^[0-9]+:[0-9]+$"))
}

/// Parse one line of player input.
pub fn parse_player_input(line: &str, ctx: &LenientCtx) -> ParsedInput {
    let trimmed = dsl_trim(line);
    if trimmed.is_empty() {
        return fail("empty input", vec![]);
    }

    // an outcome arrow is the LOG's grammar, not the player's: a typed
    // `-> DESTROY X` has no step field to land in, and silently dropping it
    // would answer a cheat with a success echo — refuse it outright
    if trimmed.contains("->") {
        return fail(
            "outcomes are the engine's to write — type the action only (`0:42 @ MARCH WAIT`, or just `wait`)",
            vec![],
        );
    }
    // canonical passthrough: clock-prefixed (the raw parser message
    // surfaces on failure)
    if passthrough_re().is_match(trimmed) {
        return match parse_sentence(trimmed, ctx.dict) {
            Ok(s) => {
                // v0.5: the begin minute — an explicit clock starts there;
                // without one the action begins NOW
                let start_min = match s.clock {
                    Some(c) => abs_min(c),
                    None => ctx.state.now,
                };
                ParsedInput::Ok {
                    step: Step {
                        start_min: Some(start_min),
                        clock: s.clock,
                        clauses: s.clauses,
                        t_real: None,
                    },
                }
            }
            Err(e) => fail(e.msg, vec![]),
        };
    }

    // split and drop the empties (the filter(Boolean) shape)
    let tokens: Vec<&str> = token_split_re().split(trimmed).filter(|t| !t.is_empty()).collect();
    if tokens.is_empty() {
        return fail("empty input", vec![]);
    }

    let (names, pools) = living_name_maps(ctx.state);

    // optional leading clock token; default: begin now (v0.5)
    let mut i = 0usize;
    let mut clock: Option<Clock> = None;
    let mut start_min = ctx.state.now;
    if clock_token_re().is_match(tokens[0]) {
        // E6 (§11): a legal clock parses exactly as the canonical grammar
        // does; an out-of-range tick field WRAPS into whole days
        // (`wrap_clock`) where the canonical `d:u @` form keeps its frozen
        // loud bound — the front door is the friendly form, the authored
        // grammar the strict one. The token shape is `[0-9]+:[0-9]+`, so the
        // failure arm is i64 overflow alone (absurd literals still refuse).
        let parsed = match parse_clock(tokens[0]) {
            Ok(c) => Some(c),
            Err(_) => wrap_clock(tokens[0]),
        };
        let Some(c) = parsed else {
            return fail(format!("bad clock '{}'", tokens[0]), vec![]);
        };
        clock = Some(c);
        start_min = abs_min(c);
        i = 1;
        // a lone clock is a pure advance — let time pass, resolving every
        // window due by that minute (v0.5). Before this branch existed the
        // input fell into the verb search and crashed in the
        // verb-suggestion path.
        if i >= tokens.len() {
            return ParsedInput::Ok {
                step: Step { start_min: Some(start_min), clock, clauses: vec![], t_real: None },
            };
        }
    }

    // find the verb: single token ('READ'), two-token preposition form
    // ('use on' → USE_ON), or the verb's own prose word (`search` for
    // EXPLORE — see [`template_aliases`])
    let aliases = template_aliases(ctx);
    let mut verb_index: Option<usize> = None;
    let mut verb_len = 1usize;
    let mut alias_verb: Option<String> = None;
    for j in i..tokens.len() {
        let up = tokens[j].to_uppercase();
        if ctx.dict.is_verb(&up) {
            verb_index = Some(j);
            verb_len = 1;
            break;
        }
        if ROLE_KEYWORDS.contains(&tokens[j].to_lowercase().as_str()) {
            break;
        }
        let next = tokens.get(j + 1).map(|t| t.to_uppercase()).unwrap_or_default();
        let joined = format!("{up}_{next}");
        if ctx.dict.is_verb(&joined) {
            verb_index = Some(j);
            verb_len = 2;
            break;
        }
        if let Some(v) = aliases.get(&tokens[j].to_lowercase()) {
            verb_index = Some(j);
            verb_len = 1;
            alias_verb = Some(v.clone());
            break;
        }
    }
    let Some(verb_index) = verb_index else {
        let verbs: Vec<String> = ctx.dict.verbs.iter().map(|v| v.to_lowercase()).collect();
        // the first token with a near-verb names the typo; the suggestion is
        // the WHOLE line with that token corrected, so it can be typed back
        let mut lines: Vec<String> = Vec::new();
        for (j, tok) in tokens.iter().enumerate().skip(i) {
            let near = suggest_verb(tok, &verbs, 3);
            if near.is_empty() {
                continue;
            }
            for v in near {
                let mut fixed: Vec<&str> = tokens[i..].to_vec();
                fixed[j - i] = &v;
                lines.push(fixed.join(" ").to_lowercase());
            }
            break;
        }
        return fail(
            format!(
                "no verb found — try one of: {}",
                verbs.iter().map(|v| v.replace('_', " ")).collect::<Vec<_>>().join(", ")
            ),
            lines,
        );
    };
    let verb = match alias_verb {
        Some(v) => v,
        None => tokens[verb_index..verb_index + verb_len].join("_").to_uppercase(),
    };
    let verb_def = ctx.content.verbs.get(&verb);
    // a role's slot query, as a preference the name resolver consults when
    // a word is ambiguous (`guard` is THE_GUARD to TALK and GUARD_VAN to
    // EXPLORE)
    let prefer_for = |role: &str| -> Box<dyn Fn(&Entity) -> bool + '_> {
        match verb_def.and_then(|v| v.slot_by_role.get(role)).and_then(|s| s.query.as_ref()) {
            Some(q) => Box::new(move |e: &Entity| match_query(e, q)),
            None => Box::new(|_: &Entity| true),
        }
    };

    // split the tail into role phrases; bare names before a keyword are
    // objects. The phrases keep AUTHORED order — the object group runs from
    // the verb to the first keyword, and each keyword starts the next group —
    // because that order is the pool hand-out order: a name filling several
    // roles of one clause binds distinct §4.2 pool copies head-first
    // through it (§11, E9).
    let mut phrases: Vec<(Group, Vec<&str>)> = vec![(Group::Object, Vec::new())];
    for tok in &tokens[verb_index + verb_len..] {
        let low = tok.to_lowercase();
        match Group::from_keyword(&low) {
            Some(g) => phrases.push((g, Vec::new())),
            None => {
                if let Some(last) = phrases.last_mut() {
                    last.1.push(tok);
                }
            }
        }
    }

    // failures collect in sentence order and the FIRST one speaks: an
    // unknown word takes the suggestion path, a name whose living pool is
    // smaller than the roles naming it refuses by name (§11, E9)
    let mut handed: IndexMap<String, usize> = IndexMap::new();
    let mut failures: Vec<PhraseFail> = Vec::new();

    let actors_raw = &tokens[i..verb_index];
    let mut actors = if !actors_raw.is_empty() {
        // the actor list is its own slot: it claims the head for its names
        let mut claimed_actors: IndexMap<String, usize> = IndexMap::new();
        let (r, f) = resolve_group(
            actors_raw,
            &names,
            &pools,
            &mut handed,
            &mut claimed_actors,
            "actors",
            &*prefer_for("actors"),
        );
        failures.extend(f);
        r
    } else {
        player_refs(ctx)
    };
    // same-group phrases accumulate (`with x … with y` is one role), so the
    // fixed slots fill exactly as before — only the resolution order changed.
    // Claims are PER SLOT (a slot claims its copy of a name once; every
    // mention inside the slot re-uses it), so a doubled name inside one role
    // still meets the engine's same-role placement refusal — only a name in
    // several ROLES hands out distinct copies.
    let mut slot: [Vec<Ref>; 5] = [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut claimed: [IndexMap<String, usize>; 5] = std::array::from_fn(|_| IndexMap::new());
    for (g, raw) in &phrases {
        let prefer = prefer_for(g.role());
        let (r, f) = resolve_group(
            raw,
            &names,
            &pools,
            &mut handed,
            &mut claimed[g.index()],
            g.role(),
            &*prefer,
        );
        failures.extend(f);
        slot[g.index()].extend(r);
    }
    let [objects, at, with_role, about, to_refs] = slot;

    if let Some(f) = failures.first() {
        match f {
            PhraseFail::Unknown { word, role } => {
                // names that fit the slot first (`talk to the gaurd` wants a
                // someone, not GUARD VAN); the whole table when none do
                let prefer = prefer_for(role);
                let fitting: Vec<String> = names
                    .values()
                    .filter(|v| prefer(v.entity))
                    .map(|v| v.entity.name.clone())
                    .collect();
                // names that fit the slot only: a suggestion that cannot be
                // typed back (`guard van` for `talk to …`) is worse than none
                // — the hint names what happened when there is one to name
                let suggestions = suggest(word, &fitting, 3);
                return ParsedInput::Fail {
                    error: format!("I don't know '{word}'"),
                    suggestions,
                    hint: unknown_name_hint(word, ctx),
                };
            }
            PhraseFail::Short { key, needed, have } => {
                return fail(
                    format!(
                        "only {} '{key}' — you need {}",
                        count_word(*have),
                        count_word(*needed)
                    ),
                    vec![],
                );
            }
        }
    }

    // the `fits` probe: does a ref satisfy a slot's query? `by_name` pools
    // hold living ids with the oldest first (§4.2), so a name ref probes the
    // pool HEAD
    let fits = |r: &Ref, role: &str| -> bool {
        let Some(verb_def) = verb_def else { return false };
        let Some(slot) = verb_def.slot_by_role.get(role) else {
            return false;
        };
        let id = match r {
            Ref::Name { name, .. } => ctx.state.by_name.get(name).and_then(|pool| pool.first()),
            Ref::Id { id } => Some(id),
            Ref::Param { .. } => None,
        };
        let Some(id) = id else {
            return false;
        };
        let Some(e) = ctx.state.entities.get(id) else {
            return false;
        };
        slot.query.as_ref().is_none_or(|q| match_query(e, q))
    };
    let [mut objects, mut at, mut with_role, mut about, mut to_refs] =
        [objects, at, with_role, about, to_refs];
    // a bare name after a verb with NO object slot (`question the guard`,
    // `talk the aether seal`) goes to the first companion slot every item
    // fits — to, at, about, with in that order
    if let Some(verb_def) = verb_def {
        if !objects.is_empty() && !verb_def.slot_by_role.contains_key("object") {
            let home = ["to", "at", "about", "with"]
                .into_iter()
                .find(|role| objects.iter().all(|r| fits(r, role)));
            if let Some(role) = home {
                let moved = std::mem::take(&mut objects);
                match role {
                    "to" => to_refs.splice(0..0, moved),
                    "at" => at.splice(0..0, moved),
                    "about" => about.splice(0..0, moved),
                    _ => with_role.splice(0..0, moved),
                };
            }
        }
    }
    // English prefers "talk with quill"; slots say `to` (nightjar) or a second
    // actor (forgotten-house) — route 'with' content to the verb's real slot,
    // but only where the entity fits (§6 slot queries); anything that does not
    // fit stays in `with` so the engine rejects the phrasing the player typed
    let mut to = to_refs.first().cloned();
    if let Some(verb_def) = verb_def {
        if !with_role.is_empty() && !verb_def.slot_by_role.contains_key("with") {
            if to.is_none() && fits(&with_role[0], "to") {
                // structural (PartialEq) equality is exact here:
                // the kept set is `with_role` filtered by `fits`, and
                // `fits` is content-determined, so every ref equal to a kept
                // one is kept too — the removed sets coincide either way.
                to = Some(with_role.remove(0));
            } else if let Some(actors_slot) = verb_def.slot_by_role.get("actors") {
                let fitting: Vec<Ref> =
                    with_role.iter().filter(|r| fits(r, "actors")).cloned().collect();
                // actors.len() + fitting.len() <= actors_slot.max
                // (an unbounded max never bounds: Option None)
                let under_max = match actors_slot.max {
                    None => true,
                    Some(m) => (actors.len() + fitting.len()) as i64 <= m,
                };
                if !fitting.is_empty() && under_max {
                    actors.extend(fitting.iter().cloned());
                    let fitting_set = fitting;
                    with_role.retain(|r| !fitting_set.contains(r));
                }
            }
        }
    }

    let clause = crate::core::sentence::Clause {
        actors,
        at: at.first().cloned(),
        verb,
        to,
        objects,
        with: with_role,
        about: about.first().cloned(),
    };
    ParsedInput::Ok {
        step: Step { start_min: Some(start_min), clock, clauses: vec![clause], t_real: None },
    }
}

// ---------- name resolution ----------

/// One resolvable name: the entity and its normalized form.
pub struct NameEntry<'a> {
    pub entity: &'a Entity,
    pub norm: String,
}

/// §4.2's living name tables, one pass over the entities (birth order):
/// `names` holds one entry per distinct normalized name — the OLDEST living
/// copy (the head of the `by_name` pool) — and `pools` holds EVERY living
/// copy of each name, in the same birth order the engine's `by_name` pools
/// keep (oldest first). Lenient input has no #id form of its own, so a pool
/// name typed in prose addresses the oldest; the engine's `resolve_ref`
/// enforces the same rule on the composed sentence. The single-mention
/// behavior lives in the `names` head; the `pools` list is the hand-out
/// order for E9's multi-role binding (§11): the first role naming a pool
/// addresses the head exactly as ever, each further ROLE takes the next
/// living copy, and a pool smaller than the roles naming it refuses.
fn living_name_maps(
    state: &State,
) -> (IndexMap<String, NameEntry<'_>>, IndexMap<String, Vec<String>>) {
    let mut names: IndexMap<String, NameEntry> = IndexMap::new();
    let mut pools: IndexMap<String, Vec<String>> = IndexMap::new();
    for e in state.entities.values() {
        if e.dead_action.is_some() {
            continue;
        }
        let key = norm(&e.name);
        names.entry(key.clone()).or_insert(NameEntry { entity: e, norm: key.clone() });
        pools.entry(key).or_default().push(e.id.clone());
    }
    (names, pools)
}

/// One failed binding inside a role phrase, in sentence order: an unknown
/// word (§11's suggestion path) or a name whose living pool has fewer copies
/// than the clause's roles demand (E9's distinct-binding refusal).
enum PhraseFail {
    Unknown { word: String, role: &'static str },
    Short { key: String, needed: usize, have: usize },
}

/// Small counts speak as words in the refusal voice ("only one 'x' — you
/// need two"); anything past nine falls back to digits.
fn count_word(n: usize) -> String {
    match n {
        1 => "one".into(),
        2 => "two".into(),
        3 => "three".into(),
        4 => "four".into(),
        5 => "five".into(),
        6 => "six".into(),
        7 => "seven".into(),
        8 => "eight".into(),
        9 => "nine".into(),
        n => n.to_string(),
    }
}

/// Normalize for matching: lowercase,
/// underscores→spaces, drop articles. The split is `/\s+/` (Q8 class).
fn norm(name: &str) -> String {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let re = static_regex(&SPLIT, || Regex::new(&format!("{DSL_WS_CLASS}+")));
    let lowered = name.to_lowercase().replace('_', " ");
    re.split(&lowered)
        .filter(|w| !w.is_empty() && !ARTICLES.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve one role phrase into refs, longest-name-first; the second element
/// carries the failed bindings in token order. `handed` counts, per name,
/// how many SLOTS the clause has already served; a slot's own `claimed` map
/// fixes its copy on the slot's first mention. The first slot naming a pool
/// binds the §4.2 head as a name ref — single-mention behavior is
/// untouched; each further SLOT binds the next living copy as an `#id` ref
/// (E9), and a pool too small for its slot records a
/// [`PhraseFail::Short`]. A second mention within the same slot re-uses the
/// slot's claim, so a doubled name inside one role still meets the engine's
/// same-role placement refusal.
fn resolve_group<'a>(
    raw: &[&str],
    names: &'a IndexMap<String, NameEntry<'a>>,
    pools: &IndexMap<String, Vec<String>>,
    handed: &mut IndexMap<String, usize>,
    claimed: &mut IndexMap<String, usize>,
    role: &'static str,
    prefer: &dyn Fn(&Entity) -> bool,
) -> (Vec<Ref>, Vec<PhraseFail>) {
    let toks: Vec<&str> =
        raw.iter().copied().filter(|t| !ARTICLES.contains(&t.to_lowercase().as_str())).collect();
    let mut refs: Vec<Ref> = Vec::new();
    let mut failed: Vec<PhraseFail> = Vec::new();
    let mut j = 0usize;
    while j < toks.len() {
        let mut hit: Option<&NameEntry> = None;
        let mut used = 0usize;
        let mut len = std::cmp::min(MAX_NAME_WORDS, toks.len() - j);
        while len >= 1 {
            // the typed run normalizes like the names do (§11: case,
            // underscores, articles) — `THE_MANIFEST` and `the manifest`
            // are one key
            let key = norm(&toks[j..j + len].join(" "));
            if let Some(e) = names.get(&key) {
                hit = Some(e);
                used = len;
                break;
            }
            len -= 1;
        }
        // an exact name that does not fit the slot yields to a unique
        // fitting name containing the word (`explore guard` → GUARD VAN,
        // not THE GUARD); no exact name at all falls back to a unique
        // containing name (`read ledger` → THE VANE LEDGER)
        if hit.is_none_or(|e| !prefer(e.entity)) {
            let word = norm(toks[j]);
            if !word.is_empty() {
                let containing: Vec<&NameEntry> =
                    names.values().filter(|e| e.norm.split(' ').any(|w| w == word)).collect();
                let fitting: Vec<&NameEntry> =
                    containing.iter().copied().filter(|e| prefer(e.entity)).collect();
                // (a lone containing name that does not fit stays
                // unresolved — the engine's kind refusal would only confuse
                // where the hint "X is no longer in play" explains)
                if fitting.len() == 1 {
                    hit = Some(fitting[0]);
                    used = 1;
                }
            }
        }
        if let Some(e) = hit {
            let key = &e.norm;
            let idx = match claimed.get(key) {
                Some(i) => *i,
                None => {
                    let k = handed.entry(key.clone()).or_insert(0);
                    let i = *k;
                    *k += 1;
                    claimed.insert(key.clone(), i);
                    i
                }
            };
            if idx == 0 {
                // the head, exactly as ever (§4.2)
                refs.push(name_ref(e.entity.name.clone(), None));
            } else {
                let empty: Vec<String> = Vec::new();
                let pool = pools.get(key).unwrap_or(&empty);
                match pool.get(idx) {
                    Some(id) => refs.push(id_ref(id.clone())),
                    None => failed.push(PhraseFail::Short {
                        key: key.clone(),
                        needed: idx + 1,
                        have: pool.len(),
                    }),
                }
            }
            j += used;
        } else {
            failed.push(PhraseFail::Unknown { word: toks[j].to_string(), role });
            j += 1;
        }
    }
    (refs, failed)
}

/// E6 (§11): the bare lenient clock wraps an out-of-range tick field
/// into whole days the way durations compose (§5.1) — `0:354` is 354
/// ticks, day 2 tick 66; `1:147` is 291 ticks, day 2 tick 3.
/// The canonical
/// `d:u @` passthrough keeps its frozen `bad tick` bound: per FORM, the
/// lenient front door wraps and the authored grammar stays loud. Only
/// reachable on a `[0-9]+:[0-9]+` token (the `clock_token_re` gate), so the
/// `None` arm is i64 saturation alone — absurd literals refuse with the
/// frozen `bad clock` message.
fn wrap_clock(tok: &str) -> Option<Clock> {
    let (day, tick) = tok.split_once(':')?;
    let total = parse_authored_int(day)
        .checked_mul(TICKS_PER_DAY)?
        .checked_add(parse_authored_int(tick))?;
    Some(clock_from_abs_min(total.checked_mul(MIN_PER_TICK)?))
}

/// The default actor as refs, when set and alive.
fn player_refs(ctx: &LenientCtx) -> Vec<Ref> {
    let Some(player) = ctx.player else {
        return vec![];
    };
    let e = ctx.state.entities.values().find(|e| e.dead_action.is_none() && e.name == player);
    e.map(|e| vec![name_ref(e.name.clone(), None)]).unwrap_or_default()
}

// ---------- suggestions ----------

/// The shared suggestion scan (§11): prefix matches on the normalized keys
/// first; with no prefix hit, the near matches (levenshtein ≤ 2) — pool
/// order within each pass, capped at `limit`.
fn suggest_scan<D: Clone>(entries: &[(D, Vec<String>)], w: &str, limit: usize) -> Vec<D> {
    let starts: Vec<&(D, Vec<String>)> =
        entries.iter().filter(|(_, keys)| keys.iter().any(|k| k.starts_with(w))).collect();
    if !starts.is_empty() {
        return starts.into_iter().take(limit).map(|(d, _)| d.clone()).collect();
    }
    entries
        .iter()
        .filter(|(_, keys)| keys.iter().any(|k| levenshtein(k, w) <= 2))
        .take(limit)
        .map(|(d, _)| d.clone())
        .collect()
}

/// Suggest close names in their typeable display form (`the vane ledger`):
/// prefix matches first, then names containing the word, then levenshtein
/// ≤ 2.
fn suggest(word: &str, pool: &[String], limit: usize) -> Vec<String> {
    let w = norm(word);
    if w.is_empty() {
        return vec![];
    }
    let entries: Vec<(String, Vec<String>)> =
        pool.iter().map(|p| (display_name(p), vec![norm(p)])).collect();
    let found = suggest_scan(&entries, &w, limit);
    if !found.is_empty() {
        return found;
    }
    // a word of the name, exact or one typo off (`ledgr` → the vane ledger)
    for near in [0usize, 2] {
        let hits: Vec<String> = entries
            .iter()
            .filter(|(_, keys)| {
                keys.iter().any(|k| k.split(' ').any(|part| levenshtein(part, &w) <= near))
            })
            .take(limit)
            .map(|(d, _)| d.clone())
            .collect();
        if !hits.is_empty() {
            return hits;
        }
    }
    vec![]
}

/// The verbs' own prose words as aliases: a `says` template whose first
/// word stands alone before a placeholder or the full stop (`{actors}
/// search {object}.`, `{actors} question {to}.`) teaches that word as the
/// verb (`search` → EXPLORE, `question` → TALK); a longer phrase (`take up
/// a station at`) teaches nothing — its first word would mislead. Registered
/// verbs always win over an alias.
fn template_aliases(ctx: &LenientCtx) -> IndexMap<String, String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = static_regex(&RE, || Regex::new(r"^\{actors\} ([a-z]+)(?: \{[a-z]+\}|\.|$)"));
    let mut out: IndexMap<String, String> = IndexMap::new();
    for (verb, tpl) in &ctx.content.templates {
        let Some(m) = re.captures(tpl) else { continue };
        let word = m[1].to_lowercase();
        let starts_compound =
            ctx.dict.verbs.iter().any(|v| v.starts_with(&format!("{}_", word.to_uppercase())));
        if ctx.dict.is_verb(&word.to_uppercase())
            || starts_compound
            || ROLE_KEYWORDS.contains(&word.as_str())
            || out.contains_key(&word)
        {
            continue;
        }
        out.insert(word, verb.clone());
    }
    out
}

/// Why an unknown word did not resolve, when the table can say: the name
/// belonged to a card that left play, or to a prototype nothing has minted
/// yet (an unmet passenger the manifest named).
fn unknown_name_hint(word: &str, ctx: &LenientCtx) -> Option<String> {
    let w = norm(word);
    if w.is_empty() {
        return None;
    }
    let contains = |name: &str| norm(name).split(' ').any(|part| part == w);
    if let Some(gone) =
        ctx.state.entities.values().find(|e| e.dead_action.is_some() && contains(&e.name))
    {
        // a card that left play in the same action another arrived in its
        // place (the masked passenger who gave a name, the lord who became
        // a body) names its successor — the old title is not a death
        let successor = gone.dead_action.and_then(|at| {
            ctx.state.entities.values().find(|e| {
                e.dead_action.is_none()
                    && e.born_action == Some(at)
                    && e.attrs.get("container") == gone.attrs.get("container")
            })
        });
        return Some(match successor {
            Some(next) => format!(
                "{} is no longer in play — {} stands there now",
                pretty(&gone.name),
                pretty(&next.name)
            ),
            None => format!("{} is no longer in play", pretty(&gone.name)),
        });
    }
    if ctx.content.prototypes.keys().any(|name| contains(name)) {
        return Some(format!(
            "nobody on the table answers to '{word}' yet — `look` shows who is here"
        ));
    }
    None
}

/// Like suggest(), but multi-word verbs
/// ('travel to') also match on their first word, so 'tavel' still lands on
/// 'travel to'; display form uses spaces. Registered verbs are UPPER_SNAKE,
/// so `/[\s_]/` can only ever split on `_` here.
fn suggest_verb(word: &str, verbs: &[String], limit: usize) -> Vec<String> {
    let w = norm(word);
    if w.is_empty() {
        return vec![];
    }
    let entries: Vec<(String, Vec<String>)> = verbs
        .iter()
        .map(|v| {
            let first = v.split('_').next().unwrap_or("").to_string();
            (v.replace('_', " "), vec![norm(&first), norm(v)])
        })
        .collect();
    suggest_scan(&entries, &w, limit)
}

/// The DP runs on chars (a UTF-16-unit index would agree
/// over the ASCII name/verb domain these feed).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in dp.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in dp[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        // the dp table is indexed — the recurrence reads three neighbors
        #[allow(clippy::needless_range_loop)]
        for j in 1..=b.len() {
            dp[i][j] = std::cmp::min(
                std::cmp::min(dp[i - 1][j] + 1, dp[i][j - 1] + 1),
                dp[i - 1][j - 1] + usize::from(a[i - 1] != b[j - 1]),
            );
        }
    }
    dp[a.len()][b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_lowercases_descores_and_drops_articles() {
        assert_eq!(norm("THE_VANE_LEDGER"), "vane ledger");
        assert_eq!(norm("the  AETHER seal"), "aether seal");
        assert_eq!(norm("my own her"), "own"); // every article word dropped
        assert_eq!(norm("___"), "");
    }

    #[test]
    fn levenshtein_is_the_standard_dp() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("ledgr", "ledger"), 1);
        assert_eq!(levenshtein("", "abc"), 3);
    }

    #[test]
    fn suggest_prefers_prefix_then_near_matches() {
        let pool = vec![
            "THE_VANE_LEDGER".to_string(),
            "THE_PATENTEE".to_string(),
            "THE_PALE_LADY".to_string(),
        ];
        // prefix hits win: this is the script-play 'read the vane ledgr' shape
        // — the failed word is 'vane' (the multi-word scan ate 'ledgr'), and
        // 'vane ledger' starts with 'vane'; suggestions print the TYPEABLE
        // display form
        assert_eq!(suggest("vane", &pool, 3), vec!["the vane ledger"]);
        // no prefix → near (levenshtein ≤ 2) fallback
        assert_eq!(suggest("ledgr", &["LEDGER".to_string()], 3), vec!["ledger"]);
        assert_eq!(
            suggest("paten", &["THE_PATENTEE".to_string()], 3),
            vec!["the patentee"] // prefix on the normalized form
        );
        // a non-prefix word still lands when it is one of the name's words
        assert_eq!(suggest("ledger", &pool, 3), vec!["the vane ledger"]);
    }

    #[test]
    fn wrap_clock_folds_tick_overflow_into_days() {
        // E6: the lenient front door wraps, the way durations compose —
        // 354 ticks = day 2 tick 66 (11:00), and the day field carries over
        assert_eq!(wrap_clock("0:354"), Some(Clock { day: 2, tick: 66 }));
        assert_eq!(wrap_clock("1:147"), Some(Clock { day: 2, tick: 3 }));
        // the boundary itself folds exactly one day
        assert_eq!(wrap_clock("0:144"), Some(Clock { day: 1, tick: 0 }));
        // the fold point's first crossings
        assert_eq!(wrap_clock("0:150"), Some(Clock { day: 1, tick: 6 }));
        // a legal clock needs no wrap (parse_clock takes it first)
        assert_eq!(wrap_clock("3:143"), Some(Clock { day: 3, tick: 143 }));
        // absurd literals saturate parse_authored_int and refuse here
        assert_eq!(wrap_clock("0:99999999999999999999"), None);
    }

    #[test]
    fn count_word_speaks_small_counts_as_words() {
        assert_eq!(count_word(1), "one");
        assert_eq!(count_word(2), "two");
        assert_eq!(count_word(3), "three");
        assert_eq!(count_word(9), "nine");
        assert_eq!(count_word(10), "10");
    }
}
