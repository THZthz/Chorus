//! fn_expand 直测子集 (PR3-01, plan §4-W1 "宏展开验收改直测子集") — the
//! subset of test/functions.test.ts that runs WITHOUT compileScript /
//! compileContent: a hand-built FnTable (the TS rig's `rawTable`), driven
//! straight through `parse_outcomes` / `parse_function_body` /
//! `collect_fn_calls` / `expand_fn_calls` / `format_sentence`.
//!
//! The full functions.test port (compileScript → executeStep 全链路) lands in
//! W3 as tests/functions.rs (plan §5.2.1); this file is its W1 direct seam.
//!
//! The recursion-cycle and depth-cap GUARDS live in content compile
//! (content.ts:543-588, W2's content.rs) — their message templates are
//! frozen here byte-for-byte over the real `collect_fn_calls` graph, by the
//! same precedent as tests/dice.rs `script_seed_error` (test-local template
//! freeze; the W2 construction sites must reproduce them exactly).

mod common;

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::format::format_sentence;
use chorus::core::fn_expand::{collect_fn_calls, expand_fn_calls, FnTable, FnTemplate, MAX_FN_DEPTH};
use chorus::core::parse::{parse_function_body, parse_outcomes};
use chorus::core::sentence::{Outcome, Sentence, SentenceKind};
use chorus::js::{ChorusError, Result};

use common::harness::assert_err_msg;

/// ts: functions.test.ts:570-577 (`rawTable`) — a parse-level FnTable the
/// way compileContent's passes build it: pass 1 inserts every name (so
/// forward refs parse), pass 2 parses the bodies against the full table.
fn raw_table(dict: &Dictionary, defs: &[(&str, &[&str], &str)]) -> FnTable {
    let mut table = FnTable::default();
    for (name, params, _) in defs {
        table.by_name.insert(
            (*name).to_string(),
            FnTemplate {
                name: (*name).to_string(),
                params: (*params).iter().map(|s| (*s).to_string()).collect(),
                body: vec![],
            },
        );
    }
    for (name, params, body) in defs {
        let parsed = parse_function_body(
            body,
            dict,
            &table,
            &(*params).iter().map(|s| (*s).to_string()).collect::<Vec<_>>(),
        )
        .unwrap();
        table.by_name.get_mut(*name).unwrap().body = parsed;
    }
    table
}

/// `formatSentence({ kind: 'action', clock: null, trigger: null,
/// clauses: [], outcomes }, dict)` — the outcome-list formatting probe used
/// for the format∘expand 对拍.
fn fmt_outcomes(out: &[Outcome], dict: &Dictionary) -> String {
    format_sentence(
        &Sentence {
            kind: SentenceKind::Action,
            clock: None,
            trigger: None,
            clauses: vec![],
            outcomes: out.to_vec(),
            span_min: None,
        },
        Some(dict),
    )
    .unwrap()
}

// ---------- expansion equivalence: call site === hand-expanded twin
// ---------- (§9.40, ts: functions.test.ts:85-107, direct subset) ----------

#[test]
fn call_site_equals_the_hand_expanded_twin_byte_for_byte() {
    let dict = core_dictionary();
    // the plan's flagship pair, verbatim grammar (functions.test.ts:46-58):
    // the call rig and its hand-expanded twin
    let fns = raw_table(
        &dict,
        &[
            (
                "sus",
                &["capital"],
                "SPAWN SUSPICION, NARRATE \"The conductor's eyes linger on {capital}.\"",
            ),
            (
                "watch",
                &["n"],
                "IF count(name:SUSPICION) >= n -> (ENDING ARRESTED) ELSE -> (NARRATE \"not yet.\")",
            ),
        ],
    );
    let call = parse_outcomes("sus(THE_GUARD), watch(3)", &dict, Some(&fns)).unwrap();
    let expanded = expand_fn_calls(&call, &fns).unwrap();
    let twin = parse_outcomes(
        "SPAWN SUSPICION, NARRATE \"The conductor's eyes linger on THE_GUARD.\", \
         IF count(name:SUSPICION) >= 3 -> (ENDING ARRESTED) ELSE -> (NARRATE \"not yet.\")",
        &dict,
        None,
    )
    .unwrap();

    // AST equality across the whole pair — the IF arm's `>= n` really became
    // `>= 3`, the ELSE body rode along, no FN node survived
    assert_eq!(expanded, twin);
    assert!(expanded.iter().all(|o| o.op != "FN" && o.call.is_none()));

    // format∘expand 对拍 (the string-substitution case included): the
    // formatted expansion is byte-identical to the formatted twin. format
    // refuses IF (authored form only — format.ts:41), so the byte compare
    // runs on the concrete prefix, exactly like the engine's concrete SPAWN
    // row would print.
    let as_sentence = |out: &[Outcome]| Sentence {
        kind: SentenceKind::Action,
        clock: None,
        trigger: None,
        clauses: vec![],
        outcomes: out.to_vec(),
        span_min: None,
    };
    let a = format_sentence(&as_sentence(&expanded[..2]), Some(&dict)).unwrap();
    let b = format_sentence(&as_sentence(&twin[..2]), Some(&dict)).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        a,
        " -> SPAWN SUSPICION, NARRATE \"The conductor's eyes linger on THE_GUARD.\""
    );
}

// ---------- grammar: call-site dispatch and arity (§9.40,
// ---------- ts: functions.test.ts:111-142, direct subset) ----------

#[test]
fn a_call_parses_into_an_authored_fn_node_expansion_is_a_separate_pass() {
    // ts: functions.test.ts:117-123
    let dict = core_dictionary();
    let table = raw_table(
        &dict,
        &[
            ("one", &["x"], "NARRATE \"x is {x}.\""),
            ("zero", &[], "NARRATE \"none.\""),
        ],
    );
    let out = parse_outcomes("one(THE_GUARD), zero()", &dict, Some(&table)).unwrap();
    assert_json_eq_ops(&out, &["FN", "FN"]);
    assert_eq!(out[0].call.as_deref(), Some("one"));
    assert_eq!(out[1].call.as_deref(), Some("zero"));
    assert_eq!(out[1].args.len(), 0);
}

fn assert_json_eq_ops(out: &[Outcome], ops: &[&str]) {
    let got: Vec<String> = out.iter().map(|o| o.op.clone()).collect();
    assert_eq!(got, ops);
}

#[test]
fn arity_mismatch_is_a_load_error_too_few_too_many_and_parameterless() {
    // ts: functions.test.ts:125-129
    let dict = core_dictionary();
    let table = raw_table(
        &dict,
        &[
            ("one", &["x"], "NARRATE \"x is {x}.\""),
            ("zero", &[], "NARRATE \"none.\""),
        ],
    );
    assert_err_msg(
        parse_outcomes("one()", &dict, Some(&table)),
        r"function 'one' expects 1 argument\(s\) \(x\), got 0",
    );
    assert_err_msg(
        parse_outcomes("one(A, B)", &dict, Some(&table)),
        r"function 'one' expects 1 argument\(s\) \(x\), got 2",
    );
    assert_err_msg(
        parse_outcomes("zero(1)", &dict, Some(&table)),
        r"function 'zero' expects 0 argument\(s\), got 1",
    );
}

#[test]
fn unknown_name_in_outcome_position_is_a_compile_error_listing_the_vocabulary() {
    // ts: functions.test.ts:131-142
    let dict = core_dictionary();
    // no functions declared: ops only
    assert_err_msg(
        parse_outcomes("mystery(3)", &dict, None),
        r"unknown 'mystery\(\.\.\.\)' in outcome position.*SPAWN, DESTROY.*no functions declared",
    );
    // with functions declared, they are listed too (declaration order)
    let table = raw_table(
        &dict,
        &[
            ("one", &["x"], "NARRATE \"x is {x}.\""),
            ("zero", &[], "NARRATE \"none.\""),
        ],
    );
    assert_err_msg(
        parse_outcomes("mystery(3)", &dict, Some(&table)),
        r"unknown 'mystery\(\.\.\.\)' in outcome position.*declared functions: one, zero",
    );
    // predicate helpers get the forgotten-IF hint
    assert_err_msg(
        parse_outcomes("exists(X) -> (NARRATE \"a\")", &dict, None),
        "IF keyword",
    );
}

// ---------- the two substitution mechanisms (§9.40,
// ---------- ts: functions.test.ts:193-236, direct subset) ----------

#[test]
fn delta_position_the_authored_sign_folds_into_the_calls_number() {
    // ts: functions.test.ts:193-206 — bump(2)/drop(4): the sign rides the
    // param and folds into the call's number at expansion (parse.ts:722-728).
    // The engine concretizes SET deltas to computed ints at runtime — the
    // log-level assertion is W3's; here the AST carries the signed delta.
    let dict = core_dictionary();
    let fns = raw_table(
        &dict,
        &[
            ("bump", &["n"], "SET(TOME, aspect:secret, +n)"),
            ("drop", &["n"], "SET(TOME, aspect:secret, -n)"),
        ],
    );
    let call = parse_outcomes("bump(2), drop(4)", &dict, Some(&fns)).unwrap();
    let expanded = expand_fn_calls(&call, &fns).unwrap();
    match (&expanded[0].args[2], &expanded[1].args[2]) {
        (chorus::core::sentence::Arg::Delta { v: plus }, chorus::core::sentence::Arg::Delta { v: minus }) => {
            assert_eq!(*plus, 2);
            assert_eq!(*minus, -4);
        }
        other => panic!("expected signed deltas, got {other:?}"),
    }
    // and the delta SPELLING interpolates with its sign (argSpelling shares
    // format_arg's delta branch — CORE-13's shared layer)
    let fns2 = raw_table(&dict, &[("tag", &["n"], "NARRATE \"+{n}\"")]);
    let call2 = parse_outcomes("tag(2)", &dict, Some(&fns2)).unwrap();
    let expanded2 = expand_fn_calls(&call2, &fns2).unwrap();
    match &expanded2[0].args[0] {
        chorus::core::sentence::Arg::Str { v } => assert_eq!(v, "+2"),
        other => panic!("expected str, got {other:?}"),
    }
}

#[test]
fn reserved_tokens_pass_through_undeclared_or_spaced_braces_are_load_errors() {
    // ts: functions.test.ts:218-236 — {actors} is §5.3 prose, never a
    // parameter; a typo'd parameter is a LOAD ERROR naming the token, the
    // function, and the reserved set; spaced spellings never rewrite (same
    // typo, same error, whether or not the identifier names a parameter);
    // commas/quotes inside braces are ordinary prose
    let dict = core_dictionary();
    // {actors} survives expansion
    let ok = raw_table(
        &dict,
        &[("witness", &["who"], "NARRATE \"{actors} saw {who}.\"")],
    );
    let call = parse_outcomes("witness(THE_GUARD)", &dict, Some(&ok)).unwrap();
    let expanded = expand_fn_calls(&call, &ok).unwrap();
    match &expanded[0].args[0] {
        chorus::core::sentence::Arg::Str { v } => assert_eq!(v, "{actors} saw THE_GUARD."),
        other => panic!("expected str, got {other:?}"),
    }
    // undeclared {nope}
    let bad = raw_table(&dict, &[("bad", &["x"], "NARRATE \"hi {x} and {nope}.\"")]);
    let call = parse_outcomes("bad(THE_GUARD)", &dict, Some(&bad)).unwrap();
    assert_err_msg(
        expand_fn_calls(&call, &bad),
        r"'\{nope\}' in a body string names neither a parameter of 'bad' nor a reserved token",
    );
    // spaced `{ x }` — even though x IS a parameter
    let s1 = raw_table(&dict, &[("s1", &["x"], "NARRATE \"hi { x }.\"")]);
    let call = parse_outcomes("s1(THE_GUARD)", &dict, Some(&s1)).unwrap();
    assert_err_msg(
        expand_fn_calls(&call, &s1),
        r"'\{ x \}' in a body string names neither a parameter",
    );
    // spaced `{ actors }` — even though actors is reserved
    let s2 = raw_table(&dict, &[("s2", &[], "NARRATE \"{ actors } gather.\"")]);
    let call = parse_outcomes("s2()", &dict, Some(&s2)).unwrap();
    assert_err_msg(
        expand_fn_calls(&call, &s2),
        r"'\{ actors \}' in a body string names neither a parameter",
    );
    // commas inside braces are ordinary prose — expands clean
    let prose = raw_table(
        &dict,
        &[("prose", &["x"], "NARRATE \"the set { a, b } holds {x}\"")],
    );
    let call = parse_outcomes("prose(THE_GUARD)", &dict, Some(&prose)).unwrap();
    let expanded = expand_fn_calls(&call, &prose).unwrap();
    match &expanded[0].args[0] {
        chorus::core::sentence::Arg::Str { v } => assert_eq!(v, "the set { a, b } holds THE_GUARD"),
        other => panic!("expected str, got {other:?}"),
    }
}

// ---------- nesting, recursion cycles, and the depth cap (§9.40,
// ---------- ts: functions.test.ts:268-319; guards mirror content.ts:543-588) ----------

/// ts: functions.test.ts:269-274 (`chain`) — f1 → … → fn, the tail
/// narrating "bottom.".
fn chain_defs(n: usize) -> Vec<(String, Vec<String>, String)> {
    (0..n)
        .map(|i| {
            let body = if i + 1 < n {
                format!("f{}()", i + 2)
            } else {
                "NARRATE \"bottom.\"".to_string()
            };
            (format!("f{}", i + 1), vec![], body)
        })
        .collect()
}

fn table_from(defs: &[(String, Vec<String>, String)]) -> FnTable {
    let dict = core_dictionary();
    let refs: Vec<(&str, Vec<&str>, &str)> = defs
        .iter()
        .map(|(n, p, b)| {
            (
                n.as_str(),
                p.iter().map(|s| s.as_str()).collect(),
                b.as_str(),
            )
        })
        .collect();
    let slices: Vec<(&str, &[&str], &str)> = refs
        .iter()
        .map(|(n, p, b)| (*n, p.as_slice(), *b))
        .collect();
    raw_table(&dict, &slices)
}

/// ts: src/content/content.ts:543-588 — pass 3 over the call graph:
/// undeclared-call check, recursion-cycle DFS (WHITE/GRAY/BLACK), then the
/// depth cap. **W2 落位说明**: the construction points belong to W2's
/// content.rs; this mirror freezes both message templates over the REAL
/// `collect_fn_calls` output so they cannot drift before then (the
/// dice.rs `script_seed_error` precedent). W2's implementation must inline
/// exactly these strings at its throw sites.
fn content_pass3_guards(table: &FnTable) -> Result<()> {
    let mut calls_of: IndexMap<String, HashSet<String>> = IndexMap::new();
    for tpl in table.by_name.values() {
        let mut calls = HashSet::new();
        collect_fn_calls(&tpl.body, &mut calls);
        for c in &calls {
            if !table.by_name.contains_key(c) {
                return Err(ChorusError::new(format!(
                    "function '{}': call to undeclared function '{}'",
                    tpl.name, c
                )));
            }
        }
        calls_of.insert(tpl.name.clone(), calls);
    }
    fn visit(
        name: &str,
        calls_of: &IndexMap<String, HashSet<String>>,
        color: &mut HashMap<String, u8>, // 0 white, 1 gray, 2 black
        stack: &mut Vec<String>,
    ) -> Result<()> {
        color.insert(name.to_string(), 1);
        stack.push(name.to_string());
        for c in &calls_of[name] {
            match color.get(c).copied().unwrap_or(0) {
                1 => {
                    let start = stack.iter().position(|s| s == c).unwrap_or(0);
                    let mut cycle: Vec<String> = stack[start..].to_vec();
                    cycle.push(c.clone());
                    return Err(ChorusError::new(format!(
                        "functions recurse: {} — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle",
                        cycle.join(" -> ")
                    )));
                }
                0 => visit(c, calls_of, color, stack)?,
                _ => {}
            }
        }
        stack.pop();
        color.insert(name.to_string(), 2);
        Ok(())
    }
    let mut color: HashMap<String, u8> = HashMap::new();
    let mut stack: Vec<String> = Vec::new();
    for name in table.by_name.keys() {
        if color.get(name).copied().unwrap_or(0) == 0 {
            visit(name, &calls_of, &mut color, &mut stack)?;
        }
    }
    fn depth_of(name: &str, calls_of: &IndexMap<String, HashSet<String>>, memo: &mut HashMap<String, u32>) -> u32 {
        if let Some(d) = memo.get(name) {
            return *d;
        }
        memo.insert(name.to_string(), 1); // safe: the graph is acyclic after the cycle check
        let mut deepest = 0;
        for c in &calls_of[name] {
            deepest = deepest.max(depth_of(c, calls_of, memo));
        }
        let v = 1 + deepest;
        memo.insert(name.to_string(), v);
        v
    }
    let mut memo: HashMap<String, u32> = HashMap::new();
    for name in table.by_name.keys() {
        let d = depth_of(name, &calls_of, &mut memo);
        if d > MAX_FN_DEPTH {
            return Err(ChorusError::new(format!(
                "function '{name}' sits {d} calls deep — the nesting cap is {MAX_FN_DEPTH}"
            )));
        }
    }
    Ok(())
}

#[test]
fn mutual_recursion_names_the_cycle_self_recursion_names_itself() {
    // ts: functions.test.ts:291-301
    let table = table_from(&[
        ("a".into(), vec![], "b()".into()),
        ("b".into(), vec![], "a()".into()),
    ]);
    let err = content_pass3_guards(&table).unwrap_err();
    assert_eq!(
        err.msg,
        "functions recurse: a -> b -> a — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle"
    );
    let table = table_from(&[("self".into(), vec![], "self()".into())]);
    let err = content_pass3_guards(&table).unwrap_err();
    assert_eq!(
        err.msg,
        "functions recurse: self -> self — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle"
    );
}

#[test]
fn a_depth_8_chain_expands_transitively_depth_9_is_a_load_error() {
    // ts: functions.test.ts:303-310 — depth(f) = 1 + max(depth of callees);
    // 8 loads, 9 errors. The LOAD gate is content compile (W2) — here the
    // guard mirror enforces it over the real call graph, and the depth-8
    // chain's EXPANSION is checked directly.
    let dict = core_dictionary();
    let eight = table_from(&chain_defs(8));
    assert!(content_pass3_guards(&eight).is_ok());
    let call = parse_outcomes("f1()", &dict, Some(&eight)).unwrap();
    let expanded = expand_fn_calls(&call, &eight).unwrap();
    assert_eq!(expanded.len(), 1);
    assert_eq!(
        fmt_outcomes(&expanded, &dict),
        " -> NARRATE \"bottom.\""
    );
    let nine = table_from(&chain_defs(9));
    let err = content_pass3_guards(&nine).unwrap_err();
    assert_eq!(err.msg, "function 'f1' sits 9 calls deep — the nesting cap is 8");
}

#[test]
fn an_undeclared_call_in_a_body_is_named_by_its_declaring_function() {
    // content.ts:549's guard. UNREACHABLE through the parse front-end (a
    // body calling an undeclared name fails at parse with unknownCallError),
    // so — exactly like the TS tree — this pass-3 check is a defensive
    // tripwire; the FN node is hand-built to reach it (the same shape the
    // fn_expand.rs smoke tests use for collect_fn_calls).
    let mut table = FnTable::default();
    table.by_name.insert(
        "lonely".to_string(),
        FnTemplate {
            name: "lonely".to_string(),
            params: vec![],
            body: vec![Outcome {
                op: "FN".to_string(),
                args: vec![],
                arms: None,
                fallback: None,
                call: Some("ghost".to_string()),
            }],
        },
    );
    let err = content_pass3_guards(&table).unwrap_err();
    assert_eq!(err.msg, "function 'lonely': call to undeclared function 'ghost'");
}
