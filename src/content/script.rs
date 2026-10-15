//! Script compiler. A game script is a piece of
//! text: meta lines, section blocks, and sentence-like declarations
//! (spec: "sentences + small blocks"). [`parse_script`] is the
//! indentation-sensitive line-segment parser; [`compile_script`] turns a
//! script into runnable game content + the initial State; [`lint_script`] is
//! the §10 warn-loudly sibling (it never throws for a script that compiles).
//!
//! Regex-domain freeze: every `(.+)`-style pattern in
//! this module runs on `split('\n')` + [`js_trim`]-trimmed SINGLE-LINE
//! text, so `.` never sees `\r`/U+2028/U+2029 (line terminators are split
//! away or trimmed by the frozen whitespace set); these patterns must NOT
//! be reused on multi-line input.
//!
//! Whitespace discipline (Q8): every `\s`/`\S` in these patterns is the
//! explicit frozen class ([`JS_WS_CLASS`]/[`JS_NON_WS_CLASS`]) and every
//! `trim()`/`trimStart()` is [`js_trim`]/a [`js_char_is_ws`] take-while —
//! the frozen set and `char::is_whitespace` disagree on U+0085/U+FEFF
//! exactly reversely.

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use crate::content::content::{
    collect_stackable, compile_content, mint_recipe_id, CompiledContent, ContentRaw, DeclaredEntity,
    DeriveAspect, DeckRaw, EndingDef, ExpireTriggerDef, FunctionDef, PrototypeDef, PrototypeDerive,
    RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use crate::core::clock::{abs_min, parse_clock, parse_duration, Clock};
use crate::core::dict::{Dictionary, KINDS};
use crate::core::query::aspects_from_pairs;
use crate::core::roles::Role;
use crate::core::types::{
    add_entity, new_state, relation_key, AttrValue, Attrs, Aspects, Entity, Relation, State,
};
use crate::core::jsonw::ordinary_own_property_keys;
use crate::core::num_parse::parse_i64_like_f64;
use crate::core::regex_util::{grp_opt, grp_text, grp0_start, static_regex};
use crate::core::text::{js_char_is_ws, js_trim, JS_NON_WS_CLASS, JS_WS_CLASS};
use crate::error::{ChorusError, Result};

// ---------------------------------------------------------------------------
// script-side types
// ---------------------------------------------------------------------------

/// Script meta: title, player, goal, start clock, dice seed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScriptMeta {
    pub title: Option<String>,
    pub player: Option<String>,
    pub goal: Option<String>,
    pub start: Option<Clock>,
    /// §9.41: the dice seed (default 0) — the run's whole dice sequence is a
    /// pure function of (seed, rng counter)
    pub seed: Option<i64>,
}

/// One declared entity line.
#[derive(Debug, Clone)]
pub struct EntitySpec {
    pub name: String,
    pub kind: String,
    /// §9.38: `NAME is KIND stackable …` — this line's card may share its name
    pub stackable: bool,
    pub aspects: Aspects,
    pub container_name: Option<String>,
}

/// One declared relation line.
#[derive(Debug, Clone)]
pub struct RelationSpec {
    pub a: String,
    /// Rust name `type_` (`type` is a keyword)
    pub type_: String,
    pub b: String,
}

/// The parsed script: meta + raw content + declared entities/relations.
#[derive(Debug, Clone)]
pub struct Script {
    pub meta: ScriptMeta,
    pub content: ContentRaw,
    pub entities: Vec<EntitySpec>,
    pub relations: Vec<RelationSpec>,
}

/// The `compile_script` return shape
/// `{ content, state, meta }`.
#[derive(Debug)]
pub struct CompiledScript {
    pub content: CompiledContent,
    pub state: State,
    pub meta: ScriptMeta,
}

// ---------------------------------------------------------------------------
// shared regex literals. Q8 class substitutions throughout:
// `\s` → JS_WS_CLASS, `\S` → JS_NON_WS_CLASS, `\b` →
// `(?-u:\b)` (the ASCII JS word boundary).
// ---------------------------------------------------------------------------

/// `/^(script|player|goal|start|seed):\s*(.+)$`
fn meta_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        // ANY lowercase identifier keys a meta line — the match arms below
        // dispatch on it, so an unknown key is RECOGNIZED as a meta attempt
        // (and named by the unknown-key error, Q15) instead of masquerading
        // as a section problem. Group 1 stays the key, group 2 the value.
        Regex::new(&format!(r"^([a-z][a-z0-9_]*):{JS_WS_CLASS}*(.+)$"))
    })
}

/// `/^-?[0-9]+$/` (seed gate; ASCII digits only)
fn seed_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^-?[0-9]+$"))
}

/// Section headers, free-form order.
fn section_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(
        &RE,
        || Regex::new(r"^(entities|prototypes|triggers|endings|verbs|templates|functions|decks):$"),
    )
}

/// `/^([A-Z_]+) says "(.*)"$`
fn template_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r#"^([A-Z_]+) says "(.*)"$"#))
}

/// `/^[A-Z][A-Z0-9_]*:$/`
fn verb_header_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Z][A-Z0-9_]*:$"))
}

/// `/^recipes:$/`
fn recipes_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^recipes:$"))
}

/// `/^([a-z]+):\s*(.+)$`
fn slot_role_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"^([a-z]+):{JS_WS_CLASS}*(.+)$"))
    })
}

/// `/^(\S+)\s+is\s+([A-Za-z_.]+)\b\s*(.*)$/`
fn entity_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+){JS_WS_CLASS}+is{JS_WS_CLASS}+([A-Za-z_.]+)(?-u:\b){JS_WS_CLASS}*(.*)$"#
        ))
    })
}

/// `/^of\s+(\S+)$/`
fn of_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"^of{JS_WS_CLASS}+({JS_NON_WS_CLASS}+)$"))
    })
}

/// `/^stackable\b\s*/` (a PREFIX match; the matched
/// text is sliced off `rest`)
fn stackable_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"^stackable(?-u:\b){JS_WS_CLASS}*"))
    })
}

/// `/(?:^|\s+)(?:at|in)\s+(\S+)\s*$/`
fn at_in_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"(?:^|{JS_WS_CLASS}+)(?:at|in){JS_WS_CLASS}+({JS_NON_WS_CLASS}+){JS_WS_CLASS}*$"#
        ))
    })
}

/// `/(?:^|\s+)with\s+(.+)$/`
fn with_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"(?:^|{JS_WS_CLASS}+)with{JS_WS_CLASS}+(.+)$"))
    })
}

/// `/(?:^|\s+)deriving\s+(.+)$/`
fn deriving_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"(?:^|{JS_WS_CLASS}+)deriving{JS_WS_CLASS}+(.+)$"))
    })
}

/// `/^\s*([A-Za-z_][A-Za-z0-9_.]*):(-?\d+)\s*$/`
fn aspect_spec_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"^{JS_WS_CLASS}*([A-Za-z_][A-Za-z0-9_.]*):(-?[0-9]+){JS_WS_CLASS}*$"
        ))
    })
}

/// `/^(\S+)\s+is\s+([a-z]+)\b\s*(.*)$/`
fn prototype_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+){JS_WS_CLASS}+is{JS_WS_CLASS}+([a-z]+)(?-u:\b){JS_WS_CLASS}*(.*)$"#
        ))
    })
}

/// `/^\s*name\s+from\s+object\s*$/`
fn name_from_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"^{JS_WS_CLASS}*name{JS_WS_CLASS}+from{JS_WS_CLASS}+object{JS_WS_CLASS}*$"
        ))
    })
}

/// `/^\s*(…)\s+from\s+object\.(…)(?:\s*-\s*(\d+))?\s*$/`
fn derive_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"^{JS_WS_CLASS}*([A-Za-z_][A-Za-z0-9_.]*){JS_WS_CLASS}+from{JS_WS_CLASS}+object\.([A-Za-z_][A-Za-z0-9_.]*)(?:{JS_WS_CLASS}*-{JS_WS_CLASS}*([0-9]+))?{JS_WS_CLASS}*$"
        ))
    })
}

/// Expiry form (§6.6, §9.32).
fn expire_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+){JS_WS_CLASS}+on{JS_WS_CLASS}+expire{JS_WS_CLASS}+of{JS_WS_CLASS}+({JS_NON_WS_CLASS}+){JS_WS_CLASS}*->{JS_WS_CLASS}*(.+)$"#
        ))
    })
}

/// Clock form: `ID [once] at (hour|halfhour) N -> …`
fn trigger_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+)({JS_WS_CLASS}+once)?{JS_WS_CLASS}+at{JS_WS_CLASS}+(hour|halfhour){JS_WS_CLASS}+([0-9]+){JS_WS_CLASS}*->{JS_WS_CLASS}*(.+)$"#
        ))
    })
}

/// Paren'd comma-separated params.
fn fn_paren_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+){JS_WS_CLASS}*\(([^)]*)\){JS_WS_CLASS}*->{JS_WS_CLASS}*(.+)$"#
        ))
    })
}

/// No params, no parens.
fn fn_arrow_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+){JS_WS_CLASS}*->{JS_WS_CLASS}*(.+)$"#
        ))
    })
}

/// Bare, space-separated params (lazy `\S.*?`).
fn fn_bare_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+){JS_WS_CLASS}+({JS_NON_WS_CLASS}.*?){JS_WS_CLASS}*->{JS_WS_CLASS}*(.+)$"#
        ))
    })
}

/// `/^(\S+)\s+spec\b\s*(.*)$/`
fn deck_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+){JS_WS_CLASS}+spec(?-u:\b){JS_WS_CLASS}*(.*)$"#
        ))
    })
}

/// `/(?:^|\s)defaultcard\s+(\S+)\s*$/` (single `\s`)
fn defaultcard_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"(?:^|{JS_WS_CLASS})defaultcard{JS_WS_CLASS}+({JS_NON_WS_CLASS}+){JS_WS_CLASS}*$"#
        ))
    })
}

/// `/(?:^|\s)reset\s*$/` (single `\s`)
fn reset_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"(?:^|{JS_WS_CLASS})reset{JS_WS_CLASS}*$"))
    })
}

/// `/^([A-Z][A-Z0-9_]*):\s*"(.+)"(?:\s+\(([a-z]+)\))?$`
fn ending_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^([A-Z][A-Z0-9_]*):{JS_WS_CLASS}*"(.+)"(?:{JS_WS_CLASS}+\(([a-z]+)\))?$"#
        ))
    })
}

/// `/^(\S+)(?:\s+(.*))?$/`
fn slot_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({JS_NON_WS_CLASS}+)(?:{JS_WS_CLASS}+(.*))?$"#
        ))
    })
}

/// `/^(\d+)(?:\.\.(\d+)?)?$/`
fn count_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^([0-9]+)(?:\.\.([0-9]+)?)?$"))
}

/// `/^default\b/`
fn default_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^default(?-u:\b)"))
}

/// `/^[a-z0-9_.]+$/` (candidate recipe ids)
fn recipe_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[a-z0-9_.]+$"))
}

/// `/hint\s+"([^"]*)"/`
fn hint_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r#"hint{JS_WS_CLASS}+"([^"]*)""#))
    })
}

/// `/(?:^|\s)when\s+(.+)$/` (single `\s`)
fn when_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"(?:^|{JS_WS_CLASS})when{JS_WS_CLASS}+(.+)$"))
    })
}

/// The count-gate scan
/// `/count\((actors|object|with|at|about|to)\)\s*(>=|<=|>|<|=|!=)\s*(\d+)/g`
fn count_gate_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"count\((actors|object|with|at|about|to)\){JS_WS_CLASS}*(>=|<=|>|<|=|!=){JS_WS_CLASS}*([0-9]+)"
        ))
    })
}

/// Template tokens `/\{([a-z_][a-z0-9_]*)\}/g`
fn template_token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"\{([a-z_][a-z0-9_]*)\}"))
}

/// Aspect-term read scan `/aspect:([a-z_][a-z0-9_.]*)/g`
fn read_aspect_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"aspect:([a-z_][a-z0-9_.]*)"))
}

/// Predicate-path read scan `/\baspect\.([a-z_][a-z0-9_.]*)/g` (`\b` is the
/// ASCII JS word boundary)
fn read_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"(?-u:\b)aspect\.([a-z_][a-z0-9_.]*)"))
}

/// SET-aspect write scan `/SET\(\s*[^,()]+,\s*aspect:([a-z_][a-z0-9_.]*)/g`
fn write_set_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"SET\({JS_WS_CLASS}*[^,()]+,{JS_WS_CLASS}*aspect:([a-z_][a-z0-9_.]*)"
        ))
    })
}

/// `/^[A-Za-z_][A-Za-z0-9_]*$/` (the name-lexeme test)
fn name_lexeme_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$"))
}

/// `split(/\s+/)` over JS whitespace: runs of JS
/// whitespace split; the inputs are always trimmed or `\S`-anchored, so
/// filtering the empty pieces yields exactly the JS token sequence.
fn split_js_ws(s: &str) -> Vec<&str> {
    s.split(js_char_is_ws).filter(|p| !p.is_empty()).collect()
}

// ---------------------------------------------------------------------------
// parse_script
// ---------------------------------------------------------------------------

/// Parse a script's text into meta + raw content + the
/// declared entity/relation lines (grammar only; all load-time validation
/// against the dictionary/world happens in `compile_script`/`compile_content`).
pub fn parse_script(text: &str, dict: &Dictionary) -> Result<Script> {
    let mut meta = ScriptMeta::default();
    let mut entities: Vec<EntitySpec> = Vec::new();
    let mut relations: Vec<RelationSpec> = Vec::new();
    let mut prototypes: IndexMap<String, PrototypeDef> = IndexMap::new();
    let mut triggers: Vec<TriggerDef> = Vec::new();
    let mut templates: IndexMap<String, String> = IndexMap::new();
    let mut endings: IndexMap<String, EndingDef> = IndexMap::new();
    let mut verbs: IndexMap<String, VerbContent> = IndexMap::new();
    let mut decks: Vec<DeckRaw> = Vec::new();
    let mut functions: Vec<FunctionDef> = Vec::new();
    let mut entity_names: IndexSet<String> = IndexSet::new();
    // minted script id (`s_{name.to_lowercase()}`) → the name holding it
    let mut minted_ids: IndexMap<String, String> = IndexMap::new();

    let mut section: Option<String> = None;
    let mut verb_name: Option<String> = None;
    let mut in_recipes = false;

    for (ln, raw_line) in text.split('\n').enumerate() {
        // EVERY tab becomes
        // two spaces (`replace(/\t/g, '  ')` semantics)
        let line = raw_line.replace('\t', "  ");
        let trimmed = js_trim(&line);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // the count of leading JS whitespace CODE UNITS (in the JS spelling,
        // the length minus the trimStart-ed length). No JS-whitespace char is
        // astral, so units == chars here.
        let indent = line.chars().take_while(|c| js_char_is_ws(*c)).count();
        let t = trimmed;

        // meta lines (indent 0, outside sections)
        if indent == 0 && section.is_none() {
            if let Some(m) = meta_re().captures(t) {
                match grp_text(&m, 1) {
                    "script" => meta.title = Some(js_trim(grp_text(&m, 2)).to_string()),
                    "player" => meta.player = Some(js_trim(grp_text(&m, 2)).to_string()),
                    "goal" => meta.goal = Some(js_trim(grp_text(&m, 2)).to_string()),
                    "seed" => {
                        // §9.41: integer seed, XORed with the rng counter as a
                        // 64-bit value. The message is constructed IN PLACE at
                        // this throw site.
                        let got = js_trim(grp_text(&m, 2));
                        if !seed_re().is_match(got) {
                            return Err(ChorusError::new(format!(
                                "line {}: seed must be an integer, got '{}'",
                                ln + 1,
                                got
                            )));
                        }
                        meta.seed = Some(parse_i64_like_f64(got));
                    }
                    // the clock key: `start` is the only one of the five
                    // that lands here explicitly, and a bad VALUE blames
                    // the clock — correct attribution for a known key
                    // (§10: `start: d:h`).
                    "start" => meta.start = Some(parse_clock(js_trim(grp_text(&m, 2)))?),
                    // Q15: an unknown meta key names itself and lists the
                    // five keys. The old wildcard arm treated anything
                    // un-listed as the clock key — the next key added to
                    // the regex without a match arm would have been blamed
                    // on "bad clock". meta_re demands a value after the
                    // colon, so this arm can never shadow a bare
                    // `section:` header (those never match meta_re).
                    other => {
                        return Err(ChorusError::new(format!(
                            "line {}: unknown meta key '{other}' (the meta keys are: script, player, goal, start, seed)",
                            ln + 1
                        )));
                    }
                }
                continue;
            }
        }

        // section headers (indent 0); free-form order — `compile_content` fixes
        // the compile order (functions first), so `functions:` may sit anywhere
        if indent == 0 && section_re().is_match(t) {
            section = Some(t[..t.len() - 1].to_string());
            verb_name = None;
            in_recipes = false;
            continue;
        }
        let Some(sec) = &section else {
            return Err(ChorusError::new(format!(
                "line {}: expected a section header, got '{t}'",
                ln + 1
            )));
        };

        match sec.as_str() {
            "entities" => {
                let declared = entities.len();
                parse_entity_line(t, ln + 1, dict, &mut entities, &mut relations)?;
                // duplicate entity declarations share one s_<name> id and
                // would silently overwrite — a load error, not a
                // last-write-wins (§9.14). Names are case-exact (§9.14), so
                // the name check is case-sensitive — but the minted id is
                // not (`s_{name.to_lowercase()}`, the compile-side id
                // scheme), so a second check pins id uniqueness: ANA/ana
                // both mint s_ana and collide. (Formerly QUIRK(Q5): only
                // the case-exact half errored.)
                if entities.len() > declared {
                    let name = match entities.last() {
                        Some(e) => e.name.clone(),
                        None => unreachable!("entities grew, last must exist"),
                    };
                    if entity_names.contains(&name) {
                        return Err(ChorusError::new(format!(
                            "line {}: duplicate entity declaration '{name}'",
                            ln + 1
                        )));
                    }
                    let minted = format!("s_{}", name.to_lowercase());
                    if let Some(holder) = minted_ids.get(&minted) {
                        return Err(ChorusError::new(format!(
                            "line {}: minted id '{minted}' already in use ('{holder}' vs '{name}')",
                            ln + 1
                        )));
                    }
                    entity_names.insert(name.clone());
                    minted_ids.insert(minted, name);
                }
            }
            "prototypes" => {
                let (key, proto) = parse_prototype_line(t, ln + 1, dict)?;
                // duplicates are a load error, the same uniform rule as
                // entities/endings (formerly QUIRK(Q5): a silent
                // last-write-wins — the later insert overwrote)
                if prototypes.contains_key(&key) {
                    return Err(ChorusError::new(format!(
                        "line {}: duplicate prototype '{key}'",
                        ln + 1
                    )));
                }
                prototypes.insert(key, proto);
            }
            "decks" => {
                decks.push(parse_deck_line(t, ln + 1)?);
            }
            "triggers" => {
                triggers.push(parse_trigger_line(t, ln + 1, dict)?);
            }
            "functions" => {
                functions.push(parse_function_line(t, ln + 1)?);
            }
            "endings" => {
                let (id, ending) = parse_ending_line(t, ln + 1)?;
                if endings.contains_key(&id) {
                    return Err(ChorusError::new(format!(
                        "line {}: duplicate ending id '{id}'",
                        ln + 1
                    )));
                }
                endings.insert(id, ending);
            }
            "templates" => {
                let Some(m) = template_re().captures(t) else {
                    return Err(ChorusError::new(format!(
                        "line {}: templates expect  VERB says \"...\"",
                        ln + 1
                    )));
                };
                // duplicates are a load error, the same uniform rule as
                // entities/endings (formerly QUIRK(Q5): a silent overwrite)
                let key = grp_text(&m, 1).to_string();
                if templates.contains_key(&key) {
                    return Err(ChorusError::new(format!(
                        "line {}: duplicate template '{key}'",
                        ln + 1
                    )));
                }
                templates.insert(key, grp_text(&m, 2).to_string());
            }
            "verbs" => {
                // verb header: indent 2, UPPER_NAME:
                if indent == 2 && verb_header_re().is_match(t) {
                    let name = t[..t.len() - 1].to_string();
                    if !dict.is_verb(&name) {
                        return Err(ChorusError::new(format!(
                            "line {}: verb '{name}' is not registered in the dictionary",
                            ln + 1
                        )));
                    }
                    // a repeated verb header is a load error, the same
                    // uniform rule as entities/endings (formerly
                    // QUIRK(Q5): the block silently RESET — a fresh empty
                    // `VerbContent` replaced the old)
                    if verbs.contains_key(&name) {
                        return Err(ChorusError::new(format!(
                            "line {}: duplicate verb declaration '{name}'",
                            ln + 1
                        )));
                    }
                    verbs.insert(name.clone(), VerbContent::default());
                    verb_name = Some(name);
                    in_recipes = false;
                    continue;
                }
                let Some(vn) = verb_name.clone() else {
                    return Err(ChorusError::new(format!(
                        "line {}: expected a verb header inside 'verbs:'",
                        ln + 1
                    )));
                };
                // the header inserted it
                let Some(v) = verbs.get_mut(&vn) else {
                    unreachable!("verb header inserted '{vn}'");
                };
                if indent == 4 && recipes_re().is_match(t) {
                    in_recipes = true;
                    continue;
                }
                if !in_recipes && indent >= 4 {
                    let Some(m) = slot_role_re().captures(t) else {
                        return Err(ChorusError::new(format!(
                            "line {}: expected 'role: count query', 'duration: X', or 'recipes:'",
                            ln + 1
                        )));
                    };
                    if grp_text(&m, 1) == "duration" {
                        v.duration = parse_duration(js_trim(grp_text(&m, 2)))?;
                        continue;
                    }
                    let slot = parse_slot_line(grp_text(&m, 1), grp_text(&m, 2), ln + 1)?;
                    v.slots.push(slot);
                } else if in_recipes && indent >= 6 {
                    let recipe = parse_recipe_line(t, ln + 1, dict)?;
                    v.recipes.push(recipe);
                } else {
                    return Err(ChorusError::new(format!(
                        "line {}: unexpected line in verb {vn}: '{t}'",
                        ln + 1
                    )));
                }
            }
            // unreachable: section only ever set from SECTION_RE's alternation
            _ => unreachable!("unknown section '{sec}'"),
        }
    }

    // §9.14: a name is one card — declared both ways, every SPAWN of the
    // prototype would collide with the living entity (§9.19); fail at load.
    // `Object.keys` semantics — JS fronts integer-like
    // keys, so the walk goes through the JS key order (prototype names are
    // `\S+`, so "5 is idea" is a real key shape here).
    for name in ordinary_own_property_keys(&prototypes) {
        if entity_names.contains(&name) {
            return Err(ChorusError::new(format!(
                "'{name}' is declared both as an entity and a prototype — spawns of it could never mint"
            )));
        }
    }

    Ok(Script {
        meta,
        content: ContentRaw {
            seed: None, // rides in via meta (compile_script merges it)
            verbs,
            triggers: Some(triggers),
            prototypes: Some(prototypes),
            templates: Some(templates),
            endings: Some(endings),
            functions: Some(functions),
            decks: Some(decks),
        },
        entities,
        relations,
    })
}

// ---------------------------------------------------------------------------
// entity lines
// ---------------------------------------------------------------------------

/// §5.1: reserved words cannot be names — as a
/// declaration they would make every unquoted ref in clauses parse as the
/// keyword, so fail at load. (Non-lexeme names like `X-Y` are legal but
/// quota every ref — linted, not errored: §9.42's unsafe-name corner.)
fn check_name(name: &str, ln: usize, dict: &Dictionary, what: &str) -> Result<()> {
    if dict.reserved.contains(name) {
        return Err(ChorusError::new(format!(
            "line {ln}: {what} '{name}' is a reserved word — rename it (§5.1)"
        )));
    }
    Ok(())
}

/// `ln` is the 1-BASED line number (the caller adds one).
fn parse_entity_line(
    t: &str,
    ln: usize,
    dict: &Dictionary,
    entities: &mut Vec<EntitySpec>,
    relations: &mut Vec<RelationSpec>,
) -> Result<()> {
    let Some(m) = entity_re().captures(t) else {
        return Err(ChorusError::new(format!(
            "line {ln}: entities expect  NAME is KIND [with ...] [at PLACE]"
        )));
    };
    let name = grp_text(&m, 1);
    let second = grp_text(&m, 2);
    let rest0 = grp_text(&m, 3);
    // relation form: A is RELTYPE of B
    if dict.relation_types.contains_key(second) {
        let Some(rel) = of_re().captures(rest0) else {
            return Err(ChorusError::new(format!(
                "line {ln}: relations expect  A is TYPE of B"
            )));
        };
        let b = grp_text(&rel, 1);
        check_name(name, ln, dict, "relation endpoint")?;
        check_name(b, ln, dict, "relation endpoint")?;
        relations.push(RelationSpec {
            a: name.to_string(),
            type_: second.to_string(),
            b: b.to_string(),
        });
        return Ok(());
    }
    check_name(name, ln, dict, "entity name")?;
    if !KINDS.contains(&second) {
        return Err(ChorusError::new(format!(
            "line {ln}: unknown kind '{second}' (expected one of {})",
            KINDS.join(", ")
        )));
    }
    let mut rest: &str = rest0;
    // §9.38: the `stackable` keyword sits before `with`/`at`/`in` clauses
    let mut stackable = false;
    if let Some(sm) = stackable_re().find(rest) {
        stackable = true;
        rest = &rest[sm.end()..];
    }
    // trailing clauses, in either order: "with specs" and "at/in PLACE" —
    // strip whichever matches last
    let mut container_name: Option<String> = None;
    let mut aspects: Aspects = IndexMap::new();
    loop {
        let at_in = at_in_re().captures(rest);
        let with_m = with_re().captures(rest);
        // the earlier match wins (whichever clause starts first);
        // byte offsets order identically to the JS UTF-16 unit offsets
        if let Some(a) = &at_in {
            let a_start = grp0_start(a);
            let with_is_first = with_m
                .as_ref()
                .is_some_and(|w| grp0_start(w) >= a_start);
            if !with_is_first {
                container_name = Some(grp_text(a, 1).to_string());
                rest = &rest[..a_start];
                continue;
            }
        }
        if let Some(w) = &with_m {
            let mut pairs: IndexMap<String, i64> = IndexMap::new();
            for spec in grp_text(w, 1).split(',') {
                let Some(sm) = aspect_spec_re().captures(spec) else {
                    return Err(ChorusError::new(format!(
                        "line {ln}: bad aspect spec '{}' (expected aspect:level)",
                        js_trim(spec)
                    )));
                };
                pairs.insert(
                    grp_text(&sm, 1).to_string(),
                    parse_i64_like_f64(grp_text(&sm, 2)),
                );
            }
            aspects = aspects_from_pairs(&pairs);
            rest = &rest[..grp0_start(w)];
            continue;
        }
        break;
    }
    if !js_trim(rest).is_empty() {
        return Err(ChorusError::new(format!(
            "line {ln}: could not parse entity line '{t}'"
        )));
    }
    entities.push(EntitySpec {
        name: name.to_string(),
        kind: second.to_string(),
        stackable,
        aspects,
        container_name,
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// prototype lines
// ---------------------------------------------------------------------------

/// `ln` is the 1-BASED line number.
fn parse_prototype_line(t: &str, ln: usize, dict: &Dictionary) -> Result<(String, PrototypeDef)> {
    let Some(m) = prototype_re().captures(t) else {
        return Err(ChorusError::new(format!(
            "line {ln}: prototypes expect  NAME is KIND [stackable] [deriving ...] [with ...]"
        )));
    };
    let name = grp_text(&m, 1);
    check_name(name, ln, dict, "prototype name")?;
    let kind = grp_text(&m, 2);
    if !KINDS.contains(&kind) {
        return Err(ChorusError::new(format!("line {ln}: unknown kind '{kind}'")));
    }
    let mut proto = PrototypeDef {
        kind: kind.to_string(),
        ..PrototypeDef::default()
    };
    let mut rest: &str = grp_text(&m, 3);
    // §9.38: the `stackable` keyword sits before `deriving`/`with` clauses
    if let Some(sm) = stackable_re().find(rest) {
        proto.stackable = true;
        rest = &rest[sm.end()..];
    }
    let mut aspects_done = false;
    let mut derive_done = false;
    loop {
        let with_m = with_re().captures(rest);
        let deriving_m = deriving_re().captures(rest);
        if let Some(w) = &with_m {
            let w_start = grp0_start(w);
            let deriving_is_first = deriving_m
                .as_ref()
                .is_some_and(|d| grp0_start(d) >= w_start);
            if !deriving_is_first {
                if aspects_done {
                    return Err(ChorusError::new(format!(
                        "line {ln}: duplicate 'with' clause"
                    )));
                }
                aspects_done = true;
                let mut pairs: IndexMap<String, i64> = IndexMap::new();
                for spec in grp_text(w, 1).split(',') {
                    let Some(sm) = aspect_spec_re().captures(spec) else {
                        return Err(ChorusError::new(format!(
                            "line {ln}: bad aspect spec '{}'",
                            js_trim(spec)
                        )));
                    };
                    pairs.insert(
                        grp_text(&sm, 1).to_string(),
                        parse_i64_like_f64(grp_text(&sm, 2)),
                    );
                }
                proto.aspects = Some(aspects_from_pairs(&pairs));
                rest = &rest[..w_start];
                continue;
            }
        }
        if let Some(dm_all) = &deriving_m {
            if derive_done {
                return Err(ChorusError::new(format!(
                    "line {ln}: duplicate 'deriving' clause"
                )));
            }
            derive_done = true;
            rest = &rest[..grp0_start(dm_all)];
            let mut derive = PrototypeDerive::default();
            for item in grp_text(dm_all, 1).split(',') {
                if name_from_re().is_match(item) {
                    derive.name_from = Some("object".to_string());
                    continue;
                }
                let Some(dm) = derive_re().captures(item) else {
                    return Err(ChorusError::new(format!(
                        "line {ln}: bad derive '{}' (expected 'name from object' or 'field from object.field-2')",
                        js_trim(item)
                    )));
                };
                let aspects = derive.aspects.get_or_insert_with(IndexMap::new);
                aspects.insert(
                    grp_text(&dm, 1).to_string(),
                    DeriveAspect {
                        from_object_aspect: grp_text(&dm, 2).to_string(),
                        minus: grp_opt(&dm, 3).map(parse_i64_like_f64).unwrap_or(0),
                    },
                );
            }
            proto.derive = Some(derive);
            continue;
        }
        break;
    }
    if !js_trim(rest).is_empty() {
        return Err(ChorusError::new(format!(
            "line {ln}: could not parse prototype line '{t}'"
        )));
    }
    Ok((name.to_string(), proto))
}

// ---------------------------------------------------------------------------
// trigger lines
// ---------------------------------------------------------------------------

/// `ln` is the 1-BASED line number. (`_dict` is never read — the trigger
/// grammar needs no dictionary.)
fn parse_trigger_line(t: &str, ln: usize, _dict: &Dictionary) -> Result<TriggerDef> {
    // expiry form: ID on expire of NAME -> OUTCOMES (§6.6, §9.32)
    if let Some(exp) = expire_re().captures(t) {
        return Ok(TriggerDef::Expire(ExpireTriggerDef {
            id: grp_text(&exp, 1).to_string(),
            on_expire_of: grp_text(&exp, 2).to_string(),
            out: vec![grp_text(&exp, 3).to_string()],
        }));
    }
    let Some(m) = trigger_re().captures(t) else {
        return Err(ChorusError::new(format!(
            "line {ln}: triggers expect  ID [once] at (hour|halfhour) N -> OUTCOMES  or  ID on expire of NAME -> OUTCOMES"
        )));
    };
    let n = parse_i64_like_f64(grp_text(&m, 4));
    if grp_text(&m, 3) == "hour" && n > 23 {
        return Err(ChorusError::new(format!(
            "line {ln}: trigger hour must be 0..23"
        )));
    }
    if grp_text(&m, 3) == "halfhour" && n > 47 {
        return Err(ChorusError::new(format!(
            "line {ln}: trigger halfhour must be 0..47"
        )));
    }
    Ok(TriggerDef::Slot(SlotTriggerDef {
        id: grp_text(&m, 1).to_string(),
        slot: if grp_text(&m, 3) == "hour" { n * 2 } else { n },
        once: Some(grp_opt(&m, 2).is_some()),
        out: vec![grp_text(&m, 5).to_string()],
    }))
}

// ---------------------------------------------------------------------------
// function lines (§9.40, §10)
// ---------------------------------------------------------------------------

/// `NAME(param, …) -> OUTCOMES` (or the bare form
/// `NAME param …`). Grammar only: name/param shape, op collisions and
/// duplicates are `compile_content`'s. `ln` is the 1-BASED line number.
fn parse_function_line(t: &str, ln: usize) -> Result<FunctionDef> {
    if let Some(m) = fn_paren_re().captures(t) {
        let params_raw = js_trim(grp_text(&m, 2));
        let params: Vec<String> = if params_raw.is_empty() {
            Vec::new()
        } else {
            params_raw.split(',').map(|s| js_trim(s).to_string()).collect()
        };
        return Ok(FunctionDef {
            name: grp_text(&m, 1).to_string(),
            params,
            body: js_trim(grp_text(&m, 3)).to_string(),
        });
    }
    if let Some(m) = fn_arrow_re().captures(t) {
        return Ok(FunctionDef {
            name: grp_text(&m, 1).to_string(),
            params: Vec::new(),
            body: js_trim(grp_text(&m, 2)).to_string(),
        });
    }
    if let Some(m) = fn_bare_re().captures(t) {
        return Ok(FunctionDef {
            name: grp_text(&m, 1).to_string(),
            params: split_js_ws(grp_text(&m, 2))
                .into_iter()
                .map(str::to_string)
                .collect(),
            body: js_trim(grp_text(&m, 3)).to_string(),
        });
    }
    Err(ChorusError::new(format!(
        "line {ln}: functions expect  NAME(param, …) -> OUTCOMES  or  NAME param … -> OUTCOMES"
    )))
}

// ---------------------------------------------------------------------------
// deck lines (§9.42, §10)
// ---------------------------------------------------------------------------

/// `ID spec NAME, … [defaultcard NAME] [reset]`.
/// Grammar only: entry/defaultcard resolution against prototypes + entities,
/// duplicate ids and the permutation are `compile_content`'s (§9.42). The
/// trailing clauses may sit in either order (stripped from the end until
/// neither matches); `defaultcard`/`reset` are keywords, not spec entries.
/// `ln` is the 1-BASED line number.
fn parse_deck_line(t: &str, ln: usize) -> Result<DeckRaw> {
    let Some(m) = deck_re().captures(t) else {
        return Err(ChorusError::new(format!(
            "line {ln}: decks expect  ID spec NAME, … [defaultcard NAME] [reset]"
        )));
    };
    let id = grp_text(&m, 1);
    let mut rest: &str = js_trim(grp_text(&m, 2));
    let mut defaultcard: Option<String> = None;
    let mut reset = false;
    loop {
        if let Some(dc) = defaultcard_re().captures(rest) {
            defaultcard = Some(grp_text(&dc, 1).to_string());
            rest = js_trim(&rest[..grp0_start(&dc)]);
            continue;
        }
        if let Some(rm) = reset_re().captures(rest) {
            reset = true;
            rest = js_trim(&rest[..grp0_start(&rm)]);
            continue;
        }
        break;
    }
    let spec: Vec<String> = rest
        .split(',')
        .map(js_trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if spec.is_empty() {
        return Err(ChorusError::new(format!(
            "line {ln}: deck {id}: the spec is empty"
        )));
    }
    Ok(DeckRaw {
        id: id.to_string(),
        spec,
        defaultcard,
        reset,
    })
}

// ---------------------------------------------------------------------------
// ending lines (§9.35, §10)
// ---------------------------------------------------------------------------

/// `ID: "Title" (flavour)` — flavour optional,
/// default melancholy (resolved at compile). `ln` is the 1-BASED line number.
fn parse_ending_line(t: &str, ln: usize) -> Result<(String, EndingDef)> {
    let Some(m) = ending_re().captures(t) else {
        return Err(ChorusError::new(format!(
            "line {ln}: endings expect  ID: \"Title\" (flavour)  — flavour optional, default melancholy"
        )));
    };
    Ok((
        grp_text(&m, 1).to_string(),
        EndingDef {
            title: grp_text(&m, 2).to_string(),
            flavour: grp_opt(&m, 3).map(str::to_string),
        },
    ))
}

// ---------------------------------------------------------------------------
// slots & recipes
// ---------------------------------------------------------------------------

/// `ln` is the 1-BASED line number.
fn parse_slot_line(role: &str, rest: &str, ln: usize) -> Result<SlotDef> {
    // §4.3's six-role list + §10's "failing loudly at load time (… bad
    // grammar)": a slot key outside the role vocabulary is a load error —
    // formerly QUIRK(Q4), the role was cast unvalidated and a typo'd slot
    // compiled only to silently never match.
    if Role::from_lower(role).is_none() {
        return Err(ChorusError::new(format!(
            "line {ln}: bad slot role '{role}' (expected one of {})",
            ROLE_NAMES.join(", ")
        )));
    };
    let trimmed = js_trim(rest);
    let Some(m) = slot_line_re().captures(trimmed) else {
        return Err(ChorusError::new(format!(
            "line {ln}: bad slot '{role}: {rest}'"
        )));
    };
    let count_spec = grp_text(&m, 1);
    let Some(cm) = count_re().captures(count_spec) else {
        return Err(ChorusError::new(format!(
            "line {ln}: bad count '{count_spec}' (expected 1, 1..2, 2.., 0..1)"
        )));
    };
    let min = parse_i64_like_f64(grp_text(&cm, 1));
    // max: the upper-bound group when present; a trailing `..` means
    // unbounded (no upper bound); no `..` pins max = min.
    let max = match grp_opt(&cm, 2) {
        Some(g) => Some(parse_i64_like_f64(g)),
        None if count_spec.ends_with("..") => None, // unbounded
        None => Some(min),
    };
    Ok(SlotDef {
        // validated above via Role::from_lower — every slot role binds
        role: role.to_string(),
        min,
        max,
        query: grp_opt(&m, 2).map(str::to_string),
        shown: None,
    })
}

/// `ln` is the 1-BASED line number. (`_dict` is never read.)
fn parse_recipe_line(t: &str, ln: usize, _dict: &Dictionary) -> Result<RecipeDef> {
    // the ASCII `->` needle aligns byte and
    // UTF-16 unit offsets, so the byte slices carry the same text
    let Some(arrow) = t.find("->") else {
        return Err(ChorusError::new(format!(
            "line {ln}: recipes expect  [id] [hidden|shown] [hint \"...\"] [when COND] -> OUTCOMES"
        )));
    };
    let lhs = js_trim(&t[..arrow]);
    let rhs = js_trim(&t[arrow + 2..]);
    let mut def = RecipeDef {
        out: vec![rhs.to_string()],
        ..RecipeDef::default()
    };
    if default_re().is_match(lhs) {
        return Ok(def);
    }
    let tokens = split_js_ws(lhs);
    let mut i = 0usize;
    if let Some(tok0) = tokens.first() {
        if recipe_id_re().is_match(tok0) && *tok0 != "when" && *tok0 != "hidden" && *tok0 != "shown"
        {
            def.id = Some((*tok0).to_string());
            i += 1;
        }
    }
    if let Some(tok) = tokens.get(i) {
        if *tok == "hidden" || *tok == "shown" {
            def.shown = Some(*tok == "shown");
            i += 1;
        }
    }
    if let Some(hint) = hint_re().captures(lhs) {
        def.hint = Some(grp_text(&hint, 1).to_string());
        // the hint is extracted positionally and consumes nothing from
        // `tokens` (the token cursor does not advance past it).
    }
    if let Some(wm) = when_re().captures(lhs) {
        def.when = Some(grp_text(&wm, 1).to_string());
    }
    let _ = i;
    Ok(def)
}

// ---------------------------------------------------------------------------
// compile: script → runnable game
// ---------------------------------------------------------------------------

/// `compile_script(text, dict)` → `{ content, state, meta }`.
pub fn compile_script(text: &str, dict: &Dictionary) -> Result<CompiledScript> {
    let script = parse_script(text, dict)?;
    // §9.41: the meta `seed:` rides into the compiled content — every die is
    // a pure function of (seed, counter), so the sequence is fixed at load.
    // The deck permutations (§9.42) are keyed on the same seed. Declared
    // entities ride along so deck spec entries may name plain entity lines
    // too. The entities go in ONCE (with
    // their stackability flags) — compile_content builds the §9.38 sets
    // from them.
    let declared: Vec<DeclaredEntity> = script
        .entities
        .iter()
        .map(|e| DeclaredEntity {
            name: e.name.clone(),
            kind: e.kind.clone(),
            stackable: e.stackable,
        })
        .collect();
    let raw = ContentRaw {
        seed: script.meta.seed,
        ..script.content
    };
    let content = compile_content(&raw, dict, &declared)?;
    let now = match script.meta.start {
        Some(start) => abs_min(start),
        None => 0,
    };
    let mut state = new_state(now);

    let id_of = |name: &str| format!("s_{}", name.to_lowercase());
    for e in &script.entities {
        add_entity(
            &mut state,
            Entity {
                id: id_of(&e.name),
                kind: e.kind.clone(),
                name: e.name.clone(),
                aspects: e.aspects.clone(),
                attrs: Attrs::new(),
                born_action: Some(0),
                dead_action: None,
                stackable: e.stackable,
            },
        )?;
    }
    for e in &script.entities {
        if let Some(container_name) = &e.container_name {
            // resolve the container FIRST (shared borrow ends), so the attrs
            // write below can take the exclusive borrow of state.entities
            let container_id = state
                .entities
                .get(&id_of(container_name))
                .map(|c| c.id.clone());
            // the entity was added in the loop above
            let Some(self_e) = state.entities.get_mut(&id_of(&e.name)) else {
                unreachable!("entity '{}' was added above", e.name);
            };
            if let Some(container_id) = container_id {
                self_e
                    .attrs
                    .insert("container".to_string(), AttrValue::Str(container_id));
            } else if e.kind == "place" {
                // a zone: a grouping with no card (§10)
                self_e
                    .attrs
                    .insert("zone".to_string(), AttrValue::Str(container_name.clone()));
            } else {
                return Err(ChorusError::new(format!(
                    "entity {}: unknown container '{container_name}'",
                    e.name
                )));
            }
        }
    }
    for rel in &script.relations {
        let a = state.entities.get(&id_of(&rel.a));
        let b = state.entities.get(&id_of(&rel.b));
        let (Some(a), Some(b)) = (a, b) else {
            return Err(ChorusError::new(format!(
                "relation {} is {} of {}: unknown entity",
                rel.a, rel.type_, rel.b
            )));
        };
        let def = dict.relation_def(&rel.type_)?;
        let key = relation_key(&a.id, &b.id, &rel.type_, def.symmetric);
        state.relations.insert(
            key,
            Relation {
                a: a.id.clone(),
                b: b.id.clone(),
                type_: rel.type_.clone(),
                value: 1,
                born_action: 0,
                dead_action: None,
            },
        );
    }
    // §11: the front door defaults actors to `player:` — resolve it against
    // the declared world here, at load, instead of at the first typed line
    if let Some(player) = &script.meta.player {
        let p = state
            .by_name
            .get(player)
            .and_then(|pool| pool.first())
            .and_then(|id| state.entities.get(id));
        let Some(p) = p else {
            return Err(ChorusError::new(format!(
                "player '{player}' is not a declared entity"
            )));
        };
        if p.kind != "character" {
            return Err(ChorusError::new(format!(
                "player '{player}' must be a character (got {})",
                p.kind
            )));
        }
    }
    Ok(CompiledScript {
        content,
        state,
        meta: script.meta,
    })
}

// ---------------------------------------------------------------------------
// load-time lint (§10's "fail loudly", warn-loudly sibling)
// ---------------------------------------------------------------------------

/// The six role names, in their canonical order.
const ROLE_NAMES: [&str; 6] = ["actors", "object", "with", "at", "about", "to"];

/// Warnings for content that compiles but is
/// probably not what the author meant. `compile_script` still owns errors;
/// this never throws for a script that compiles, and returns one
/// human-readable string per finding. Empty for a clean script.
pub fn lint_script(text: &str, dict: &Dictionary) -> Result<Vec<String>> {
    let script = parse_script(text, dict)?;
    compile_script(text, dict)?; // lints run on scripts that already compile
    let mut warnings: Vec<String> = Vec::new();
    let verbs = &script.content.verbs;
    // raw default recipes carry no id — mirror compile_content's minting.
    // Q1 unified: the SAME 0-based mint on both sides (`.recipe<i>`,
    // §6.4) — the mirror's old 1-based base is retired.
    let recipe_id = |verb: &str, r: &RecipeDef, i: usize| {
        mint_recipe_id(&verb.to_lowercase(), r.when.as_deref(), i)
    };

    // a configured verb nobody can ever succeed with
    for (name, verb) in verbs {
        // verb keys are
        // `[A-Z][A-Z0-9_]*` (never integer-like), so insertion order IS the
        // canonical key order
        if !verb.slots.is_empty() && verb.recipes.is_empty() {
            warnings.push(format!(
                "verb {name}: declares slots but no recipes — every action rejects at selection (§6.6)"
            ));
        }
    }

    // dead recipes: after a catch-all, shadowed by an identical when, or a
    // count(role) bound the slot's cardinality can never satisfy
    for (name, verb) in verbs {
        // a repeated role keeps its original slot but last-wins on lookup
        let mut slot_of: IndexMap<&str, &SlotDef> = IndexMap::new();
        for s in &verb.slots {
            slot_of.insert(s.role.as_str(), s);
        }
        let mut catch_all: Option<String> = None;
        let mut whens: IndexMap<String, String> = IndexMap::new();
        for (i, r) in verb.recipes.iter().enumerate() {
            let id = r
                .id
                .clone()
                .unwrap_or_else(|| recipe_id(name, r, i));
            if let Some(ca) = &catch_all {
                warnings.push(format!(
                    "recipe {id} ({name}): unreachable — {ca} already matches everything (first match wins, §6.4)"
                ));
            }
            let Some(when) = &r.when else {
                if catch_all.is_none() {
                    catch_all = Some(id);
                }
                continue;
            };
            // JS whitespace runs collapse to one space, then JS-trim
            // (`replace(/\s+/g, ' ').trim()` semantics)
            let norm = when
                .split(js_char_is_ws)
                .filter(|p| !p.is_empty())
                .collect::<Vec<&str>>()
                .join(" ");
            if let Some(prior) = whens.get(&norm) {
                warnings.push(format!(
                    "recipe {id} ({name}): shadowed by {prior} — identical when, it can never fire (§6.4)"
                ));
            } else {
                whens.insert(norm, id.clone());
            }
            for cap in count_gate_re().captures_iter(when) {
                let role = grp_text(&cap, 1);
                let Some(slot) = slot_of.get(role) else {
                    continue;
                };
                let k = parse_i64_like_f64(grp_text(&cap, 3));
                let op = grp_text(&cap, 2);
                // slot.max: None = unbounded — no finite count is ever over
                // an absent bound, so the over-max arms are false
                let dead = match op {
                    "=" => k < slot.min || slot.max.is_some_and(|mx| k > mx),
                    ">" => slot.max.is_some_and(|mx| k >= mx),
                    ">=" => slot.max.is_some_and(|mx| k > mx),
                    "<" => k <= slot.min,
                    "<=" => k < slot.min,
                    _ => unreachable!("count-gate alternation only yields >=|<=|>|<|=|!="),
                };
                if dead {
                    let range = match slot.max {
                        Some(mx) => format!("{}..{mx}", slot.min),
                        None => format!("{}..∞", slot.min),
                    };
                    warnings.push(format!(
                        "recipe {id} ({name}): count({role}) {op} {k} can never hold — the slot takes {range} (§6.1)"
                    ));
                }
            }
        }
    }

    // template tokens: {actors} {object} {with} {at} {about} {to} {roll} — a
    // typo renders literally at render time (§5.3); function bodies also
    // interpolate their params (§9.40)
    let known: Vec<&str> = ROLE_NAMES.iter().copied().chain(["roll"]).collect();
    let scan_tokens = |warnings: &mut Vec<String>, where_: &str, s: &str, extra: &[&str]| {
        for cap in template_token_re().captures_iter(s) {
            let tok = grp_text(&cap, 1);
            if !known.contains(&tok) && !extra.contains(&tok) {
                warnings.push(format!(
                    "{where_}: unknown template token {{{tok}}} — roles are {{actors}} {{object}} {{with}} {{at}} {{about}} {{to}}, plus {{roll}} (§5.3)"
                ));
            }
        }
    };
    for (name, verb) in verbs {
        for (i, r) in verb.recipes.iter().enumerate() {
            let id = r.id.clone().unwrap_or_else(|| recipe_id(name, r, i));
            let out0 = r.out.first().map(String::as_str).unwrap_or("");
            scan_tokens(
                &mut warnings,
                &format!("recipe {id}"),
                out0,
                &[],
            );
        }
    }
    if let Some(triggers) = &script.content.triggers {
        for t in triggers {
            let out0 = t.out().first().map(String::as_str).unwrap_or("");
            scan_tokens(&mut warnings, &format!("trigger {}", t.id()), out0, &[]);
        }
    }
    if let Some(functions) = &script.content.functions {
        for f in functions {
            let extra: Vec<&str> = f.params.iter().map(String::as_str).collect();
            scan_tokens(&mut warnings, &format!("function {}", f.name), &f.body, &extra);
        }
    }

    // the §10 livelock rule: deck entries should be stackable (or consumed
    // and renamed on the same step) — served twice while a copy lives, the
    // second spawn name-collides and rejects the step (§9.19)
    let declared: Vec<DeclaredEntity> = script
        .entities
        .iter()
        .map(|e| DeclaredEntity {
            name: e.name.clone(),
            kind: e.kind.clone(),
            stackable: e.stackable,
        })
        .collect();
    let (stackable_names, stackable_kinds) =
        collect_stackable(&declared, script.content.prototypes.as_ref());
    if let Some(decks) = &script.content.decks {
        for d in decks {
            let mut entries: Vec<&String> = d.spec.iter().collect();
            if let Some(dc) = &d.defaultcard {
                entries.push(dc);
            }
            for entry in entries {
                // prototype kind first, then the
                // declared entity line's kind
                let kind = script
                    .content
                    .prototypes
                    .as_ref()
                    .and_then(|ps| ps.get(entry))
                    .map(|p| p.kind.as_str())
                    .or_else(|| {
                        script
                            .entities
                            .iter()
                            .find(|e| &e.name == entry)
                            .map(|e| e.kind.as_str())
                    });
                if !stackable_names.contains(entry)
                    && !kind.is_some_and(|k| stackable_kinds.contains(k))
                {
                    warnings.push(format!(
                        "deck {}: entry {entry} is not stackable — served again while a copy lives, the spawn collides (§10 livelock rule)",
                        d.id
                    ));
                }
            }
        }
    }

    // aspect reads with no writer anywhere — a typo'd aspect silently never
    // matches (aspects are an open vocabulary, so this is a warning, not an
    // error)
    let mut reads: IndexSet<String> = IndexSet::new();
    let mut writes: IndexSet<String> = IndexSet::new();
    fn add_write(writes: &mut IndexSet<String>, path: &str) {
        writes.insert(path.to_string());
        // every dotted prefix is also written ('a.b.c' writes 'a.b' and 'a')
        let mut dot = path.find('.');
        while let Some(d) = dot {
            writes.insert(path[..d].to_string());
            dot = path[d + 1..].find('.').map(|next| d + 1 + next);
        }
    }
    fn walk_aspects(writes: &mut IndexSet<String>, a: &Aspects, prefix: &str) {
        for (k, v) in a {
            let path = if prefix.is_empty() {
                k.clone()
            } else {
                format!("{prefix}.{k}")
            };
            add_write(writes, &path);
            // the only non-number shape is the one-level sub-map (Q13), whose
            // leaves are all numbers, so no further recursion is reachable
            if let crate::core::types::AspectVal::Sub(sub) = v {
                for sk in sub.keys() {
                    let sub_path = format!("{path}.{sk}");
                    add_write(writes, &sub_path);
                }
            }
        }
    }
    for e in &script.entities {
        walk_aspects(&mut writes, &e.aspects, "");
    }
    if let Some(prototypes) = &script.content.prototypes {
        for p in prototypes.values() {
            if let Some(aspects) = &p.aspects {
                walk_aspects(&mut writes, aspects, "");
            }
            // derive writes the declared field and reads the object's aspect (§6.4)
            if let Some(daspects) = p.derive.as_ref().and_then(|d| d.aspects.as_ref()) {
                for (d_key, d) in daspects {
                    add_write(&mut writes, d_key);
                    reads.insert(d.from_object_aspect.clone());
                }
            }
        }
    }
    fn scan_reads(reads: &mut IndexSet<String>, s: &str) {
        // two syntaxes read aspects: query terms (`aspect:secret`) and
        // predicate paths (`object.aspect.secret >= 3`) — §4.5 vs §6.3
        for cap in read_aspect_re().captures_iter(s) {
            reads.insert(grp_text(&cap, 1).to_string());
        }
        for cap in read_path_re().captures_iter(s) {
            reads.insert(grp_text(&cap, 1).to_string());
        }
    }
    fn scan_writes(writes: &mut IndexSet<String>, s: &str) {
        for cap in write_set_re().captures_iter(s) {
            add_write(writes, grp_text(&cap, 1));
        }
    }
    for verb in verbs.values() {
        for slot in &verb.slots {
            if let Some(query) = &slot.query {
                scan_reads(&mut reads, query);
            }
        }
        for r in &verb.recipes {
            if let Some(w) = &r.when {
                scan_reads(&mut reads, w);
            }
            let out0 = r.out.first().map(String::as_str).unwrap_or("");
            scan_writes(&mut writes, out0);
        }
    }
    if let Some(triggers) = &script.content.triggers {
        for t in triggers {
            let out0 = t.out().first().map(String::as_str).unwrap_or("");
            scan_writes(&mut writes, out0);
        }
    }
    if let Some(functions) = &script.content.functions {
        for f in functions {
            scan_writes(&mut writes, &f.body);
        }
    }
    for r in &reads {
        if !writes.contains(r) {
            warnings.push(format!(
                "aspect '{r}' is queried but never declared on any entity or prototype — check the spelling"
            ));
        }
    }

    // non-lexeme names are legal (§9.42's unsafe-name corner) but every ref
    // to them must be quoted — say so at load
    for e in &script.entities {
        if !name_lexeme_re().is_match(&e.name) {
            warnings.push(format!(
                "entity '{}': not a name lexeme — every ref to it must be quoted (§5.1, §9.42)",
                e.name
            ));
        }
    }

    Ok(warnings)
}
