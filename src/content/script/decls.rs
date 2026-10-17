//! The declared-line parsers: entity, prototype, trigger, function, deck,
//! and ending lines — one sentence each, each with its own frozen regex.

use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;

use super::complete::{find_on_complete, parse_complete_clauses, CompleteClause};
use super::types::{EntitySpec, RelationSpec};
use crate::content::DeckRaw;
use crate::content::{
    DeriveAspect, EndingDef, ExpireTriggerDef, FunctionDef, PrototypeDef, PrototypeDerive,
    SlotTriggerDef, TriggerDef,
};
use crate::core::dict::{Dictionary, KINDS};
use crate::core::num_parse::parse_authored_int;
use crate::core::regex_util::{grp0_start, grp_opt, grp_text, static_regex};
use crate::core::text::split_dsl_ws;
use crate::core::text::{dsl_trim, DSL_NON_WS_CLASS, DSL_WS_CLASS};
use crate::core::types::aspects_from_pairs;
use crate::core::types::Aspects;
use crate::error::{ChorusError, Result};

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
    static_regex(&RE, || Regex::new(&format!(r"^of{DSL_WS_CLASS}+({DSL_NON_WS_CLASS}+)$")))
}

/// `/^stackable\b\s*/` (a PREFIX match; the matched
/// text is sliced off `rest`)
fn stackable_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(&format!(r"^stackable(?-u:\b){DSL_WS_CLASS}*")))
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
    static_regex(&RE, || Regex::new(&format!(r"(?:^|{DSL_WS_CLASS}+)with{DSL_WS_CLASS}+(.+)$")))
}

/// `/(?:^|\s+)deriving\s+(.+)$/`
fn deriving_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(&format!(r"(?:^|{DSL_WS_CLASS}+)deriving{DSL_WS_CLASS}+(.+)$")))
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

/// Expiry form (§6.6).
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
        Regex::new(&format!(r#"^({DSL_NON_WS_CLASS}+){DSL_WS_CLASS}*->{DSL_WS_CLASS}*(.+)$"#))
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
    static_regex(&RE, || Regex::new(&format!(r"(?:^|{DSL_WS_CLASS})reset{DSL_WS_CLASS}*$")))
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

/// §5.1: reserved words cannot be names — as a
/// declaration they would make every unquoted ref in clauses parse as the
/// keyword, so fail at load. (Non-lexeme names like `X-Y` are legal but
/// quota every ref — linted, not errored: §4.4's unsafe-name corner.)
fn check_name(name: &str, ln: usize, dict: &Dictionary, what: &str) -> Result<()> {
    if dict.reserved.contains(name) {
        return Err(ChorusError::new(format!(
            "line {ln}: {what} '{name}' is a reserved word — rename it (§5.1)"
        )));
    }
    Ok(())
}

/// `ln` is the 1-BASED line number (the caller adds one). Returns the line's
/// `on complete` clauses (§10) for the caller to attach to the declared
/// entity — they are stripped from the line BEFORE the with/at-in clause walk
/// so the clause scanners never see (and eat) the clause region or an aspect
/// named like a clause keyword.
pub(super) fn parse_entity_line(
    t: &str,
    ln: usize,
    dict: &Dictionary,
    entities: &mut Vec<EntitySpec>,
    relations: &mut Vec<RelationSpec>,
) -> Result<Vec<CompleteClause>> {
    // §10: trailing `on complete …` clauses split off first
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
                "line {ln}: a relation line cannot carry `on complete` clauses (§10)"
            )));
        }
        let Some(rel) = of_re().captures(rest0) else {
            return Err(ChorusError::new(format!("line {ln}: relations expect  A is TYPE of B")));
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
    // §4.2: the `stackable` keyword sits before `with`/`at`/`in` clauses
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
                pairs.insert(grp_text(&sm, 1).to_string(), parse_authored_int(grp_text(&sm, 2)));
            }
            aspects = aspects_from_pairs(&pairs);
            rest = &rest[..grp0_start(w)];
            continue;
        }
        break;
    }
    if !dsl_trim(rest).is_empty() {
        return Err(ChorusError::new(format!("line {ln}: could not parse entity line '{t}'")));
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

/// `ln` is the 1-BASED line number. Returns (name, prototype, the line's
/// `on complete` clauses (§10) — stripped BEFORE the deriving/with walk,
/// exactly like the entity line).
pub(super) fn parse_prototype_line(
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
        return Err(ChorusError::new(format!("line {ln}: unknown kind '{kind}'")));
    }
    let mut proto = PrototypeDef { kind: kind.to_string(), ..PrototypeDef::default() };
    let mut rest: &str = grp_text(&m, 3);
    // §4.2: the `stackable` keyword sits before `deriving`/`with` clauses
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
            let deriving_is_first = deriving_m.as_ref().is_some_and(|d| grp0_start(d) >= w_start);
            if !deriving_is_first {
                if aspects_done {
                    return Err(ChorusError::new(format!("line {ln}: duplicate 'with' clause")));
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
                    pairs
                        .insert(grp_text(&sm, 1).to_string(), parse_authored_int(grp_text(&sm, 2)));
                }
                proto.aspects = Some(aspects_from_pairs(&pairs));
                rest = &rest[..w_start];
                continue;
            }
        }
        if let Some(dm_all) = &deriving_m {
            if derive_done {
                return Err(ChorusError::new(format!("line {ln}: duplicate 'deriving' clause")));
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
        return Err(ChorusError::new(format!("line {ln}: could not parse prototype line '{t}'")));
    }
    Ok((name.to_string(), proto, clause_src.unwrap_or_default()))
}

/// `ln` is the 1-BASED line number. (the trigger grammar needs no dictionary.)
pub(super) fn parse_trigger_line(t: &str, ln: usize) -> Result<TriggerDef> {
    // expiry form: ID on expire of NAME -> OUTCOMES (§6.6)
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
        return Err(ChorusError::new(format!("line {ln}: trigger hour must be 0..23")));
    }
    if grp_text(&m, 3) == "halfhour" && n > 47 {
        return Err(ChorusError::new(format!("line {ln}: trigger halfhour must be 0..47")));
    }
    Ok(TriggerDef::Slot(SlotTriggerDef {
        id: grp_text(&m, 1).to_string(),
        slot: if grp_text(&m, 3) == "hour" { n * 2 } else { n },
        once: Some(grp_opt(&m, 2).is_some()),
        out: vec![grp_text(&m, 5).to_string()],
    }))
}

/// `NAME(param, …) -> OUTCOMES` (or the bare form
/// `NAME param …`). Grammar only: name/param shape, op collisions and
/// duplicates are `compile_content`'s. `ln` is the 1-BASED line number.
pub(super) fn parse_function_line(t: &str, ln: usize) -> Result<FunctionDef> {
    if let Some(m) = fn_paren_re().captures(t) {
        let params_raw = dsl_trim(grp_text(&m, 2));
        let params: Vec<String> = if params_raw.is_empty() {
            Vec::new()
        } else {
            params_raw.split(',').map(|s| dsl_trim(s).to_string()).collect()
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
            params: split_dsl_ws(grp_text(&m, 2)).into_iter().map(str::to_string).collect(),
            body: dsl_trim(grp_text(&m, 3)).to_string(),
        });
    }
    Err(ChorusError::new(format!(
        "line {ln}: functions expect  NAME(param, …) -> OUTCOMES  or  NAME param … -> OUTCOMES"
    )))
}

/// `ID spec NAME, … [defaultcard NAME] [reset]`.
/// Grammar only: entry/defaultcard resolution against prototypes + entities,
/// duplicate ids and the permutation are `compile_content`'s (§4.4). The
/// trailing clauses may sit in either order (stripped from the end until
/// neither matches); `defaultcard`/`reset` are keywords, not spec entries.
/// `ln` is the 1-BASED line number.
pub(super) fn parse_deck_line(t: &str, ln: usize) -> Result<DeckRaw> {
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
    let spec: Vec<String> =
        rest.split(',').map(dsl_trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
    if spec.is_empty() {
        return Err(ChorusError::new(format!("line {ln}: deck {id}: the spec is empty")));
    }
    Ok(DeckRaw { id: id.to_string(), spec, defaultcard, reset })
}

/// `ID: "Title" (flavour)` — flavour optional,
/// default melancholy (resolved at compile). `ln` is the 1-BASED line number.
pub(super) fn parse_ending_line(t: &str, ln: usize) -> Result<(String, EndingDef)> {
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
