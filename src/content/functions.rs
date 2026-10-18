//! The §10 call tables: `functions:` and `ops:` declarations both compile
//! to call templates BEFORE any
//! verb/trigger body parses, and every authored outcome string expands
//! through the combined table in the same step (verb/trigger bodies are
//! compiled fully expanded, so every guard and the engine see plain
//! outcomes). The two tables share one template shape and one expansion
//! pass; what separates them is the namespace (UPPER_SNAKE ops) and the
//! composition rules — a function may never call an op, an op may call
//! anything.

use indexmap::{IndexMap, IndexSet};

use super::compile::{ident_re, upper_snake_re};
use super::model::{ContentRaw, FunctionDef};
use crate::core::dict::Dictionary;
use crate::core::fn_expand::{
    check_body_tokens, collect_fn_calls, expand_fn_calls, CallKind, FnTable, FnTemplate,
    MAX_FN_DEPTH,
};
use crate::core::parse::{parse_function_body, parse_op_body, parse_outcomes};
use crate::core::sentence::Outcome;
use crate::error::{ChorusError, Result};

/// Allow several outcomes in one string; usually
/// one per array element. §10: with a call table, parse dispatches
/// NAME( calls (functions and script ops alike) and the expansion pass runs
/// in the same step — verb/trigger
/// bodies are compiled fully expanded, so every guard (and the engine) sees
/// plain outcomes.
pub(super) fn parse_outcome_string(
    src: &str,
    dict: &Dictionary,
    fns: &FnTable,
) -> Result<Vec<Outcome>> {
    let out = parse_outcomes(src, dict, Some(fns))?;
    if !fns.is_empty() {
        expand_fn_calls(&out, fns)
    } else {
        Ok(out)
    }
}

/// Build the combined call table (§10) — functions AND script ops. The
/// build order is load-bearing:
/// 1. ops pass 1 — op DECLARATIONS (names + params) so fn bodies can see an
///    op name and give the fn→op ban its specific error;
/// 2. functions — the full three-pass fn build (bodies parse with op calls
///    BANNED, and an fn name sharing an op name is the collision error);
/// 3. ops pass 2 — op BODIES parse against the completed fn table plus the
///    op names (an op may call functions and other ops), and every declared
///    body's braced tokens are checked, called or not (§10's rule);
/// 4. ops pass 3 — the op→op call graph: recursion cycles and the depth cap
///    (the fn graph never reaches ops — the ban — so its own build already
///    settled it).
pub(super) fn build_call_tables(raw: &ContentRaw, dict: &Dictionary) -> Result<FnTable> {
    let mut table = FnTable::default();
    declare_ops(raw.ops.as_deref().unwrap_or(&[]), dict, &mut table)?;
    build_fns(raw.functions.as_deref().unwrap_or(&[]), dict, &mut table)?;
    build_op_bodies(raw.ops.as_deref().unwrap_or(&[]), dict, &mut table)?;
    check_op_graph(&table)?;
    Ok(table)
}

/// ops pass 1 — declarations: name/param shape, collisions, duplicates.
/// No entity/verb name constraint (calls only ever parse in outcome
/// position), so op names are deliberately NOT added to `reserved` — an
/// entity may legitimately be named like an op, and a bare `ENCHANT` in an
/// outcome list stays SPAWN sugar while `ENCHANT(x)` is the call (§10).
/// The reserved check runs against the LOAD-EFFECTIVE dictionary — the
/// caller registered the script's verbs before compiling — so a declared
/// verb lexeme is as taken as a built-in reserved word.
fn declare_ops(defs: &[FunctionDef], dict: &Dictionary, table: &mut FnTable) -> Result<()> {
    for def in defs {
        // the shape rule is the verb/ending UPPER_SNAKE grammar (§5.1), not
        // the fn ident rule: a call is written and read exactly like a
        // built-in op call, so the name must look like one
        if !upper_snake_re().is_match(&def.name) {
            return Err(ChorusError::new(format!("op name '{}' must be UPPER_SNAKE", def.name)));
        }
        if dict.is_op(&def.name) {
            return Err(ChorusError::new(format!(
                "op '{}' collides with an outcome op — the built-in vocabulary is closed; rename the op",
                def.name
            )));
        }
        if dict.reserved.contains(&def.name) {
            return Err(ChorusError::new(format!("op '{}' is a reserved word (§5.1)", def.name)));
        }
        if table.ops.contains_key(&def.name) {
            return Err(ChorusError::new(format!("duplicate op '{}'", def.name)));
        }
        for p in &def.params {
            if !ident_re().is_match(p) {
                return Err(ChorusError::new(format!(
                    "op '{}': parameter '{p}' must be an identifier",
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
                "op '{}': duplicate parameter(s) {}",
                def.name,
                dups.join(", ")
            )));
        }
        table.ops.insert(
            def.name.clone(),
            FnTemplate { name: def.name.clone(), params: def.params.clone(), body: Vec::new() },
        );
    }
    Ok(())
}

/// ops pass 2 — bodies: each body parses to a TEMPLATE against the fn table
/// plus the declared op names (nested-call arity checks included), then the
/// body's braced tokens are checked — called or not, the v0.7.2 rule.
fn build_op_bodies(defs: &[FunctionDef], dict: &Dictionary, table: &mut FnTable) -> Result<()> {
    for def in defs {
        // the body parse error is wrapped: `op '<name>': <message>`
        let body = match parse_op_body(&def.body, dict, table, &def.params) {
            Ok(b) => b,
            Err(err) => return Err(ChorusError::new(format!("op '{}': {}", def.name, err.msg))),
        };
        // pass 1 inserted it
        match table.ops.get_mut(&def.name) {
            Some(tpl) => {
                tpl.body = body;
                // every declared op body's braced tokens are checked at
                // load, called or not — expansion runs the same check only
                // for the bodies a call reaches
                check_body_tokens(tpl, CallKind::Op)?;
            }
            None => unreachable!("pass 1 inserted '{}'", def.name),
        }
    }
    Ok(())
}

/// ops pass 3 — the op→op call graph. Every callee must resolve (parse
/// already guarantees it; the check is the build's belt to the parse's
/// braces), recursion cycles name the cycle, and the depth cap reuses the
/// functions'. Only op→op edges are walked: fn→op is banned and the fn
/// graph is already acyclic and capped by its own build, so fn calls are
/// leaves here — an op chain's depth is its op hops.
fn check_op_graph(table: &FnTable) -> Result<()> {
    let mut calls_of: IndexMap<String, IndexSet<String>> = IndexMap::new();
    for tpl in table.ops.values() {
        let mut calls = IndexSet::new();
        collect_fn_calls(&tpl.body, &mut calls);
        for c in &calls {
            if table.resolve(c).is_none() {
                return Err(ChorusError::new(format!(
                    "op '{}': call to undeclared function or op '{c}'",
                    tpl.name
                )));
            }
            // the graph keeps op→op edges only (see above)
            if table.ops.contains_key(c) {
                calls_of.entry(tpl.name.clone()).or_default().insert(c.clone());
            }
        }
        calls_of.entry(tpl.name.clone()).or_default();
    }
    reject_cycles(&calls_of, "ops")?;
    reject_depth(&calls_of, "op")
}

/// Build the function table into the combined table; functions parse BEFORE
/// any verb/trigger body (§10), in three ordered passes:
/// 1. declarations: name/param shape, op-name collisions (built-in AND
///    script-declared), IF/ELSE, duplicates. No entity/verb name constraint.
/// 2. bodies: each body parses to a TEMPLATE against the table's names —
///    with op calls BANNED (the parser gives the specific §10 error; the
///    ops map is filled precisely so the ban can name the op).
/// 3. the call graph: colored DFS rejects recursion naming the cycle; the
///    same graph's depth rejects nesting beyond MAX_FN_DEPTH. Both are load
///    errors even for never-called functions.
fn build_fns(defs: &[FunctionDef], dict: &Dictionary, table: &mut FnTable) -> Result<()> {
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
        if table.ops.contains_key(&def.name) {
            // the fn-name ident rule admits UPPER names, so the two tables'
            // namespaces are NOT case-disjoint by construction — resolve()
            // checks functions first and would silently shadow the op
            return Err(ChorusError::new(format!(
                "function '{}' collides with a declared op — calls resolve to one table; rename one",
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
        let body = match parse_function_body(&def.body, dict, table, &def.params) {
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
                // load, called or not — expansion alone would only check
                // the bodies a call reaches
                check_body_tokens(tpl, CallKind::Function)?;
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
    reject_cycles(&calls_of, "functions")?;
    reject_depth(&calls_of, "function")
}

/// The colored DFS both tables share: recursion is a load error that NAMES
/// the cycle (`{plural} recurse: a -> b -> a — …`). A GRAY node is by
/// construction on the DFS stack.
fn reject_cycles(calls_of: &IndexMap<String, IndexSet<String>>, plural: &str) -> Result<()> {
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
        plural: &str,
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
                        "{plural} recurse: {} — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle",
                        cycle.join(" -> ")
                    )));
                }
                if col == WHITE {
                    visit(c, calls_of, color, stack, plural)?;
                }
            }
        }
        stack.pop();
        color.insert(name.to_string(), BLACK);
        Ok(())
    }
    for name in calls_of.keys() {
        if color.get(name).copied().unwrap_or(WHITE) == WHITE {
            visit(name, calls_of, &mut color, &mut stack, plural)?;
        }
    }
    Ok(())
}

/// The nesting cap both tables share (`{singular} '{name}' sits {d} calls
/// deep — the nesting cap is MAX_FN_DEPTH`): depth(name) = 1 + max(depth of
/// its callees), a body with no calls is depth 1. The graph must already be
/// acyclic (the cycle check runs first).
fn reject_depth(calls_of: &IndexMap<String, IndexSet<String>>, singular: &str) -> Result<()> {
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
    for name in calls_of.keys() {
        let d = depth_of(name, calls_of, &mut depth);
        if d > MAX_FN_DEPTH {
            // pinned at tests/fn_expand_direct.rs
            // `a_depth_8_chain_expands_transitively_...`
            return Err(ChorusError::new(format!(
                "{singular} '{name}' sits {d} calls deep — the nesting cap is {MAX_FN_DEPTH}"
            )));
        }
    }
    Ok(())
}
