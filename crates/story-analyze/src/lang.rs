//! Query-goal parsing behind the optional `lang` feature (story-analyze
//! spec §1.1): query goals written directly in `.sw` condition syntax, with
//! inline declared predicates, compiled by the real `story-lang` pipeline.
//! The dependency law is untouched: `story-analyze` already depends on the
//! compiler as a tool-layer consumer; the kernel never sees either.

use story_content::ir::Expr;

/// Compile one `.sw` condition expression (the right-hand side of a
/// `predicate` or `when` line) into the engine-side `Expr`.
///
/// `decls` carries the declaration blocks the condition reads — aspect,
/// card, legacy, and zone declarations in `.sw` syntax (e.g. `aspects`,
/// `  heat: 0`, `cards`, `  card warden`, `legacies`, `  luck: 0`); they
/// are spliced into the synthetic compilation target so the full pipeline
/// (predicate inlining, the literal grammar, totality checks) applies
/// unchanged. `predicates` carries whole `predicate` lines. Only the
/// lowered expression of the condition is kept.
pub fn parse_cond(decls: &[&str], predicates: &[&str], cond: &str) -> Result<Expr, String> {
    let mut src = String::from(
        "game \"query\"\n  seed host\n  turn 1m\n  home qz\n\nzones\n  qz\n\n",
    );
    for block in decls {
        src.push_str(block.trim_end());
        src.push_str("\n\n");
    }
    if !predicates.is_empty() {
        src.push_str("predicates\n");
        for p in predicates {
            src.push_str("  ");
            src.push_str(p.trim());
            src.push('\n');
        }
        src.push('\n');
    }
    src.push_str(
        "verbs\n  verb qv committable\n    recipe qr\n      craftable\n      duration 1m\n      when ",
    );
    src.push_str(cond.trim());
    src.push('\n');

    let out = story_lang::compile_source("query.sw", &src);
    let errors: Vec<String> = out
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| format!("{}", d.message))
        .collect();
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }
    let content = out
        .content
        .ok_or_else(|| "the query content did not compile".to_string())?;
    let verb = content
        .verbs
        .get("qv")
        .ok_or_else(|| "the query recipe is missing".to_string())?;
    let recipe = verb
        .recipes
        .values()
        .next()
        .ok_or_else(|| "the query recipe is missing".to_string())?;
    recipe
        .when
        .clone()
        .ok_or_else(|| "the query condition lowered to nothing".to_string())
}

/// Compile a goal expression into [`crate::query::GoalPredicate::Expr`].
pub fn parse_goal(
    decls: &[&str],
    predicates: &[&str],
    cond: &str,
) -> Result<crate::query::GoalPredicate, String> {
    Ok(crate::query::GoalPredicate::Expr(parse_cond(
        decls, predicates, cond,
    )?))
}
