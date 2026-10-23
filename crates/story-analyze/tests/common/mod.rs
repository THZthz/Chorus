//! Shared helpers for the story-analyze tests: fixture compilation, boot,
//! and the analyzer construction. Each test crate compiles this module
//! whole; it uses a subset.

#![allow(dead_code)]

use std::sync::Arc;

use story_analyze::{Analyzer, ReachQuery};
use story_content::Content;
use story_engine::{EffectRegistry, Engine};

/// Compile a fixture; any error diagnostic fails the test.
pub fn compile(src: &str) -> Arc<Content> {
    let out = story_lang::compile_source("fixture", src);
    let errors: Vec<String> = out
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "fixture failed to compile:\n{}",
        errors.join("\n")
    );
    Arc::new(out.content.expect("no errors means content"))
}

/// Boot the analyzer; any error diagnostic fails the test.
pub fn analyzer(content: Arc<Content>, seed: u64) -> Analyzer {
    let (a, diags) = Analyzer::boot(content, EffectRegistry::new(), seed, None);
    let errors: Vec<String> = diags
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect();
    assert!(errors.is_empty(), "boot failed:\n{}", errors.join("\n"));
    a
}

/// Boot a plain engine (state inspectors in the fixtures).
pub fn boot(content: Arc<Content>, seed: u64) -> Engine {
    let (engine, diags) = Engine::boot(content, EffectRegistry::new(), seed, None);
    let errors: Vec<String> = diags
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect();
    assert!(errors.is_empty(), "boot failed:\n{}", errors.join("\n"));
    engine
}

/// A bounded query with Dijkstra semantics (heuristic off at the analyzer).
pub fn dijkstra_query(goal: story_analyze::GoalPredicate) -> ReachQuery {
    let mut q = ReachQuery::new(goal);
    q.max_states = 20_000;
    q
}
