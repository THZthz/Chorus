//! Query mini-language (§4.5) and condition predicates (§6.3).
//!
//! Conventions: one `#[test]` per behavior; fresh State/Dictionary per
//! test; error assertions go through `assert_err_msg(res, "re")`.
//!
//! Binding shape: tests may pass standalone entities; the bindings hold
//! entity IDS read back through `state.entities` — every bound card
//! therefore lives in the test State under the same id, which is exactly
//! the engine's own invariant (bindings always resolve into the live
//! state).

use indexmap::IndexMap;

use chorus::content::script::compile_script;

use chorus::core::dict::core_dictionary;
use chorus::core::predicate::{eval_pred, parse_predicate, Pred, PredCtx, RoleBindings};
use chorus::core::query::{match_query, parse_query};
use chorus::core::roles::Role;
use chorus::core::types::aspects_from_pairs;
use chorus::core::types::{add_entity, new_state, Aspects, AttrValue, Attrs, Entity, State};

use crate::common::harness::assert_err_msg;

/// A bare entity: empty aspects/attrs, never born, never dead, not
/// stackable.
fn ent(id: &str, kind: &str, name: &str) -> Entity {
    Entity {
        id: id.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        aspects: Aspects::new(),
        attrs: Attrs::new(),
        born_action: None,
        dead_action: None,
        stackable: false,
    }
}

/// `{ secret: 3, language: { latin: 2 } }` object-literal shape — dotted
/// keys go through aspects_from_pairs (query.rs), the object-literal
/// semantics (Q13 one-level dots).
fn with_aspects(mut e: Entity, pairs: &[(&str, i64)]) -> Entity {
    let mut m = IndexMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), *v);
    }
    e.aspects = aspects_from_pairs(&m);
    e
}

fn with_attrs(mut e: Entity, pairs: &[(&str, AttrValue)]) -> Entity {
    for (k, v) in pairs {
        e.attrs.insert((*k).to_string(), v.clone());
    }
    e
}

fn stackable(mut e: Entity) -> Entity {
    e.stackable = true;
    e
}

/// strict/roll unset (when:-style evaluation).
fn ctx<'a>(b: &'a RoleBindings, state: &'a State) -> PredCtx<'a> {
    PredCtx { b, state, strict: false, roll: None }
}

/// Role → bound ids, insertion order kept.
fn bindings(pairs: &[(Role, &[&str])]) -> RoleBindings {
    pairs
        .iter()
        .map(|(role, ids)| (*role, ids.iter().map(|s| (*s).to_string()).collect()))
        .collect()
}

/// eval_pred wrapper — returns a bare boolean.
fn ev(p: &Pred, b: &RoleBindings, s: &State) -> bool {
    eval_pred(p, &ctx(b, s)).unwrap()
}

/// No roles bound at all.
fn no_binds() -> RoleBindings {
    RoleBindings::new()
}
// ---------- slot queries, §4.5 ---------------------------------------------

#[test]
fn a_matching_card_passes_every_term() {
    let dict = core_dictionary();
    let q = parse_query(
        "kind:text aspect:secret>=3 aspect:language.latin>=2 state:!sealed -aspect:winter",
        &dict,
    )
    .unwrap();
    let book = with_aspects(ent("b", "text", "B"), &[("secret", 3), ("language.latin", 2)]);
    assert!(match_query(&book, &q));
}

#[test]
fn each_term_can_fail_the_card() {
    let dict = core_dictionary();
    let q = parse_query(
        "kind:text aspect:secret>=3 aspect:language.latin>=2 state:!sealed -aspect:winter",
        &dict,
    )
    .unwrap();
    assert!(!match_query(&ent("x", "item", "X"), &q)); // kind
    assert!(!match_query(&with_aspects(ent("x", "text", "X"), &[("secret", 2)]), &q)); // level
    assert!(!match_query(
        &with_aspects(
            ent("x", "text", "X"),
            &[("secret", 3), ("language.latin", 2), ("winter", 1)]
        ),
        &q
    )); // negation
    let sealed = with_attrs(
        with_aspects(ent("x", "text", "X"), &[("secret", 3), ("language.latin", 2)]),
        &[("state.sealed", AttrValue::Bool(true))],
    );
    assert!(!match_query(&sealed, &q));
}

#[test]
fn bare_aspect_term_means_level_ge_1() {
    let dict = core_dictionary();
    let q1 = parse_query("aspect:lantern", &dict).unwrap();
    assert!(match_query(&with_aspects(ent("a", "character", "A"), &[("lantern", 1)]), &q1));
    assert!(!match_query(&ent("a", "character", "A"), &q1));
}

// ---------- alternation, §4.5 ----------------------------------------------

#[test]
fn alternation_matches_when_any_group_matches() {
    let dict = core_dictionary();
    let q = parse_query("aspect:ability | aspect:job", &dict).unwrap();
    assert!(match_query(&with_aspects(ent("a", "item", "A"), &[("ability", 1)]), &q));
    assert!(match_query(&with_aspects(ent("j", "item", "J"), &[("job", 2)]), &q));
    assert!(!match_query(&ent("n", "item", "N"), &q));
}

#[test]
fn alternation_binds_loosest_each_side_is_a_conjunction() {
    let dict = core_dictionary();
    // the spec's own example (§4.5): ability OR (job AND ritual) —
    // a card with job but no ritual does not match
    let q = parse_query("aspect:ability | aspect:job aspect:ritual", &dict).unwrap();
    assert_eq!(q.len(), 2);
    assert!(match_query(&with_aspects(ent("a", "item", "A"), &[("ability", 1)]), &q));
    assert!(match_query(&with_aspects(ent("b", "item", "B"), &[("job", 1), ("ritual", 1)]), &q));
    assert!(!match_query(&with_aspects(ent("c", "item", "C"), &[("job", 1)]), &q));
}

#[test]
fn alternation_groups_take_levels_negation_and_sub_aspects() {
    let dict = core_dictionary();
    let q = parse_query("aspect:lore>=6 | -name:SUSPICION aspect:language.latin", &dict).unwrap();
    assert_eq!(q.len(), 2);
    // group 1: the level threshold
    assert!(match_query(&with_aspects(ent("l", "text", "L"), &[("lore", 6)]), &q));
    assert!(!match_query(&with_aspects(ent("m", "text", "M"), &[("lore", 5)]), &q));
    // group 2: per-term negation and a sub-aspect key compose INSIDE the group
    assert!(match_query(&with_aspects(ent("u", "rite", "U"), &[("language.latin", 1)]), &q));
    assert!(!match_query(
        &with_aspects(ent("s", "rite", "SUSPICION"), &[("language.latin", 1)]),
        &q
    )); // the negated name fails the group
    assert!(!match_query(&with_aspects(ent("g", "rite", "G"), &[("language.greek", 1)]), &q));
    // the sub-aspect misses
}

#[test]
fn alternation_group_edges_are_parse_errors_a_glued_pipe_is_a_term_error() {
    let dict = core_dictionary();
    // a lone '|' separates groups ONLY as its own whitespace token: a
    // leading '|', a trailing '|', or an empty middle group is a loud
    // parse error
    assert_err_msg(parse_query("| aspect:x", &dict), r"expected a group before '\|'");
    assert_err_msg(parse_query("aspect:x |", &dict), r"expected a group after '\|'");
    assert_err_msg(parse_query("aspect:x | | aspect:y", &dict), r"expected a group before '\|'");
    // glued into a term's value it rides along and the ATOM grammar
    // rejects it — never a silent second group
    assert_err_msg(parse_query("aspect:a|b", &dict), "bad aspect term");
}

// ---------- condition predicates, §6.3 -------------------------------------

/// The section-level state.
fn cond_state() -> State {
    let mut state = new_state(0);
    add_entity(
        &mut state,
        with_aspects(ent("ana", "character", "ANA"), &[("occult", 2), ("lantern", 1)]),
    )
    .unwrap();
    add_entity(&mut state, with_aspects(ent("bertha", "character", "BERTHA"), &[("occult", 0)]))
        .unwrap();
    add_entity(&mut state, with_aspects(ent("rite", "rite", "RITE"), &[("demand", 3)])).unwrap();
    state.relations.insert(
        "ana|bertha|parent_child".to_string(),
        chorus::core::types::Relation {
            a: "ana".to_string(),
            b: "bertha".to_string(),
            type_: "parent_child".to_string(),
            value: 1,
            born_action: 0,
            dead_action: None,
        },
    );
    state
}

#[test]
fn count_actors_equals_2() {
    let dict = core_dictionary();
    let state = cond_state();
    let p = parse_predicate("count(actors) = 2", &dict).unwrap();
    assert!(ev(&p, &bindings(&[(Role::Actors, &["ana", "bertha"])]), &state));
    assert!(!ev(&p, &bindings(&[(Role::Actors, &["ana"])]), &state));
}

#[test]
fn rel_parent_child_actors_pairwise_existence() {
    let dict = core_dictionary();
    // Symmetric, order-free
    let state = cond_state();
    let p = parse_predicate("rel(parent_child, actors)", &dict).unwrap();
    assert!(ev(&p, &bindings(&[(Role::Actors, &["ana", "bertha"])]), &state));
    assert!(ev(&p, &bindings(&[(Role::Actors, &["bertha", "ana"])]), &state));
    assert!(!ev(&p, &bindings(&[(Role::Actors, &["ana", "rite"])]), &state));
}

#[test]
fn every_any_quantifiers() {
    let dict = core_dictionary();
    let state = cond_state();
    let every = parse_predicate("every(actors, aspect:occult >= 2)", &dict).unwrap();
    let any = parse_predicate("any(actors, aspect:occult >= 2)", &dict).unwrap();
    assert!(ev(&every, &bindings(&[(Role::Actors, &["ana"])]), &state));
    assert!(!ev(&every, &bindings(&[(Role::Actors, &["ana", "bertha"])]), &state));
    assert!(ev(&any, &bindings(&[(Role::Actors, &["ana", "bertha"])]), &state));
    assert!(!ev(&any, &bindings(&[(Role::Actors, &["bertha"])]), &state));
}

#[test]
fn aggregate_vs_path_operand() {
    let dict = core_dictionary();
    // Total(actors, …) < object.aspect.demand
    let state = cond_state();
    let p = parse_predicate("total(actors, aspect:lantern) < object.aspect.demand", &dict).unwrap();
    assert!(ev(&p, &bindings(&[(Role::Actors, &["ana"]), (Role::Object, &["rite"])]), &state)); // 1 < 3
    assert!(ev(
        &p,
        &bindings(&[(Role::Actors, &["ana", "bertha"]), (Role::Object, &["rite"])]),
        &state
    )); // 1+0 < 3
}

// ---------- attr paths in comparisons, E1 ----------------------------------
// A numeric attr entry ABSENT from an ALIVE bound card compares as 0 (the
// §4.4 SET-delta rule "a missing entry counts as 0" and total()'s
// unwrap_or(0); script entities mint empty attrs, so a `silver < cost`
// broke-gate must hold for a first-time buyer). ASPECT paths keep §6.3's
// frozen missing→false; a role with no bound card and all-dead bindings
// still never compare.

/// `{ actors: a purse-less character, object: a cost-1 rite }` — the
/// broke-gate shape, plus a priced purse to pin the present-attr reads.
fn attr_state() -> State {
    let mut state = new_state(0);
    add_entity(&mut state, ent("pauper", "character", "PAUPER")).unwrap(); // empty attrs, alive
    add_entity(
        &mut state,
        with_attrs(ent("priced", "character", "PRICED"), &[("silver", AttrValue::Num(2))]),
    )
    .unwrap();
    add_entity(
        &mut state,
        with_attrs(ent("ticket", "rite", "TICKET"), &[("seal", AttrValue::Str("wax".into()))]),
    )
    .unwrap();
    add_entity(&mut state, with_aspects(ent("priced_rite", "rite", "PRICED_RITE"), &[("cost", 1)]))
        .unwrap();
    state
}

#[test]
fn a_missing_attr_on_a_living_card_compares_as_0() {
    let dict = core_dictionary();
    let s = attr_state();
    let b = bindings(&[(Role::Actors, &["pauper"])]);
    assert!(ev(&parse_predicate("actors.attr.silver < 3", &dict).unwrap(), &b, &s));
    assert!(!ev(&parse_predicate("actors.attr.silver >= 3", &dict).unwrap(), &b, &s));
    assert!(ev(&parse_predicate("actors.attr.silver = 0", &dict).unwrap(), &b, &s));
    assert!(!ev(&parse_predicate("actors.attr.silver != 0", &dict).unwrap(), &b, &s));
}

#[test]
fn a_present_attr_compares_exactly_as_authored() {
    let dict = core_dictionary();
    let s = attr_state();
    let b = bindings(&[(Role::Actors, &["priced"])]);
    assert!(ev(&parse_predicate("actors.attr.silver < 3", &dict).unwrap(), &b, &s)); // 2 < 3
    assert!(!ev(&parse_predicate("actors.attr.silver >= 3", &dict).unwrap(), &b, &s));
    assert!(ev(&parse_predicate("actors.attr.silver = 2", &dict).unwrap(), &b, &s));
    assert!(ev(&parse_predicate("actors.attr.silver != 0", &dict).unwrap(), &b, &s));
}

#[test]
fn the_missing_reads_0_on_both_operands_of_the_broke_gate_shape() {
    let dict = core_dictionary();
    let s = attr_state();
    // the slice's gate: a purse-less actor against a cost-1 object — the
    // refusal must fire for the first-time buyer
    let b = bindings(&[(Role::Actors, &["pauper"]), (Role::Object, &["priced_rite"])]);
    assert!(ev(
        &parse_predicate("actors.attr.silver < object.aspect.cost", &dict).unwrap(),
        &b,
        &s
    ));
    // BOTH sides missing reads 0 < 0, false — and 0 <= 0, true, so the
    // default provably serves the right operand too
    assert!(!ev(
        &parse_predicate("actors.attr.silver < object.attr.price", &dict).unwrap(),
        &b,
        &s
    ));
    assert!(ev(
        &parse_predicate("object.attr.price <= actors.attr.silver", &dict).unwrap(),
        &b,
        &s
    ));
}

#[test]
fn the_frozen_edges_still_never_compare() {
    let dict = core_dictionary();
    let s = attr_state();
    // §6.3 frozen: a missing ASPECT on a bound card reads false — only the
    // attr namespace defaults
    let aspect = bindings(&[(Role::Object, &["ticket"])]);
    assert!(!ev(&parse_predicate("object.aspect.cost < 3", &dict).unwrap(), &aspect, &s));
    // no bound card at all: the when:-side silent false (and the strict
    // §6.3 rejection below keeps firing inside eval_expr)
    assert!(!ev(&parse_predicate("actors.attr.silver < 3", &dict).unwrap(), &no_binds(), &s));
    // all-dead bindings: the dead-stamp silent false, not a 0-read
    let mut dead = attr_state();
    dead.entities.get_mut("pauper").unwrap().dead_action = Some(1);
    assert!(!ev(
        &parse_predicate("actors.attr.silver < 3", &dict).unwrap(),
        &bindings(&[(Role::Actors, &["pauper"])]),
        &dead
    ));
    // a PRESENT non-numeric entry never compares (it is not an absent entry)
    let sealed = bindings(&[(Role::Actors, &["pauper"]), (Role::Object, &["ticket"])]);
    assert!(!ev(&parse_predicate("object.attr.seal = 0", &dict).unwrap(), &sealed, &s));
}

#[test]
fn strict_keeps_rejecting_an_unbound_role_operand() {
    let dict = core_dictionary();
    // §6.3's asymmetric discipline is untouched: an IF-condition path over
    // a role with NO bound card rejects the step — the default fires only
    // when an ALIVE card is bound and the ENTRY is absent
    let s = attr_state();
    let p = parse_predicate("actors.attr.silver < 3", &dict).unwrap();
    let strict = PredCtx { b: &no_binds(), state: &s, strict: true, roll: None };
    assert_err_msg(
        eval_pred(&p, &strict).map(|_| ()),
        "operand 'actors.attr.silver' cannot resolve",
    );
}

#[test]
fn group_gate_with_not_any_the_magick_gate_shape() {
    let dict = core_dictionary();
    // The standalone-entity binding shape: the magick card rides
    // the bindings alone; the Rust id-handle model
    // needs it in the state — same id, semantics unchanged.
    let mut state = cond_state();
    add_entity(&mut state, with_aspects(ent("m", "text", "M"), &[("magick", 1)])).unwrap();
    let p =
        parse_predicate("object.aspect.magick >= 1 AND NOT any(actors, aspect:occult >= 2)", &dict)
            .unwrap();
    assert!(ev(&p, &bindings(&[(Role::Actors, &["bertha"]), (Role::Object, &["m"])]), &state)); // nobody knows
    assert!(!ev(
        &p,
        &bindings(&[(Role::Actors, &["ana", "bertha"]), (Role::Object, &["m"])]),
        &state
    )); // ANA knows
}

// ---------- exists(...), §6.3 CS extantreqs --------------------------------

/// The hunter prowls the villa; nothing
/// else in play has hunter/secret.
fn world() -> State {
    let mut s = new_state(0);
    add_entity(
        &mut s,
        with_attrs(
            with_aspects(ent("hunter", "character", "HUNTER"), &[("hunter", 2)]),
            &[("container", AttrValue::Str("villa".into()))],
        ),
    )
    .unwrap();
    add_entity(
        &mut s,
        with_attrs(
            with_aspects(ent("rite", "rite", "RITE_OF_DAWN"), &[("ritual", 1)]),
            &[("container", AttrValue::Str("house".into()))],
        ),
    )
    .unwrap();
    s
}

#[test]
fn name_form_true_while_the_named_card_is_alive_false_after_destroy() {
    let dict = core_dictionary();
    let mut s = world();
    let p = parse_predicate("exists(HUNTER)", &dict).unwrap();
    assert!(ev(&p, &no_binds(), &s)); // no roles bound at all
    s.entities.get_mut("hunter").unwrap().dead_action = Some(1); // DESTROY stamps dead_action…
    s.by_name.remove("HUNTER"); // …and releases the name (§5.1)
    assert!(!ev(&p, &no_binds(), &s));
    assert!(ev(&parse_predicate("NOT exists(HUNTER)", &dict).unwrap(), &no_binds(), &s));
}

#[test]
fn name_form_is_exact_and_never_matches_the_dead_or_the_absent() {
    let dict = core_dictionary();
    let mut s = world();
    assert!(!ev(&parse_predicate("exists(hunter)", &dict).unwrap(), &no_binds(), &s)); // case-exact, like by_name
    assert!(!ev(&parse_predicate("exists(THE_SEALED_DOOR)", &dict).unwrap(), &no_binds(), &s));
    let mut dead = ent("ghost", "idea", "GHOST");
    dead.dead_action = Some(3);
    add_entity(&mut s, dead).unwrap(); // a dead entity never enters by_name
    assert!(!ev(&parse_predicate("exists(GHOST)", &dict).unwrap(), &no_binds(), &s));
}

#[test]
fn query_form_aspect_threshold() {
    let dict = core_dictionary();
    let s = world();
    assert!(ev(&parse_predicate("exists(aspect:hunter>=1)", &dict).unwrap(), &no_binds(), &s));
    assert!(!ev(&parse_predicate("exists(aspect:hunter >= 3)", &dict).unwrap(), &no_binds(), &s)); // spaces tighten to >=3
    assert!(!ev(&parse_predicate("exists(aspect:secret)", &dict).unwrap(), &no_binds(), &s));
    // bare term = level >= 1
}

#[test]
fn query_form_kind_aspect_conjunction() {
    let dict = core_dictionary();
    let s = world();
    assert!(ev(
        &parse_predicate("exists(kind:rite aspect:ritual>=1)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(kind:rite aspect:ritual>=2)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(kind:character aspect:ritual>=1)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
}

#[test]
fn query_form_negation_term() {
    let dict = core_dictionary();
    let s = world();
    assert!(!ev(
        &parse_predicate("exists(kind:rite -aspect:ritual)", &dict).unwrap(),
        &no_binds(),
        &s
    )); // the only rite has ritual
    assert!(ev(&parse_predicate("exists(-aspect:ritual)", &dict).unwrap(), &no_binds(), &s));
    // the hunter lacks it
}

#[test]
fn query_form_state_terms_read_the_state_attr_namespace() {
    let dict = core_dictionary();
    let mut s = world();
    assert!(!ev(&parse_predicate("exists(state:sealed)", &dict).unwrap(), &no_binds(), &s));
    assert!(ev(&parse_predicate("exists(-state:sealed)", &dict).unwrap(), &no_binds(), &s)); // nobody is sealed
    s.entities
        .get_mut("rite")
        .unwrap()
        .attrs
        .insert("state.sealed".to_string(), AttrValue::Bool(true));
    assert!(ev(&parse_predicate("exists(state:sealed)", &dict).unwrap(), &no_binds(), &s));
    assert!(ev(
        &parse_predicate("exists(kind:rite state:sealed)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(kind:character state:sealed)", &dict).unwrap(),
        &no_binds(),
        &s
    )); // hunter unsealed
    assert!(ev(&parse_predicate("exists(-state:sealed)", &dict).unwrap(), &no_binds(), &s));
    // …but still matches the negation
}

#[test]
fn not_binds_tighter_than_and_around_exists() {
    let dict = core_dictionary();
    // (§6.3 precedence) — HUNTER is
    // alive and nothing has ritual>=5: the spec grouping
    // (NOT exists(HUNTER)) AND exists(aspect:ritual>=5) is false — the wrong
    // reading NOT (exists AND exists) would be true here
    let s = world();
    assert!(!ev(
        &parse_predicate("NOT exists(HUNTER) AND exists(aspect:ritual>=5)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    // flip an operand and it turns true, proving both sides are read as written
    assert!(ev(
        &parse_predicate("NOT exists(GHOST) AND exists(aspect:ritual>=1)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
}

#[test]
fn scope_finds_a_card_bound_to_no_role_sitting_in_another_place_than_the_actors() {
    let dict = core_dictionary();
    // Actors = the rite at the house;
    // the hunter is in the villa and in no role
    let s = world();
    let p = parse_predicate("exists(aspect:hunter>=1) OR exists(HUNTER)", &dict).unwrap();
    assert!(ev(&p, &bindings(&[(Role::Actors, &["rite"])]), &s));
}

#[test]
fn disambiguation_a_starter_word_without_a_colon_is_a_name_not_a_query() {
    let dict = core_dictionary();
    let s = world();
    assert!(!ev(&parse_predicate("exists(state)", &dict).unwrap(), &no_binds(), &s)); // no card named 'state'
    assert!(!ev(&parse_predicate("exists(kind)", &dict).unwrap(), &no_binds(), &s));
}

#[test]
fn composability_and_parse_errors() {
    let dict = core_dictionary();
    let s = world();
    let yes = parse_predicate("exists(HUNTER) AND NOT exists(aspect:ritual>=5)", &dict).unwrap();
    assert!(ev(&yes, &no_binds(), &s));
    assert!(!ev(
        &parse_predicate("NOT exists(HUNTER) OR exists(aspect:ritual>=5)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    assert_err_msg(parse_predicate("exists()", &dict), "needs a name or a slot query");
    assert_err_msg(parse_predicate("exists(TWO WORDS)", &dict), "takes one name");
}

// ---------- the name: query term and count(<query>), §4.5
// ---------------------------------------------------------------------------

/// A two-copy SUSPICION pool plus an
/// unrelated card.
fn pool_world() -> State {
    let mut s = new_state(0);
    add_entity(
        &mut s,
        stackable(with_aspects(ent("sus1", "state", "SUSPICION"), &[("weight", 1)])),
    )
    .unwrap();
    add_entity(
        &mut s,
        stackable(with_attrs(
            with_aspects(ent("sus2", "state", "SUSPICION"), &[("weight", 1)]),
            &[("container", AttrValue::Str("sus1".into()))],
        )),
    )
    .unwrap();
    add_entity(&mut s, with_aspects(ent("rite", "rite", "RITE"), &[("ritual", 1)])).unwrap();
    s
}

#[test]
fn name_matches_exactly_case_exact_like_exists() {
    let dict = core_dictionary();
    let s = pool_world();
    let sus1 = s.entities.get("sus1").unwrap();
    let rite = s.entities.get("rite").unwrap();
    assert!(match_query(sus1, &parse_query("name:SUSPICION", &dict).unwrap()));
    assert!(!match_query(rite, &parse_query("name:SUSPICION", &dict).unwrap()));
    assert!(!match_query(sus1, &parse_query("name:suspicion", &dict).unwrap())); // case-exact
    assert!(match_query(rite, &parse_query("-name:SUSPICION", &dict).unwrap()));
    // negation
}

#[test]
fn name_composes_with_other_terms_and_negation_composes_at_the_term_level() {
    let dict = core_dictionary();
    let s = pool_world();
    assert!(ev(
        &parse_predicate("exists(kind:state name:SUSPICION)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(kind:rite name:SUSPICION)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    assert!(ev(&parse_predicate("exists(-name:SUSPICION)", &dict).unwrap(), &no_binds(), &s)); // the rite
    assert!(!ev(
        &parse_predicate("exists(kind:state -name:SUSPICION)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    // both copies match
}

#[test]
fn count_name_x_counts_every_living_copy_count_neg_name_x_counts_the_rest() {
    let dict = core_dictionary();
    let s = pool_world();
    assert!(ev(&parse_predicate("count(name:SUSPICION) = 2", &dict).unwrap(), &no_binds(), &s));
    assert!(!ev(&parse_predicate("count(name:SUSPICION) >= 3", &dict).unwrap(), &no_binds(), &s));
    assert!(ev(&parse_predicate("count(-name:SUSPICION) = 1", &dict).unwrap(), &no_binds(), &s)); // the rite only
    assert!(ev(&parse_predicate("count(name:RITE) = 1", &dict).unwrap(), &no_binds(), &s));
}

#[test]
fn count_query_composes_full_slot_queries_count_role_is_unchanged() {
    let dict = core_dictionary();
    let s = pool_world();
    assert!(ev(
        &parse_predicate("count(kind:state aspect:weight>=1) = 2", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    assert!(ev(
        &parse_predicate("count(actors) = 1", &dict).unwrap(),
        &bindings(&[(Role::Actors, &["sus1"])]),
        &s
    )); // role form untouched
}

#[test]
fn exists_and_count_inner_queries_take_alternation() {
    let dict = core_dictionary();
    // §4.5's legal homes include the §4.5 inner queries — same parser,
    // same DNF behavior
    let mut s = new_state(0);
    add_entity(&mut s, with_aspects(ent("a", "item", "A"), &[("ability", 1)])).unwrap();
    add_entity(&mut s, with_aspects(ent("j", "item", "J"), &[("job", 1)])).unwrap();
    add_entity(&mut s, with_aspects(ent("r", "item", "R"), &[("job", 1), ("ritual", 1)])).unwrap();
    // exists reads the DNF: ability OR job — a matches
    assert!(ev(
        &parse_predicate("exists(aspect:ability | aspect:job)", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    // count reads it too: a (ability), j (job), r (job) = 3
    assert!(ev(
        &parse_predicate("count(aspect:ability | aspect:job) = 3", &dict).unwrap(),
        &no_binds(),
        &s
    ));
    // no card has ability AND ritual, and nothing levitates → no group matches
    assert!(!ev(
        &parse_predicate("exists(aspect:ability aspect:ritual | aspect:levitation)", &dict)
            .unwrap(),
        &no_binds(),
        &s
    ));
    // ability, or job AND ritual: a and r — the lone job card stays out
    assert!(ev(
        &parse_predicate("count(aspect:ability | aspect:job aspect:ritual) = 2", &dict).unwrap(),
        &no_binds(),
        &s
    ));
}

#[test]
fn count_with_a_non_role_non_query_argument_is_a_parse_shaped_role() {
    let dict = core_dictionary();
    // Parseable (it looks like a
    // role), evaluates to 0 — which is why compile_content rejects it (see
    // Lint tests)
    let s = pool_world();
    assert!(ev(&parse_predicate("count(DREAD) = 0", &dict).unwrap(), &no_binds(), &s));
}

#[test]
fn kind_terms_reject_empty_and_unknown_kinds_at_parse() {
    let dict = core_dictionary();
    // §4.5/§6.1: a kind is one of the dictionary's §4.1 kinds — an empty or
    // unknown kind is a parse error (name:/aspect:/state: reject empties
    // too). The suggestion list is the dictionary's insertion order —
    // built-ins first, then script declarations (§4.1) — so the
    // built-in pin below is byte-exact and grows with `kinds:`.
    assert_err_msg(parse_query("kind:", &dict), "bad kind term 'kind:' \\(expected kind:<KIND>\\)");
    assert_err_msg(
        parse_query("kind:txet", &dict),
        "bad kind term 'kind:txet' \\(unknown kind 'txet' — expected one of character, place, item, text, idea, state, rite, resource\\)",
    );
    // the embedded forms ride the same parser
    assert_err_msg(parse_predicate("exists(kind:txet)", &dict), "bad kind term");
    assert_err_msg(parse_predicate("count(-kind:txet) >= 1", &dict), "bad kind term");
    assert!(parse_query("kind:text", &dict).is_ok());
    assert!(parse_query("-kind:rite aspect:ritual>=1", &dict).is_ok());
    // a declared kind joins the list in declaration order, and its term parses
    let mut d = core_dictionary();
    d.register_kind("omen").unwrap();
    assert_err_msg(
        parse_query("kind:txet", &d),
        "bad kind term 'kind:txet' \\(unknown kind 'txet' — expected one of character, place, item, text, idea, state, rite, resource, omen\\)",
    );
    assert!(parse_query("kind:omen", &d).is_ok());
}

#[test]
fn a_comparison_inside_a_name_term_is_a_load_error_it_would_silently_bake_the_name() {
    let dict = core_dictionary();
    // (§4.5) — the exists()/count()
    // glue-tightening turns 'SUSPICION >= 1' into the single token
    // 'SUSPICION>=1'; as a name that matches nothing (silent 0)
    assert_err_msg(parse_query("name:SUSPICION>=1", &dict), "bad name term");
    assert_err_msg(parse_predicate("count(name:SUSPICION >= 1) >= 1", &dict), "bad name term");
    assert_err_msg(parse_predicate("exists(name:SUSPICION >= 1)", &dict), "bad name term");
    // total() never accepted the query form — now it says so instead of
    // failing with a cryptic `expected ','`
    assert_err_msg(
        parse_predicate("total(name:SUSPICION, aspect:weight) >= 1", &dict),
        r"total\(\) pools a bound role",
    );
}

// ---------- the predicate sum scopes, §6.3 ---------------------------------
// `total(bound, aspect:k)` sums the WINDOW's bindings across all its roles
// (per-binding: a card bound to two roles counts twice); `total(table,
// aspect:k)` sums every ALIVE card on the world projection. Missing aspects
// read 0 (total()'s unwrap_or(0)), dead-stamped ids drop like every role
// read.

/// `{ object: EMBER (dread 1), with: LAMP (dread 1) }` over a world holding
/// both plus a bare character.
fn sum_scope_state() -> State {
    let mut s = new_state(0);
    add_entity(&mut s, ent("ana", "character", "ANA")).unwrap();
    add_entity(&mut s, with_aspects(ent("ember", "idea", "EMBER"), &[("dread", 1)])).unwrap();
    add_entity(&mut s, with_aspects(ent("lamp", "idea", "LAMP"), &[("dread", 1)])).unwrap();
    add_entity(&mut s, with_aspects(ent("coin", "item", "COIN"), &[("funds", 3)])).unwrap();
    s
}

#[test]
fn total_bound_sums_across_two_roles() {
    let dict = core_dictionary();
    let s = sum_scope_state();
    let p = parse_predicate("total(bound, aspect:dread) >= 2", &dict).unwrap();
    // object EMBER (1) + with LAMP (1) — the window's sum crosses
    assert!(ev(&p, &bindings(&[(Role::Object, &["ember"]), (Role::With, &["lamp"])]), &s));
    // either card alone sums to 1 — the gate fails
    assert!(!ev(&p, &bindings(&[(Role::Object, &["ember"])]), &s));
    assert!(!ev(&p, &bindings(&[(Role::With, &["lamp"])]), &s));
}

#[test]
fn total_bound_counts_a_card_bound_to_two_roles_twice() {
    let dict = core_dictionary();
    let s = sum_scope_state();
    // the engine's per-binding bookkeeping: validate_slots forbids a card
    // twice in ONE role but nothing forbids two roles, so EMBER in object
    // and with contributes its dread 1 PER BINDING
    let p = parse_predicate("total(bound, aspect:dread) >= 2", &dict).unwrap();
    let both_roles = bindings(&[(Role::Object, &["ember"]), (Role::With, &["ember"])]);
    assert!(ev(&p, &both_roles, &s));
    let exact = parse_predicate("total(bound, aspect:dread) = 2", &dict).unwrap();
    assert!(ev(&exact, &both_roles, &s));
}

#[test]
fn total_bound_reads_zero_when_nothing_is_bound() {
    let dict = core_dictionary();
    let s = sum_scope_state();
    let p = parse_predicate("total(bound, aspect:dread) = 0", &dict).unwrap();
    assert!(ev(&p, &no_binds(), &s));
    let gte = parse_predicate("total(bound, aspect:dread) >= 1", &dict).unwrap();
    assert!(!ev(&gte, &no_binds(), &s));
    // characters bound but aspect-less: still 0 (the missing reads 0)
    assert!(ev(&p, &bindings(&[(Role::Actors, &["ana"])]), &s));
}

#[test]
fn total_bound_drops_dead_bindings_and_missing_aspects_read_zero() {
    let dict = core_dictionary();
    let mut s = sum_scope_state();
    let _ = s.entities.get_mut("ember").map(|e| e.dead_action = Some(3));
    // the dead EMBER drops; only LAMP's 1 remains
    let p = parse_predicate("total(bound, aspect:dread) = 1", &dict).unwrap();
    assert!(ev(&p, &bindings(&[(Role::Object, &["ember"]), (Role::With, &["lamp"])]), &s));
    // a role nobody bears the aspect on reads 0, not false
    let zero = parse_predicate("total(bound, aspect:funds) = 0", &dict).unwrap();
    assert!(ev(&zero, &bindings(&[(Role::Object, &["ember"])]), &s));
}

#[test]
fn total_table_sums_the_living_world_and_grows_after_a_mint() {
    let dict = core_dictionary();
    let mut s = sum_scope_state();
    let p3 = parse_predicate("total(table, aspect:funds) >= 3", &dict).unwrap();
    // COIN alone carries funds 3 — every other card reads its missing as 0
    assert!(ev(&p3, &no_binds(), &s));
    let exact = parse_predicate("total(table, aspect:funds) = 3", &dict).unwrap();
    assert!(ev(&exact, &no_binds(), &s));
    // a mint lands (the SPAWN face): the world sum grows
    add_entity(&mut s, with_aspects(ent("purse", "resource", "PURSE"), &[("funds", 2)])).unwrap();
    let p5 = parse_predicate("total(table, aspect:funds) >= 5", &dict).unwrap();
    assert!(ev(&p5, &no_binds(), &s));
    // a dead card drops out of the table sum
    let _ = s.entities.get_mut("purse").map(|e| e.dead_action = Some(1));
    assert!(ev(&exact, &no_binds(), &s));
}

#[test]
fn the_sum_scopes_render_in_the_authored_spelling() {
    // the explain-face round-trip: the two scope forms render exactly as
    // authored
    let dict = core_dictionary();
    let p = parse_predicate("total(bound, aspect:dread) >= 2", &dict).unwrap();
    assert_eq!(chorus::session::explain::pred_text(&p), "total(bound, aspect:dread) >= 2");
    let p = parse_predicate("total(table, aspect:dread) >= 2", &dict).unwrap();
    assert_eq!(chorus::session::explain::pred_text(&p), "total(table, aspect:dread) >= 2");
}

#[test]
fn total_bound_in_a_slot_trigger_if_is_a_load_error_and_loads_in_an_on_complete_if() {
    // the compile-level split (§6.3/§6.6): a slot trigger's event bindings
    // are empty, so the window scope would silently read 0 there — a load
    // error, the same always-0 rule the role forms wear; a completion
    // trigger's window bindings are live, so the identical IF loads.
    let dict = core_dictionary();
    let slot = "
script: Split Rig
player: ANA
start: 0:0

entities:
  ANA is character at HOUSE
  HOUSE is place

triggers:
  tick at hour 1 -> IF total(bound, aspect:dread) >= 1 -> (NARRATE \"ding\")
";
    assert_err_msg(
        compile_script(slot, &dict).map(|_| ()),
        r"trigger tick: IF operand total\(bound, aspect:dread\) reads the window's bindings — trigger bindings are empty, it would always be 0",
    );
    let complete = "
script: Split Rig
player: ANA
start: 0:0

entities:
  ANA is character at HOUSE
  HOUSE is place
  on complete -> (IF total(bound, aspect:dread) >= 1 -> (NARRATE \"ding\"))
";
    let compiled = compile_script(complete, &dict).unwrap();
    assert_eq!(compiled.content.triggers.len(), 1);
    assert!(matches!(
        compiled.content.triggers[0],
        chorus::content::CompiledTrigger::Complete { .. }
    ));
}
