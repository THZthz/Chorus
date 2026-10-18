//! The line-segment parser: [`parse_script`] turns script text into meta +
//! raw content + the declared entity/relation lines — grammar only; all
//! load-time validation against the dictionary/world happens in
//! `compile_script`/`compile_content`.

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use super::complete::{find_on_complete, parse_complete_clauses, push_complete};
use super::decls::{
    parse_deck_line, parse_ending_line, parse_entity_line, parse_function_line, parse_op_line,
    parse_prototype_line, parse_trigger_line,
};
use super::types::{EntitySpec, RelationSpec, Script, ScriptMeta};
use super::verbblock::{parse_recipe_line, parse_slot_line};
use crate::content::DeckRaw;
use crate::content::{
    ContentRaw, EndingDef, FunctionDef, OpDef, PrototypeDef, TriggerDef, VerbContent,
};
use crate::core::clock::{parse_clock, parse_duration};
use crate::core::dict::Dictionary;
use crate::core::jsonw::canonical_key_order;
use crate::core::num_parse::parse_authored_int;
use crate::core::regex_util::{grp_text, static_regex};
use crate::core::text::{dsl_trim, is_dsl_ws_char, DSL_WS_CLASS};
use crate::error::{ChorusError, Result};

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

/// The script's section headers, free-form order — `compile_content` fixes
/// the compile order (functions first), so `functions:` may sit anywhere.
/// [`Section::from_header`] is the single source of the header vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Kinds,
    Entities,
    Prototypes,
    Triggers,
    Endings,
    Verbs,
    Templates,
    Functions,
    Ops,
    Decks,
}

impl Section {
    /// The section a `name:` header names — `None` when the line is not a
    /// section header at all.
    fn from_header(name: &str) -> Option<Section> {
        match name {
            "kinds" => Some(Section::Kinds),
            "entities" => Some(Section::Entities),
            "prototypes" => Some(Section::Prototypes),
            "triggers" => Some(Section::Triggers),
            "endings" => Some(Section::Endings),
            "verbs" => Some(Section::Verbs),
            "templates" => Some(Section::Templates),
            "functions" => Some(Section::Functions),
            "ops" => Some(Section::Ops),
            "decks" => Some(Section::Decks),
            _ => None,
        }
    }
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

/// `/^([a-z]+):\s*(.+)$` — any verb-block `key: value` line (a slot spec,
/// or `duration: X`; the arm dispatches on the key).
fn verb_kv_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(&format!(r"^([a-z]+):{DSL_WS_CLASS}*(.+)$")))
}

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
    // §10: the script's `ops:` declarations, section order — the
    // record the call-table build reads (functions.rs)
    let mut ops: Vec<OpDef> = Vec::new();
    // §4.1: the script's `kinds:` declarations, section order — the
    // record the loader's compile half re-registers (compile_parsed)
    let mut kinds: IndexSet<String> = IndexSet::new();
    // the EFFECTIVE dictionary: the caller's plus every kind this script has
    // declared so far, so entity/prototype lines read one vocabulary. The
    // caller's dictionary is never mutated — kinds are load-time vocabulary
    // only (the engine never reads them), and a per-load clone keeps loading
    // the same text against one dictionary idempotent (a shared dict would
    // otherwise fail its second load already-registered). compile_parsed
    // rebuilds the same augmentation from `kinds` above.
    let mut eff = dict.clone();
    let mut entity_names: IndexSet<String> = IndexSet::new();
    // minted script id (`s_{name.to_lowercase()}`) → the name holding it
    let mut minted_ids: IndexMap<String, String> = IndexMap::new();
    // §6.7: per-home clause ordinal for the minted trigger ids
    let mut complete_ordinal: IndexMap<String, usize> = IndexMap::new();
    // §10: the last declared prototype NAME — a standalone `on complete`
    // line in prototypes: attaches to it
    let mut last_prototype: Option<String> = None;

    let mut section: Option<Section> = None;
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
                        // §4.4: integer seed, XORed with the rng counter as a
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
        if indent == 0 {
            if let Some(header) = t.strip_suffix(':').and_then(Section::from_header) {
                section = Some(header);
                verb_name = None;
                in_recipes = false;
                continue;
            }
        }
        let Some(sec) = section else {
            return Err(ChorusError::new(format!(
                "line {}: expected a section header, got '{t}'",
                ln + 1
            )));
        };

        match sec {
            Section::Kinds => {
                // §4.1: one indent-2 lower_snake kind name per line.
                // register_kind owns the whole grammar (shape, relation-type
                // collision, duplicates — built-ins included), so the error
                // vocabulary lives in one place; the loader wraps it with the
                // line number. Registration order IS the suggestion order in
                // every later unknown-kind message. A kind is visible to the
                // lines AFTER its declaration only — use before `kinds:` is
                // the ordinary unknown-kind load error (the same
                // declare-then-use order containers already follow, §10).
                if indent != 2 {
                    return Err(ChorusError::new(format!(
                        "line {}: kinds expect an indent-2 kind name, got '{t}'",
                        ln + 1
                    )));
                }
                eff.register_kind(t)
                    .map_err(|e| ChorusError::new(format!("line {}: {e}", ln + 1)))?;
                kinds.insert(t.to_string());
            }
            Section::Entities => {
                // §10: a standalone `on complete` line attaches to the
                // most recently declared entity — multiple lines per entity
                // are legal and fire in declaration order
                if find_on_complete(t) == Some(0) {
                    let Some(home) = entities.last().map(|e| e.name.clone()) else {
                        return Err(ChorusError::new(format!(
                            "line {}: 'on complete' needs a preceding entity declaration (§10)",
                            ln + 1
                        )));
                    };
                    for clause in parse_complete_clauses(t, ln + 1)? {
                        push_complete(&mut triggers, &home, clause, &mut complete_ordinal);
                    }
                    continue;
                }
                let declared = entities.len();
                let clauses = parse_entity_line(t, ln + 1, &eff, &mut entities, &mut relations)?;
                // duplicate entity declarations share one s_<name> id and
                // would silently overwrite — a load error, not a
                // last-write-wins (§5.1). Names are case-exact (§5.1), so
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
                    // §10: the line's own `on complete` clauses attach to
                    // the entity just declared, in line order
                    for clause in clauses {
                        push_complete(&mut triggers, &name, clause, &mut complete_ordinal);
                    }
                }
            }
            Section::Prototypes => {
                // §10: a standalone `on complete` line attaches to the most
                // recently declared prototype
                if find_on_complete(t) == Some(0) {
                    let Some(home) = last_prototype.clone() else {
                        return Err(ChorusError::new(format!(
                            "line {}: 'on complete' needs a preceding prototype declaration (§10)",
                            ln + 1
                        )));
                    };
                    for clause in parse_complete_clauses(t, ln + 1)? {
                        push_complete(&mut triggers, &home, clause, &mut complete_ordinal);
                    }
                    continue;
                }
                let (key, proto, clauses) = parse_prototype_line(t, ln + 1, &eff)?;
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
            Section::Decks => {
                decks.push(parse_deck_line(t, ln + 1)?);
            }
            Section::Triggers => {
                triggers.push(parse_trigger_line(t, ln + 1)?);
            }
            Section::Functions => {
                functions.push(parse_function_line(t, ln + 1)?);
            }
            Section::Ops => {
                ops.push(parse_op_line(t, ln + 1)?);
            }
            Section::Endings => {
                let (id, ending) = parse_ending_line(t, ln + 1)?;
                if endings.contains_key(&id) {
                    return Err(ChorusError::new(format!(
                        "line {}: duplicate ending id '{id}'",
                        ln + 1
                    )));
                }
                endings.insert(id, ending);
            }
            Section::Templates => {
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
            Section::Verbs => {
                // verb header: indent 2, UPPER_NAME:
                if indent == 2 && verb_header_re().is_match(t) {
                    let name = t[..t.len() - 1].to_string();
                    // v0.7.6: the header no longer demands a PRE-registered
                    // lexeme — any UPPER_SNAKE name declares a brand-new verb
                    // or configures an existing one; the shape rule lives in
                    // verb_header_re above, and the load's registration (the
                    // `dict_with_vocabulary` half of compile_parsed)
                    // re-validates the lexeme against the whole nameable
                    // grammar — a reserved word (EVENT, TO, …) fails loud
                    // there, naming the lexeme. A repeated verb header is a
                    // load error, the same
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
                    let Some(m) = verb_kv_re().captures(t) else {
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
        }
    }

    // §5.1: a name is one card — declared both ways, every SPAWN of the
    // prototype would collide with the living entity (§9.2); fail at load.
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
            kinds,
            triggers: Some(triggers),
            prototypes: Some(prototypes),
            templates: Some(templates),
            endings: Some(endings),
            functions: Some(functions),
            ops: Some(ops),
            decks: Some(decks),
        },
        entities,
        relations,
    })
}
