//! The load-time lint (§10's "fail loudly", warn-loudly sibling): one
//! human-readable finding per problem, SEVERITY-CLASSIFIED (E11's two-class
//! design).

use std::sync::OnceLock;

use indexmap::{IndexMap, IndexSet};
use regex::Regex;

use super::load::compile_parsed;
use super::parse::parse_script;
use super::types::declared_entities;
use super::when_dead::{when_verdict, WhenCx, WhenRange, WhenVerdict};
use crate::content::{collect_stackable, mint_recipe_id, RecipeDef, SlotDef};
use crate::core::dict::Dictionary;
use crate::core::num_parse::parse_authored_int;
use crate::core::predicate::parse_predicate;
use crate::core::regex_util::{grp_text, static_regex};
use crate::core::roles::role_names;
use crate::core::text::{is_dsl_ws_char, DSL_WS_CLASS};
use crate::core::types::Aspects;
use crate::error::Result;

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

/// Severity class of one lint finding (E11's two-class design).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity {
    /// The load-time finding set (dead recipes, shadowed whens,
    /// slot-violating count gates, unknown template tokens, unwritten aspect
    /// reads, unstackable deck entries, non-lexeme names). Errors are
    /// counted as findings; the exit code
    /// discipline is the caller's (the lint-scripts bin's constant-0 stands).
    Error,
    /// A recipe `when` that is PROVABLY false for every §6.6 selection
    /// binding (see [`when_verdict`]). Advisory by design: a dead `when` is
    /// sometimes the point — the CS converter emits deliberate dead-marker
    /// twins under exactly such negation whens — so this class warns with
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
        LintFinding { severity: LintSeverity::Error, message }
    }
}

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
    // lints run on scripts that already compile — and the compile hands back
    // the load-effective dictionary on `CompiledScript`, so the E11
    // when-verdict re-parse below binds THAT instead of rebuilding an
    // identical one (a script-declared kind or verb must not silently shrink
    // the finding set: a dead-when verdict over `kind:custom` still parses)
    let compiled = compile_parsed(script.clone(), dict)?;
    let eff = &compiled.dict;
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
        let slot_ranges: IndexMap<String, WhenRange> =
            slot_of.iter().map(|(role, s)| (role.to_string(), (s.min, s.max))).collect();
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
            // one `when`, one verdict: a count-gate error and the
            // never-matching warning name the SAME deadness (a gate the
            // slot cannot satisfy already proves the when false), so the
            // error speaks and the warning stands down. A count over a role
            // the verb never declared has no error to stand down for — its
            // count binds nothing (§6.6), and the warning is the only
            // finding that names it.
            let mut gate_error = false;
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
                // arm the whole lint run aborts on any authored
                // `count(role) != k`.
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
                    gate_error = true;
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
            // never a panic (the lint never throws). A count-gate error on
            // this recipe already reported the deadness (one root cause,
            // one finding); without one, the detector speaks.
            if !gate_error {
                if let Ok(pred) = parse_predicate(when, eff) {
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
    }

    // template tokens: {actors} {object} {with} {at} {about} {to} {roll} — a
    // typo renders literally at render time (§5.3); function bodies also
    // interpolate their params (§10)
    let known: Vec<&str> = role_names().into_iter().chain(["roll"]).collect();
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
            // parse output carries exactly one outcome string per def; walk
            // them all so a future multi-string def can't hide a token
            for out in &r.out {
                scan_tokens(&mut findings, &format!("recipe {id}"), out, &[]);
            }
        }
    }
    if let Some(triggers) = &script.content.triggers {
        for t in triggers {
            for out in t.out() {
                scan_tokens(&mut findings, &format!("trigger {}", t.id()), out, &[]);
            }
        }
    }
    if let Some(functions) = &script.content.functions {
        for f in functions {
            let extra: Vec<&str> = f.params.iter().map(String::as_str).collect();
            scan_tokens(&mut findings, &format!("function {}", f.name), &f.body, &extra);
        }
    }
    // op bodies get the same template-token scan as function bodies (§10).
    // The lint never sees through a CALL SITE — a recipe line
    // reading `BOOST(OBJECT)` contributes no finding itself (§13.2's note);
    // the writes are counted from the declared bodies further below.
    if let Some(ops) = &script.content.ops {
        for o in ops {
            let extra: Vec<&str> = o.params.iter().map(String::as_str).collect();
            scan_tokens(&mut findings, &format!("op {}", o.name), &o.body, &extra);
        }
    }

    // the §10 livelock rule: deck entries should be stackable (or consumed
    // and renamed on the same step) — served twice while a copy lives, the
    // second spawn name-collides and rejects the step (§9.2). A non-stackable
    // spec entry that can physically be served only ONCE cannot collide with
    // itself: the deck exhausts before a second copy is ever asked for.
    let declared = declared_entities(&script.entities);
    let (stackable_names, stackable_kinds) =
        collect_stackable(&declared, script.content.prototypes.as_ref());
    if let Some(decks) = &script.content.decks {
        // the serve census: how many spec/defaultcard positions across ALL
        // decks serve each name — a name with a second serving position can
        // meet the copy that position dealt, whatever each deck's own shape
        let mut served: IndexMap<&str, usize> = IndexMap::new();
        for d in decks {
            for e in &d.spec {
                *served.entry(e).or_insert(0) += 1;
            }
            if let Some(dc) = &d.defaultcard {
                *served.entry(dc).or_insert(0) += 1;
            }
        }
        for d in decks {
            // `spec X defaultcard X` is one problem about one name — the
            // defaultcard occurrence adds no second finding (the spec
            // occurrence speaks for the name)
            let mut entries: Vec<&String> = d.spec.iter().collect();
            if let Some(dc) = &d.defaultcard {
                if !d.spec.contains(dc) {
                    entries.push(dc);
                }
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
                        script.entities.iter().find(|e| &e.name == entry).map(|e| e.kind.as_str())
                    });
                if stackable_names.contains(entry)
                    || kind.is_some_and(|k| stackable_kinds.contains(k))
                {
                    continue;
                }
                // the once-only physics, all three required: the entry comes
                // from the spec (a defaultcard serves forever), exactly once
                // in this deck's spec, and no other position anywhere serves
                // the name; `reset` wraps the spec, so every entry is served
                // again after the wrap
                let in_spec_once = d.spec.iter().filter(|e| *e == entry).count() == 1;
                let served_once = served.get(entry.as_str()).is_some_and(|&n| n == 1);
                if in_spec_once && !d.reset && served_once {
                    continue;
                }
                findings.push(LintFinding::error(format!(
                    "deck {}: entry {entry} is not stackable — served again while a copy lives, the spawn collides (§10 livelock rule)",
                    d.id
                )));
            }
        }
    }

    // aspect reads with no writer anywhere — a typo'd aspect silently never
    // matches. ERROR severity (the pre-E11 finding set, §13.2): aspects are
    // an open vocabulary, but a read nothing ever writes is dead content
    // either way
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
            let path = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
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
            // parse output carries exactly one outcome string per def; walk
            // them all so a future multi-string def can't hide a write
            for out in &r.out {
                scan_writes(&mut writes, out);
            }
        }
    }
    if let Some(triggers) = &script.content.triggers {
        for t in triggers {
            for out in t.out() {
                scan_writes(&mut writes, out);
            }
        }
    }
    if let Some(functions) = &script.content.functions {
        for f in functions {
            scan_writes(&mut writes, &f.body);
        }
    }
    // op bodies count as writers exactly like function bodies — the raw
    // `SET(…, aspect:x)` inside a declared op body is a write (§10)
    if let Some(ops) = &script.content.ops {
        for o in ops {
            scan_writes(&mut writes, &o.body);
        }
    }
    for r in &reads {
        if !writes.contains(r) {
            findings.push(LintFinding::error(format!(
                "aspect '{r}' is queried but never declared on any entity or prototype — check the spelling"
            )));
        }
    }

    // non-lexeme names are legal (§4.4's unsafe-name corner) but every ref
    // to them must be quoted — say so at load
    for e in &script.entities {
        if !name_lexeme_re().is_match(&e.name) {
            findings.push(LintFinding::error(format!(
                "entity '{}': not a name lexeme — every ref to it must be quoted (§5.1, §4.4)",
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
