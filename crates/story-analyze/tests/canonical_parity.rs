//! Differential parity between the fingerprint key and the historical
//! full-form key: over explored states and targeted mutations, both must
//! partition states identically — same states key equal, different states
//! key different.

#[path = "common/old_canonical_ref.rs"]
mod old_canonical_ref;

use std::collections::HashMap;
use std::sync::Arc;

use story_analyze::canonical::CanonicalStateKey;
use story_analyze::search::{Analyzer, ExploreConfig};
use story_engine::EffectRegistry;

const EMBERWATCH: &str = include_str!("../../../docs/emberwatch.sw");

fn boot(content: Arc<story_content::Content>) -> Analyzer {
    let (analyzer, diags) = Analyzer::boot(content, EffectRegistry::new(), 7, None);
    assert!(diags.iter().all(|d| !d.is_error()), "{diags:?}");
    analyzer
}

/// Two partitions agree when they induce the same equivalence relation:
/// every old group maps to exactly one new group and vice versa.
fn partitions_agree(
    old_groups: &HashMap<old_canonical_ref::CanonicalStateKey, Vec<usize>>,
    new_groups: &HashMap<CanonicalStateKey, Vec<usize>>,
    n: usize,
) -> bool {
    let mut old_of = vec![0usize; n];
    for (gi, members) in old_groups.values().enumerate() {
        for &i in members {
            old_of[i] = gi;
        }
    }
    let mut new_of = vec![0usize; n];
    for (gi, members) in new_groups.values().enumerate() {
        for &i in members {
            new_of[i] = gi;
        }
    }
    let mut forward: HashMap<usize, usize> = HashMap::new();
    let mut backward: HashMap<usize, usize> = HashMap::new();
    for i in 0..n {
        let o = old_of[i];
        let w = new_of[i];
        match forward.get(&o) {
            Some(&g) if g != w => return false,
            Some(_) => {}
            None => {
                forward.insert(o, w);
            }
        }
        match backward.get(&w) {
            Some(&g) if g != o => return false,
            Some(_) => {}
            None => {
                backward.insert(w, o);
            }
        }
    }
    true
}

#[test]
fn fingerprint_partitions_explored_states_like_the_full_key() {
    let out = story_lang::compile_source("emberwatch", EMBERWATCH);
    let content = out.content.expect("emberwatch compiles");
    let mut analyzer = boot(content.into());
    let graph = analyzer.explore(&ExploreConfig {
        max_states: 3000,
        ..Default::default()
    });

    let states: Vec<&story_core::State> = graph.nodes.iter().map(|n| &n.state).collect();
    let mut old_groups: HashMap<old_canonical_ref::CanonicalStateKey, Vec<usize>> = HashMap::new();
    let mut new_groups: HashMap<CanonicalStateKey, Vec<usize>> = HashMap::new();
    for (i, s) in states.iter().enumerate() {
        old_groups
            .entry(old_canonical_ref::CanonicalStateKey::of(s, analyzer.content()))
            .or_default()
            .push(i);
        new_groups
            .entry(CanonicalStateKey::of(s, analyzer.content()))
            .or_default()
            .push(i);
    }
    assert!(
        partitions_agree(&old_groups, &new_groups, states.len()),
        "the fingerprint partitions the explored states differently from the full key: \
         {} old groups vs {} new groups over {} states",
        old_groups.len(),
        new_groups.len(),
        states.len()
    );
}

#[test]
fn fingerprint_reacts_to_every_field_the_full_key_reads() {
    let out = story_lang::compile_source("emberwatch", EMBERWATCH);
    let content = out.content.expect("emberwatch compiles");
    let analyzer = boot(content.into());
    let base = analyzer.start().clone();
    let content = analyzer.content();

    let old = |s: &story_core::State| old_canonical_ref::CanonicalStateKey::of(s, content);
    let new = |s: &story_core::State| CanonicalStateKey::of(s, content);

    let cases: Vec<(&str, Box<dyn Fn(&mut story_core::State)>)> = vec![
        ("rng_counter", Box::new(|s| s.rng_counter += 1)),
        ("now", Box::new(|s| s.now += 10)),
        (
            "legacy",
            Box::new(|s| {
                s.legacy.insert("dawns_seen".into(), 3);
            }),
        ),
        (
            "entity aspect",
            Box::new(|s| {
                let e = *s.entities.keys().next().expect("an entity");
                s.entities
                    .get_mut(&e)
                    .unwrap()
                    .aspects
                    .entry("heat".into())
                    .and_modify(|v| *v += 1);
            }),
        ),
        (
            "entity expiry",
            Box::new(|s| {
                let e = *s.entities.keys().next().expect("an entity");
                s.entities.get_mut(&e).unwrap().expires_at = Some(42);
            }),
        ),
        (
            "entity zone move",
            Box::new(|s| {
                let e = *s.entities.keys().next().expect("an entity");
                let z = *s.zones.keys().next().expect("a zone");
                s.entities.get_mut(&e).unwrap().zone = z;
            }),
        ),
        (
            "zone policy bit",
            Box::new(|s| {
                let z = *s.zones.keys().next().expect("a zone");
                let zone = s.zones.get_mut(&z).unwrap();
                zone.public = !zone.public;
            }),
        ),
        (
            "entity name",
            Box::new(|s| {
                let e = *s.entities.keys().next().expect("an entity");
                s.entities.get_mut(&e).unwrap().name = "renamed".into();
            }),
        ),
    ];

    for (name, mutate) in &cases {
        let mut s = base.clone();
        mutate(&mut s);
        let moved_old = old(&s) != old(&base);
        let moved_new = new(&s) != new(&base);
        assert_eq!(
            moved_old, moved_new,
            "mutation `{name}`: full key moved = {moved_old}, fingerprint moved = {moved_new}"
        );
    }
}

