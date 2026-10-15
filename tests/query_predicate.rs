//! Port of test/query-predicate.test.ts (W1, plan §5.2.1) — query
//! mini-language (§4.5) and condition predicates (§6.3).
//!
//! Mapping (plan §5.2): one `#[test]` per TS `test`, TS name kept
//! (snake_case); fresh State/Dictionary per test; `toThrow(/re/)` →
//! `assert_err_msg(res, "re")`.
//!
//! Ownership adaptation (plan §2.6.2): TS `PredCtx.b` holds ENTITY objects,
//! so its tests may pass standalone entities; the Rust bindings hold entity
//! IDS read back through `state.entities` — every bound card therefore lives
//! in the test State under the same id, which is exactly the engine's own
//! invariant (bindings always resolve into the live state).

mod common;

use indexmap::IndexMap;

use chorus::core::predicate::{eval_pred, parse_predicate, Pred, PredCtx, RoleBindings};
use chorus::core::query::{match_query, parse_query};
use chorus::core::roles::Role;
use chorus::core::types::{
    add_entity, new_state, AttrValue, Attrs, Entity, State, Aspects,
};
use chorus::core::query::aspects_from_pairs;

use common::harness::assert_err_msg;

/// ts: query-predicate.test.ts:7-9 — `{ aspects: {}, attrs: {},
/// bornAction: null, deadAction: null, stackable: false, ...partial }`.
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
/// keys go through aspects_from_pairs (query.rs), the TS literal's own
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

/// ts: `(b: PredCtx['b']): PredCtx => ({ b, state })` — strict/roll unset
/// (when:-style evaluation).
fn ctx<'a>(b: &'a RoleBindings, state: &'a State) -> PredCtx<'a> {
    PredCtx {
        b,
        state,
        strict: false,
        roll: None,
    }
}

/// `ctx({ actors: [A, B] })` — bindings under the plan §2.6.2 id-handle
/// shape (Role → bound ids, insertion order kept).
fn bindings(pairs: &[(Role, &[&str])]) -> RoleBindings {
    pairs
        .iter()
        .map(|(role, ids)| (*role, ids.iter().map(|s| (*s).to_string()).collect()))
        .collect()
}

/// `evalPred(p, ctx(...))` — TS returns a bare boolean.
fn ev(p: &Pred, b: &RoleBindings, s: &State) -> bool {
    eval_pred(p, &ctx(b, s)).unwrap()
}

/// `ctx({})` — no roles bound at all.
fn no_binds() -> RoleBindings {
    RoleBindings::new()
}
// ---------- slot queries, §4.5 (ts: query-predicate.test.ts:11-32) ----------

#[test]
fn a_matching_card_passes_every_term() {
    // ts: query-predicate.test.ts:14-17
    let q = parse_query("kind:text aspect:secret>=3 aspect:language.latin>=2 state:!sealed -aspect:winter").unwrap();
    let book = with_aspects(ent("b", "text", "B"), &[("secret", 3), ("language.latin", 2)]);
    assert!(match_query(&book, &q));
}

#[test]
fn each_term_can_fail_the_card() {
    // ts: query-predicate.test.ts:19-25
    let q = parse_query("kind:text aspect:secret>=3 aspect:language.latin>=2 state:!sealed -aspect:winter").unwrap();
    assert!(!match_query(&ent("x", "item", "X"), &q)); // kind
    assert!(!match_query(
        &with_aspects(ent("x", "text", "X"), &[("secret", 2)]),
        &q
    )); // level
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
    // ts: query-predicate.test.ts:27-31
    let q1 = parse_query("aspect:lantern").unwrap();
    assert!(match_query(
        &with_aspects(ent("a", "character", "A"), &[("lantern", 1)]),
        &q1
    ));
    assert!(!match_query(&ent("a", "character", "A"), &q1));
}

// ---------- condition predicates, §6.3 (ts: query-predicate.test.ts:34-83) ----------

/// ts: query-predicate.test.ts:35-39 — the describe-level state.
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
    // ts: query-predicate.test.ts:44-48
    let state = cond_state();
    let p = parse_predicate("count(actors) = 2").unwrap();
    assert!(ev(&p, &bindings(&[(Role::Actors, &["ana", "bertha"])]), &state));
    assert!(!ev(&p, &bindings(&[(Role::Actors, &["ana"])]), &state));
}

#[test]
fn rel_parent_child_actors_pairwise_existence() {
    // ts: query-predicate.test.ts:50-55 — symmetric, order-free
    let state = cond_state();
    let p = parse_predicate("rel(parent_child, actors)").unwrap();
    assert!(ev(&p, &bindings(&[(Role::Actors, &["ana", "bertha"])]), &state));
    assert!(ev(&p, &bindings(&[(Role::Actors, &["bertha", "ana"])]), &state));
    assert!(!ev(&p, &bindings(&[(Role::Actors, &["ana", "rite"])]), &state));
}

#[test]
fn every_any_quantifiers() {
    // ts: query-predicate.test.ts:57-64
    let state = cond_state();
    let every = parse_predicate("every(actors, aspect:occult >= 2)").unwrap();
    let any = parse_predicate("any(actors, aspect:occult >= 2)").unwrap();
    assert!(ev(&every, &bindings(&[(Role::Actors, &["ana"])]), &state));
    assert!(!ev(&every, &bindings(&[(Role::Actors, &["ana", "bertha"])]), &state));
    assert!(ev(&any, &bindings(&[(Role::Actors, &["ana", "bertha"])]), &state));
    assert!(!ev(&any, &bindings(&[(Role::Actors, &["bertha"])]), &state));
}

#[test]
fn aggregate_vs_path_operand() {
    // ts: query-predicate.test.ts:66-70 — total(actors, …) < object.aspect.demand
    let state = cond_state();
    let p = parse_predicate("total(actors, aspect:lantern) < object.aspect.demand").unwrap();
    assert!(ev(
        &p,
        &bindings(&[(Role::Actors, &["ana"]), (Role::Object, &["rite"])]),
        &state
    )); // 1 < 3
    assert!(ev(
        &p,
        &bindings(&[(Role::Actors, &["ana", "bertha"]), (Role::Object, &["rite"])]),
        &state
    )); // 1+0 < 3
}

#[test]
fn not_binds_tighter_than_and() {
    // ts: query-predicate.test.ts:72-75 (§6.3)
    let state = cond_state();
    let p = parse_predicate("NOT 1 = 2 AND 2 = 2").unwrap();
    assert!(ev(&p, &no_binds(), &state));
}

#[test]
fn group_gate_with_not_any_the_magick_gate_shape() {
    // ts: query-predicate.test.ts:77-82. TS passes the magick card as a
    // STANDALONE entity in the bindings; the Rust id-handle model (plan
    // §2.6.2) needs it in the state — same id, semantics unchanged.
    let mut state = cond_state();
    add_entity(
        &mut state,
        with_aspects(ent("m", "text", "M"), &[("magick", 1)]),
    )
    .unwrap();
    let p = parse_predicate("object.aspect.magick >= 1 AND NOT any(actors, aspect:occult >= 2)").unwrap();
    assert!(ev(
        &p,
        &bindings(&[(Role::Actors, &["bertha"]), (Role::Object, &["m"])]),
        &state
    )); // nobody knows
    assert!(!ev(
        &p,
        &bindings(&[(Role::Actors, &["ana", "bertha"]), (Role::Object, &["m"])]),
        &state
    )); // ANA knows
}

// ---------- exists(...), §6.3 CS extantreqs (ts: query-predicate.test.ts:85-177) ----------

/// ts: query-predicate.test.ts:86-92 — the hunter prowls the villa; nothing
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
    // ts: query-predicate.test.ts:95-103
    let mut s = world();
    let p = parse_predicate("exists(HUNTER)").unwrap();
    assert!(ev(&p, &no_binds(), &s)); // no roles bound at all
    s.entities.get_mut("hunter").unwrap().dead_action = Some(1); // DESTROY stamps deadAction…
    s.by_name.remove("HUNTER"); // …and releases the name (§9.14)
    assert!(!ev(&p, &no_binds(), &s));
    assert!(ev(&parse_predicate("NOT exists(HUNTER)").unwrap(), &no_binds(), &s));
}

#[test]
fn name_form_is_exact_and_never_matches_the_dead_or_the_absent() {
    // ts: query-predicate.test.ts:105-113
    let mut s = world();
    assert!(!ev(
        &parse_predicate("exists(hunter)").unwrap(),
        &no_binds(),
        &s
    )); // case-exact, like byName
    assert!(!ev(
        &parse_predicate("exists(THE_SEALED_DOOR)").unwrap(),
        &no_binds(),
        &s
    ));
    let mut dead = ent("ghost", "idea", "GHOST");
    dead.dead_action = Some(3);
    add_entity(&mut s, dead).unwrap(); // a dead entity never enters byName
    assert!(!ev(&parse_predicate("exists(GHOST)").unwrap(), &no_binds(), &s));
}

#[test]
fn query_form_aspect_threshold() {
    // ts: query-predicate.test.ts:115-120
    let s = world();
    assert!(ev(
        &parse_predicate("exists(aspect:hunter>=1)").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(aspect:hunter >= 3)").unwrap(),
        &no_binds(),
        &s
    )); // spaces tighten to >=3
    assert!(!ev(
        &parse_predicate("exists(aspect:secret)").unwrap(),
        &no_binds(),
        &s
    )); // bare term = level >= 1
}

#[test]
fn query_form_kind_aspect_conjunction() {
    // ts: query-predicate.test.ts:122-127
    let s = world();
    assert!(ev(
        &parse_predicate("exists(kind:rite aspect:ritual>=1)").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(kind:rite aspect:ritual>=2)").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(kind:character aspect:ritual>=1)").unwrap(),
        &no_binds(),
        &s
    ));
}

#[test]
fn query_form_negation_term() {
    // ts: query-predicate.test.ts:129-133
    let s = world();
    assert!(!ev(
        &parse_predicate("exists(kind:rite -aspect:ritual)").unwrap(),
        &no_binds(),
        &s
    )); // the only rite has ritual
    assert!(ev(
        &parse_predicate("exists(-aspect:ritual)").unwrap(),
        &no_binds(),
        &s
    )); // the hunter lacks it
}

#[test]
fn query_form_state_terms_read_the_state_attr_namespace() {
    // ts: query-predicate.test.ts:135-144
    let mut s = world();
    assert!(!ev(&parse_predicate("exists(state:sealed)").unwrap(), &no_binds(), &s));
    assert!(ev(
        &parse_predicate("exists(-state:sealed)").unwrap(),
        &no_binds(),
        &s
    )); // nobody is sealed
    s.entities
        .get_mut("rite")
        .unwrap()
        .attrs
        .insert("state.sealed".to_string(), AttrValue::Bool(true));
    assert!(ev(&parse_predicate("exists(state:sealed)").unwrap(), &no_binds(), &s));
    assert!(ev(
        &parse_predicate("exists(kind:rite state:sealed)").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(kind:character state:sealed)").unwrap(),
        &no_binds(),
        &s
    )); // hunter unsealed
    assert!(ev(
        &parse_predicate("exists(-state:sealed)").unwrap(),
        &no_binds(),
        &s
    )); // …but still matches the negation
}

#[test]
fn not_binds_tighter_than_and_around_exists() {
    // ts: query-predicate.test.ts:146-154 (§6.3 precedence) — HUNTER is
    // alive and nothing has ritual>=5: the spec grouping
    // (NOT exists(HUNTER)) AND exists(aspect:ritual>=5) is false — the wrong
    // reading NOT (exists AND exists) would be true here
    let s = world();
    assert!(!ev(
        &parse_predicate("NOT exists(HUNTER) AND exists(aspect:ritual>=5)").unwrap(),
        &no_binds(),
        &s
    ));
    // flip an operand and it turns true, proving both sides are read as written
    assert!(ev(
        &parse_predicate("NOT exists(GHOST) AND exists(aspect:ritual>=1)").unwrap(),
        &no_binds(),
        &s
    ));
}

#[test]
fn scope_finds_a_card_bound_to_no_role_sitting_in_another_place_than_the_actors() {
    // ts: query-predicate.test.ts:156-161 — actors = the rite at the house;
    // the hunter is in the villa and in no role
    let s = world();
    let p = parse_predicate("exists(aspect:hunter>=1) OR exists(HUNTER)").unwrap();
    assert!(ev(&p, &bindings(&[(Role::Actors, &["rite"])]), &s));
}

#[test]
fn disambiguation_a_starter_word_without_a_colon_is_a_name_not_a_query() {
    // ts: query-predicate.test.ts:163-167
    let s = world();
    assert!(!ev(&parse_predicate("exists(state)").unwrap(), &no_binds(), &s)); // no card named 'state'
    assert!(!ev(&parse_predicate("exists(kind)").unwrap(), &no_binds(), &s));
}

#[test]
fn composability_and_parse_errors() {
    // ts: query-predicate.test.ts:169-176
    let s = world();
    let yes = parse_predicate("exists(HUNTER) AND NOT exists(aspect:ritual>=5)").unwrap();
    assert!(ev(&yes, &no_binds(), &s));
    assert!(!ev(
        &parse_predicate("NOT exists(HUNTER) OR exists(aspect:ritual>=5)").unwrap(),
        &no_binds(),
        &s
    ));
    assert_err_msg(parse_predicate("exists()"), "needs a name or a slot query");
    assert_err_msg(parse_predicate("exists(TWO WORDS)"), "takes one name");
}

// ---------- the name: query term and count(<query>), §9.38
// ---------- (ts: query-predicate.test.ts:179-238) ----------

/// ts: query-predicate.test.ts:180-187 — a two-copy SUSPICION pool plus an
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
    // ts: query-predicate.test.ts:190-196
    let s = pool_world();
    let sus1 = s.entities.get("sus1").unwrap();
    let rite = s.entities.get("rite").unwrap();
    assert!(match_query(sus1, &parse_query("name:SUSPICION").unwrap()));
    assert!(!match_query(rite, &parse_query("name:SUSPICION").unwrap()));
    assert!(!match_query(sus1, &parse_query("name:suspicion").unwrap())); // case-exact
    assert!(match_query(rite, &parse_query("-name:SUSPICION").unwrap())); // negation
}

#[test]
fn name_composes_with_other_terms_and_negation_composes_at_the_term_level() {
    // ts: query-predicate.test.ts:198-204
    let s = pool_world();
    assert!(ev(
        &parse_predicate("exists(kind:state name:SUSPICION)").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("exists(kind:rite name:SUSPICION)").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(ev(
        &parse_predicate("exists(-name:SUSPICION)").unwrap(),
        &no_binds(),
        &s
    )); // the rite
    assert!(!ev(
        &parse_predicate("exists(kind:state -name:SUSPICION)").unwrap(),
        &no_binds(),
        &s
    )); // both copies match
}

#[test]
fn count_name_x_counts_every_living_copy_count_neg_name_x_counts_the_rest() {
    // ts: query-predicate.test.ts:206-212
    let s = pool_world();
    assert!(ev(
        &parse_predicate("count(name:SUSPICION) = 2").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(!ev(
        &parse_predicate("count(name:SUSPICION) >= 3").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(ev(
        &parse_predicate("count(-name:SUSPICION) = 1").unwrap(),
        &no_binds(),
        &s
    )); // the rite only
    assert!(ev(
        &parse_predicate("count(name:RITE) = 1").unwrap(),
        &no_binds(),
        &s
    ));
}

#[test]
fn count_query_composes_full_slot_queries_count_role_is_unchanged() {
    // ts: query-predicate.test.ts:214-219
    let s = pool_world();
    assert!(ev(
        &parse_predicate("count(kind:state aspect:weight>=1) = 2").unwrap(),
        &no_binds(),
        &s
    ));
    assert!(ev(
        &parse_predicate("count(actors) = 1").unwrap(),
        &bindings(&[(Role::Actors, &["sus1"])]),
        &s
    )); // role form untouched
}

#[test]
fn count_with_a_non_role_non_query_argument_is_a_parse_shaped_role() {
    // ts: query-predicate.test.ts:221-226 — parseable (it looks like a
    // role), evaluates to 0 — which is why compileContent rejects it (see
    // test/stackable.test.ts lint tests)
    let s = pool_world();
    assert!(ev(
        &parse_predicate("count(DREAD) = 0").unwrap(),
        &no_binds(),
        &s
    ));
}

#[test]
fn a_comparison_inside_a_name_term_is_a_load_error_it_would_silently_bake_the_name() {
    // ts: query-predicate.test.ts:228-237 (§9.38) — the exists()/count()
    // glue-tightening turns 'SUSPICION >= 1' into the single token
    // 'SUSPICION>=1'; as a name that matches nothing (silent 0)
    assert_err_msg(parse_query("name:SUSPICION>=1"), "bad name term");
    assert_err_msg(
        parse_predicate("count(name:SUSPICION >= 1) >= 1"),
        "bad name term",
    );
    assert_err_msg(
        parse_predicate("exists(name:SUSPICION >= 1)"),
        "bad name term",
    );
    // total() never accepted the query form — now it says so instead of
    // failing with a cryptic `expected ','`
    assert_err_msg(
        parse_predicate("total(name:SUSPICION, aspect:weight) >= 1"),
        r"total\(\) pools a bound role",
    );
}
