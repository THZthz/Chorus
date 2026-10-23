//! Tier-1 hyperedge shapes over a small fixture content (story-analyze
//! spec §4): recipes as hyperedges with their `when`/slot requirements, the
//! abort hyperedges of fallible lists, decks, boot instantiations, trigger
//! books, provenance, and the plausibility pre-check.

use std::collections::BTreeSet;

use story_analyze::causal::{CausalGraph, Fact, Hyperedge, Plausibility, Site};
use story_content::Content;
use story_core::CardId;

fn compile(src: &str) -> Content {
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
    out.content.expect("no errors means content")
}

fn fact(s: &str) -> CardId {
    CardId::from(s.to_string())
}

const SRC: &str = r#"
game "Causalgraph"
  seed host
  turn 1m
  home hall

zones
  hall
  shore

aspects
  heat: 0
  charge: 0

cards
  card lamp
    kind fixture  unique
    aspects heat 1
    start in hall
  card coal
    kind matter  stackable
    aspects charge 1
    start in hall count 2
    on complete
      rule echo
        effects
          spawn ash count 1 into hall
  card ash
    kind matter  stackable
  card relic
    kind treasure
  card ward
    kind charm  unique
    lifetime 2h
    on expire
      rule fade
        effects
          set subject.charge += 1

decks
  deck pile
    entries coal, ash
    fallback ash

legacies
  dawns: 0

endings
  ending lit "Lit" grand
  ending drowned "Drowned" dark

verbs
  verb tend committable instances 1
    recipe stoke
      priority 10  craftable
      duration 0
      when lamp.heat >= 2
      slots
        fuel: 1..1 consuming aspect:charge
      effects
        set lamp.heat += total(slot:fuel, aspect:charge)
        legacy dawns += 1
    recipe ignite
      priority 20  craftable
      duration 30m
      effects
        roll 1d6 as omen
        consume aspect:charge 2
        ending lit
    recipe drown
      priority 30  craftable
      duration 0
      effects
        draw from pile count 1 into hall
  verb pulse autonomous instances 1
    recipe beat
      duration 1m
      effects
        signal relight
  verb call committable instances 1
    recipe summon
      duration 0
      effects
        start secret
  verb secret uncommittable instances 1
    recipe unveil
      chain-only
      duration 0
      effects
        spawn relic into hall

on event relight
  rule again when legacy.dawns >= 1
    effects
      set lamp.heat += 1
"#;

fn edge<'a>(graph: &'a CausalGraph, site: &Site) -> Option<&'a Hyperedge> {
    graph.edges().iter().find(|e| &e.site == site)
}

fn set(items: &[&Fact]) -> BTreeSet<Fact> {
    items.iter().map(|f| (*f).clone()).collect()
}

#[test]
fn boot_state_opens_player_and_heartbeat_recipes() {
    let graph = CausalGraph::build(&compile(SRC));
    let boot = edge(&graph, &Site::BootState).expect("boot state edge");
    assert!(boot.requires.is_empty());
    for r in ["tend.stoke", "tend.ignite", "tend.drown", "pulse.beat", "call.summon"] {
        assert!(
            boot.produces.contains(&Fact::RecipeCanOpen(r.into())),
            "boot should open {r}"
        );
    }
    assert!(!boot.produces.contains(&Fact::RecipeCanOpen("secret.unveil".into())));
}

#[test]
fn boot_cards_produce_their_declarations() {
    let graph = CausalGraph::build(&compile(SRC));
    let lamp = edge(&graph, &Site::Boot(fact("lamp"))).expect("lamp boots");
    assert_eq!(
        lamp.produces,
        set(&[
            &Fact::CardLive(fact("lamp")),
            &Fact::CardInZone(fact("lamp"), "hall".into()),
            &Fact::CardHasAspect(fact("lamp"), "heat".into()),
        ])
    );
    assert!(edge(&graph, &Site::Boot(fact("ash"))).is_none(), "ash never boots");
}

#[test]
fn recipe_edge_carries_when_reads_and_outputs() {
    let graph = CausalGraph::build(&compile(SRC));
    let stoke = edge(&graph, &Site::Recipe("tend.stoke".into())).expect("stoke edge");
    // `lamp.heat >= 2` sits above the declared default (1), so the read is
    // a necessary fact; a default-sufficient read (`>= 1`) would require
    // nothing.
    assert_eq!(
        stoke.requires,
        set(&[
            &Fact::RecipeCanOpen("tend.stoke".into()),
            &Fact::CardHasAspect(fact("lamp"), "heat".into()),
        ])
    );
    assert_eq!(
        stoke.produces,
        set(&[
            &Fact::AspectCanIncrease {
                card: Some(fact("lamp")),
                key: "heat".into(),
                unclamped: false
            },
            &Fact::LegacyCanChange("dawns".into()),
            &Fact::RecipeCanComplete("tend.stoke".into()),
        ])
    );
    // Stoke's list cannot fail: no abort hyperedge.
    assert!(edge(&graph, &Site::RecipeAbort("tend.stoke".into())).is_none());
}

#[test]
fn fallible_list_emits_the_abort_hyperedge() {
    let graph = CausalGraph::build(&compile(SRC));
    // Ignite: a consume whose target can come up short -> the abort path.
    let ignite = edge(&graph, &Site::Recipe("tend.ignite".into())).expect("ignite edge");
    assert!(ignite.produces.contains(&Fact::EndingReached("lit".into())));
    let abort = edge(&graph, &Site::RecipeAbort("tend.ignite".into()))
        .expect("ignite can abort");
    assert_eq!(abort.requires, ignite.requires);
    assert!(abort.produces.is_empty(), "no completion book walks alone");
    // Unveil: a SingleCopy spawn can hard-fail with NameInUse.
    assert!(edge(&graph, &Site::RecipeAbort("secret.unveil".into())).is_some());
    // Drown: the deck declares a fallback, so the draw cannot fail.
    assert!(edge(&graph, &Site::RecipeAbort("tend.drown".into())).is_none());
}

#[test]
fn draw_produces_every_deck_entry_and_the_fallback() {
    let graph = CausalGraph::build(&compile(SRC));
    let drown = edge(&graph, &Site::Recipe("tend.drown".into())).expect("drown edge");
    for card in ["coal", "ash"] {
        assert!(drown.produces.contains(&Fact::CardLive(fact(card))));
        assert!(
            drown
                .produces
                .contains(&Fact::CardInZone(fact(card), "hall".into()))
        );
    }
    let deck = edge(&graph, &Site::Deck("pile".into())).expect("deck edge");
    assert!(deck.produces.contains(&Fact::CardLive(fact("coal"))));
    assert!(deck.produces.contains(&Fact::CardLive(fact("ash"))));
}

#[test]
fn card_book_connects_to_binding_and_storage_routes() {
    let graph = CausalGraph::build(&compile(SRC));
    let site = Site::CardBook {
        card: fact("coal"),
        qualifier: None,
    };
    let routes: Vec<&Hyperedge> = graph.edges().iter().filter(|e| e.site == site).collect();
    // tend.stoke can bind coal (its fuel filter matches); tend.drown can
    // mint it into an interior (its draw produces it). Each route spawns
    // ash into the hall.
    let mut required_routes = BTreeSet::new();
    for e in &routes {
        assert!(e.produces.contains(&Fact::CardLive(fact("ash"))));
        assert_eq!(e.requires.len(), 1, "one gate per route");
        match e.requires.iter().next() {
            Some(Fact::RecipeCanComplete(r)) => {
                required_routes.insert(r.clone());
            }
            other => panic!("unexpected requirement {other:?}"),
        }
    }
    assert!(required_routes.contains(&"tend.stoke".into()));
    assert!(required_routes.contains(&"tend.drown".into()));
}

#[test]
fn expiry_book_requires_a_live_card_and_names_the_subject() {
    let graph = CausalGraph::build(&compile(SRC));
    let fade = edge(&graph, &Site::CardExpire(fact("ward"))).expect("ward expiry");
    assert_eq!(fade.requires, set(&[&Fact::CardLive(fact("ward"))]));
    assert_eq!(
        fade.produces,
        set(&[&Fact::AspectCanIncrease {
            card: Some(fact("ward")),
            key: "charge".into(),
            unclamped: false
        }])
    );
}

#[test]
fn event_book_chains_from_signal_to_products() {
    let graph = CausalGraph::build(&compile(SRC));
    let relight = edge(&graph, &Site::Event("relight".into())).expect("relight book");
    assert_eq!(
        relight.requires,
        set(&[
            &Fact::EventSignaled("relight".into()),
            &Fact::LegacyCanChange("dawns".into()),
        ])
    );
    assert!(relight.produces.contains(&Fact::AspectCanIncrease {
        card: Some(fact("lamp")),
        key: "heat".into(),
        unclamped: false
    }));
    // The signal site produces the ignition fact.
    let beat = edge(&graph, &Site::Recipe("pulse.beat".into())).expect("beat edge");
    assert!(beat.produces.contains(&Fact::EventSignaled("relight".into())));
}

#[test]
fn provenance_chains_ending_to_the_boot_state() {
    let graph = CausalGraph::build(&compile(SRC));
    let goal = Fact::EndingReached("lit".into());
    let proof = graph.explain_how_to_get(&goal).expect("lit is reachable");
    assert_eq!(proof.fact, goal);
    assert_eq!(proof.site, Site::Recipe("tend.ignite".into()));
    // The open gate (grounded in the boot state) and the consume target's
    // liveness (grounded in the boot instantiation), each a root itself.
    assert_eq!(proof.requires.len(), 2);
    assert!(proof.requires.iter().all(|p| p.requires.is_empty()));
    assert!(proof.requires.iter().any(|p| {
        p.fact == Fact::RecipeCanOpen("tend.ignite".into()) && p.site == Site::BootState
    }));
    assert!(proof.requires.iter().any(|p| {
        p.fact == Fact::CardLive(fact("coal")) && p.site == Site::Boot(fact("coal"))
    }));
}

#[test]
fn polarity_producers_bridge_to_card_has_aspect() {
    let graph = CausalGraph::build(&compile(SRC));
    let goal = Fact::CardHasAspect(fact("lamp"), "heat".into());
    let producers = graph.producers_of(&goal);
    let sites: BTreeSet<Site> = producers
        .iter()
        .map(|&i| graph.edges()[i].site.clone())
        .collect();
    // The boot declaration and both increase producers (stoke's set and the
    // event book's set) all produce the aspect fact.
    assert!(sites.contains(&Site::Boot(fact("lamp"))));
    assert!(sites.contains(&Site::Recipe("tend.stoke".into())));
    assert!(sites.contains(&Site::Event("relight".into())));
    assert_eq!(
        graph.plausibility(&goal),
        Plausibility::Plausible,
    );
}

#[test]
fn plausibility_pre_check_classifies_goals() {
    let graph = CausalGraph::build(&compile(SRC));
    // A start route grounds the chain-only recipe's open.
    assert_eq!(
        graph.plausibility(&Fact::RecipeCanOpen("secret.unveil".into())),
        Plausibility::Plausible
    );
    // An ending no effect ever fires is statically impossible.
    assert_eq!(
        graph.plausibility(&Fact::EndingReached("drowned".into())),
        Plausibility::StaticallyImpossible
    );
}

#[test]
fn host_ops_degrade_impossible_to_unknown() {
    // Content referencing a host op is opaque: no static impossibility.
    let mut content = compile(SRC);
    let summon = content
        .verbs
        .get_mut("call")
        .unwrap()
        .recipes
        .get_mut("summon")
        .unwrap();
    summon.effects = vec![story_content::ir::EffectNode::Op {
        name: "grant".to_string(),
        args: vec![],
    }];
    let graph = CausalGraph::build(&content);
    assert!(graph.host_opaque());
    assert_eq!(
        graph.plausibility(&Fact::EndingReached("drowned".into())),
        Plausibility::UnknownHost
    );
}
