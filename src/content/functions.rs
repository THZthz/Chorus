//! The §10 function table: declarations compile to call templates BEFORE any
//! verb/trigger body parses, and every authored outcome string expands
//! through the table in the same step (verb/trigger bodies are compiled
//! fully expanded, so every guard and the engine see plain outcomes).

use indexmap::{IndexMap, IndexSet};

use super::compile::ident_re;
use super::model::FunctionDef;
use crate::core::dict::Dictionary;
use crate::core::fn_expand::{
    check_body_tokens, collect_fn_calls, expand_fn_calls, FnTable, FnTemplate, MAX_FN_DEPTH,
};
use crate::core::parse::{parse_function_body, parse_outcomes};
use crate::core::sentence::Outcome;
use crate::error::{ChorusError, Result};

/// Allow several outcomes in one string; usually
/// one per array element. §10: with a function table, parse dispatches
/// NAME( calls and the expansion pass runs in the same step — verb/trigger
/// bodies are compiled fully expanded, so every guard (and the engine) sees
/// plain outcomes.
pub(super) fn parse_outcome_string(
    src: &str,
    dict: &Dictionary,
    fns: &FnTable,
) -> Result<Vec<Outcome>> {
    let out = parse_outcomes(src, dict, Some(fns))?;
    if !fns.by_name.is_empty() {
        expand_fn_calls(&out, fns)
    } else {
        Ok(out)
    }
}

/// Build the function table; functions parse BEFORE
/// any verb/trigger body (§10), in three ordered passes:
/// 1. declarations: name/param shape, op-name collisions, IF/ELSE,
///    duplicates. No entity/verb name constraint.
/// 2. bodies: each body parses to a TEMPLATE against the table's names.
/// 3. the call graph: colored DFS rejects recursion naming the cycle; the
///    same graph's depth rejects nesting beyond MAX_FN_DEPTH. Both are load
///    errors even for never-called functions.
pub(super) fn build_fn_table(defs: &[FunctionDef], dict: &Dictionary) -> Result<FnTable> {
    let mut table = FnTable::default();
    // pass 1 — declarations
    for def in defs {
        if !ident_re().is_match(&def.name) {
            return Err(ChorusError::new(format!(
                "function name '{}' must be an identifier (letters, digits, _)",
                def.name
            )));
        }
        if dict.is_op(&def.name) {
            return Err(ChorusError::new(format!(
                "function '{}' collides with an outcome op — functions and ops share one caller vocabulary; rename the function",
                def.name
            )));
        }
        if def.name == "IF" || def.name == "ELSE" {
            return Err(ChorusError::new(format!(
                "function '{}' cannot be named a conditional keyword — IF/ELSE dispatch before functions in outcome position",
                def.name
            )));
        }
        if table.by_name.contains_key(&def.name) {
            return Err(ChorusError::new(format!("duplicate function '{}'", def.name)));
        }
        for p in &def.params {
            if !ident_re().is_match(p) {
                return Err(ChorusError::new(format!(
                    "function '{}': parameter '{p}' must be an identifier",
                    def.name
                )));
            }
        }
        // first-occurrence duplicates, deduped
        // preserving first-seen order (set iteration semantics)
        let mut dup_set: IndexSet<&str> = IndexSet::new();
        let mut dups: Vec<&str> = Vec::new();
        for (i, p) in def.params.iter().enumerate() {
            if def.params[..i].contains(p) && dup_set.insert(p) {
                dups.push(p);
            }
        }
        if !dups.is_empty() {
            return Err(ChorusError::new(format!(
                "function '{}': duplicate parameter(s) {}",
                def.name,
                dups.join(", ")
            )));
        }
        table.by_name.insert(
            def.name.clone(),
            FnTemplate { name: def.name.clone(), params: def.params.clone(), body: Vec::new() },
        );
    }
    // pass 2 — bodies (nested-call arity checks against the declared params)
    for def in defs {
        // the body parse error is wrapped:
        // `function '<name>': <message>`
        let body = match parse_function_body(&def.body, dict, &table, &def.params) {
            Ok(b) => b,
            Err(err) => {
                return Err(ChorusError::new(format!("function '{}': {}", def.name, err.msg)))
            }
        };
        // pass 1 inserted it
        match table.by_name.get_mut(&def.name) {
            Some(tpl) => {
                tpl.body = body;
                // §10: every declared body's braced tokens are checked at
                // load, called or not — expansion runs the same check only
                // for the bodies a call reaches, so a never-called `{ x }`
                // or `{typo}` used to load clean
                check_body_tokens(tpl)?;
            }
            None => unreachable!("pass 1 inserted '{}'", def.name),
        }
    }
    // pass 3 — the call graph: recursion cycles, then the depth cap
    let mut calls_of: IndexMap<String, IndexSet<String>> = IndexMap::new();
    for tpl in table.by_name.values() {
        let mut calls = IndexSet::new();
        collect_fn_calls(&tpl.body, &mut calls);
        for c in &calls {
            // pinned at tests/fn_expand_direct.rs `an_undeclared_call_in_a_body_...`
            if !table.by_name.contains_key(c) {
                return Err(ChorusError::new(format!(
                    "function '{}': call to undeclared function '{c}'",
                    tpl.name
                )));
            }
        }
        calls_of.insert(tpl.name.clone(), calls);
    }
    const WHITE: u8 = 0;
    const GRAY: u8 = 1;
    const BLACK: u8 = 2;
    let mut color: IndexMap<String, u8> = IndexMap::new();
    let mut stack: Vec<String> = Vec::new();
    fn visit(
        name: &str,
        calls_of: &IndexMap<String, IndexSet<String>>,
        color: &mut IndexMap<String, u8>,
        stack: &mut Vec<String>,
    ) -> Result<()> {
        color.insert(name.to_string(), GRAY);
        stack.push(name.to_string());
        if let Some(calls) = calls_of.get(name) {
            for c in calls {
                let col = color.get(c).copied().unwrap_or(WHITE);
                if col == GRAY {
                    // the cycle is `stack[position(c)..] + [c]`;
                    // a GRAY node is by construction on the stack
                    let start = match stack.iter().position(|x| x == c) {
                        Some(s) => s,
                        None => unreachable!("GRAY '{c}' is on the DFS stack"),
                    };
                    let mut cycle: Vec<String> = stack[start..].to_vec();
                    cycle.push(c.clone());
                    // pinned at tests/fn_expand_direct.rs
                    // `mutual_recursion_names_the_cycle_...`
                    return Err(ChorusError::new(format!(
                        "functions recurse: {} — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle",
                        cycle.join(" -> ")
                    )));
                }
                if col == WHITE {
                    visit(c, calls_of, color, stack)?;
                }
            }
        }
        stack.pop();
        color.insert(name.to_string(), BLACK);
        Ok(())
    }
    for name in table.by_name.keys() {
        if color.get(name).copied().unwrap_or(WHITE) == WHITE {
            visit(name, &calls_of, &mut color, &mut stack)?;
        }
    }
    fn depth_of(
        name: &str,
        calls_of: &IndexMap<String, IndexSet<String>>,
        depth: &mut IndexMap<String, u32>,
    ) -> u32 {
        if let Some(memo) = depth.get(name) {
            return *memo;
        }
        // safe: the graph is acyclic after the cycle check
        depth.insert(name.to_string(), 1);
        let mut deepest: u32 = 0;
        if let Some(calls) = calls_of.get(name) {
            for c in calls {
                deepest = deepest.max(depth_of(c, calls_of, depth));
            }
        }
        let v = 1 + deepest;
        depth.insert(name.to_string(), v);
        v
    }
    // the `depth` memo
    let mut depth: IndexMap<String, u32> = IndexMap::new();
    for name in table.by_name.keys() {
        let d = depth_of(name, &calls_of, &mut depth);
        if d > MAX_FN_DEPTH {
            // pinned at tests/fn_expand_direct.rs
            // `a_depth_8_chain_expands_transitively_...`
            return Err(ChorusError::new(format!(
                "function '{name}' sits {d} calls deep — the nesting cap is {MAX_FN_DEPTH}"
            )));
        }
    }
    Ok(table)
}
