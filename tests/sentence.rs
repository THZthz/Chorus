//! Sentence spec-conformance tests —
//! every §5.2 worked example parses, canonicalizes, and round-trips. Plus
//! actor-order equality (§9.6), sugar canonicalization, reserved words
//! (§9.14).
//!
//! Conventions: one `#[test]` per behavior; a FRESH Dictionary per test
//! (this choice matters for reserved-word-collision tests);
//! error assertions go through `assert_err_msg(res, "re")` with the pattern
//! copied verbatim.

mod common;

use chorus::core::dict::core_dictionary;
use chorus::core::parse::parse_sentence;
use chorus::core::text::cmp_utf16;

use common::harness::{assert_err_msg, assert_json_eq, canonical};

// ---------- spec §5.2 worked examples --------------------------------------

#[test]
fn solo_read_with_at_context_bare_ref_shorthand_becomes_spawn() {
    let dict = core_dictionary();
    assert_eq!(
        canonical("0:14 @ ANA AT HOUSE READ THE_FORGETTING -> CLUE_SECRET_HIDEOUT", &dict),
        "0:14 @ ANA AT HOUSE READ THE_FORGETTING -> SPAWN CLUE_SECRET_HIDEOUT"
    );
}

#[test]
fn two_characters_talking_actor_order_is_not_semantic() {
    // (§9.6)
    let dict = core_dictionary();
    let a = canonical(
        "2:19 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT -> SPAWN PLAN_INFILTRATION AS #p1",
        &dict,
    );
    let b = canonical(
        "2:19 @ BERTHA, ANA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT -> SPAWN PLAN_INFILTRATION AS #p1",
        &dict,
    );
    assert_eq!(a, b);
    assert_eq!(
        a,
        "2:19 @ ANA, BERTHA AT DINING_HALL TALK ABOUT CLUE_SECRET_HIDEOUT -> SPAWN PLAN_INFILTRATION AS #p1"
    );
}

#[test]
fn two_clause_step() {
    let dict = core_dictionary();
    assert_eq!(
        canonical(
            "0:14 @ ANA TRAVEL_TO VILLA; ANA AT THE_STUDY READ THE_PALE_LADY -> CLUE_SECRET_HIDEOUT",
            &dict
        ),
        "0:14 @ ANA TRAVEL_TO VILLA; ANA AT THE_STUDY READ THE_PALE_LADY -> SPAWN CLUE_SECRET_HIDEOUT"
    );
}

#[test]
fn event_sentence() {
    // NARRATE's canonical form is paren-less;
    // SPAWN stays pretty …but the paren form parses equally
    let dict = core_dictionary();
    assert_eq!(
        canonical(
            "0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE(\"The lamps come on.\")",
            &dict
        ),
        "0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE \"The lamps come on.\""
    );
    assert_eq!(
        canonical(
            "0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE \"The lamps come on.\"",
            &dict
        ),
        "0:16 @ EVENT dusk_falls -> SPAWN DREAD, NARRATE \"The lamps come on.\""
    );
}

#[test]
fn engine_written_expiry_trigger_ids_round_trip() {
    // (§6.6) — the built-in expiry path writes
    // `expire:<NAME>` into the row; a segment that cannot lex as a name
    // prints quoted — and reassembles
    let dict = core_dictionary();
    assert_eq!(
        canonical("1:20 @ EVENT expire:THE_TONIC_GLASS -> DESTROY(#e1_1)", &dict),
        "1:20 @ EVENT expire:THE_TONIC_GLASS -> DESTROY(#e1_1)"
    );
    assert_eq!(
        canonical("1:20 @ EVENT expire:\"TONIC-GLASS\" -> DESTROY(#e1_1)", &dict),
        "1:20 @ EVENT expire:\"TONIC-GLASS\" -> DESTROY(#e1_1)"
    );
}

#[test]
fn rite_with_instruments_and_into() {
    let dict = core_dictionary();
    assert_eq!(
        canonical(
            "1:23 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE -> DESTROY(CHALICE), SPAWN MARK_OF_ASCENSION INTO HOUSE",
            &dict
        ),
        "1:23 @ ANA, MY_FOLLOWER AT HOUSE USE_ON RITE_OF_ASCENSION WITH CHALICE -> DESTROY(CHALICE), SPAWN MARK_OF_ASCENSION INTO HOUSE"
    );
}

// ---------- canonicalization -----------------------------------------------

#[test]
fn typed_ref_sugar_kind_name_drops_the_kind_in_canonical_form() {
    let dict = core_dictionary();
    assert_eq!(
        canonical("ANA READ text:THE_PALE_LADY AT THE_STUDY", &dict),
        "ANA AT THE_STUDY READ THE_PALE_LADY"
    );
}

#[test]
fn post_verbal_at_moves_pre_verbal() {
    // (§9.10)
    let dict = core_dictionary();
    assert_eq!(
        canonical("ANA READ THE_PALE_LADY AT THE_STUDY", &dict),
        "ANA AT THE_STUDY READ THE_PALE_LADY"
    );
}

#[test]
fn id_refs_and_quoted_names_round_trip() {
    let dict = core_dictionary();
    assert_eq!(
        canonical("#ana READ \"ODD NAME, WITH COMMA\"", &dict),
        "#ana READ \"ODD NAME, WITH COMMA\""
    );
}

#[test]
fn role_refs_in_outcomes_render_uppercase() {
    // (MOVE ACTORS INTO OBJECT)
    let dict = core_dictionary();
    assert_eq!(
        canonical("ANA TRAVEL_TO VILLA -> MOVE ACTORS INTO OBJECT", &dict),
        "ANA TRAVEL_TO VILLA -> MOVE(ACTORS, INTO OBJECT)"
    );
}

// ---------- SET deltas and aspect keys, §9.31 ------------------------------

#[test]
fn signed_values_parse_as_deltas_and_print_with_their_sign() {
    let dict = core_dictionary();
    assert_eq!(
        canonical("0:0 @ ANA WAIT -> SET(ANA, level, +1)", &dict),
        "0:0 @ ANA WAIT -> SET(ANA, level, +1)"
    );
    assert_eq!(
        canonical("0:0 @ ANA WAIT -> SET(ANA, level, -2)", &dict),
        "0:0 @ ANA WAIT -> SET(ANA, level, -2)"
    );
}

#[test]
fn aspect_keys_keep_their_prefix_dotted_attr_keys_print_bare() {
    // (§5.1 dottedName)
    let dict = core_dictionary();
    assert_eq!(
        canonical("0:0 @ ANA WAIT -> SET(ANA, aspect:dread, -1)", &dict),
        "0:0 @ ANA WAIT -> SET(ANA, aspect:dread, -1)"
    );
    assert_eq!(
        canonical("0:0 @ ANA WAIT -> SET(ANA, aspect:language.latin, 2)", &dict),
        "0:0 @ ANA WAIT -> SET(ANA, aspect:language.latin, 2)"
    );
    assert_eq!(
        canonical("0:0 @ ANA WAIT -> SET(ANA, state.sealed, true)", &dict),
        "0:0 @ ANA WAIT -> SET(ANA, state.sealed, true)"
    );
}

#[test]
fn deltas_are_integers_only_a_duration_after_a_sign_is_a_parse_error() {
    // (§9.31)
    let dict = core_dictionary();
    assert_err_msg(
        parse_sentence("0:0 @ ANA WAIT -> SET(ANA, funds, +2h)", &dict),
        "integers only",
    );
    assert_err_msg(
        parse_sentence("0:0 @ ANA WAIT -> SET(ANA, funds, +)", &dict),
        "expected an integer",
    );
}

// ---------- lexical discipline, §9.14 --------------------------------------

#[test]
fn reserved_words_cannot_be_names() {
    let dict = core_dictionary();
    assert_err_msg(parse_sentence("0:0 @ ABOUT READ X", &dict), "reserved");
    assert_err_msg(parse_sentence("ANA READ INTO", &dict), "reserved");
    // WITH can't start the verb slot either
    assert_err_msg(parse_sentence("ANA WITH VILLA", &dict), "registered verb");
}

#[test]
fn unregistered_verbs_are_rejected_at_the_actors_verb_boundary() {
    let dict = core_dictionary();
    assert_err_msg(parse_sentence("ANA FLY VILLA", &dict), "registered verb");
}

#[test]
fn names_colliding_with_verbs_must_be_quoted() {
    let dict = core_dictionary();
    assert_err_msg(parse_sentence("0:0 @ READ TALK ABOUT X", &dict), "reserved");
    assert_eq!(
        canonical("0:0 @ \"READ\" TALK ABOUT X", &dict),
        "0:0 @ \"READ\" TALK ABOUT X"
    );
}

// ---------- the core verb set is twelve, §4.3 v0.3 -------------------------

#[test]
fn study_and_mansus_register_and_are_reserved_like_every_verb_lexeme() {
    let dict = core_dictionary();
    // the JS default sort is UTF-16 code-unit
    // order — that comparator is the contract — so the Rust sort goes
    // through cmp_utf16
    let mut verbs: Vec<String> = dict.verbs.iter().cloned().collect();
    verbs.sort_by(|a, b| cmp_utf16(a, b));
    assert_json_eq(
        serde_json::json!(verbs),
        serde_json::json!([
            "ACQUIRE", "DREAM", "EXPLORE", "LOCATE_AT", "MANSUS", "READ",
            "STUDY", "TALK", "TRAVEL_TO", "USE_ON", "WAIT", "WORK",
        ]),
    );
    assert!(dict.is_verb("STUDY"));
    assert!(dict.is_verb("MANSUS"));
    assert_err_msg(parse_sentence("0:0 @ STUDY READ X", &dict), "reserved");
    assert_err_msg(
        parse_sentence("0:0 @ ANA WAIT -> SPAWN MANSUS", &dict),
        "reserved",
    );
}

#[test]
fn the_new_verbs_round_trip_like_the_old_ones() {
    // (§5.1); post-verbal AT canonicalizes
    // pre-verbal for the new verbs too (§9.10)
    let dict = core_dictionary();
    assert_eq!(
        canonical("0:14 @ ANA STUDY THE_MANIFEST", &dict),
        "0:14 @ ANA STUDY THE_MANIFEST"
    );
    assert_eq!(
        canonical("0:14 @ ANA STUDY THE_MANIFEST AT HOUSE", &dict),
        "0:14 @ ANA AT HOUSE STUDY THE_MANIFEST"
    );
    assert_eq!(
        canonical("0:14 @ ANA MANSUS THE_WHITE_DOOR", &dict),
        "0:14 @ ANA MANSUS THE_WHITE_DOOR"
    );
}
