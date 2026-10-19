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
use crate::content::{
    collect_stackable, collect_uniques, for_each_authored_outcome_infallible, mint_recipe_id,
    CompiledTrigger, RecipeDef, SlotDef,
};
use crate::core::dict::Dictionary;
use crate::core::num_parse::parse_authored_int;
use crate::core::predicate::{parse_predicate, Pred, ValExpr};
use crate::core::query::{CmpOp, Query, QueryTerm};
use crate::core::r#ref::Ref;
use crate::core::regex_util::{grp_text, static_regex};
use crate::core::roles::role_names;
use crate::core::sentence::{Arg, Outcome};
use crate::core::text::{is_dsl_ws_char, DSL_WS_CLASS};
use crate::core::types::Aspects;
use crate::error::Result;

/// The count-gate scan
/// `/count\((actors|object|with|at|about|to|held)\)\s*(>=|<=|>|<|=|!=)\s*(\d+)/g`
fn count_gate_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || {
        Regex::new(&format!(
            r"count\((actors|object|with|at|about|to|held)\){DSL_WS_CLASS}*(>=|<=|>|<|=|!=){DSL_WS_CLASS}*([0-9]+)"
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

/// Does the predicate read the held scope (§4.3's seventh role)? A read is
/// any role position naming `held` — a count/total/path operand, a
/// quantifier's role, a rel pair (`held` or its `.container` redirect).
fn pred_reads_held(p: &Pred) -> bool {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => pred_reads_held(a) || pred_reads_held(b),
        Pred::Not(a) => pred_reads_held(a),
        Pred::Cmp { l, r, .. } => expr_reads_held(l) || expr_reads_held(r),
        Pred::Rel { role, role2, .. } => [Some(role.as_str()), role2.as_deref()]
            .into_iter()
            .flatten()
            .any(|r| r == "held" || r.strip_suffix(".container") == Some("held")),
        Pred::Every { role, .. } | Pred::Any { role, .. } => role == "held",
        Pred::Exists { .. } | Pred::ExistsQuery { .. } => false,
    }
}

/// The operand half of [`pred_reads_held`].
fn expr_reads_held(x: &ValExpr) -> bool {
    match x {
        ValExpr::Count { role } | ValExpr::Total { role, .. } | ValExpr::Path { role, .. } => {
            role == "held"
        }
        _ => false,
    }
}

/// Severity class of one lint finding (E11's two-class design).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LintSeverity {
    /// The load-time finding set (dead recipes, shadowed whens,
    /// slot-violating count gates, unknown template tokens, unwritten aspect
    /// reads, unstackable deck entries, entity-form SPAWNs, non-lexeme
    /// names). Errors are counted as findings; the exit code
    /// discipline is the caller's (the lint-scripts bin's constant-0 stands).
    Error,
    /// A recipe `when` that is PROVABLY false for every §6.6 selection
    /// binding (see [`when_verdict`]), or PROVABLY always-true at a
    /// world-form gate whose aspect is borne from load or rides a deck
    /// defaultcard (the W1 check).
    /// Advisory by design: a dead `when` is sometimes the point (a
    /// deliberately parked clause documents a branch the world can no
    /// longer reach), so this class warns with the recipe id + reason and
    /// never fails anything by itself.
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
/// design): [`LintSeverity::Error`] is the pre-E11 finding set plus the
/// entity-form-SPAWN check (dead recipes, shadowed whens, slot-violating
/// count gates, unknown tokens, unwritten aspect reads, …);
/// [`LintSeverity::Warning`] is the never-matching-`when`
/// detector (a recipe whose `when` is provably false for every §6.6
/// selection binding — advisory, because a dead `when` is sometimes the
/// point: the converter emits deliberate dead-marker twins under exactly
/// such negation whens) and the always-true world-form gate check (an
/// `exists(aspect:k…)`, a `count(aspect:k…) >= v`, or a
/// `total(table, aspect:k…) >= v` whose aspect the player cannot prevent —
/// borne from load, or riding a deck defaultcard after its first serve — so
/// the gate can never gate).
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
        // canonical key order. §6.9: an ambient verb with no recipes joins
        // the same family — its walk never selects, so no window ever opens
        // (slots cannot be the witness there: an ambient verb declares none,
        // a compile error).
        if (!verb.slots.is_empty() || verb.ambient) && verb.recipes.is_empty() {
            findings.push(LintFinding::error(if verb.ambient {
                format!(
                    "verb {name}: declares no recipes — the ambient walk never opens a window (§6.9)"
                )
            } else {
                format!(
                    "verb {name}: declares slots but no recipes — every action rejects at selection (§6.6)"
                )
            }));
        }
        // the advisory dead-slot face (§6.1, §6.4): a held slot no recipe
        // reads can never flip the deferred selection — the player can fill
        // it to no effect. Recipes exist (the empty case is the ERROR above)
        // and none of their whens reaches the held scope.
        if verb.slots.iter().any(|s| s.role == "held")
            && !verb.recipes.is_empty()
            && verb.recipes.iter().all(|r| {
                r.when
                    .as_deref()
                    .is_none_or(|w| parse_predicate(w, eff).is_ok_and(|p| !pred_reads_held(&p)))
            })
        {
            findings.push(LintFinding {
                severity: LintSeverity::Warning,
                message: format!(
                    "verb {name}: declares a held slot but no recipe reads it — no hold can flip the deferred selection (§6.1, §6.4)"
                ),
            });
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
            // one `when`, one verdict: the deadness errors below (the held
            // ERROR face, the count-gate) and the never-matching warning
            // name the SAME deadness (an error that already proves the when
            // false), so the error speaks and the warning stands down. A
            // count over a role the verb never declared has no error to
            // stand down for — its count binds nothing (§6.6), and the
            // warning is the only finding that names it.
            let mut gate_error = false;
            // the held ERROR face (§6.1, §6.3): a recipe reading the held
            // scope on a verb that declares no held slot — the scope is
            // ALWAYS empty there (no HOLD step can ever bind), so every
            // held-reading clause is dead content. The slot vocabulary is
            // exact: a `held.container` rel role reads the scope the same way.
            if let Ok(pred) = parse_predicate(when, eff) {
                if pred_reads_held(&pred) && !slot_of.contains_key("held") {
                    gate_error = true;
                    findings.push(LintFinding::error(format!(
                        "recipe {id} ({name}): reads the held scope but {name} declares no held slot — the scope is always empty there (§6.1, §6.3)"
                    )));
                }
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
            // never a panic (the lint never throws). A deadness error on
            // this recipe (the held face, the count-gate) already reported
            // it (one root cause, one finding); without one, the detector
            // speaks.
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

    // The dead-chain warning (§6.7): a chain-op list whose EVERY candidate's
    // target `when` is PROVABLY dead can never pin. Verdicts evaluate in the
    // gate's ACTUAL binding domain (§6.7's merged walk): slot/expire trigger
    // lists gate over the EMPTY bindings (§6.6 — event bindings are empty,
    // so the projection produces no refs and every role's count is 0), so a
    // verdict over the empty-binding domain is exactly can-never-pin there;
    // recipe and `on complete` lists gate over merged LIVE bindings (the
    // parent overlay, dead refs dropping — the static model pins no single
    // interval per role), so the warning demands deadness under BOTH the
    // §6.1 selection domain AND the empty-binding domain — deadness
    // independent of the binding counts. Conservative at the documented
    // boundary: world reads (`exists`, `count(<query>)`) are Unknown, a
    // when-less default candidate is always live, ONE live candidate
    // silences the whole list. The owner is the carrying recipe or trigger;
    // the walk runs on the compiled outcomes, so a chain op inside an IF
    // branch or an expanded op/function call is still seen.
    let dead_chain_check = |findings: &mut Vec<LintFinding>,
                            owner: &str,
                            outs: &[Outcome],
                            empty_bindings: bool| {
        for_each_authored_outcome_infallible(outs, &mut |o| {
            if o.op != "RUN" && o.op != "FORK" {
                return;
            }
            let candidates: Vec<&str> = o
                .args
                .iter()
                .filter_map(|a| match a {
                    Arg::Ref { r#ref: Ref::Name { name, .. } } => Some(name.as_str()),
                    _ => None,
                })
                .collect();
            if candidates.is_empty() {
                return;
            }
            let all_dead = candidates.iter().all(|id| {
                let Some(tv_name) = compiled.content.recipe_verbs.get(*id) else {
                    return false;
                };
                let Some(tv) = compiled.content.verbs.get(tv_name) else {
                    return false;
                };
                let Some(tr) = tv.recipes.iter().find(|cr| cr.id == *id) else {
                    return false;
                };
                // a when-less candidate is the default recipe — always live
                let Some(w) = &tr.when else { return false };
                let selection_ranges: IndexMap<String, WhenRange> = tv
                    .slot_by_role
                    .iter()
                    .map(|(role, s)| (role.clone(), (s.min, s.max)))
                    .collect();
                // the empty-binding domain: every declared role's count is
                // 0 (an undeclared role binds nothing either way, §6.6)
                let empty_ranges: IndexMap<String, WhenRange> =
                    tv.slot_by_role.keys().map(|role| (role.clone(), (0, Some(0)))).collect();
                let dead_over = |ranges: &IndexMap<String, WhenRange>| {
                    let mut cx = WhenCx::new(ranges);
                    matches!(when_verdict(w, false, &mut cx), WhenVerdict::Dead(_))
                };
                if empty_bindings {
                    dead_over(&empty_ranges)
                } else {
                    dead_over(&selection_ranges) && dead_over(&empty_ranges)
                }
            });
            if all_dead {
                findings.push(LintFinding {
                        severity: LintSeverity::Warning,
                        message: format!(
                            "{owner}: the {} list can never pin — every candidate's when is dead (§6.7)",
                            o.op
                        ),
                    });
            }
        });
    };
    for v in compiled.content.verbs.values() {
        for r in &v.recipes {
            dead_chain_check(&mut findings, &format!("recipe {}", r.id), &r.out, false);
        }
    }
    for t in &compiled.content.triggers {
        // §6.6: slot/expire bodies gate over the EMPTY bindings; an `on
        // complete` body sees the window's re-resolved bindings (§6.7) — the
        // recipe-body demand
        let empty_bindings = !matches!(t, CompiledTrigger::Complete { .. });
        dead_chain_check(&mut findings, &format!("trigger {}", t.id()), t.out(), empty_bindings);
    }

    // the held ERROR face over recipe BODIES (§6.1, §6.3): an IF condition
    // reading the held scope on a verb that declares no held slot — the
    // same dead content the when-face above names, one branch deeper (the
    // held-reading arm can never take). The walk runs on the compiled
    // outcomes, so an IF inside an expanded call is still seen; one finding
    // per recipe.
    let mut held_if_seen: IndexSet<String> = IndexSet::new();
    for (vname, v) in &compiled.content.verbs {
        let held_slot = v.slot_by_role.contains_key("held");
        for r in &v.recipes {
            for_each_authored_outcome_infallible(&r.out, &mut |o| {
                if o.op != "IF" || held_slot {
                    return;
                }
                let Some(arms) = &o.arms else { return };
                if arms.iter().any(|arm| pred_reads_held(&arm.when))
                    && held_if_seen.insert(r.id.clone())
                {
                    findings.push(LintFinding::error(format!(
                        "recipe {} ({vname}): an IF condition reads the held scope but {vname} declares no held slot — the scope is always empty there (§6.1, §6.3)",
                        r.id
                    )));
                }
            });
        }
    }

    // template tokens: {actors} {object} {with} {at} {about} {to} {held}
    // {roll} — a typo renders literally at render time (§5.3); function
    // bodies also interpolate their params (§10)
    let known: Vec<&str> = role_names().into_iter().chain(["roll"]).collect();
    let scan_tokens = |findings: &mut Vec<LintFinding>, where_: &str, s: &str, extra: &[&str]| {
        for cap in template_token_re().captures_iter(s) {
            let tok = grp_text(&cap, 1);
            if !known.contains(&tok) && !extra.contains(&tok) {
                findings.push(LintFinding::error(format!(
                    "{where_}: unknown template token {{{tok}}} — roles are {{actors}} {{object}} {{with}} {{at}} {{about}} {{to}} {{held}}, plus {{roll}} (§5.3)"
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
    // A UNIQUE entry is exempt too: its mint face skips while a copy lives
    // (§4.2), so the second serve deals nothing and the deck never pins.
    let declared = declared_entities(&script.entities);
    let (stackable_names, stackable_kinds) =
        collect_stackable(&declared, script.content.prototypes.as_ref());
    let unique_names = collect_uniques(script.content.prototypes.as_ref());
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
                if unique_names.contains(entry) {
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

    // W1: the always-true world-form gate (the WARNING class) — the inverse
    // face of the no-writer check above. An `exists(aspect:k…)`, an
    // open-ended `count(aspect:k…) >= v` gate, or an open-ended
    // `total(table, aspect:k…) >= v` sum whose aspect rides cards the
    // player cannot prevent reads true in every relevant world, so the gate
    // can never gate. Two bearer faces: an aspect borne FROM LOAD — a
    // declared `entities:` line — reads true in every load-present world
    // (the count form counts these cards; the total form sums their
    // levels); a deck `defaultcard` (whose aspects ride its prototype) reads true
    // once the deck's first serve lands, so its finding carries that face
    // (before the serve the gate genuinely gates — and the count/total
    // forms never read a defaultcard: the load-present pool does not include
    // it). `total(bound, …)` is window-local — the bindings can genuinely
    // gate — and never warns.
    // Silent on a name-constrained form (`exists(name:X aspect:k)` — the
    // name half can genuinely gate), on a multi-term or negated query, on a
    // closed count comparison (the pool can grow past it), on an aspect
    // with no writer (the no-writer error above is that finding), and on
    // `state.*` reads (an attrs write can flip them at play time). The
    // claim must also SURVIVE PLAY: an authored outcome that kills a
    // bearer (`DESTROY <bearer>` / `REPLACE <bearer> WITH …`) stands the
    // bearer out of the finding — an aspect that can leave the world can
    // genuinely gate.
    let load_bearers: Vec<(&str, &Aspects)> =
        script.entities.iter().map(|e| (e.name.as_str(), &e.aspects)).collect();
    let deck_bearers: Vec<(&str, &str, &Aspects)> = compiled
        .content
        .decks
        .values()
        .filter_map(|d| {
            let dc = d.defaultcard.as_ref()?;
            let aspects = compiled.content.prototypes.get(&dc.name)?.aspects.as_ref()?;
            Some((dc.name.as_str(), d.id.as_str(), aspects))
        })
        .collect();
    // the killed-name census: every name a DESTROY/REPLACE first arg
    // reaches, walked on the compiled outcomes — the same walk E1 uses
    let mut killed: IndexSet<String> = IndexSet::new();
    let scan_kills = |killed: &mut IndexSet<String>, outs: &[Outcome]| {
        for_each_authored_outcome_infallible(outs, &mut |o| {
            if o.op != "DESTROY" && o.op != "REPLACE" {
                return;
            }
            if let Some(Arg::Ref { r#ref: Ref::Name { name, .. } }) = o.args.first() {
                killed.insert(name.clone());
            }
        });
    };
    for (_, v) in &compiled.content.verbs {
        for r in &v.recipes {
            scan_kills(&mut killed, &r.out);
        }
    }
    for t in &compiled.content.triggers {
        scan_kills(&mut killed, t.out());
    }
    // the one positive `aspect:k` term of a world form's query — None for
    // any multi-group, multi-term, negated, or name/kind/state-bearing form
    fn single_aspect_term(query: &Query) -> Option<(&str, Option<CmpOp>, i64)> {
        let [group] = query.as_slice() else { return None };
        let [term] = group.as_slice() else { return None };
        match term {
            QueryTerm::Aspect { neg: false, path, op, level } => Some((path.as_str(), *op, *level)),
            _ => None,
        }
    }
    fn aspect_term_text(path: &str, op: Option<CmpOp>, level: i64) -> String {
        match op {
            Some(op) => format!("aspect:{path}{}{level}", op.as_str()),
            None => format!("aspect:{path}"),
        }
    }
    /// The Q13 read of one bearer's aspect map by dotted path — the same
    /// two shapes `aspect_get` walks on an Entity (a scalar leaf, or a
    /// one-level sub-map's leaf), scoped to the map alone.
    fn bearer_aspect(a: &Aspects, path: &str) -> Option<i64> {
        match path.find('.') {
            None => a.get(path).and_then(|v| match v {
                crate::core::types::AspectVal::Num(n) => Some(*n),
                crate::core::types::AspectVal::Sub(_) => None,
            }),
            Some(dot) => match a.get(&path[..dot]) {
                // a sub-map's leaves are all numbers (Q13)
                Some(crate::core::types::AspectVal::Sub(m)) => m.get(&path[dot + 1..]).copied(),
                _ => None,
            },
        }
    }
    /// The comparison flipped operand-order (`3 <= count(q)` ≡
    /// `count(q) >= 3`) — the count form normalizes the int literal to the
    /// right before its op checks.
    fn mirror_cmp_op(op: CmpOp) -> CmpOp {
        match op {
            CmpOp::Ge => CmpOp::Le,
            CmpOp::Le => CmpOp::Ge,
            CmpOp::Gt => CmpOp::Lt,
            CmpOp::Lt => CmpOp::Gt,
            CmpOp::Eq => CmpOp::Eq,
            CmpOp::Ne => CmpOp::Ne,
        }
    }
    /// The W1 walk's shared context, threaded as one borrow: the two
    /// bearer tables, the killed-name census and the written-path set.
    struct W1Cx<'a> {
        load_bearers: &'a [(&'a str, &'a Aspects)],
        deck_bearers: &'a [(&'a str, &'a str, &'a Aspects)],
        killed: &'a IndexSet<String>,
        writes: &'a IndexSet<String>,
    }
    fn w1_walk(cx: &W1Cx<'_>, p: &Pred, id: &str, verb: &str, out: &mut IndexSet<String>) {
        let W1Cx { load_bearers, deck_bearers, killed, writes } = *cx;
        match p {
            Pred::And(a, b) | Pred::Or(a, b) => {
                w1_walk(cx, a, id, verb, out);
                w1_walk(cx, b, id, verb, out);
            }
            // a NOT-wrapped world form flips the when's truth — the
            // never-matching detector's territory, never the always-true one
            Pred::Not(_) => {}
            Pred::ExistsQuery { query } => {
                let Some((path, op, level)) = single_aspect_term(query) else {
                    return;
                };
                if !writes.contains(path) {
                    return;
                }
                let meets = |(name, a): &(&str, &Aspects)| {
                    !killed.contains(*name)
                        && bearer_aspect(a, path).is_some_and(|v| match op {
                            None => v >= level,
                            Some(op) => op.num_cmp(v, level),
                        })
                };
                let bearing: Vec<&str> =
                    load_bearers.iter().copied().filter(meets).map(|(n, _)| n).collect();
                if !bearing.is_empty() {
                    out.insert(format!(
                        "recipe {id} ({verb}): exists({}) is always true — aspect '{path}' is borne from load by {}, so the gate can never gate (§6.3)",
                        aspect_term_text(path, op, level),
                        bearing.join(", "),
                    ));
                    return;
                }
                // the defaultcard face: the aspect rides the served default
                // card, so the gate reads false only before the deck's
                // first serve — the finding names that face, not from-load
                let served: Vec<String> = deck_bearers
                    .iter()
                    .filter(|(n, _, a)| {
                        !killed.contains(*n)
                            && bearer_aspect(a, path).is_some_and(|v| match op {
                                None => v >= level,
                                Some(op) => op.num_cmp(v, level),
                            })
                    })
                    .map(|(n, _, _)| n.to_string())
                    .collect();
                if !served.is_empty() {
                    out.insert(format!(
                        "recipe {id} ({verb}): exists({}) is always true once the deck serves its default card {} — aspect '{path}' rides the default card, so the gate can never gate after the deck's first serve (§6.3)",
                        aspect_term_text(path, op, level),
                        served.join(", "),
                    ));
                }
            }
            Pred::Cmp { l, r, op } => {
                // the count and total(table, …) forms, operands normalized
                // (the int literal to the right): only the open-ended
                // comparison warns — the load-present pool (count) and the
                // load-present sum (total) are floors, and a floor already
                // over the threshold never gates. Both read LOAD bearers
                // only: a defaultcard is not on the table before its deck's
                // first serve, and a killed bearer cannot hold the floor.
                let (expr, op, k) = match (l, r) {
                    (e, ValExpr::Int { v }) => (e, *op, *v),
                    (ValExpr::Int { v }, e) => (e, mirror_cmp_op(*op), *v),
                    _ => return,
                };
                if !matches!(op, CmpOp::Ge | CmpOp::Gt) {
                    return;
                }
                match expr {
                    ValExpr::CountQuery { query } => {
                        let Some((path, aop, level)) = single_aspect_term(query) else {
                            return;
                        };
                        if !writes.contains(path) {
                            return;
                        }
                        let n = load_bearers
                            .iter()
                            .filter(|(name, a)| {
                                !killed.contains(*name)
                                    && bearer_aspect(a, path).is_some_and(|v| match aop {
                                        None => v >= level,
                                        Some(op) => op.num_cmp(v, level),
                                    })
                            })
                            .count() as i64;
                        if !op.num_cmp(n, k) {
                            return;
                        }
                        out.insert(format!(
                            "recipe {id} ({verb}): count({}) {} {k} is always true — aspect '{path}' is borne from load by {n} card(s) from load, so the gate can never gate (§6.3)",
                            aspect_term_text(path, aop, level),
                            op.as_str(),
                        ));
                    }
                    ValExpr::TotalTable { path } => {
                        if !writes.contains(path) {
                            return;
                        }
                        // the summed level over the load-present cards
                        // (missing → 0, the engine's house rule)
                        let n: i64 = load_bearers
                            .iter()
                            .filter(|(name, _)| !killed.contains(*name))
                            .map(|(_, a)| bearer_aspect(a, path).unwrap_or(0))
                            .sum();
                        if !op.num_cmp(n, k) {
                            return;
                        }
                        out.insert(format!(
                            "recipe {id} ({verb}): total(table, aspect:{path}) {} {k} is always true — aspect '{path}' is borne from load totaling {n}, so the gate can never gate (§6.3)",
                            op.as_str(),
                        ));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for (name, verb) in verbs {
        for (i, r) in verb.recipes.iter().enumerate() {
            let Some(when) = &r.when else { continue };
            // every when parsed at compile — a failure is skipped, never a
            // panic (the lint never throws)
            let Ok(pred) = parse_predicate(when, eff) else { continue };
            let id = r.id.clone().unwrap_or_else(|| recipe_id(name, r, i));
            let mut msgs: IndexSet<String> = IndexSet::new();
            let cx = W1Cx {
                load_bearers: &load_bearers,
                deck_bearers: &deck_bearers,
                killed: &killed,
                writes: &writes,
            };
            w1_walk(&cx, &pred, &id, name, &mut msgs);
            for m in msgs {
                findings.push(LintFinding { severity: LintSeverity::Warning, message: m });
            }
        }
    }

    // E1: SPAWN of a non-prototype — an un-kinded ref resolving ONLY to a
    // declared entity has no kind to mint: `apply_spawn` refuses the step
    // every time ("SPAWN <name>: no kind (prototype or kind:NAME
    // required)" — pinned by the ops suite), and the load is clean either
    // way (mint names resolve at runtime, §4.4). The fix is a prototype
    // (or a `_CARD` twin) under the same name; an entity declaring no
    // aspects gets the finding all the same — the refusal is identical, so
    // there is no bare-mint idiom to protect. A kinded ref
    // (`SPAWN kind:NAME`) carries its kind and mints — outside this
    // finding. The compile artifacts resolve the refs: a name in
    // `prototypes` spawns cleanly (an entity and a prototype cannot share
    // a name — the `_CARD` twin pattern), and a name in NEITHER table also
    // loads clean but names no declared entity, so it is outside this
    // finding's scope. The walk runs on the COMPILED content — op and
    // function bodies expand into the recipe/trigger outcomes at compile
    // (§10), so a SPAWN inside a call site is still seen.
    let mut spawn_seen: IndexSet<String> = IndexSet::new();
    let mut scan_spawns = |findings: &mut Vec<LintFinding>, owner: &str, outs: &[Outcome]| {
        for_each_authored_outcome_infallible(outs, &mut |o| {
            if o.op != "SPAWN" {
                return;
            }
            let Some(Arg::Ref { r#ref: Ref::Name { name, kind, .. } }) = o.args.first() else {
                return;
            };
            // a kinded ref carries its kind — the spawn mints (aspectless,
            // but nothing refuses)
            if kind.is_some() {
                return;
            }
            if compiled.content.prototypes.contains_key(name) {
                return;
            }
            if !script.entities.iter().any(|e| &e.name == name) {
                return; // neither table — names no declared entity
            }
            if spawn_seen.insert(format!("{owner}\u{0}{name}")) {
                findings.push(LintFinding::error(format!(
                    "{owner}: SPAWN {name} names a declared entity, not a prototype — the spawn has no kind to mint (prototype or kind:NAME required) and refuses at play every time; declare a prototype (or a `_CARD` twin) and spawn that (§4.2)"
                )));
            }
        });
    };
    for (vname, v) in &compiled.content.verbs {
        for r in &v.recipes {
            scan_spawns(&mut findings, &format!("recipe {} ({vname})", r.id), &r.out);
        }
    }
    for t in &compiled.content.triggers {
        scan_spawns(&mut findings, &format!("trigger {}", t.id()), t.out());
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
