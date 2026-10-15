//! Lenient player input → canonical [`Step`].
//! The player types English-ish text; the engine assigns the clock, resolves
//! names fuzzily, and suggests fixes (spec §11):
//!   "read the forgetting"                          → ANA READ THE_FORGETTING
//!   "ana, bertha talk about clue at dining hall"   → multi-actor + ABOUT + AT
//!   "0:20 @ ANA WAIT"                              → canonical passthrough
//!
//! QUIRK(Q8): JS `\s` includes `\uFEFF` and excludes `\u{85}`;
//! Rust's `\s` (White_Space) is exactly the reverse. The four
//! `\s` regex sites are therefore spelled with
//! [`JS_WS_CLASS`], never the crate's class; `/^\d+:\d+$/` has no
//! `\s`, and JS `\d` is ASCII `[0-9]`.

use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;

use crate::content::content::CompiledContent;
use crate::core::clock::{abs_min, parse_clock, Clock};
use crate::core::dict::Dictionary;
use crate::core::parse::parse_sentence;
use crate::core::query::match_query;
use crate::core::r#ref::{name_ref, Ref};
use crate::core::types::{Entity, State};
use crate::engine::engine::Step;
use crate::js::{js_trim, static_regex, JS_WS_CLASS};

/// The parse context: dictionaries, content, state and the default actor.
pub struct LenientCtx<'a> {
    pub dict: &'a Dictionary,
    pub content: &'a CompiledContent,
    pub state: &'a State,
    /// default actor when the player omits one
    pub player: Option<&'a str>,
}

/// Success carries the parsed [`Step`] and its canonical text; failure
/// carries the error message and a suggestion pool ([`ParsedInput::is_ok`]
/// discriminates the two variants).
#[derive(Debug, Clone)]
pub enum ParsedInput {
    Ok {
        step: Step,
        canonical: String,
    },
    Fail {
        error: String,
        suggestions: Vec<String>,
    },
}

impl ParsedInput {
    /// The `parsed.ok` test.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        matches!(self, ParsedInput::Ok { .. })
    }
}

fn fail(error: impl Into<String>, suggestions: Vec<String>) -> ParsedInput {
    ParsedInput::Fail {
        error: error.into(),
        suggestions,
    }
}

/// Articles dropped during normalization.
const ARTICLES: [&str; 7] = ["the", "a", "an", "my", "her", "his", "their"];

/// Role keywords that route phrases.
const ROLE_KEYWORDS: [&str; 4] = ["at", "with", "about", "to"];

// `type GroupKey = 'object' | 'at' | 'with' |
// 'about' | 'to'`; the discriminants index the fixed-size groups array.
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
// `\s` → JS_WS_CLASS, `\d` → ASCII [0-9].
fn passthrough_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(&format!("^[0-9]+:[0-9]+{JS_WS_CLASS}@")))
}

// `split(/[\s,;]+)/`: the JS_WS class
// extended with the two literal separators inside ONE bracket class.
fn token_split_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        // JS_WS_CLASS is "[…]" — splice its inner run into the union class
        let inner = &JS_WS_CLASS[1..JS_WS_CLASS.len() - 1];
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
    let trimmed = js_trim(line);
    if trimmed.is_empty() {
        return fail("empty input", vec![]);
    }

    // canonical passthrough: clock-prefixed or containing an outcome arrow
    // (the raw parser message surfaces on failure)
    if passthrough_re().is_match(trimmed) || trimmed.contains("->") {
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
                    canonical: trimmed.to_string(),
                }
            }
            Err(e) => ParsedInput::Fail {
                error: e.msg,
                suggestions: vec![],
            },
        };
    }

    // split and drop the empties JS's filter(Boolean) drops
    let tokens: Vec<&str> = token_split_re()
        .split(trimmed)
        .filter(|t| !t.is_empty())
        .collect();
    if tokens.is_empty() {
        return fail("empty input", vec![]);
    }

    let names = alive_names(ctx.state);

    // optional leading clock token; default: begin now (v0.5)
    let mut i = 0usize;
    let mut clock: Option<Clock> = None;
    let mut start_min = ctx.state.now;
    if clock_token_re().is_match(tokens[0]) {
        match parse_clock(tokens[0]) {
            Ok(c) => {
                clock = Some(c);
                start_min = abs_min(c);
            }
            Err(_) => {
                return fail(format!("bad clock '{}'", tokens[0]), vec![]);
            }
        }
        i = 1;
        // a lone clock is a pure advance — let time pass, resolving every
        // window due by that minute (v0.5). Before this branch existed the
        // input fell into the verb search and crashed in the
        // verb-suggestion path.
        if i >= tokens.len() {
            return ParsedInput::Ok {
                step: Step {
                    start_min: Some(start_min),
                    clock,
                    clauses: vec![],
                    t_real: None,
                },
                canonical: trimmed.to_string(),
            };
        }
    }

    // find the verb: single token ('READ') or two-token preposition form
    // ('use on' → USE_ON)
    let mut verb_index: Option<usize> = None;
    let mut verb_len = 1usize;
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
        let next = tokens
            .get(j + 1)
            .map(|t| t.to_uppercase())
            .unwrap_or_default();
        let joined = format!("{up}_{next}");
        if ctx.dict.is_verb(&joined) {
            verb_index = Some(j);
            verb_len = 2;
            break;
        }
    }
    let Some(verb_index) = verb_index else {
        let verbs: Vec<String> = ctx.dict.verbs.iter().map(|v| v.to_lowercase()).collect();
        return ParsedInput::Fail {
            error: format!(
                "no verb found — try one of: {}",
                verbs
                    .iter()
                    .map(|v| v.replace('_', " "))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            suggestions: suggest_verb(tokens[i], &verbs, 3),
        };
    };
    let verb = tokens[verb_index..verb_index + verb_len]
        .join("_")
        .to_uppercase();

    // split the tail into role phrases; bare names before a keyword are
    // objects
    let mut groups: [Vec<&str>; 5] = [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut current = Group::Object;
    for tok in &tokens[verb_index + verb_len..] {
        let low = tok.to_lowercase();
        match Group::from_keyword(&low) {
            Some(g) => current = g,
            None => groups[current.index()].push(tok),
        }
    }

    // failures collect in a fixed order: actors, objects,
    // to, with, THEN at/about (unknown[0] carries the error message)
    let mut unknown: Vec<String> = Vec::new();
    let actors_raw = &tokens[i..verb_index];
    let mut actors = if !actors_raw.is_empty() {
        let (r, f) = resolve_group(actors_raw, &names);
        unknown = f;
        r
    } else {
        player_refs(ctx)
    };
    let (objects, f) = resolve_group(&groups[Group::Object.index()], &names);
    unknown.extend(f);
    let at = resolve_group(&groups[Group::At.index()], &names);
    let about = resolve_group(&groups[Group::About.index()], &names);
    let (to_refs, f) = resolve_group(&groups[Group::To.index()], &names);
    unknown.extend(f);
    let (mut with_role, f) = resolve_group(&groups[Group::With.index()], &names);
    unknown.extend(f);
    unknown.extend(at.1);
    unknown.extend(about.1); // about/at fail loudly too (§11)

    if !unknown.is_empty() {
        let word = &unknown[0];
        let pool: Vec<String> = names.values().map(|v| v.entity.name.clone()).collect();
        return ParsedInput::Fail {
            error: format!("I don't know '{word}'"),
            suggestions: suggest(word, &pool, 3),
        };
    }

    // English prefers "talk with quill"; slots say `to` (nightjar) or a second
    // actor (forgotten-house) — route 'with' content to the verb's real slot,
    // but only where the entity fits (§6 slot queries); anything that does not
    // fit stays in `with` so the engine rejects the phrasing the player typed
    let mut to = to_refs.first().cloned();
    if let Some(verb_def) = ctx.content.verbs.get(&verb) {
        if !with_role.is_empty() && !verb_def.slot_by_role.contains_key("with") {
            // the `fits` probe: `by_name` pools hold
            // living ids with the oldest first (§9.38), so a name ref probes
            // the pool HEAD
            let fits = |r: &Ref, role: &str| -> bool {
                let Some(slot) = verb_def.slot_by_role.get(role) else {
                    return false;
                };
                let id = match r {
                    Ref::Name { name, .. } => {
                        ctx.state.by_name.get(name).and_then(|pool| pool.first())
                    }
                    _ => None,
                };
                let Some(id) = id else {
                    return false;
                };
                let Some(e) = ctx.state.entities.get(id) else {
                    return false;
                };
                slot.query.as_ref().is_none_or(|q| match_query(e, q))
            };
            if to.is_none() && fits(&with_role[0], "to") {
                // structural (PartialEq) equality is exact here:
                // the kept set is `with_role` filtered by `fits`, and
                // `fits` is content-determined, so every ref equal to a kept
                // one is kept too — the removed sets coincide either way.
                to = Some(with_role.remove(0));
            } else if let Some(actors_slot) = verb_def.slot_by_role.get("actors") {
                let fitting: Vec<Ref> = with_role
                    .iter()
                    .filter(|r| fits(r, "actors"))
                    .cloned()
                    .collect();
                // actors.len() + fitting.len() <= actors_slot.max
                // (a JS Infinity max never bounds: Option None)
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
        at: at.0.first().cloned(),
        verb,
        to,
        objects,
        with: with_role,
        about: about.0.first().cloned(),
    };
    ParsedInput::Ok {
        step: Step {
            start_min: Some(start_min),
            clock,
            clauses: vec![clause],
            t_real: None,
        },
        canonical: String::new(),
    }
}

// ---------- name resolution ----------

/// One resolvable name: the entity and its normalized form.
pub struct NameEntry<'a> {
    pub entity: &'a Entity,
    pub norm: String,
}

/// §9.38: one entry per distinct name — the
/// OLDEST living copy (the head of the `by_name` pool; entities iterate in birth
/// order). Lenient input has no #id form of its own, so a pool name typed in
/// prose addresses the oldest; the engine's `resolve_ref` enforces the same rule
/// on the composed sentence.
fn alive_names(state: &State) -> IndexMap<String, NameEntry<'_>> {
    let mut map: IndexMap<String, NameEntry> = IndexMap::new();
    for e in state.entities.values() {
        if e.dead_action.is_some() {
            continue;
        }
        let key = norm(&e.name);
        map.entry(key.clone()).or_insert(NameEntry {
            entity: e,
            norm: key,
        });
    }
    map
}

/// Normalize for matching: lowercase,
/// underscores→spaces, drop articles. The split is JS `/\s+/` (Q8 class).
fn norm(name: &str) -> String {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let re = static_regex(&SPLIT, || Regex::new(&format!("{JS_WS_CLASS}+")));
    let lowered = name.to_lowercase().replace('_', " ");
    re.split(&lowered)
        .filter(|w| !w.is_empty() && !ARTICLES.contains(w))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Resolve one role phrase into refs, longest-name-first; the second element
/// carries the words that resolved to nothing.
fn resolve_group(raw: &[&str], names: &IndexMap<String, NameEntry>) -> (Vec<Ref>, Vec<String>) {
    let toks: Vec<&str> = raw
        .iter()
        .copied()
        .filter(|t| !ARTICLES.contains(&t.to_lowercase().as_str()))
        .collect();
    let mut refs: Vec<Ref> = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    let mut j = 0usize;
    while j < toks.len() {
        let mut hit: Option<&NameEntry> = None;
        let mut used = 0usize;
        // `Math.min(4, toks.length - j)` — the magic 4 is the
        // MAX_NAME_WORDS constant
        const MAX_NAME_WORDS: usize = 4;
        let mut len = std::cmp::min(MAX_NAME_WORDS, toks.len() - j);
        while len >= 1 {
            let key = toks[j..j + len].join(" ").to_lowercase();
            if let Some(e) = names.get(&key) {
                hit = Some(e);
                used = len;
                break;
            }
            len -= 1;
        }
        match hit {
            Some(e) => {
                refs.push(name_ref(e.entity.name.clone(), None));
                j += used;
            }
            None => {
                failed.push(toks[j].to_string());
                j += 1;
            }
        }
    }
    (refs, failed)
}

/// The default actor as refs, when set and alive.
fn player_refs(ctx: &LenientCtx) -> Vec<Ref> {
    let Some(player) = ctx.player else {
        return vec![];
    };
    let e = ctx
        .state
        .entities
        .values()
        .find(|e| e.dead_action.is_none() && e.name == player);
    e.map(|e| vec![name_ref(e.name.clone(), None)])
        .unwrap_or_default()
}

// ---------- suggestions ----------

/// Suggest close names: prefix matches first, then levenshtein ≤ 2.
fn suggest(word: &str, pool: &[String], limit: usize) -> Vec<String> {
    let w = norm(word);
    if w.is_empty() {
        return vec![];
    }
    let normed: Vec<(&String, String)> = pool.iter().map(|p| (p, norm(p))).collect();
    let starts: Vec<&(&String, String)> =
        normed.iter().filter(|(_, n)| n.starts_with(&w)).collect();
    if !starts.is_empty() {
        return starts
            .into_iter()
            .take(limit)
            .map(|(p, _)| (*p).clone())
            .collect();
    }
    normed
        .iter()
        .filter(|(_, n)| levenshtein(n, &w) <= 2)
        .take(limit)
        .map(|(p, _)| (*p).clone())
        .collect()
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
    let normed: Vec<(String, String, String)> = verbs
        .iter()
        .map(|v| {
            let first = v.split('_').next().unwrap_or("").to_string();
            (v.clone(), norm(v), norm(&first))
        })
        .collect();
    let show = |v: &str| v.replace('_', " ");
    let starts: Vec<&(String, String, String)> = normed
        .iter()
        .filter(|(_, whole, first)| whole.starts_with(&w) || first.starts_with(&w))
        .collect();
    if !starts.is_empty() {
        return starts
            .into_iter()
            .take(limit)
            .map(|(v, ..)| show(v))
            .collect();
    }
    normed
        .iter()
        .filter(|(_, whole, first)| levenshtein(first, &w) <= 2 || levenshtein(whole, &w) <= 2)
        .take(limit)
        .map(|(v, ..)| show(v))
        .collect()
}

/// The DP runs on chars (the JS reference indexes
/// UTF-16 units; identical for the ASCII name/verb domain these feed).
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
        // (the dp table is indexed)
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
        // 'vane ledger' starts with 'vane'
        assert_eq!(suggest("vane", &pool, 3), vec!["THE_VANE_LEDGER"]);
        // no prefix → near (levenshtein ≤ 2) fallback
        assert_eq!(
            suggest("ledgr", &["LEDGER".to_string()], 3),
            vec!["LEDGER"]
        );
        assert_eq!(
            suggest("paten", &["THE_PATENTEE".to_string()], 3),
            vec!["THE_PATENTEE"] // prefix on the normalized form
        );
    }
}
