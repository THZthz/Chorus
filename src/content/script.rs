//! Script compiler. A game script is a piece of
//! text: meta lines, section blocks, and sentence-like declarations
//! (spec: "sentences + small blocks"). [`parse_script`] is the
//! indentation-sensitive line-segment parser; [`compile_script`] turns a
//! script into runnable game content + the initial State; [`lint_script`] is
//! the §10 warn-loudly sibling (it never throws for a script that compiles).
//!
//! Regex-domain freeze: every `(.+)`-style pattern in
//! this module runs on `split('\n')` + [`dsl_trim`]-trimmed SINGLE-LINE
//! text, so `.` never sees `\r`/U+2028/U+2029 (line terminators are split
//! away or trimmed by the frozen whitespace set); these patterns must NOT
//! be reused on multi-line input.
//!
//! Whitespace discipline (Q8): every `\s`/`\S` in these patterns is the
//! explicit frozen class ([`DSL_WS_CLASS`]/[`DSL_NON_WS_CLASS`]) and every
//! `trim()`/`trimStart()` is [`dsl_trim`]/a [`is_dsl_ws_char`] take-while —
//! the frozen set and `char::is_whitespace` disagree on U+0085/U+FEFF
//! exactly reversely.

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use crate::content::content::{
    collect_stackable, compile_content, mint_recipe_id, CompiledContent, CompleteTriggerDef,
    ContentRaw, DeckRaw, DeclaredEntity, DeriveAspect, EndingDef, ExpireTriggerDef, FunctionDef,
    PrototypeDef, PrototypeDerive, RecipeDef, SlotDef, SlotTriggerDef, TriggerDef, VerbContent,
};
use crate::core::clock::{abs_min, parse_clock, parse_duration, Clock};
use crate::core::dict::{Dictionary, KINDS};
use crate::core::jsonw::canonical_key_order;
use crate::core::num_parse::parse_authored_int;
use crate::core::predicate::{parse_predicate, Pred, ValExpr};
use crate::core::query::{aspects_from_pairs, CmpOp};
use crate::core::regex_util::{grp0_start, grp_opt, grp_text, static_regex};
use crate::core::roles::Role;
use crate::core::text::{dsl_trim, is_dsl_ws_char, DSL_NON_WS_CLASS, DSL_WS_CLASS};
use crate::core::types::{
    add_entity, new_state, relation_key, Aspects, AttrValue, Attrs, Entity, Relation, State,
};
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
// `\s` → DSL_WS_CLASS, `\S` → DSL_NON_WS_CLASS, `\b` →
// `(?-u:\b)` (the ASCII word boundary).
// ---------------------------------------------------------------------------

/// `/^(script|player|goal|start|seed):\s*(.+)$`
fn meta_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        // ANY lowercase identifier keys a meta line — the match arms below
        // dispatch on it, so an unknown key is RECOGNIZED as a meta attempt
        // (and named by the unknown-key error, Q15) instead of masquerading
        // as a section problem. Group 1 stays the key, group 2 the value.
        Regex::new(&format!(r"^([a-z][a-z0-9_]*):{DSL_WS_CLASS}*(.+)$"))
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
    static_regex(&RE, || {
        Regex::new(r"^(entities|prototypes|triggers|endings|verbs|templates|functions|decks):$")
    })
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
        Regex::new(&format!(r"^([a-z]+):{DSL_WS_CLASS}*(.+)$"))
    })
}

/// `/^(\S+)\s+is\s+([A-Za-z_.]+)\b\s*(.*)$/`
fn entity_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}+is{DSL_WS_CLASS}+([A-Za-z_.]+)(?-u:\b){DSL_WS_CLASS}*(.*)$"#
        ))
    })
}

/// `/^of\s+(\S+)$/`
fn of_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"^of{DSL_WS_CLASS}+({DSL_NON_WS_CLASS}+)$"))
    })
}

/// `/^stackable\b\s*/` (a PREFIX match; the matched
/// text is sliced off `rest`)
fn stackable_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"^stackable(?-u:\b){DSL_WS_CLASS}*"))
    })
}

/// `/(?:^|\s+)(?:at|in)\s+(\S+)\s*$/`
fn at_in_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"(?:^|{DSL_WS_CLASS}+)(?:at|in){DSL_WS_CLASS}+({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}*$"#
        ))
    })
}

/// `/(?:^|\s+)with\s+(.+)$/`
fn with_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"(?:^|{DSL_WS_CLASS}+)with{DSL_WS_CLASS}+(.+)$"))
    })
}

/// `/(?:^|\s+)deriving\s+(.+)$/`
fn deriving_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"(?:^|{DSL_WS_CLASS}+)deriving{DSL_WS_CLASS}+(.+)$"
        ))
    })
}

/// `/^\s*([A-Za-z_][A-Za-z0-9_.]*):(-?\d+)\s*$/`
fn aspect_spec_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"^{DSL_WS_CLASS}*([A-Za-z_][A-Za-z0-9_.]*):(-?[0-9]+){DSL_WS_CLASS}*$"
        ))
    })
}

/// `/^(\S+)\s+is\s+([a-z]+)\b\s*(.*)$/`
fn prototype_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}+is{DSL_WS_CLASS}+([a-z]+)(?-u:\b){DSL_WS_CLASS}*(.*)$"#
        ))
    })
}

/// `/^\s*name\s+from\s+object\s*$/`
fn name_from_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"^{DSL_WS_CLASS}*name{DSL_WS_CLASS}+from{DSL_WS_CLASS}+object{DSL_WS_CLASS}*$"
        ))
    })
}

/// `/^\s*(…)\s+from\s+object\.(…)(?:\s*-\s*(\d+))?\s*$/`
fn derive_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"^{DSL_WS_CLASS}*([A-Za-z_][A-Za-z0-9_.]*){DSL_WS_CLASS}+from{DSL_WS_CLASS}+object\.([A-Za-z_][A-Za-z0-9_.]*)(?:{DSL_WS_CLASS}*-{DSL_WS_CLASS}*([0-9]+))?{DSL_WS_CLASS}*$"
        ))
    })
}

/// Expiry form (§6.6, §9.32).
fn expire_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}+on{DSL_WS_CLASS}+expire{DSL_WS_CLASS}+of{DSL_WS_CLASS}+({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}*->{DSL_WS_CLASS}*(.+)$"#
        ))
    })
}

/// Clock form: `ID [once] at (hour|halfhour) N -> …`
fn trigger_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+)({DSL_WS_CLASS}+once)?{DSL_WS_CLASS}+at{DSL_WS_CLASS}+(hour|halfhour){DSL_WS_CLASS}+([0-9]+){DSL_WS_CLASS}*->{DSL_WS_CLASS}*(.+)$"#
        ))
    })
}

/// Paren'd comma-separated params.
fn fn_paren_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}*\(([^)]*)\){DSL_WS_CLASS}*->{DSL_WS_CLASS}*(.+)$"#
        ))
    })
}

/// No params, no parens.
fn fn_arrow_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}*->{DSL_WS_CLASS}*(.+)$"#
        ))
    })
}

/// Bare, space-separated params (lazy `\S.*?`).
fn fn_bare_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}+({DSL_NON_WS_CLASS}.*?){DSL_WS_CLASS}*->{DSL_WS_CLASS}*(.+)$"#
        ))
    })
}

/// `/^(\S+)\s+spec\b\s*(.*)$/`
fn deck_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}+spec(?-u:\b){DSL_WS_CLASS}*(.*)$"#
        ))
    })
}

/// `/(?:^|\s)defaultcard\s+(\S+)\s*$/` (single `\s`)
fn defaultcard_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"(?:^|{DSL_WS_CLASS})defaultcard{DSL_WS_CLASS}+({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}*$"#
        ))
    })
}

/// `/(?:^|\s)reset\s*$/` (single `\s`)
fn reset_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"(?:^|{DSL_WS_CLASS})reset{DSL_WS_CLASS}*$"))
    })
}

/// `/^([A-Z][A-Z0-9_]*):\s*"(.+)"(?:\s+\(([a-z]+)\))?$`
fn ending_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^([A-Z][A-Z0-9_]*):{DSL_WS_CLASS}*"(.+)"(?:{DSL_WS_CLASS}+\(([a-z]+)\))?$"#
        ))
    })
}

/// `/^(\S+)(?:\s+(.*))?$/`
fn slot_line_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r#"^({DSL_NON_WS_CLASS}+)(?:{DSL_WS_CLASS}+(.*))?$"#
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
        Regex::new(&format!(r#"hint{DSL_WS_CLASS}+"([^"]*)""#))
    })
}

/// `/(?:^|\s)when\s+(.+)$/` (single `\s`)
fn when_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(r"(?:^|{DSL_WS_CLASS})when{DSL_WS_CLASS}+(.+)$"))
    })
}

/// The count-gate scan
/// `/count\((actors|object|with|at|about|to)\)\s*(>=|<=|>|<|=|!=)\s*(\d+)/g`
fn count_gate_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"count\((actors|object|with|at|about|to)\){DSL_WS_CLASS}*(>=|<=|>|<|=|!=){DSL_WS_CLASS}*([0-9]+)"
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
/// ASCII word boundary)
fn read_path_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"(?-u:\b)aspect\.([a-z_][a-z0-9_.]*)"))
}

/// SET-aspect write scan `/SET\(\s*[^,()]+,\s*aspect:([a-z_][a-z0-9_.]*)/g`
fn write_set_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"SET\({DSL_WS_CLASS}*[^,()]+,{DSL_WS_CLASS}*aspect:([a-z_][a-z0-9_.]*)"
        ))
    })
}

/// `/^[A-Za-z_][A-Za-z0-9_]*$/` (the name-lexeme test)
fn name_lexeme_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$"))
}

/// `split(/\s+/)` over the DSL whitespace class: runs of
/// whitespace split; the inputs are always trimmed or `\S`-anchored, so
/// filtering the empty pieces yields exactly the canonical token sequence.
fn split_dsl_ws(s: &str) -> Vec<&str> {
    s.split(is_dsl_ws_char).filter(|p| !p.is_empty()).collect()
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
    // §9.48: per-home clause ordinal for the minted trigger ids
    let mut complete_ordinal: IndexMap<String, usize> = IndexMap::new();
    // §9.48: the last declared prototype NAME — a standalone `on complete`
    // line in prototypes: attaches to it
    let mut last_prototype: Option<String> = None;

    /// Push one parsed clause as a completion trigger under `home` (§9.48).
    fn push_complete(
        triggers: &mut Vec<TriggerDef>,
        home: &str,
        clause: CompleteClause,
        ordinals: &mut IndexMap<String, usize>,
    ) {
        triggers.push(TriggerDef::Complete(CompleteTriggerDef {
            id: mint_complete_id(home, ordinals),
            home: home.to_string(),
            aspect: clause.aspect,
            out: vec![clause.body],
        }));
    }

    let mut section: Option<String> = None;
    let mut verb_name: Option<String> = None;
    let mut in_recipes = false;

    for (ln, raw_line) in text.split('\n').enumerate() {
        // EVERY tab becomes
        // two spaces (`replace(/\t/g, '  ')` semantics)
        let line = raw_line.replace('\t', "  ");
        let trimmed = dsl_trim(&line);
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // the count of leading DSL whitespace CODE UNITS (equivalently,
        // the length minus the trimmed length). No DSL-whitespace char is
        // astral, so units == chars here.
        let indent = line.chars().take_while(|c| is_dsl_ws_char(*c)).count();
        let t = trimmed;

        // meta lines (indent 0, outside sections)
        if indent == 0 && section.is_none() {
            if let Some(m) = meta_re().captures(t) {
                match grp_text(&m, 1) {
                    "script" => meta.title = Some(dsl_trim(grp_text(&m, 2)).to_string()),
                    "player" => meta.player = Some(dsl_trim(grp_text(&m, 2)).to_string()),
                    "goal" => meta.goal = Some(dsl_trim(grp_text(&m, 2)).to_string()),
                    "seed" => {
                        // §9.41: integer seed, XORed with the rng counter as a
                        // 64-bit value. The message is constructed IN PLACE at
                        // this throw site.
                        let got = dsl_trim(grp_text(&m, 2));
                        if !seed_re().is_match(got) {
                            return Err(ChorusError::new(format!(
                                "line {}: seed must be an integer, got '{}'",
                                ln + 1,
                                got
                            )));
                        }
                        meta.seed = Some(parse_authored_int(got));
                    }
                    // the clock key: `start` is the only one of the five
                    // that lands here explicitly, and a bad VALUE blames
                    // the clock — correct attribution for a known key
                    // (§10: `start: d:h`).
                    "start" => meta.start = Some(parse_clock(dsl_trim(grp_text(&m, 2)))?),
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
                // §9.48: a standalone `on complete` line attaches to the
                // most recently declared entity — multiple lines per entity
                // are legal and fire in declaration order
                if find_on_complete(t) == Some(0) {
                    let Some(home) = entities.last().map(|e| e.name.clone()) else {
                        return Err(ChorusError::new(format!(
                            "line {}: 'on complete' needs a preceding entity declaration (§9.48)",
                            ln + 1
                        )));
                    };
                    for clause in parse_complete_clauses(t, ln + 1)? {
                        push_complete(&mut triggers, &home, clause, &mut complete_ordinal);
                    }
                    continue;
                }
                let declared = entities.len();
                let clauses = parse_entity_line(t, ln + 1, dict, &mut entities, &mut relations)?;
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
                    minted_ids.insert(minted, name.clone());
                    // §9.48: the line's own `on complete` clauses attach to
                    // the entity just declared, in line order
                    for clause in clauses {
                        push_complete(&mut triggers, &name, clause, &mut complete_ordinal);
                    }
                }
            }
            "prototypes" => {
                // §9.48: a standalone `on complete` line attaches to the most
                // recently declared prototype
                if find_on_complete(t) == Some(0) {
                    let Some(home) = last_prototype.clone() else {
                        return Err(ChorusError::new(format!(
                            "line {}: 'on complete' needs a preceding prototype declaration (§9.48)",
                            ln + 1
                        )));
                    };
                    for clause in parse_complete_clauses(t, ln + 1)? {
                        push_complete(&mut triggers, &home, clause, &mut complete_ordinal);
                    }
                    continue;
                }
                let (key, proto, clauses) = parse_prototype_line(t, ln + 1, dict)?;
                // duplicates are a load error, the same uniform rule as
                // entities/endings (formerly QUIRK(Q5): a silent
                // last-write-wins — the later insert overwrote)
                if prototypes.contains_key(&key) {
                    return Err(ChorusError::new(format!(
                        "line {}: duplicate prototype '{key}'",
                        ln + 1
                    )));
                }
                for clause in clauses {
                    push_complete(&mut triggers, &key, clause, &mut complete_ordinal);
                }
                last_prototype = Some(key.clone());
                prototypes.insert(key, proto);
            }
            "decks" => {
                decks.push(parse_deck_line(t, ln + 1)?);
            }
            "triggers" => {
                triggers.push(parse_trigger_line(t, ln + 1)?);
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
                        v.duration = parse_duration(dsl_trim(grp_text(&m, 2)))?;
                        continue;
                    }
                    let slot = parse_slot_line(grp_text(&m, 1), grp_text(&m, 2), ln + 1)?;
                    v.slots.push(slot);
                } else if in_recipes && indent >= 6 {
                    let recipe = parse_recipe_line(t, ln + 1)?;
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
    // The canonical order fronts integer-like keys, so the walk goes
    // through `canonical_key_order` (prototype names are
    // `\S+`, so "5 is idea" is a real key shape here).
    for name in canonical_key_order(&prototypes) {
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
// completion-trigger clauses (§9.48)
// ---------------------------------------------------------------------------

/// One parsed `on complete [<aspect>] -> ( body )` clause (§9.48). The body
/// is the RAW outcome text inside the parens — compiled like every trigger
/// body (function expansion, the load guards) in `compile_content`.
#[derive(Debug, Clone)]
pub(crate) struct CompleteClause {
    pub aspect: Option<String>,
    pub body: String,
}

/// The clause keywords an aspect name may not collide with (§9.48): the
/// entity/prototype-line clause heads and the trigger's own keywords —
/// `with` is the load-bearing one (the with-clause scanner works the same
/// line), the rest keep the vocabulary closed.
const COMPLETE_ASPECT_BANNED: [&str; 7] = [
    "with",
    "at",
    "in",
    "on",
    "complete",
    "deriving",
    "stackable",
];

/// Byte offset of the first TOP-LEVEL `on complete` in `text`, or `None`.
/// Top-level means paren-depth 0 and outside a string literal — an `on
/// complete` inside a clause body (e.g. in a NARRATE string) never splits the
/// line. Runs on split('\n') + trimmed SINGLE-LINE text only (the module's
/// frozen regex domain). A token start is the text start or a position after
/// a DSL-whitespace CHARACTER (the frozen class — U+3000 and U+FEFF
/// included), so a name like `only_complete` never matches and a wide-space
/// boundary splits exactly like an ASCII one; the walk is char_indices, so
/// no multi-byte char is ever read as its first byte.
fn find_on_complete(text: &str) -> Option<usize> {
    let mut depth: usize = 0;
    let mut in_str = false;
    let mut prev: Option<char> = None;
    for (i, c) in text.char_indices() {
        if in_str {
            if c == '"' {
                in_str = false;
            }
            prev = Some(c);
            continue;
        }
        match c {
            '"' => in_str = true,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {
                let at_token_start = prev.is_none_or(is_dsl_ws_char);
                if depth == 0 && at_token_start && c == 'o' && text[i..].starts_with("on") {
                    // the boundary after `on`, then the `complete` token, then
                    // its own boundary (an aspect token or the arrow follows)
                    let after_on = &text[i + "on".len()..];
                    let ws_run = after_on.len() - after_on.trim_start_matches(is_dsl_ws_char).len();
                    if let Some(after) = after_on[ws_run..].strip_prefix("complete") {
                        let bounded = after
                            .chars()
                            .next()
                            .is_none_or(|n| is_dsl_ws_char(n) || n == '-');
                        if bounded {
                            return Some(i);
                        }
                    }
                }
            }
        }
        prev = Some(c);
    }
    None
}

/// Offset of the ')' matching the '(' at offset 0 of `s` — string-literal
/// aware (the string grammar has no escapes: the hint form `[^"]*` froze
/// that). `None` when unbalanced.
fn matching_paren(s: &str) -> Option<usize> {
    let mut depth: usize = 0;
    let mut in_str = false;
    for (i, b) in s.bytes().enumerate() {
        if in_str {
            if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'(' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse the clause SEQUENCE
/// `on complete [<aspect>] -> ( body ) [on complete …]` from `src` — the
/// caller locates the first clause with [`find_on_complete`], so `src` starts
/// there. The paren'd body may itself carry nested parens (IF arms) and
/// strings; the depth scan is string-aware for both. `ln` is 1-BASED.
/// (No dictionary needed: the aspect class is syntactic, and its collision
/// set is the local clause-keyword list.)
fn parse_complete_clauses(src: &str, ln: usize) -> Result<Vec<CompleteClause>> {
    const SYNTAX: &str = "on complete [<aspect>] -> ( outcomes )";
    let mut rest = dsl_trim(src);
    let mut clauses = Vec::new();
    loop {
        if rest.is_empty() {
            return Ok(clauses);
        }
        let Some(after_on) = rest.strip_prefix("on").and_then(|r| {
            let r = r.trim_start_matches(is_dsl_ws_char);
            r.strip_prefix("complete")
        }) else {
            return Err(ChorusError::new(format!(
                "line {ln}: expected `{SYNTAX}`, got '{rest}'"
            )));
        };
        // the keyword ends at a boundary — an aspect token or the arrow
        if !after_on
            .chars()
            .next()
            .is_none_or(|c| is_dsl_ws_char(c) || c == '-')
        {
            return Err(ChorusError::new(format!(
                "line {ln}: expected `{SYNTAX}`, got '{rest}'"
            )));
        }
        let mut cur = after_on.trim_start_matches(is_dsl_ws_char);
        // the optional aspect: one bare lowercase name token — DOTTED keys
        // included, the exact class the recipe `aspects` list declares (the
        // gate matches the full dotted key, §9.48)
        let mut aspect: Option<String> = None;
        let name_end = cur
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == '_' || *c == '.')
            .count();
        if name_end > 0 && cur.starts_with(|c: char| c.is_ascii_lowercase()) {
            let tok = &cur[..name_end];
            if COMPLETE_ASPECT_BANNED.contains(&tok) {
                return Err(ChorusError::new(format!(
                    "line {ln}: aspect '{tok}' is a reserved clause keyword (§9.48)"
                )));
            }
            if tok.ends_with('.') {
                return Err(ChorusError::new(format!(
                    "line {ln}: bad aspect '{tok}' — a dotted aspect name cannot end at its dot (§9.48)"
                )));
            }
            aspect = Some(tok.to_string());
            cur = cur[name_end..].trim_start_matches(is_dsl_ws_char);
        }
        let Some(after_arrow) = cur.strip_prefix("->") else {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` needs `-> ( outcomes )`{}",
                aspect
                    .as_ref()
                    .map(|a| format!(" after aspect '{a}'"))
                    .unwrap_or_default()
            )));
        };
        let body_src = after_arrow.trim_start_matches(is_dsl_ws_char);
        let Some(open_rel) = body_src.find('(') else {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` needs a paren'd outcome list: {SYNTAX}"
            )));
        };
        if !dsl_trim(&body_src[..open_rel]).is_empty() {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` needs a paren'd outcome list: {SYNTAX}"
            )));
        }
        let Some(close_rel) = matching_paren(&body_src[open_rel..]) else {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` outcome list is missing its closing ')'"
            )));
        };
        let open = open_rel + 1;
        let close = open_rel + close_rel;
        let body = dsl_trim(&body_src[open..close]);
        if body.is_empty() {
            return Err(ChorusError::new(format!(
                "line {ln}: `on complete` needs an outcome list: {SYNTAX}"
            )));
        }
        clauses.push(CompleteClause {
            aspect,
            body: body.to_string(),
        });
        rest = dsl_trim(&body_src[close + 1..]);
    }
}

/// The engine-minted completion-trigger id (§9.48): `complete:{home}` for the
/// first clause authored on a home, `complete:{home}#n` from the second on —
/// deterministic and unique, shaped like the engine-written `expire:{name}`
/// id. The id surfaces in load errors only: completion triggers log no EVENT
/// rows and never touch `state.fired`.
fn mint_complete_id(home: &str, ordinals: &mut IndexMap<String, usize>) -> String {
    let n = ordinals.entry(home.to_string()).or_insert(0);
    *n += 1;
    if *n == 1 {
        format!("complete:{home}")
    } else {
        format!("complete:{home}#{}", n)
    }
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

/// `ln` is the 1-BASED line number (the caller adds one). Returns the line's
/// `on complete` clauses (§9.48) for the caller to attach to the declared
/// entity — they are stripped from the line BEFORE the with/at-in clause walk
/// so the clause scanners never see (and eat) the clause region or an aspect
/// named like a clause keyword.
fn parse_entity_line(
    t: &str,
    ln: usize,
    dict: &Dictionary,
    entities: &mut Vec<EntitySpec>,
    relations: &mut Vec<RelationSpec>,
) -> Result<Vec<CompleteClause>> {
    // §9.48: trailing `on complete …` clauses split off first
    let (base, clause_src) = match find_on_complete(t) {
        Some(at) => (&t[..at], Some(parse_complete_clauses(&t[at..], ln)?)),
        None => (t, None),
    };
    let Some(m) = entity_re().captures(base) else {
        return Err(ChorusError::new(format!(
            "line {ln}: entities expect  NAME is KIND [with ...] [at PLACE] [on complete [<aspect>] -> ( outcomes )]"
        )));
    };
    let name = grp_text(&m, 1);
    let second = grp_text(&m, 2);
    let rest0 = grp_text(&m, 3);
    // relation form: A is RELTYPE of B
    if dict.relation_types.contains_key(second) {
        if clause_src.is_some() {
            return Err(ChorusError::new(format!(
                "line {ln}: a relation line cannot carry `on complete` clauses (§9.48)"
            )));
        }
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
        return Ok(Vec::new());
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
        // byte offsets order identically to the UTF-16 unit offsets
        if let Some(a) = &at_in {
            let a_start = grp0_start(a);
            let with_is_first = with_m.as_ref().is_some_and(|w| grp0_start(w) >= a_start);
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
                        dsl_trim(spec)
                    )));
                };
                pairs.insert(
                    grp_text(&sm, 1).to_string(),
                    parse_authored_int(grp_text(&sm, 2)),
                );
            }
            aspects = aspects_from_pairs(&pairs);
            rest = &rest[..grp0_start(w)];
            continue;
        }
        break;
    }
    if !dsl_trim(rest).is_empty() {
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
    Ok(clause_src.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// prototype lines
// ---------------------------------------------------------------------------

/// `ln` is the 1-BASED line number. Returns (name, prototype, the line's
/// `on complete` clauses (§9.48) — stripped BEFORE the deriving/with walk,
/// exactly like the entity line).
fn parse_prototype_line(
    t: &str,
    ln: usize,
    dict: &Dictionary,
) -> Result<(String, PrototypeDef, Vec<CompleteClause>)> {
    let (base, clause_src) = match find_on_complete(t) {
        Some(at) => (&t[..at], Some(parse_complete_clauses(&t[at..], ln)?)),
        None => (t, None),
    };
    let Some(m) = prototype_re().captures(base) else {
        return Err(ChorusError::new(format!(
            "line {ln}: prototypes expect  NAME is KIND [stackable] [deriving ...] [with ...] [on complete [<aspect>] -> ( outcomes )]"
        )));
    };
    let name = grp_text(&m, 1);
    check_name(name, ln, dict, "prototype name")?;
    let kind = grp_text(&m, 2);
    if !KINDS.contains(&kind) {
        return Err(ChorusError::new(format!(
            "line {ln}: unknown kind '{kind}'"
        )));
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
                            dsl_trim(spec)
                        )));
                    };
                    pairs.insert(
                        grp_text(&sm, 1).to_string(),
                        parse_authored_int(grp_text(&sm, 2)),
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
                        dsl_trim(item)
                    )));
                };
                let aspects = derive.aspects.get_or_insert_with(IndexMap::new);
                aspects.insert(
                    grp_text(&dm, 1).to_string(),
                    DeriveAspect {
                        from_object_aspect: grp_text(&dm, 2).to_string(),
                        minus: grp_opt(&dm, 3).map(parse_authored_int).unwrap_or(0),
                    },
                );
            }
            proto.derive = Some(derive);
            continue;
        }
        break;
    }
    if !dsl_trim(rest).is_empty() {
        return Err(ChorusError::new(format!(
            "line {ln}: could not parse prototype line '{t}'"
        )));
    }
    Ok((name.to_string(), proto, clause_src.unwrap_or_default()))
}

// ---------------------------------------------------------------------------
// trigger lines
// ---------------------------------------------------------------------------

/// `ln` is the 1-BASED line number. (the trigger grammar needs no dictionary.)
fn parse_trigger_line(t: &str, ln: usize) -> Result<TriggerDef> {
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
    let n = parse_authored_int(grp_text(&m, 4));
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
        let params_raw = dsl_trim(grp_text(&m, 2));
        let params: Vec<String> = if params_raw.is_empty() {
            Vec::new()
        } else {
            params_raw
                .split(',')
                .map(|s| dsl_trim(s).to_string())
                .collect()
        };
        return Ok(FunctionDef {
            name: grp_text(&m, 1).to_string(),
            params,
            body: dsl_trim(grp_text(&m, 3)).to_string(),
        });
    }
    if let Some(m) = fn_arrow_re().captures(t) {
        return Ok(FunctionDef {
            name: grp_text(&m, 1).to_string(),
            params: Vec::new(),
            body: dsl_trim(grp_text(&m, 2)).to_string(),
        });
    }
    if let Some(m) = fn_bare_re().captures(t) {
        return Ok(FunctionDef {
            name: grp_text(&m, 1).to_string(),
            params: split_dsl_ws(grp_text(&m, 2))
                .into_iter()
                .map(str::to_string)
                .collect(),
            body: dsl_trim(grp_text(&m, 3)).to_string(),
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
    let mut rest: &str = dsl_trim(grp_text(&m, 2));
    let mut defaultcard: Option<String> = None;
    let mut reset = false;
    loop {
        if let Some(dc) = defaultcard_re().captures(rest) {
            defaultcard = Some(grp_text(&dc, 1).to_string());
            rest = dsl_trim(&rest[..grp0_start(&dc)]);
            continue;
        }
        if let Some(rm) = reset_re().captures(rest) {
            reset = true;
            rest = dsl_trim(&rest[..grp0_start(&rm)]);
            continue;
        }
        break;
    }
    let spec: Vec<String> = rest
        .split(',')
        .map(dsl_trim)
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
    let trimmed = dsl_trim(rest);
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
    let min = parse_authored_int(grp_text(&cm, 1));
    // max: the upper-bound group when present; a trailing `..` means
    // unbounded (no upper bound); no `..` pins max = min.
    let max = match grp_opt(&cm, 2) {
        Some(g) => Some(parse_authored_int(g)),
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

/// `ln` is the 1-BASED line number.
fn parse_recipe_line(t: &str, ln: usize) -> Result<RecipeDef> {
    // the ASCII `->` needle aligns byte and
    // UTF-16 unit offsets, so the byte slices carry the same text
    let Some(arrow) = t.find("->") else {
        return Err(ChorusError::new(format!(
            "line {ln}: recipes expect  [id] [hidden|shown] [hint \"...\"] [max N] [aspects NAME, ...] [when COND] -> OUTCOMES"
        )));
    };
    let lhs = dsl_trim(&t[..arrow]);
    let rhs = dsl_trim(&t[arrow + 2..]);
    let mut def = RecipeDef {
        out: vec![rhs.to_string()],
        ..RecipeDef::default()
    };
    if default_re().is_match(lhs) {
        return Ok(def);
    }
    let tokens = split_dsl_ws(lhs);
    let mut i = 0usize;
    if let Some(tok0) = tokens.first() {
        // §9.50/§9.48: `max` and `aspects` are keywords — a leading `max N`
        // or `aspects NAME, …` must never parse as a recipe id
        if recipe_id_re().is_match(tok0)
            && *tok0 != "when"
            && *tok0 != "hidden"
            && *tok0 != "shown"
            && *tok0 != "max"
            && *tok0 != "aspects"
        {
            def.id = Some((*tok0).to_string());
            i += 1;
        }
    }
    // §9.50: the positional walk — `hidden|shown` and `max N` compose in any
    // order around the regex-extracted clauses (N ≥ 1: a `max 0` or negative
    // is a load error — such a recipe could never fire, so it fails loudly
    // instead). §9.48 adds the `aspects NAME, …` presence list to the same
    // walk — both orders around `max` parse. `hint "…"` and `when COND` are
    // matched on the lhs BELOW, so their tokens stay on the stream: the walk
    // jumps the hint's span by byte offset, and a token left at the cursor
    // that neither clause owns is a load error — positionals are never
    // silently dropped.
    let hint_cap = hint_re().captures(lhs);
    let hint_end = hint_cap.as_ref().and_then(|c| c.get(0)).map(|m| m.end());
    loop {
        match tokens.get(i) {
            Some(tok) if *tok == "hidden" || *tok == "shown" => {
                def.shown = Some(*tok == "shown");
                i += 1;
            }
            Some(tok) if *tok == "max" => {
                i += 1;
                let Some(n_tok) = tokens.get(i) else {
                    return Err(ChorusError::new(format!(
                        "line {ln}: `max` needs an integer N ≥ 1"
                    )));
                };
                let n = parse_authored_int(n_tok);
                if n < 1 {
                    return Err(ChorusError::new(format!(
                        "line {ln}: `max {n_tok}` — N must be an integer ≥ 1"
                    )));
                }
                def.max = Some(n);
                i += 1;
            }
            Some(tok) if *tok == "aspects" => {
                // §9.48: `aspects <name> [, <name>]*` — the presence list an
                // `on complete <aspect>` gate reads. The whitespace split
                // leaves each separator comma riding its member token; every
                // member but the last must end in one, so `aspects a max 1`
                // never swallows the positional `max` (the keyword set is
                // excluded from members entirely).
                i += 1;
                let mut names: Vec<String> = Vec::new();
                while let Some(tok) = tokens.get(i) {
                    if !is_aspects_member(tok) {
                        break;
                    }
                    let ended = !tok.ends_with(',');
                    names.push(tok.trim_end_matches(',').to_string());
                    i += 1;
                    if ended {
                        break;
                    }
                }
                if names.is_empty() {
                    return Err(ChorusError::new(format!(
                        "line {ln}: `aspects` needs at least one name — aspects <name> [, <name>]*"
                    )));
                }
                def.aspects.extend(names);
            }
            // the quoted hint text may contain whitespace — skip its whole
            // span by offset. A span that ends before the cursor (a second
            // `hint`) leaves i unmoved: the trailing check below errors.
            Some(tok) if *tok == "hint" && hint_end.is_some() => {
                let end = hint_end.unwrap_or(0);
                let mut pos = 0usize;
                let mut starts: Vec<usize> = Vec::with_capacity(tokens.len());
                for t in &tokens {
                    while let Some(ch) = lhs[pos..].chars().next() {
                        if is_dsl_ws_char(ch) {
                            pos += ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    starts.push(pos);
                    pos += t.len();
                }
                let after = starts.partition_point(|&s| s < end);
                if after > i {
                    i = after;
                } else {
                    break;
                }
            }
            _ => break,
        }
    }
    if let Some(tok) = tokens.get(i) {
        if *tok != "when" || !when_re().is_match(lhs) {
            return Err(ChorusError::new(format!(
                "line {ln}: unexpected '{tok}' before '->' — positionals are [id] [hidden|shown] [hint \"...\"] [max N] [aspects NAME, ...], then [when COND]"
            )));
        }
    }
    if let Some(hint) = &hint_cap {
        def.hint = Some(grp_text(hint, 1).to_string());
    }
    if let Some(wm) = when_re().captures(lhs) {
        def.when = Some(grp_text(&wm, 1).to_string());
    }
    Ok(def)
}

/// An `aspects` list member after the whitespace split (§9.48): a lowercase
/// name with an optional trailing separator comma. The positional keywords
/// are excluded, so a list never swallows `max`/`when`/…, and a dotted
/// sub-aspect (`language.latin`) is one member — the exact class the
/// `on complete <aspect>` head reads (its trailing-dot case errors there).
fn is_aspects_member(tok: &str) -> bool {
    let t = tok.strip_suffix(',').unwrap_or(tok);
    !t.is_empty()
        && t.starts_with(|c: char| c.is_ascii_lowercase())
        && t.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        && !matches!(
            t,
            "max" | "when" | "hidden" | "shown" | "hint" | "default" | "aspects"
        )
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
/// human-readable finding per problem, SEVERITY-CLASSIFIED (E11's two-class
/// design): [`LintSeverity::Error`] is today's finding set (dead recipes,
/// shadowed whens, slot-violating count gates, unknown tokens, unwritten
/// aspect reads, …); [`LintSeverity::Warning`] is the never-matching-`when`
/// detector (a recipe whose `when` is provably false for every §6.6
/// selection binding — advisory, because a dead `when` is sometimes the
/// point: the converter emits deliberate dead-marker twins under exactly
/// such negation whens).
pub fn lint_script_findings(text: &str, dict: &Dictionary) -> Result<Vec<LintFinding>> {
    let script = parse_script(text, dict)?;
    compile_script(text, dict)?; // lints run on scripts that already compile
    let mut findings: Vec<LintFinding> = Vec::new();
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
            findings.push(LintFinding::error(format!(
                "verb {name}: declares slots but no recipes — every action rejects at selection (§6.6)"
            )));
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
        // the §6.1 cardinality domain the never-matching-when detector
        // (E11 below) reasons over: each declared role's binding count is
        // its slot's min..max at selection time (the same model the
        // count-gate check above uses)
        let slot_ranges: IndexMap<String, WhenRange> = slot_of
            .iter()
            .map(|(role, s)| (role.to_string(), (s.min, s.max)))
            .collect();
        let mut catch_all: Option<String> = None;
        let mut whens: IndexMap<String, String> = IndexMap::new();
        for (i, r) in verb.recipes.iter().enumerate() {
            let id = r.id.clone().unwrap_or_else(|| recipe_id(name, r, i));
            if let Some(ca) = &catch_all {
                findings.push(LintFinding::error(format!(
                    "recipe {id} ({name}): unreachable — {ca} already matches everything (first match wins, §6.4)"
                )));
            }
            let Some(when) = &r.when else {
                if catch_all.is_none() {
                    catch_all = Some(id);
                }
                continue;
            };
            // DSL whitespace runs collapse to one space, then a DSL trim
            // (`replace(/\s+/g, ' ').trim()` semantics)
            let norm = when
                .split(is_dsl_ws_char)
                .filter(|p| !p.is_empty())
                .collect::<Vec<&str>>()
                .join(" ");
            if let Some(prior) = whens.get(&norm) {
                findings.push(LintFinding::error(format!(
                    "recipe {id} ({name}): shadowed by {prior} — identical when, it can never fire (§6.4)"
                )));
            } else {
                whens.insert(norm, id.clone());
            }
            for cap in count_gate_re().captures_iter(when) {
                let role = grp_text(&cap, 1);
                let Some(slot) = slot_of.get(role) else {
                    continue;
                };
                let k = parse_authored_int(grp_text(&cap, 3));
                let op = grp_text(&cap, 2);
                // slot.max: None = unbounded — no finite count is ever over
                // an absent bound, so the over-max arms are false.
                // `!=` (the alternation's sixth op) is dead only when the
                // slot's WHOLE range is the excluded value — without this
                // arm the unreachable below aborted the whole lint run on
                // any authored `count(role) != k` (repro'd at HEAD).
                let dead = match op {
                    "=" => k < slot.min || slot.max.is_some_and(|mx| k > mx),
                    "!=" => slot.min == k && slot.max == Some(k),
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
                    findings.push(LintFinding::error(format!(
                        "recipe {id} ({name}): count({role}) {op} {k} can never hold — the slot takes {range} (§6.1)"
                    )));
                }
            }
            // E11: the never-matching-`when` detector (the WARNING class).
            // compile_script above already parsed (and arg-linted) every
            // when, so the parse here cannot fail — a failure is skipped,
            // never a panic (the lint never throws).
            if let Ok(pred) = parse_predicate(when) {
                let mut cx = WhenCx::new(&slot_ranges);
                if let WhenVerdict::Dead(reason) = when_verdict(&pred, false, &mut cx) {
                    findings.push(LintFinding {
                        severity: LintSeverity::Warning,
                        message: format!(
                            "recipe {id} ({name}): the when can never match — {reason} (§6.3)"
                        ),
                    });
                }
            }
        }
    }

    // template tokens: {actors} {object} {with} {at} {about} {to} {roll} — a
    // typo renders literally at render time (§5.3); function bodies also
    // interpolate their params (§9.40)
    let known: Vec<&str> = ROLE_NAMES.iter().copied().chain(["roll"]).collect();
    let scan_tokens = |findings: &mut Vec<LintFinding>, where_: &str, s: &str, extra: &[&str]| {
        for cap in template_token_re().captures_iter(s) {
            let tok = grp_text(&cap, 1);
            if !known.contains(&tok) && !extra.contains(&tok) {
                findings.push(LintFinding::error(format!(
                    "{where_}: unknown template token {{{tok}}} — roles are {{actors}} {{object}} {{with}} {{at}} {{about}} {{to}}, plus {{roll}} (§5.3)"
                )));
            }
        }
    };
    for (name, verb) in verbs {
        for (i, r) in verb.recipes.iter().enumerate() {
            let id = r.id.clone().unwrap_or_else(|| recipe_id(name, r, i));
            let out0 = r.out.first().map(String::as_str).unwrap_or("");
            scan_tokens(&mut findings, &format!("recipe {id}"), out0, &[]);
        }
    }
    if let Some(triggers) = &script.content.triggers {
        for t in triggers {
            let out0 = t.out().first().map(String::as_str).unwrap_or("");
            scan_tokens(&mut findings, &format!("trigger {}", t.id()), out0, &[]);
        }
    }
    if let Some(functions) = &script.content.functions {
        for f in functions {
            let extra: Vec<&str> = f.params.iter().map(String::as_str).collect();
            scan_tokens(
                &mut findings,
                &format!("function {}", f.name),
                &f.body,
                &extra,
            );
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
                    findings.push(LintFinding::error(format!(
                        "deck {}: entry {entry} is not stackable — served again while a copy lives, the spawn collides (§10 livelock rule)",
                        d.id
                    )));
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
            findings.push(LintFinding::error(format!(
                "aspect '{r}' is queried but never declared on any entity or prototype — check the spelling"
            )));
        }
    }

    // non-lexeme names are legal (§9.42's unsafe-name corner) but every ref
    // to them must be quoted — say so at load
    for e in &script.entities {
        if !name_lexeme_re().is_match(&e.name) {
            findings.push(LintFinding::error(format!(
                "entity '{}': not a name lexeme — every ref to it must be quoted (§5.1, §9.42)",
                e.name
            )));
        }
    }

    Ok(findings)
}

/// The string form of the lint (the pre-E11 shape): one human-readable
/// string per finding, every severity flattened. Kept for the load-path
/// callers and tests that only read messages — the bin and the smoke tests
/// read [`lint_script_findings`] to separate the WARNING class.
pub fn lint_script(text: &str, dict: &Dictionary) -> Result<Vec<String>> {
    let findings = lint_script_findings(text, dict)?;
    Ok(findings.into_iter().map(|f| f.message).collect())
}

// ---------------------------------------------------------------------------
// E11 — the never-matching-`when` detector (the WARNING class)
// ---------------------------------------------------------------------------

/// Severity class of one lint finding (E11's two-class design).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity {
    /// The pre-existing finding set (dead recipes, shadowed whens,
    /// slot-violating count gates, unknown template tokens, unwritten aspect
    /// reads, unstackable deck entries, non-lexeme names). Errors keep the
    /// reporting they always had — counted as findings; the exit code
    /// discipline is the caller's (the lint-scripts bin's constant-0 stands).
    Error,
    /// A recipe `when` that is PROVABLY false for every §6.6 selection
    /// binding (see [`when_verdict`]). Advisory by design: a dead `when` is
    /// sometimes the point — the CS converter emits deliberate dead-marker
    /// twins under `NOT (count(actors) …)` whens — so this class warns with
    /// the recipe id + reason and never fails anything by itself.
    Warning,
}

/// One lint finding: the severity class plus the human-readable message
/// (the exact string [`lint_script`] reports).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintFinding {
    pub severity: LintSeverity,
    pub message: String,
}

impl LintFinding {
    /// The pre-E11 finding set (the ERROR class).
    fn error(message: String) -> Self {
        LintFinding {
            severity: LintSeverity::Error,
            message,
        }
    }
}

/// One statically-typed scalar of a `when` (§6.3): a role's binding count or
/// the `total(role, path)` pool sum.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum WhenVar {
    Count(String),
    Total(String, String),
}

impl WhenVar {
    /// The authored spelling (message text pins it).
    fn text(&self) -> String {
        match self {
            WhenVar::Count(role) => format!("count({role})"),
            WhenVar::Total(role, path) => format!("total({role}, aspect:{path})"),
        }
    }
    /// The role the variable reads.
    fn role(&self) -> &str {
        match self {
            WhenVar::Count(role) | WhenVar::Total(role, _) => role,
        }
    }
}

/// The values a [`WhenVar`] can still take under one conjunction: `lo..hi`
/// with `hi: None` = unbounded above; `lo == NEG_INF` = unbounded below.
/// (Counts are cardinalities — always finite-below; a total sums authored
/// aspect values, and aspects parse signed, so a total is unbounded below.)
type WhenRange = (i64, Option<i64>);

/// The `lo` sentinel for "no lower bound" (a total over signed aspects).
const NEG_INF: i64 = i64::MIN;

fn range_text(r: &WhenRange) -> String {
    let lo = if r.0 == NEG_INF {
        "-∞".to_string()
    } else {
        r.0.to_string()
    };
    match r.1 {
        Some(hi) => format!("{lo}..{hi}"),
        None => format!("{lo}..∞"),
    }
}

/// Narrow `r` to the values satisfying `value {op} k`; `None` = none do.
/// `Ne` cannot narrow an interval — the point-range case is decided by the
/// satisfiability helpers below.
fn range_narrow(r: WhenRange, op: CmpOp, k: i64) -> Option<WhenRange> {
    let (lo, hi) = match op {
        CmpOp::Ge => (r.0.max(k), r.1),
        CmpOp::Gt => (r.0.max(k.saturating_add(1)), r.1),
        CmpOp::Le => (r.0, Some(r.1.map_or(k, |h| h.min(k)))),
        CmpOp::Lt => (
            r.0,
            Some(r.1.map_or(k.saturating_sub(1), |h| h.min(k.saturating_sub(1)))),
        ),
        CmpOp::Eq => (r.0.max(k), Some(r.1.map_or(k, |h| h.min(k)))),
        CmpOp::Ne => r,
    };
    if hi.is_some_and(|hi| lo > hi) {
        None
    } else {
        Some((lo, hi))
    }
}

/// Does EVERY value of `r` satisfy `value {op} k`?
fn range_all_satisfy(r: WhenRange, op: CmpOp, k: i64) -> bool {
    let (lo, hi) = r;
    match op {
        CmpOp::Ge => lo >= k,
        CmpOp::Gt => lo > k,
        CmpOp::Le => hi.is_some_and(|h| h <= k),
        CmpOp::Lt => hi.is_some_and(|h| h < k),
        CmpOp::Eq => lo == k && hi == Some(k),
        CmpOp::Ne => k < lo || hi.is_some_and(|h| k > h) || (hi == Some(lo) && lo != k),
    }
}

/// Does SOME value of `r` satisfy `value {op} k`?
fn range_any_satisfies(r: WhenRange, op: CmpOp, k: i64) -> bool {
    let (lo, hi) = r;
    match op {
        CmpOp::Ge => hi.is_none_or(|h| h >= k),
        CmpOp::Gt => hi.is_none_or(|h| h > k),
        CmpOp::Le => lo <= k,
        CmpOp::Lt => lo < k,
        CmpOp::Eq => lo <= k && hi.is_none_or(|h| h >= k),
        CmpOp::Ne => !(lo == k && hi == Some(k)),
    }
}

/// The comparison flipped operand-order (`3 > count(a)` ≡ `count(a) < 3`).
fn mirror_op(op: CmpOp) -> CmpOp {
    match op {
        CmpOp::Ge => CmpOp::Le,
        CmpOp::Le => CmpOp::Ge,
        CmpOp::Gt => CmpOp::Lt,
        CmpOp::Lt => CmpOp::Gt,
        CmpOp::Eq => CmpOp::Eq,
        CmpOp::Ne => CmpOp::Ne,
    }
}

/// Three-valued verdict for one (polarity-applied) predicate node. `Dead`
/// and `Always` are EXACT (provable over the whole §6.6 domain), so NOT
/// complements them and OR/AND combine them classically; `Unknown` is the
/// honest third value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WhenVerdict {
    /// False for every selection binding — the WARNING fires.
    Dead(String),
    /// True for every selection binding (a NOT-wrapped dead atom).
    Always,
    /// Statically undecidable — world reads (`exists`, `count(<query>)`),
    /// `every(...)`'s vacuous-true corner, non-int comparisons.
    Unknown,
}

/// What a role's binding state is under the current conjunction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoleBound {
    /// The slot declares it and no conjunct pinned it empty.
    Bindable,
    /// A conjunct pinned the count to 0 (`NOT (count(r) >= 1)`,
    /// `count(r) = 0`, a narrowed interval `..0`).
    PinnedEmpty,
    /// No slot for the role on this verb (or a `0..0` slot) — the role
    /// never binds anything (§6.6: validate_slots refuses any ref to an
    /// undeclared role; refs_to_clause zeroes it on RUN projection).
    NeverBinds,
}

/// The conjunction state: working ranges per typed variable plus the roles
/// pinned empty. Cloned per OR branch (small maps, never hot).
#[derive(Clone)]
struct WhenCx<'a> {
    /// each declared role's §6.1 cardinality, spelled lowercase
    slots: &'a IndexMap<String, WhenRange>,
    vars: IndexMap<WhenVar, WhenRange>,
    empty: IndexSet<String>,
    bound: IndexSet<String>,
}

impl<'a> WhenCx<'a> {
    fn new(slots: &'a IndexMap<String, WhenRange>) -> Self {
        WhenCx {
            slots,
            vars: IndexMap::new(),
            empty: IndexSet::new(),
            bound: IndexSet::new(),
        }
    }

    /// The §6.1 domain of one variable: a declared role's slot range; an
    /// undeclared role binds nothing, so its count is exactly 0; a total
    /// sums 0 over an empty role (§6.3's fold) and signed aspect values
    /// above — pinned to {0} only when the role can never bind.
    fn initial(&self, var: &WhenVar) -> WhenRange {
        match var {
            WhenVar::Count(role) => match self.slots.get(role) {
                Some(range) => *range,
                None => (0, Some(0)),
            },
            WhenVar::Total(role, _) => match self.slots.get(role) {
                Some((_, Some(0))) => (0, Some(0)),
                Some(_) => (NEG_INF, None),
                None => (0, Some(0)),
            },
        }
    }

    fn working(&self, var: &WhenVar) -> WhenRange {
        self.vars
            .get(var)
            .copied()
            .unwrap_or_else(|| self.initial(var))
    }

    /// A role's binding state under the current conjunction.
    fn role_bound(&self, role: &str) -> RoleBound {
        if self.empty.contains(role) {
            return RoleBound::PinnedEmpty;
        }
        match self.slots.get(role) {
            Some((_, Some(0))) | None => RoleBound::NeverBinds,
            Some(_) => RoleBound::Bindable,
        }
    }

    /// Record that an atom proved the role bound; `Some(reason)` = this
    /// contradicts a conjunct that pinned the role empty.
    fn require_bound(&mut self, role: &str) -> Option<String> {
        if self.empty.contains(role) {
            return Some(format!(
                "the when requires {role} both empty and bound (§6.3, §9.39)"
            ));
        }
        self.bound.insert(role.to_string());
        None
    }
}

/// The statically-typed variable of a cmp operand, if one side is.
fn typed_var(x: &ValExpr) -> Option<WhenVar> {
    match x {
        ValExpr::Count { role } => Some(WhenVar::Count(role.clone())),
        ValExpr::Total { role, path } => Some(WhenVar::Total(role.clone(), path.clone())),
        _ => None,
    }
}

/// The never-matching-`when` detector's core (E11). SOUNDNESS BOUNDARY: a
/// WARNING fires only when the `when` is provably false for EVERY §6.6
/// selection binding, over this static model — the count-gate check's own
/// domain (§6.1): each declared role's count is its slot's `min..max`, an
/// undeclared role binds nothing (count ≡ 0), a total sums over signed
/// aspects (0 exactly when the role cannot bind). Within one conjunction
/// the typed atoms narrow per-variable intervals and couple role-emptiness:
/// a count pinned to 0 (the `NOT (count(r) >= 1)` shape) vs anything
/// needing the role bound — `total(…) >= 1`, a positive `r.aspect.x` path,
/// `any(r, …)`, `rel(…, r…)`. NOT is exact (op complement on leaves, De
/// Morgan on AND/OR — the verdicts are exact); OR is dead only when EVERY
/// branch is.
///
/// Deliberately NOT caught (the documented boundary): world reads —
/// `exists(...)` and the `count(<query>)` pool form — stay `Unknown` (no
/// world model, so `exists(A) AND NOT exists(A)` passes silently);
/// `every(...)` is `Unknown` (vacuously true over an empty role, §9.39);
/// non-int comparisons (`count(a) > object.attr.seats`) are `Unknown`; and
/// the E7 RUN corner — a cross-verb RUN candidate re-evaluates its when
/// against the PARENT's committed bindings (refs_to_clause), outside the
/// §6.6 domain — exactly the simplification the §6.1 count-gate check has
/// always made.
fn when_verdict(p: &Pred, neg: bool, cx: &mut WhenCx) -> WhenVerdict {
    match p {
        Pred::Not(a) => when_verdict(a, !neg, cx),
        Pred::And(a, b) | Pred::Or(a, b) => {
            // AND shares the narrowing state when positive; OR forks it.
            // Under NOT the shapes swap (De Morgan): ¬(a∧b) = ¬a∨¬b forks,
            // ¬(a∨b) = ¬a∧¬b shares.
            let is_and = matches!(p, Pred::And(..));
            if is_and != neg {
                // conjunction: Dead if EITHER side is; Always only if BOTH are
                // (an Unknown side keeps the whole Unknown — an Always that
                // leaks through NOT would fabricate a warning)
                let va = when_verdict(a, neg, cx);
                if let WhenVerdict::Dead(reason) = va {
                    return WhenVerdict::Dead(reason);
                }
                match when_verdict(b, neg, cx) {
                    WhenVerdict::Dead(reason) => WhenVerdict::Dead(reason),
                    WhenVerdict::Always if va == WhenVerdict::Always => WhenVerdict::Always,
                    _ => WhenVerdict::Unknown,
                }
            } else {
                // disjunction: Dead only if BOTH sides are; Always if either is
                let mut a_cx = cx.clone();
                let mut b_cx = cx.clone();
                match (
                    when_verdict(a, neg, &mut a_cx),
                    when_verdict(b, neg, &mut b_cx),
                ) {
                    (WhenVerdict::Dead(ra), WhenVerdict::Dead(_)) => WhenVerdict::Dead(ra),
                    (WhenVerdict::Always, _) | (_, WhenVerdict::Always) => WhenVerdict::Always,
                    _ => WhenVerdict::Unknown,
                }
            }
        }
        Pred::Cmp { l, r, op } => when_cmp(l, r, *op, neg, cx),
        Pred::Any { role, .. } => {
            // any(...) is false over an empty role (§9.39) — a positive atom
            // proves the role bound; negated it proves nothing
            if neg {
                return WhenVerdict::Unknown;
            }
            role_bound_verdict(cx, role, "any(…) needs a bound member")
        }
        Pred::Every { .. } => {
            // every(...) is VACUOUSLY TRUE over an empty role (§9.39) — no
            // static verdict either way (the documented boundary)
            WhenVerdict::Unknown
        }
        Pred::Rel { role, role2, .. } => {
            // ∃ a pair — needing one is the sound half (a true rel needs
            // every named role bound); negated proves nothing
            if neg {
                return WhenVerdict::Unknown;
            }
            for role in [Some(role.as_str()), role2.as_deref()]
                .into_iter()
                .flatten()
            {
                if let WhenVerdict::Dead(reason) =
                    role_bound_verdict(cx, role, "rel(…) needs bound members")
                {
                    return WhenVerdict::Dead(reason);
                }
            }
            WhenVerdict::Unknown
        }
        // world reads: no world model at load — the documented boundary
        Pred::Exists { .. } | Pred::ExistsQuery { .. } => WhenVerdict::Unknown,
    }
}

/// A positive role-reading atom (`any`, `rel`, a path operand): Dead against
/// a pinned-empty or never-binding role, Unknown otherwise (with the role
/// recorded bound for the emptiness coupling).
fn role_bound_verdict(cx: &mut WhenCx, role: &str, why: &str) -> WhenVerdict {
    match cx.role_bound(role) {
        RoleBound::PinnedEmpty => WhenVerdict::Dead(format!(
            "the when requires {role} both empty and bound — this clause needs a bound card, another pinned it empty ({why}, §6.3, §9.39)"
        )),
        RoleBound::NeverBinds => WhenVerdict::Dead(format!(
            "{role} never binds — the verb declares no slot for it (or a 0..0 slot), so this clause is always false ({why}, §6.6)"
        )),
        RoleBound::Bindable => {
            // recording is pointless after an Unknown — but free and exact
            cx.require_bound(role);
            WhenVerdict::Unknown
        }
    }
}

/// One cmp leaf, polarity applied. The statically-decidable core: a typed
/// variable vs an int literal (either operand order). A path operand reads
/// the role's first bound card — unresolvable over an empty role reads
/// FALSE in the when-discipline (§9.39) — so a positive path atom also
/// proves its role bound.
fn when_cmp(l: &ValExpr, r: &ValExpr, op: CmpOp, neg: bool, cx: &mut WhenCx) -> WhenVerdict {
    // positive path atoms prove the role bound (the dead-stamp/unresolvable
    // rule reads false over an empty role, §9.39); a negated path atom is
    // satisfiable via the empty role and undecidable otherwise
    if !neg {
        for x in [l, r] {
            if let ValExpr::Path { role, field, key } = x {
                let why = format!("{role}.{}.{key} reads a bound card", field.as_str());
                if let WhenVerdict::Dead(reason) = role_bound_verdict(cx, role, &why) {
                    return WhenVerdict::Dead(reason);
                }
            }
        }
    }
    // the typed-var-vs-int core (either operand order)
    let (var, op, k) = match (typed_var(l), typed_var(r)) {
        (Some(v), _) if matches!(r, ValExpr::Int { .. }) => (v, op, int_of(r)),
        (_, Some(v)) if matches!(l, ValExpr::Int { .. }) => (v, mirror_op(op), int_of(l)),
        _ => return WhenVerdict::Unknown,
    };
    let op = if neg {
        // NOT over a leaf: the exact op complement (≥↔<, =↔≠)
        match op {
            CmpOp::Ge => CmpOp::Lt,
            CmpOp::Lt => CmpOp::Ge,
            CmpOp::Gt => CmpOp::Le,
            CmpOp::Le => CmpOp::Gt,
            CmpOp::Eq => CmpOp::Ne,
            CmpOp::Ne => CmpOp::Eq,
        }
    } else {
        op
    };
    let init = cx.initial(&var);
    let lit = format!("{} {} {k}", var.text(), op.as_str());
    if !range_any_satisfies(init, op, k) {
        // self-contained: no §6.1 value satisfies the literal
        return WhenVerdict::Dead(match (&var, cx.slots.get(var.role())) {
            (WhenVar::Count(_), None) => format!(
                "{lit} can never hold — the verb declares no {} slot, the role binds nothing (§6.6)",
                var.role()
            ),
            _ => format!(
                "{lit} can never hold — {} takes {} here (§6.1)",
                var.role(),
                range_text(&init)
            ),
        });
    }
    // conjunction narrowing: this literal vs what earlier conjuncts pinned
    let prior = cx.working(&var);
    let Some(narrowed) = range_narrow(prior, op, k) else {
        return WhenVerdict::Dead(format!(
            "{lit} can never hold — the when already pins {} to {} (§6.3)",
            var.text(),
            range_text(&prior)
        ));
    };
    cx.vars.insert(var.clone(), narrowed);
    // the emptiness coupling flows both ways through the count
    match &var {
        WhenVar::Count(role) => {
            if narrowed.1 == Some(0) {
                cx.empty.insert(role.clone());
            }
            if narrowed.0 >= 1 {
                cx.bound.insert(role.clone());
            }
        }
        WhenVar::Total(role, _) => {
            if narrowed.0 >= 1 {
                cx.bound.insert(role.clone());
            }
        }
    }
    if let Some(role) = cx.empty.intersection(&cx.bound).next() {
        return WhenVerdict::Dead(format!(
            "the when requires {role} both empty and bound (§6.3)"
        ));
    }
    if range_all_satisfy(init, op, k) {
        WhenVerdict::Always
    } else {
        WhenVerdict::Unknown
    }
}

fn int_of(x: &ValExpr) -> i64 {
    match x {
        ValExpr::Int { v } => *v,
        _ => 0, // unreachable: every caller guards with matches!(.., Int)
    }
}
