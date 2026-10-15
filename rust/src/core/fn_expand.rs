//! Expansion: compile-time call substitution — ts: src/core/parse.ts:25-84
//! (FnTemplate/FnTable/collectFnCalls) and :646-853 (expandFnCalls and the
//! substitution helpers). CORE-01's sanctioned pure-move split: this layer
//! is only ever invoked by content compile, after `parse_outcomes` /
//! `parse_function_body`, BEFORE the load guards — so expanded bodies are
//! guarded exactly like literal ones and the engine (and therefore the log)
//! never sees a call.
//!
//! Canonical equals expansion: a call site IS its expanded body, byte for
//! byte (parse.ts:654-655).

use indexmap::{IndexMap, IndexSet};

use crate::core::clock::format_duration;
use crate::core::dict::OUTCOME_OPS;
use crate::core::predicate::{Pred, ValExpr};
use crate::core::sentence::{Arg, IfArm, Outcome};
use crate::js::{static_regex, ChorusError, Result};
use std::sync::OnceLock;
use regex::Regex;

/// ts: src/core/parse.ts:30-34 — one declared function: its parameter names
/// and its body compiled to a TEMPLATE — an outcome list that may still hold
/// param placeholders and nested `FN` call nodes.
#[derive(Debug, Clone)]
pub struct FnTemplate {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Outcome>,
}

/// ts: src/core/parse.ts:39-41 — the load-time function table, threaded
/// EXPLICITLY into every parse that may contain calls (never Dictionary
/// state). byName MUST preserve declaration order (IndexMap): iteration
/// feeds `unknownCallError`'s "declared functions:" list (parse.ts:845/849,
/// CORE-06) and the content-compile walk points (content.ts:543/571/585).
#[derive(Debug, Clone, Default)]
pub struct FnTable {
    pub by_name: IndexMap<String, FnTemplate>,
}

/// ts: src/core/parse.ts:73-84 — every function a body calls, directly
/// (branch bodies included) — the call graph content compile walks for
/// recursion cycles and the depth cap. IndexSet, NOT HashSet: the TS
/// `Set<string>` iterates in INSERTION order and content.ts pass-3
/// (:545-561, W2) observes it — the FIRST undeclared name enters the error
/// text (:548) and the DFS decides which cycle is found first and how
/// `a -> b -> a` spells (wave-1 must-fix-2).
pub fn collect_fn_calls(out: &[Outcome], into: &mut IndexSet<String>) {
    for o in out {
        if o.op == "FN" {
            if let Some(call) = &o.call {
                into.insert(call.clone());
            }
        } else if o.op == "IF" {
            if let Some(arms) = &o.arms {
                for arm in arms {
                    collect_fn_calls(&arm.then, into);
                    // conditions carry no calls — params substitute into cmp
                    // operands only
                }
            }
            if let Some(fallback) = &o.fallback {
                collect_fn_calls(fallback, into);
            }
        }
    }
}

/// ts: src/core/parse.ts:676-678 — replace every FN call in an authored
/// outcome list with its function's body, substituting parameters.
pub fn expand_fn_calls(out: &[Outcome], fns: &FnTable) -> Result<Vec<Outcome>> {
    expand_list(out, fns, &mut IndexMap::new(), None)
}

/// ts: src/core/parse.ts:682 — tokens a body string may carry that are never
/// parameter names: the §5.3 prose substitutions and F4's {roll}. The SET
/// INSERTION ORDER is load-bearing: `undeclaredTokenError` joins it
/// (`{${[...FN_RESERVED_TOKENS].join('} {')}}`, parse.ts:784).
pub const FN_RESERVED_TOKENS: [&str; 7] = ["actors", "object", "with", "at", "about", "to", "roll"];

/// ts: src/core/parse.ts:686 — nesting cap for legitimate chains (§9.40):
/// depth(f) = 1 + max(depth of its callees), a body with no calls is depth 1
/// — depth 9 is a load error (rejected over the call graph at content load).
pub const MAX_FN_DEPTH: u32 = 8;

/// ts: src/core/parse.ts:688-715
fn expand_list(
    list: &[Outcome],
    fns: &FnTable,
    bind: &mut IndexMap<String, Arg>,
    fn_: Option<&str>,
) -> Result<Vec<Outcome>> {
    let mut out: Vec<Outcome> = Vec::new();
    for o in list {
        if o.op == "FN" {
            let Some(name) = &o.call else {
                // ts: parse.ts:692 `o.call!` — parse-fn_call always sets it;
                // hand-built FN nodes without a call are a programmer error
                unreachable!("FN node without a call (ts: parse.ts:692)")
            };
            let Some(tpl) = fns.by_name.get(name) else {
                return Err(ChorusError::new(format!(
                    "internal: call to undeclared function '{name}' survived parsing"
                )));
            };
            // bind this call's args (themselves substituted by the enclosing
            // function's parameters, if any) — always concrete before
            // expansion
            let mut bind2: IndexMap<String, Arg> = IndexMap::new();
            for (i, p) in tpl.params.iter().enumerate() {
                let Some(arg) = o.args.get(i) else {
                    // ts: parse.ts:698 `o.args[i]!` — arity checked at
                    // fn_call; hand-built lists violating it are a
                    // programmer error (rule 11)
                    unreachable!("FN call missing arg {i} for '{name}' (ts: parse.ts:698)")
                };
                bind2.insert(p.clone(), subst_arg(arg, fn_, bind)?);
            }
            let body = expand_list(&tpl.body, fns, &mut bind2, Some(name))?;
            out.extend(body);
        } else if o.op == "IF" {
            let arms: Vec<IfArm> = o
                .arms
                .as_deref()
                .unwrap_or(&[])
                .iter()
                .map(|a| {
                    Ok(IfArm {
                        when: match fn_ {
                            Some(fn_name) => subst_pred(&a.when, fn_name, bind)?,
                            None => a.when.clone(),
                        },
                        then: expand_list(&a.then, fns, bind, fn_)?,
                    })
                })
                .collect::<Result<Vec<IfArm>>>()?;
            let fallback = match &o.fallback {
                Some(fallback) => Some(expand_list(fallback, fns, bind, fn_)?),
                None => None,
            };
            out.push(Outcome {
                op: "IF".to_string(),
                args: Vec::new(),
                arms: Some(arms),
                fallback,
                call: None,
            });
        } else {
            let args = match fn_ {
                Some(_) => o
                    .args
                    .iter()
                    .map(|a| subst_arg(a, fn_, bind))
                    .collect::<Result<Vec<Arg>>>()?,
                None => o.args.clone(),
            };
            out.push(Outcome {
                op: o.op.clone(),
                args,
                arms: None,
                fallback: None,
                call: None,
            });
        }
    }
    Ok(out)
}

/// ts: src/core/parse.ts:718-759 — substitute one arg against the enclosing
/// function's bindings.
fn subst_arg(a: &Arg, fn_: Option<&str>, bind: &mut IndexMap<String, Arg>) -> Result<Arg> {
    match a {
        Arg::Param { name, sign } => {
            let Some(bound) = bind.get(name) else {
                return Err(ChorusError::new(format!(
                    "internal: parameter '{name}' of '{}' has no binding",
                    fn_.unwrap_or("?")
                )));
            };
            if let Some(sign) = sign {
                // a delta position (`+n`/`-n`): fold the authored sign into
                // the call's number; anything else splices through for the
                // runtime kind error
                return Ok(match bound {
                    Arg::Int { v } => Arg::Delta {
                        v: sign.apply(*v),
                    },
                    Arg::Delta { v } => Arg::Delta {
                        v: sign.apply(*v),
                    },
                    other => other.clone(),
                });
            }
            Ok(bound.clone())
        }
        Arg::Vexpr { v } => {
            // §9.41: a ROLL argument is a value expression; a body template
            // may carry a param operand inside it (`ROLL(n, 6)`), substituted
            // by the same rule as IF-condition cmp operands
            Ok(Arg::Vexpr {
                v: subst_expr(v, fn_.unwrap_or("?"), bind)?,
            })
        }
        Arg::Ref {
            r#ref: crate::core::r#ref::Ref::Param { name },
        } => {
            // whole-ref position: the bound arg splices in as-is — a ref or a
            // role ref behaves exactly like the same text authored at the
            // call site; a value literal lands where a ref belongs and is
            // rejected at runtime
            let Some(bound) = bind.get(name) else {
                return Err(ChorusError::new(format!(
                    "internal: parameter '{name}' has no binding"
                )));
            };
            Ok(bound.clone())
        }
        Arg::Into {
            r#ref: crate::core::sentence::IntoTarget::Ref(crate::core::r#ref::Ref::Param { name }),
        } => {
            let Some(bound) = bind.get(name) else {
                return Err(ChorusError::new(format!(
                    "internal: parameter '{name}' has no binding"
                )));
            };
            match bound {
                Arg::Ref { r#ref } => Ok(Arg::Into {
                    r#ref: crate::core::sentence::IntoTarget::Ref(r#ref.clone()),
                }),
                Arg::Role { role } => Ok(Arg::Into {
                    r#ref: crate::core::sentence::IntoTarget::Role { role: *role },
                }),
                other => Err(ChorusError::new(format!(
                    "function '{}': parameter '{name}' stands in an INTO position — the call supplies a {}, not a ref or role",
                    fn_.unwrap_or("?"),
                    other.tag()
                ))),
            }
        }
        Arg::Str { v } => match fn_ {
            Some(fn_name) => Ok(Arg::Str {
                v: subst_string(v, fn_name, bind)?,
            }),
            None => Ok(a.clone()),
        },
        _ => Ok(a.clone()),
    }
}

/// ts: src/core/parse.ts:769-777 — `{param}` → the call-site arg's canonical
/// spelling, inside a function body string. One NON-RESCANNING pass over the
/// body string (JS /g replace ≡ single-pass non-overlapping, and the
/// replacements are never rescanned — regex `replace_all` + manual assembly
/// matches exactly): the exact spelling `{name}` substitutes (reserved
/// tokens pass through; undeclared names are a load error); a SPACED
/// spelling (`{ x }`, `{ actors }`) never rewrites. Commas/quotes inside
/// braces are ordinary prose.
fn subst_string(s: &str, fn_: &str, bind: &mut IndexMap<String, Arg>) -> Result<String> {
    // appendix A.1: parse.ts:770 — `\{(\s*)([A-Za-z_][A-Za-z0-9_]*)(\s*)\}`
    // with the JS `\s` class (Q8); R (replace_all + captures, $-free)
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = static_regex(&RE, || {
        // ts: parse.ts:770 — the exact TS shape `\{(\s*)(name)(\s*)\}`:
        // the whitespace classes are CAPTURE GROUPS (pre = $1, post = $3)
        let mut pat = String::from("\\{("); // regex \{(
        pat.push_str(crate::js::JS_WS_CLASS);
        pat.push_str("*)");
        pat.push_str("([A-Za-z_][A-Za-z0-9_]*)");
        pat.push('(');
        pat.push_str(crate::js::JS_WS_CLASS);
        pat.push_str("*)\\}"); // )}

        Regex::new(&pat)
    });
    let mut out = String::new();
    let mut last = 0;
    for caps in re.captures_iter(s) {
        let m = caps.get(0).map(|m| (m.start(), m.end())).unwrap_or_else(|| {
            unreachable!("regex group 0 is always present") // rule 11
        });
        out.push_str(&s[last..m.0]);
        let whole = &s[m.0..m.1];
        // TS closure receives pre/name/post; both spaced spellings error
        let pre = caps.get(1).map(|g| g.as_str()).unwrap_or_default();
        let name = caps.get(2).map(|g| g.as_str()).unwrap_or_default();
        let post = caps.get(3).map(|g| g.as_str()).unwrap_or_default();
        if !pre.is_empty() || !post.is_empty() {
            return Err(undeclared_token_error(fn_, whole));
        }
        if FN_RESERVED_TOKENS.contains(&name) {
            out.push_str(whole);
        } else {
            let Some(bound) = bind.get(name) else {
                return Err(undeclared_token_error(fn_, whole));
            };
            let spelling = arg_spelling(bound, fn_, name)?;
            out.push_str(&spelling);
        }
        last = m.1;
    }
    out.push_str(&s[last..]);
    Ok(out)
}

/// ts: src/core/parse.ts:781-788 — the undeclared-interpolation load error:
/// names the token, the function, and the reserved set it would have had to
/// belong to.
#[must_use]
fn undeclared_token_error(fn_: &str, token: &str) -> ChorusError {
    ChorusError::new(format!(
        "function '{fn_}': '{token}' in a body string names neither a parameter of '{fn_}' nor a reserved token ({{{}}}) — undeclared interpolation is a load error, not a silent literal (braced tokens must be spelled exactly, without inner spaces)",
        FN_RESERVED_TOKENS.join("} {")
    ))
}

/// ts: src/core/parse.ts:791-808 — the canonical spelling of a call-site
/// arg, for string interpolation. CORE-13 (标红): the str/pair spellings are
/// DELIBERATELY UNQUOTED here — format.rs's format_arg quotes them. The two
/// spellings are frozen separately; merging them would break byte-for-byte
/// expansion. Only delta/int/dur/bool spellings are shared with format_arg.
fn arg_spelling(a: &Arg, fn_: &str, param: &str) -> Result<String> {
    match a {
        Arg::Ref { r#ref } => match r#ref {
            crate::core::r#ref::Ref::Id { id } => Ok(format!("#{id}")),
            crate::core::r#ref::Ref::Param { .. } => Err(ChorusError::new(format!(
                "internal: parameter '{param}' bound to a parameter"
            ))),
            crate::core::r#ref::Ref::Name { kind, name } => Ok(match kind {
                Some(k) => format!("{k}:{name}"),
                None => name.clone(),
            }),
        },
        Arg::Role { role } => Ok(role.as_str().to_uppercase()),
        Arg::Int { v } => Ok(v.to_string()),
        Arg::Delta { v } => Ok(if *v < 0 {
            v.to_string()
        } else {
            format!("+{v}")
        }),
        Arg::Str { v } => Ok(v.clone()), // RAW — the deliberate unquoted spelling (CORE-13)
        Arg::Bool { v } => Ok(v.to_string()),
        Arg::Dur { v } => format_duration(*v),
        Arg::Key { v } => Ok(v.clone()),
        Arg::Pair { k, v } => {
            // ts: parse.ts:804 `${a.k}=${String(a.v)}` — the value RAW
            // (formatArg quotes it; CORE-13's frozen asymmetry)
            Ok(format!("{k}={}", v.to_js_string()))
        }
        other => Err(ChorusError::new(format!(
            "function '{fn_}': parameter '{param}' cannot interpolate into a string (bound to a {})",
            other.tag()
        ))),
    }
}

/// ts: src/core/parse.ts:813-823 — substitute a condition: parameters stand
/// in cmp operands (numeric literal positions); quantifiers/exists/query
/// terms take none — the grammar already rejected them at parse.
fn subst_pred(p: &Pred, fn_: &str, bind: &mut IndexMap<String, Arg>) -> Result<Pred> {
    match p {
        Pred::And(a, b) => Ok(Pred::And(
            Box::new(subst_pred(a, fn_, bind)?),
            Box::new(subst_pred(b, fn_, bind)?),
        )),
        Pred::Or(a, b) => Ok(Pred::Or(
            Box::new(subst_pred(a, fn_, bind)?),
            Box::new(subst_pred(b, fn_, bind)?),
        )),
        Pred::Not(a) => Ok(Pred::Not(Box::new(subst_pred(a, fn_, bind)?))),
        Pred::Cmp { l, op, r } => Ok(Pred::Cmp {
            l: subst_expr(l, fn_, bind)?,
            op: *op,
            r: subst_expr(r, fn_, bind)?,
        }),
        // rel/every/any/exists/existsQuery carry no parameters
        _ => Ok(p.clone()),
    }
}

/// ts: src/core/parse.ts:825-838
fn subst_expr(x: &ValExpr, fn_: &str, bind: &mut IndexMap<String, Arg>) -> Result<ValExpr> {
    if let ValExpr::Param { name } = x {
        let Some(bound) = bind.get(name) else {
            return Err(ChorusError::new(format!(
                "internal: parameter '{name}' of '{fn_}' has no binding"
            )));
        };
        return match bound {
            Arg::Int { v } => Ok(ValExpr::Int { v: *v }),
            // the call's signed magnitude…
            Arg::Delta { v } => Ok(ValExpr::Int { v: *v }),
            // durations are minutes
            Arg::Dur { v } => Ok(ValExpr::Int { v: *v }),
            other => Err(ChorusError::new(format!(
                "function '{fn_}': parameter '{name}' stands in an IF condition — the call supplies a {}, not a number",
                other.tag()
            ))),
        };
    }
    Ok(x.clone())
}

/// ts: src/core/parse.ts:843-853 — the unknown `NAME(...)` compile error:
/// the full outcome vocabulary — ops, then declared functions — so the
/// author can see what IS legal; a name that looks like a predicate helper
/// gets the forgotten-IF hint.
#[must_use]
pub fn unknown_call_error(name: &str, fns: Option<&FnTable>) -> String {
    let pred_helpers = ["exists", "count", "total", "rel", "every", "any"];
    let fns_listed: Vec<&str> = match fns {
        Some(fns) => fns.by_name.keys().map(String::as_str).collect(),
        None => Vec::new(),
    };
    format!(
        "unknown '{name}(...)' in outcome position — the outcome vocabulary is: {ops}{fns}{hint} (bare names spawn — an op's parens are part of its grammar; calls need a declared function)",
        ops = OUTCOME_OPS.join(", "),
        fns = if !fns_listed.is_empty() {
            format!("; declared functions: {}", fns_listed.join(", "))
        } else {
            "; no functions declared".to_string()
        },
        hint = if pred_helpers.contains(&name) {
            format!(" (a condition wants the IF keyword: IF …{name}(…) -> ( outcomes ))")
        } else {
            String::new()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dict::core_dictionary;
    use crate::core::dict::Dictionary;
    use crate::core::parse::{parse_function_body, parse_outcomes};
    use crate::core::sentence::Sign;

    fn dict() -> Dictionary {
        core_dictionary()
    }

    fn table(name: &str, params: &[&str], body: &str) -> FnTable {
        let d = dict();
        let mut fns = FnTable::default();
        let body = parse_function_body(body, &d, &fns, &params.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
        fns.by_name.insert(
            name.to_string(),
            FnTemplate {
                name: name.to_string(),
                params: params.iter().map(|s| s.to_string()).collect(),
                body,
            },
        );
        fns
    }

    #[test]
    fn expands_a_call_to_its_body_byte_for_byte() {
        let d = dict();
        // a param in a REF position is the bare name inside a body
        let fns = table("GIVE_RUMOUR", &["who"], "SPAWN RUMOUR, DESTROY who");
        let call = parse_outcomes("GIVE_RUMOUR(#x1)", &d, Some(&fns)).unwrap();
        let expanded = expand_fn_calls(&call, &fns).unwrap();
        assert_eq!(expanded.len(), 2);
        assert_eq!(expanded[0].op, "SPAWN");
        // the param ref spliced into DESTROY's arg position
        match &expanded[1].args[0] {
            Arg::Ref { r#ref } => assert_eq!(r#ref, &crate::core::r#ref::Ref::Id { id: "x1".into() }),
            other => panic!("expected ref arg, got {other:?}"),
        }
    }

    #[test]
    fn string_interpolation_uses_the_raw_arg_spelling() {
        let d = dict();
        let fns = table("TELL", &["what", "n"], "NARRATE \"you find {what} x{n}\"");
        let call = parse_outcomes("TELL(\"a rumour\", 3)", &d, Some(&fns)).unwrap();
        let expanded = expand_fn_calls(&call, &fns).unwrap();
        match &expanded[0].args[0] {
            Arg::Str { v } => assert_eq!(v, "you find a rumour x3"),
            other => panic!("expected str arg, got {other:?}"),
        }
    }

    #[test]
    fn reserved_tokens_pass_through_and_spaced_braces_error() {
        let d = dict();
        // {actors} is a reserved prose token — never param-substituted
        let fns = table("P", &["x"], "NARRATE \"{actors} {x}\"");
        let call = parse_outcomes("P(7)", &d, Some(&fns)).unwrap();
        let expanded = expand_fn_calls(&call, &fns).unwrap();
        match &expanded[0].args[0] {
            Arg::Str { v } => assert_eq!(v, "{actors} 7"),
            other => panic!("expected str arg, got {other:?}"),
        }
        // a SPACED spelling is the undeclared-token load error (exact spelling only)
        let fns2 = table("Q", &["x"], "NARRATE \"{ x }\"");
        let call2 = parse_outcomes("Q(7)", &d, Some(&fns2)).unwrap();
        let err = expand_fn_calls(&call2, &fns2).unwrap_err();
        assert_eq!(
            err.msg,
            "function 'Q': '{ x }' in a body string names neither a parameter of 'Q' nor a reserved token ({actors} {object} {with} {at} {about} {to} {roll}) — undeclared interpolation is a load error, not a silent literal (braced tokens must be spelled exactly, without inner spaces)"
        );
    }

    #[test]
    fn delta_sign_folds_into_the_bound_number() {
        let d = dict();
        let fns = table("BUMP", &["n"], "NARRATE \"+{n}\", SET(level, +n), SET(level, -n)");
        let body_call = parse_outcomes("BUMP(2)", &d, Some(&fns)).unwrap();
        // +{n} interpolates "+2" into the string; +n/-n fold the authored
        // sign into the call's number (parse.ts:722-728)
        let expanded = expand_fn_calls(&body_call, &fns).unwrap();
        match (&expanded[0].args[0], &expanded[1].args[1], &expanded[2].args[1]) {
            (Arg::Str { v }, Arg::Delta { v: a }, Arg::Delta { v: b }) => {
                assert_eq!(v, "+2");
                assert_eq!(*a, 2);
                assert_eq!(*b, -2);
            }
            other => panic!("expected str+deltas, got {other:?}"),
        }
    }

    #[test]
    fn arity_and_unknown_call_errors_list_the_vocabulary() {
        let d = dict();
        let fns = table("F", &["a", "b"], "NARRATE \"x\"");
        let call = parse_outcomes("F(1)", &d, Some(&fns)).unwrap_err();
        assert_eq!(call.msg, "function 'F' expects 2 argument(s) (a, b), got 1");
        // unknown NAME( lists ops then declared functions, insertion order
        let fns2 = {
            let mut t = FnTable::default();
            let d2 = dict();
            let body = parse_function_body("NARRATE \"x\"", &d2, &t, &[]).unwrap();
            t.by_name.insert("alpha".into(), FnTemplate { name: "alpha".into(), params: vec![], body });
            let body_zeta = parse_function_body("NARRATE \"y\"", &d2, &t, &[]).unwrap();
            t.by_name.insert("zeta".into(), FnTemplate { name: "zeta".into(), params: vec![], body: body_zeta });
            t
        };
        let err = parse_outcomes("count(actors)", &d, Some(&fns2)).unwrap_err();
        assert_eq!(
            err.msg,
            "unknown 'count(...)' in outcome position — the outcome vocabulary is: SPAWN, DESTROY, REPLACE, SET, MOVE, REVEAL, ADVANCE, NARRATE, PROGRESS, RELATE, UNRELATE, REFUSE, ENDING, ROLL, ROLLED, DRAW, DRAWN; declared functions: alpha, zeta (a condition wants the IF keyword: IF …count(…) -> ( outcomes )) (bare names spawn — an op's parens are part of its grammar; calls need a declared function)"
        );
    }

    #[test]
    fn into_position_rejects_non_ref_bindings_with_the_rule() {
        // the INTO target is the param form (bare param name in a body)
        let d2 = dict();
        let mut fns2 = FnTable::default();
        let body = parse_function_body("SPAWN RUMOUR INTO t", &d2, &fns2, &["t".to_string()]).unwrap();
        fns2.by_name.insert("M2".into(), FnTemplate { name: "M2".into(), params: vec!["t".into()], body });
        // call with a NUMBER binding — the INTO slot refuses with the exact text
        let call = parse_outcomes("M2(5)", &d2, Some(&fns2)).unwrap();
        let err = expand_fn_calls(&call, &fns2).unwrap_err();
        assert_eq!(
            err.msg,
            "function 'M2': parameter 't' stands in an INTO position — the call supplies a int, not a ref or role"
        );
    }

    #[test]
    fn collect_fn_calls_walks_branch_bodies() {
        // undeclared fns fail at parse, so build the tree by hand
        let arm = IfArm {
            when: Pred::Cmp {
                l: ValExpr::Int { v: 1 },
                op: crate::core::query::CmpOp::Eq,
                r: ValExpr::Int { v: 1 },
            },
            then: vec![Outcome {
                op: "FN".into(),
                args: vec![],
                arms: None,
                fallback: None,
                call: Some("GIVE".into()),
            }],
        };
        let o = Outcome {
            op: "IF".into(),
            args: vec![],
            arms: Some(vec![arm]),
            fallback: Some(vec![Outcome {
                op: "FN".into(),
                args: vec![],
                arms: None,
                fallback: None,
                call: Some("TAKE".into()),
            }]),
            call: None,
        };
        let mut into = IndexSet::new();
        collect_fn_calls(&[o], &mut into);
        let mut want = IndexSet::new();
        want.insert("GIVE".to_string());
        want.insert("TAKE".to_string());
        assert_eq!(into, want);
    }

    #[test]
    fn collect_fn_calls_preserves_ts_set_insertion_order() {
        // wave-1 must-fix-2: collectFnCalls fills a JS `Set<string>`, whose
        // iteration is INSERTION order — content.ts pass-3 (:545-561) reads
        // it in that order (first undeclared callee in the error text; the
        // first cycle found and its `a -> b -> a` spelling). Hand-built FN
        // nodes, as in the branch test above (undeclared fns fail at parse).
        let fn_ = |call: &str| Outcome {
            op: "FN".into(),
            args: vec![],
            arms: None,
            fallback: None,
            call: Some(call.into()),
        };
        let arm = IfArm {
            when: Pred::Cmp {
                l: ValExpr::Int { v: 1 },
                op: crate::core::query::CmpOp::Eq,
                r: ValExpr::Int { v: 1 },
            },
            then: vec![fn_("zulu")], // arm bodies walk before the fallback
        };
        let o = Outcome {
            op: "IF".into(),
            args: vec![],
            arms: Some(vec![arm]),
            fallback: Some(vec![fn_("alpha")]),
            call: None,
        };
        let mut into = IndexSet::new();
        // declaration order across the list, plus a DUPLICATE: Set.add keeps
        // the element at its FIRST position (TS and IndexSet alike)
        collect_fn_calls(&[o, fn_("mike"), fn_("zulu")], &mut into);
        let names: Vec<&str> = into.iter().map(String::as_str).collect();
        assert_eq!(names, vec!["zulu", "alpha", "mike"]);
    }

    #[test]
    fn param_sign_on_delta_position_folds_negation() {
        // direct subst_arg check of the sign rule (parse.ts:722-728)
        let mut bind = IndexMap::new();
        bind.insert("n".to_string(), Arg::Int { v: 4 });
        let a = Arg::Param {
            name: "n".into(),
            sign: Some(Sign::Neg),
        };
        match subst_arg(&a, Some("f"), &mut bind).unwrap() {
            Arg::Delta { v } => assert_eq!(v, -4),
            other => panic!("expected delta, got {other:?}"),
        }
        // a non-numeric binding splices through for the runtime kind error
        bind.insert("s".to_string(), Arg::Str { v: "x".into() });
        let a2 = Arg::Param {
            name: "s".into(),
            sign: Some(Sign::Pos),
        };
        match subst_arg(&a2, Some("f"), &mut bind).unwrap() {
            Arg::Str { v } => assert_eq!(v, "x"),
            other => panic!("expected splice-through, got {other:?}"),
        }
    }
}
