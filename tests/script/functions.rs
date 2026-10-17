//! Functions (27 tests): parametric outcome macros, compile-time expanded
//! (spec §10). A `functions:` declaration expands at LOAD: every
//! call site becomes its function's body with parameters substituted (AST
//! splicing + string interpolation), BEFORE the trigger guards and the
//! endings check run — canonical equals expansion, so the engine and the log
//! never see a call. Recursion cycles, depth > 8, arity mismatches, op-name
//! collisions and undeclared `{token}` interpolation are load errors.
//!
//! This file is the compile→execute FULL-CHAIN expansion
//! verification that the direct subset (tests/fn_expand_direct.rs) could not
//! cover — the cycle/depth texts (frozen in tests/fn_expand_direct.rs) are
//! re-checked here
//! through the real compile path. Error-message patterns are copied CHARACTER FOR
//! CHARACTER; each test owns a fresh Dictionary with STEW registered
//! (per-test).

#[path = "../common/mod.rs"]
mod common;

use common::harness::assert_err_msg;
use common::settle::{act, done, refusal, settle_step};
use common::snap::snap;

use chorus::content::content::{
    compile_content, CompiledContent, ContentRaw, EndingDef, FunctionDef, PrototypeDef, RecipeDef,
    SlotDef, TriggerDef, VerbContent,
};
use chorus::content::script::{compile_script, CompiledScript};
use chorus::core::clock::Clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::fn_expand::{FnTable, FnTemplate};
use chorus::core::format::format_sentence;
use chorus::core::parse::{parse_function_body, parse_outcomes, parse_sentence};
use chorus::core::r#ref::Ref;
use chorus::core::sentence::{Arg, Outcome, Sentence, SentenceKind};
use chorus::core::types::{
    add_entity, clone_state, new_state, AspectVal, Aspects, AttrValue, Attrs, Entity, State,
};
use chorus::engine::engine::{ActionRecord, ExecResult, Step};
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use indexmap::IndexMap;

// ---------------------------------------------------------------- rig ------

/// The file-level dict registers STEW (the
/// rig's own verb lexeme; verbs are code-registered, §10).
fn stew_dict() -> Dictionary {
    let mut d = core_dictionary();
    d.register_verb("STEW").unwrap();
    d
}

/// settle_step with the exact id-generator
/// closure: `x{first_record_id}_{n}`, the counter incrementing per minted id.
fn run(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    src: &str,
    first_record_id: i64,
) -> ExecResult {
    let s = parse_sentence(src, dict).unwrap();
    let mut n: i64 = 0;
    let mut id_gen = || {
        n += 1;
        format!("x{first_record_id}_{n}")
    };
    settle_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        first_record_id,
        Some(&mut id_gen),
    )
    .unwrap()
}

/// `said` — v0.5: NARRATE defers to the
/// completion — gather across the COMPLETE rows.
fn said(r: &ExecResult) -> Vec<String> {
    r.records
        .iter()
        .filter(|x| x.kind == "complete")
        .flat_map(|x| {
            x.outcomes.iter().filter(|o| o.op == "NARRATE").map(|o| match o.args.first() {
                Some(Arg::Str { v }) => v.clone(),
                _ => "(not a string)".to_string(),
            })
        })
        .collect()
}

// ---------- rig: the flagship pair, verbatim grammar ----------------

const CALL_FUNCTIONS: &str = r#"functions:
  sus capital -> SPAWN SUSPICION, NARRATE "The conductor's eyes linger on {capital}."
  watch(n) -> IF count(name:SUSPICION) >= n -> (ENDING ARRESTED) ELSE -> (NARRATE "not yet.")
"#;

const CALL_RECIPE: &str = "default -> sus(THE_GUARD), watch(3)";

/// The hand-expanded twin: each call replaced
/// by its body with the argument substituted — the expansion-equivalence
/// oracle.
const TWIN_RECIPE: &str = concat!(
    "default -> SPAWN SUSPICION, NARRATE \"The conductor's eyes linger on THE_GUARD.\", ",
    "IF count(name:SUSPICION) >= 3 -> (ENDING ARRESTED) ELSE -> (NARRATE \"not yet.\")"
);

/// The rig script text.
fn rig_script(recipe: &str, functions: &str) -> String {
    format!(
        r#"
script: Function Rig
player: ANA
start: 0:0

entities:
  ANA is character
  THE_GUARD is character

prototypes:
  SUSPICION is state stackable

endings:
  ARRESTED: "Taken by the constabulary"
{functions}
verbs:
  STEW:
    actors: 1 kind:character
    duration: 0m
    recipes:
      {recipe}
"#
    )
}

/// The rig verb map.
fn rig_verb(recipes: &[&str]) -> IndexMap<String, VerbContent> {
    [(
        "STEW".to_string(),
        VerbContent {
            slots: vec![SlotDef {
                role: "actors".to_string(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".to_string()),
                shown: None,
            }],
            duration: 0,
            recipes: recipes
                .iter()
                .map(|out| RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: vec![out.to_string()],
                })
                .collect(),
        },
    )]
    .into_iter()
    .collect()
}

/// World — ANA (actor), THE_GUARD (shadow
/// target), TOME (secret:1)
fn world() -> State {
    let mut s = new_state(0);
    let mut card = |id: &str, kind: &str, name: &str, aspects: Aspects, attrs: Attrs| {
        add_entity(
            &mut s,
            Entity {
                id: id.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                aspects,
                attrs,
                born_action: None,
                dead_action: None,
                stackable: false,
            },
        )
        .unwrap();
    };
    card("s_house", "place", "HOUSE", Aspects::new(), Attrs::new());
    card(
        "s_ana",
        "character",
        "ANA",
        Aspects::new(),
        [("container".to_string(), AttrValue::Str("s_house".to_string()))].into_iter().collect(),
    );
    card(
        "s_the_guard",
        "character",
        "THE_GUARD",
        Aspects::new(),
        [("container".to_string(), AttrValue::Str("s_house".to_string()))].into_iter().collect(),
    );
    card(
        "s_tome",
        "text",
        "TOME",
        [("secret".to_string(), AspectVal::Num(1))].into_iter().collect(),
        [("container".to_string(), AttrValue::Str("s_ana".to_string()))].into_iter().collect(),
    );
    s
}

/// Add n SUSPICION pool cards.
fn add_suspicion(s: &mut State, n: i64) {
    for _ in 0..n {
        let id = format!("w{}", s.entities.len());
        add_entity(
            s,
            Entity {
                id,
                kind: "state".to_string(),
                name: "SUSPICION".to_string(),
                aspects: Aspects::new(),
                attrs: Attrs::new(),
                born_action: None,
                dead_action: None,
                stackable: true,
            },
        )
        .unwrap();
    }
}

/// Build a parse-level FnTable the
/// way compile_content's passes do.
fn raw_table(dict: &Dictionary, defs: &[FunctionDef]) -> FnTable {
    let mut table = FnTable::default();
    for d in defs {
        table.by_name.insert(
            d.name.clone(),
            FnTemplate { name: d.name.clone(), params: d.params.clone(), body: vec![] },
        );
    }
    for d in defs {
        let params = table.by_name.get(&d.name).unwrap().params.clone();
        let body = parse_function_body(&d.body, dict, &table, &params).unwrap();
        table.by_name.get_mut(&d.name).unwrap().body = body;
    }
    table
}

fn fdef(name: &str, params: &[&str], body: &str) -> FunctionDef {
    FunctionDef {
        name: name.to_string(),
        params: params.iter().map(|s| s.to_string()).collect(),
        body: body.to_string(),
    }
}

/// Rig — the substitution section's content:
/// watch's body carries ENDING ARRESTED — the endings check runs on expanded
/// bodies, so the rig declares the table; SUSPICION is the mint target for
/// ref-position params.
fn rig(dict: &Dictionary, functions: Vec<FunctionDef>, recipe: &str) -> CompiledContent {
    compile_content(
        &ContentRaw {
            verbs: rig_verb(&[recipe]),
            functions: Some(functions),
            prototypes: Some(
                [(
                    "SUSPICION".to_string(),
                    PrototypeDef {
                        kind: "state".to_string(),
                        stackable: true,
                        ..PrototypeDef::default()
                    },
                )]
                .into_iter()
                .collect(),
            ),
            endings: Some(
                [(
                    "ARRESTED".to_string(),
                    EndingDef { title: "Taken by the constabulary".to_string(), flavour: None },
                )]
                .into_iter()
                .collect(),
            ),
            ..ContentRaw::default()
        },
        dict,
        &[],
    )
    .unwrap()
}

/// chain(n)
fn chain(n: usize) -> Vec<FunctionDef> {
    (0..n)
        .map(|i| {
            let body =
                if i + 1 < n { format!("f{}()", i + 2) } else { "NARRATE \"bottom.\"".to_string() };
            FunctionDef { name: format!("f{}", i + 1), params: vec![], body }
        })
        .collect()
}

// ---------- expansion equivalence (the string-substitution case included) ------

mod expansion_equivalence {
    use super::*;

    /// The compiled recipe's outcome
    /// ASTs serialized (the JSON wire form).
    fn recipe_out(c: &CompiledContent) -> String {
        serde_json::to_string(&c.verbs.get("STEW").unwrap().recipes[0].out).unwrap()
    }

    #[test]
    fn compiled_recipe_outcomes_are_identical_byte_for_byte() {
        let dict = stew_dict();
        let call = compile_script(&rig_script(CALL_RECIPE, CALL_FUNCTIONS), &dict).unwrap();
        let twin = compile_script(&rig_script(TWIN_RECIPE, ""), &dict).unwrap();
        assert_eq!(recipe_out(&call.content), recipe_out(&twin.content));
    }

    #[test]
    fn playing_the_call_and_the_twin_produces_byte_identical_records() {
        let dict = stew_dict();
        let call: CompiledScript =
            compile_script(&rig_script(CALL_RECIPE, CALL_FUNCTIONS), &dict).unwrap();
        let twin = compile_script(&rig_script(TWIN_RECIPE, ""), &dict).unwrap();
        let call_state = clone_state(&call.state);
        let ra = run(&call.content, &dict, call.state, "0:0 @ ANA STEW", 1);
        let rb = run(&twin.content, &dict, twin.state, "0:0 @ ANA STEW", 1);
        assert_eq!(ra.rejected, None);
        assert_eq!(rb.rejected, None);
        assert_eq!(
            serde_json::to_string(&ra.records).unwrap(),
            serde_json::to_string(&rb.records).unwrap()
        );
        // and the substitution really did land: the interpolated spelling,
        // the spawned pool, and the taken watch arm are all there
        assert_eq!(said(&ra), ["The conductor's eyes linger on THE_GUARD.", "not yet."]);
        assert_eq!(ra.state.by_name.get("SUSPICION").unwrap().len(), 1);
        assert_eq!(
            snap(&fold(&ra.records, &call_state, &dict, &call.content).unwrap()),
            snap(&ra.state)
        );
    }
}

// ---------- grammar: declaration forms and dispatch ------------------------

mod call_site_dispatch_and_arity {
    use super::*;

    #[test]
    fn declaring_a_function_named_like_an_outcome_op_is_a_compile_error() {
        let dict = stew_dict();
        let verb = rig_verb(&["noop()"]);
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: verb.clone(),
                    functions: Some(vec![fdef("SET", &[], "NARRATE \"no.\"")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"function 'SET' collides with an outcome op",
        );
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: verb.clone(),
                    functions: Some(vec![fdef("IF", &[], "NARRATE \"no.\"")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"cannot be named a conditional keyword",
        );
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: verb,
                    functions: Some(vec![
                        fdef("dup", &[], "NARRATE \"a\""),
                        fdef("dup", &[], "NARRATE \"b\""),
                    ]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"duplicate function 'dup'",
        );
    }

    #[test]
    fn the_content_raw_path_compiles_and_plays_end_to_end_compile_content_is_the_only_entry() {
        let dict = stew_dict();
        let c = compile_content(
            &ContentRaw {
                verbs: rig_verb(&["one(THE_GUARD)"]),
                functions: Some(vec![fdef("one", &["x"], "NARRATE \"x is {x}.\"")]),
                ..ContentRaw::default()
            },
            &dict,
            &[],
        )
        .unwrap();
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(said(&r), ["x is THE_GUARD."]);
    }
}

// ---------- the two substitution mechanisms --------------------------------

mod parameter_substitution {
    use super::*;

    #[test]
    fn ref_position_a_param_mints_whatever_prototype_the_call_names() {
        let dict = stew_dict();
        let c = rig(&dict, vec![fdef("mint", &["card"], "SPAWN card")], "mint(SUSPICION)");
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        let spawn = &done(&r).unwrap().outcomes[0];
        assert_eq!(spawn.op, "SPAWN");
        let Arg::Ref { r#ref: Ref::Name { name, kind } } = &spawn.args[0] else {
            panic!("expected a name ref, got {:?}", spawn.args[0]);
        };
        assert_eq!(name, "SUSPICION");
        assert_eq!(kind.as_deref(), Some("state"));
        assert_eq!(r.state.by_name.get("SUSPICION").unwrap().len(), 1);
    }

    #[test]
    fn delta_position_the_authored_sign_folds_into_the_call_s_number() {
        let dict = stew_dict();
        let c = rig(
            &dict,
            vec![
                fdef("bump", &["n"], "SET(TOME, aspect:secret, +n)"),
                fdef("drop", &["n"], "SET(TOME, aspect:secret, -n)"),
            ],
            "bump(2), drop(4)",
        );
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        // secret starts at 1: +2 → 3, then −4 → −1; the log carries computed
        // absolutes (§4.4)
        let values: Vec<serde_json::Value> = done(&r)
            .unwrap()
            .outcomes
            .iter()
            .map(|o| serde_json::to_value(&o.args[2]).unwrap())
            .collect();
        assert_eq!(
            values,
            vec![
                serde_json::json!({ "t": "int", "v": 3 }),
                serde_json::json!({ "t": "int", "v": -1 }),
            ]
        );
        assert_eq!(r.state.entities["s_tome"].aspects.get("secret"), Some(&AspectVal::Num(-1)));
    }

    #[test]
    fn string_position_paren_less_and_paren_d_narrate_take_a_param_interpolates() {
        let dict = stew_dict();
        let c = rig(
            &dict,
            vec![fdef("cry", &["msg", "who"], "NARRATE msg, NARRATE(\"so does {who}.\")")],
            "cry(\"halt!\", THE_GUARD)",
        );
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(said(&r), ["halt!", "so does THE_GUARD."]);
    }

    #[test]
    fn reserved_tokens_pass_through_an_undeclared_token_is_a_load_error() {
        let dict = stew_dict();
        // {actors} is §5.3 prose, never a parameter — it survives expansion
        let ok = rig(
            &dict,
            vec![fdef("witness", &["who"], "NARRATE \"{actors} saw {who}.\"")],
            "witness(THE_GUARD)",
        );
        let r = run(&ok, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(said(&r), ["{actors} saw THE_GUARD."]);
        // a typo'd parameter would silently bake into every call's journal
        // prose — this compiler has no warning channel, so undeclared
        // interpolation is a load error naming the token, the function, and
        // the reserved set
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["bad(THE_GUARD)"]),
                    functions: Some(vec![fdef("bad", &["x"], "NARRATE \"hi {x} and {nope}.\"")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"'\{nope\}' in a body string names neither a parameter of 'bad' nor a reserved token",
        );
        // spaced spellings never rewrite (§5.3 reads exact {token}s either),
        // so a spaced brace can only bake literally — the same typo, same
        // load error, whether or not the identifier names a parameter
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["s1(THE_GUARD)"]),
                    functions: Some(vec![fdef("s1", &["x"], "NARRATE \"hi { x }.\"")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"'\{ x \}' in a body string names neither a parameter",
        );
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["s2()"]),
                    functions: Some(vec![fdef("s2", &[], "NARRATE \"{ actors } gather.\"")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"'\{ actors \}' in a body string names neither a parameter",
        );
        // commas/quotes inside braces are ordinary prose, not a token
        let prose = compile_content(
            &ContentRaw {
                verbs: rig_verb(&["prose(THE_GUARD)"]),
                functions: Some(vec![fdef(
                    "prose",
                    &["x"],
                    "NARRATE \"the set { a, b } holds {x}\"",
                )]),
                ..ContentRaw::default()
            },
            &dict,
            &[],
        );
        assert!(prose.is_ok(), "commas inside braces are ordinary prose: {prose:?}");
    }

    #[test]
    fn if_condition_position_watch_3_makes_the_count_threshold_the_call_s_number() {
        let dict = stew_dict();
        let c = rig(
            &dict,
            vec![fdef(
                "watch",
                &["n"],
                "IF count(name:SUSPICION) >= n -> (ENDING ARRESTED) ELSE -> (NARRATE \"not yet.\")",
            )],
            "watch(3)",
        );
        let mut s = world();
        add_suspicion(&mut s, 2); // 2 >= 3 is false → the ELSE arm runs
        let r = run(&c, &dict, s, "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(said(&r), ["not yet."]);
        assert_eq!(r.state.ended, None);
        let mut state = r.state;
        add_suspicion(&mut state, 1); // now 3 >= 3: the same compiled content arrests
        let r2 = run(&c, &dict, state, "0:1 @ ANA STEW", 10);
        let ended = r2.state.ended.as_ref().unwrap();
        assert_eq!(ended.id, "ARRESTED");
        assert_eq!(ended.title, "Taken by the constabulary");
        assert_eq!(ended.flavour, "melancholy");
    }

    #[test]
    fn a_kind_mismatch_the_ast_can_represent_refuses_the_window_at_runtime_like_any_kind_error() {
        let dict = stew_dict();
        // DESTROY of a value literal would iterate nothing (iteration-style
        // op); SPAWN actually needs its name ref, so the mismatch refuses
        let c = rig(&dict, vec![fdef("mint", &["card"], "SPAWN card")], "mint(7)");
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        assert_refusal_matches(&r, r"SPAWN needs a name ref");
    }
}

// ---------- the pair-position param vs the SPAWN expires modifier (§4.4, §10) ------

/// A §10 parameter standing in a SPAWN `k=v` pair position expands to the
/// caller's WHOLE bound value — a bare Dur/Str arg the SPAWN grammar itself
/// can never author. The `expires <duration>` mint modifier (§4.4) is its
/// own dedicated arg shape precisely so such an expansion can never
/// masquerade as it: the residue is a LOUD application failure (the E12
/// review's chosen discipline — it used to drop the arg inertly, silently
/// losing the author's intent).
mod param_in_a_pair_position_never_reads_as_the_expires_modifier {
    use super::*;

    /// A DURATION-bound param in a pair position: loud refusal at the
    /// completion — never a silent arming of the modifier.
    #[test]
    fn a_dur_bound_param_in_a_pair_position_refuses_loud_not_arms_expiry() {
        let dict = stew_dict();
        let c = rig(
            &dict,
            vec![fdef("decay", &["p"], "SPAWN SUSPICION ttl=p, NARRATE \"minted.\"")],
            "decay(2h)",
        );
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None, "the expansion compiles and runs to completion");
        assert_refusal_matches(
            &r,
            r"a bare dur arg is not a SPAWN arg shape — a §10 parameter standing in a k=v pair position expands to the caller's whole bound value",
        );
        // nothing landed: no silent mint, no silent arming
        assert!(!r.state.by_name.contains_key("SUSPICION"));
    }

    /// A STRING-bound param: the same loud refusal — both inert pre-review
    /// shapes now name the §10 shape instead of dropping silently.
    #[test]
    fn a_str_bound_param_in_a_pair_position_refuses_loud_the_same_way() {
        let dict = stew_dict();
        let c = rig(
            &dict,
            vec![fdef("tag", &["p"], "SPAWN SUSPICION note=p, NARRATE \"minted.\"")],
            "tag(\"soon\")",
        );
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        assert_refusal_matches(
            &r,
            r"a bare str arg is not a SPAWN arg shape — a §10 parameter standing in a k=v pair position expands to the caller's whole bound value",
        );
        assert!(!r.state.by_name.contains_key("SUSPICION"));
    }

    /// The modifier's own shape is untouched by the guard: a body-authored
    /// `expires <duration>` arms the mint through the same call path.
    #[test]
    fn the_modifier_itself_still_arms_through_a_function_body() {
        let dict = stew_dict();
        let c = rig(
            &dict,
            vec![fdef("decay", &[], "SPAWN SUSPICION expires 2h, NARRATE \"minted.\"")],
            "decay()",
        );
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(refusal(&r), None);
        let id = &r.state.by_name["SUSPICION"][0];
        assert_eq!(
            r.state.entities[id.as_str()].attrs.get("expiresAt"),
            Some(&AttrValue::Num(120))
        );
    }
}

fn assert_refusal_matches(r: &ExecResult, pattern: &str) {
    let msg =
        refusal(r).unwrap_or_else(|| panic!("expected a refusal matching /{pattern}/, got None"));
    let re = regex::Regex::new(pattern).unwrap();
    assert!(re.is_match(&msg), "refusal {msg:?} does not match /{pattern}/");
}

// ---------- nesting, recursion, depth --------------------------------------

mod nesting_recursion_and_the_depth_cap {
    use super::*;

    #[test]
    fn a_body_calling_a_function_expands_transitively_at_the_call_site_forward_refs_fine() {
        let dict = stew_dict();
        let c = compile_content(
            &ContentRaw {
                verbs: rig_verb(&["outer(THE_GUARD)"]),
                functions: Some(vec![
                    fdef("inner", &["x"], "NARRATE \"inner {x}.\""),
                    fdef("outer", &["x"], "inner(x), SPAWN SUSPICION"),
                ]),
                prototypes: Some(
                    [(
                        "SUSPICION".to_string(),
                        PrototypeDef {
                            kind: "state".to_string(),
                            stackable: true,
                            ..PrototypeDef::default()
                        },
                    )]
                    .into_iter()
                    .collect(),
                ),
                ..ContentRaw::default()
            },
            &dict,
            &[],
        )
        .unwrap();
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(said(&r), ["inner THE_GUARD."]);
        assert_eq!(r.state.by_name.get("SUSPICION").unwrap().len(), 1);
    }

    /// Mutual recursion names the cycle;
    /// self-recursion names itself (frozen text, full-chain re-check)
    #[test]
    fn mutual_recursion_is_a_load_error_naming_the_cycle_self_recursion_names_itself() {
        let dict = stew_dict();
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: IndexMap::new(),
                    functions: Some(vec![fdef("a", &[], "b()"), fdef("b", &[], "a()")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"functions recurse: a -> b -> a",
        );
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: IndexMap::new(),
                    functions: Some(vec![fdef("self", &[], "self()")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"functions recurse: self -> self",
        );
    }

    /// A depth-8 chain of distinct functions
    /// loads; depth 9 is a load error (frozen text, full-chain re-check)
    #[test]
    fn a_depth_8_chain_of_distinct_functions_loads_depth_9_is_a_load_error() {
        let dict = stew_dict();
        let verb = rig_verb(&["f1()"]);
        let eight = compile_content(
            &ContentRaw { verbs: verb.clone(), functions: Some(chain(8)), ..ContentRaw::default() },
            &dict,
            &[],
        )
        .unwrap();
        let r = run(&eight, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(said(&r), ["bottom."]);
        assert_err_msg(
            compile_content(
                &ContentRaw { verbs: verb, functions: Some(chain(9)), ..ContentRaw::default() },
                &dict,
                &[],
            ),
            r"function 'f1' sits 9 calls deep — the nesting cap is 8",
        );
    }

    #[test]
    fn nested_call_arity_mismatches_are_load_errors_even_before_any_call_site_runs() {
        let dict = stew_dict();
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: IndexMap::new(),
                    functions: Some(vec![
                        fdef("one", &["x"], "NARRATE \"x is {x}\""),
                        fdef("outer", &[], "one()"),
                    ]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"function 'outer': function 'one' expects 1 argument\(s\)",
        );
    }
}

// ---------- param shadowing ------------------------------------------------

mod parameter_shadows_an_entity_of_the_same_name {
    use super::*;

    #[test]
    fn destroy_guard_in_the_body_destroys_the_bound_card_not_the_entity_guard() {
        let dict = stew_dict();
        let c = compile_content(
            &ContentRaw {
                verbs: rig_verb(&["f(THE_GUARD)"]),
                functions: Some(vec![fdef("f", &["GUARD"], "DESTROY GUARD")]),
                ..ContentRaw::default()
            },
            &dict,
            &[],
        )
        .unwrap();
        let mut s = world();
        // GUARD is a decoy entity living beside THE_GUARD
        add_entity(
            &mut s,
            Entity {
                id: "s_guard".to_string(),
                kind: "character".to_string(),
                name: "GUARD".to_string(),
                aspects: Aspects::new(),
                attrs: Attrs::new(),
                born_action: None,
                dead_action: None,
                stackable: false,
            },
        )
        .unwrap();
        let r = run(&c, &dict, s, "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        assert!(r.state.entities["s_the_guard"].dead_action.is_some()); // the bound card died
        assert!(r.state.entities["s_guard"].dead_action.is_none()); // the namesake lives
    }
}

// ---------- call sites in triggers; expansion before the guards ------------

mod trigger_call_sites_and_expansion_before_guards {
    use super::*;

    #[test]
    fn a_trigger_outcome_calls_a_function_its_expansion_rides_the_event_record() {
        let dict = stew_dict();
        let c = compile_script(
            r#"
script: Trigger Rig
player: ANA
start: 0:0

entities:
  ANA is character
  THE_GUARD is character

prototypes:
  SUSPICION is state stackable

triggers:
  patrol at hour 1 -> sus(THE_GUARD)
functions:
  sus capital -> SPAWN SUSPICION, NARRATE "The conductor's eyes linger on {capital}."
  watch(n) -> IF count(name:SUSPICION) >= n -> (ENDING ARRESTED) ELSE -> (NARRATE "not yet.")
verbs:
  STEW:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "time passes."
"#,
            &dict,
        )
        .unwrap();
        // the 1h duration crosses hour 1 (slot 2)
        let c_state = clone_state(&c.state);
        let r = run(&c.content, &dict, c.state, "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        let event = r.records.iter().find(|x| x.kind == "event").expect("the patrol event");
        assert_eq!(event.trigger.as_deref(), Some("patrol"));
        // the trigger's narration reads straight off the EVENT row's outcomes
        let narrate =
            event.outcomes.iter().find(|o| o.op == "NARRATE").expect("the NARRATE outcome");
        let Arg::Str { v } = &narrate.args[0] else {
            panic!("NARRATE arg is a string");
        };
        assert_eq!(v, "The conductor's eyes linger on THE_GUARD.");
        assert_eq!(r.state.by_name.get("SUSPICION").unwrap().len(), 1);
        assert_eq!(snap(&fold(&r.records, &c_state, &dict, &c.content).unwrap()), snap(&r.state));
    }

    #[test]
    fn a_role_ref_body_outcome_from_a_trigger_rejects_the_step_at_runtime() {
        let dict = stew_dict();
        let c = compile_script(
            r#"
script: Quake Rig
player: ANA
start: 0:0

entities:
  ANA is character
  TOME is text at ANA

triggers:
  quake at hour 1 -> shake()

functions:
  shake -> MOVE ACTORS INTO OBJECT

verbs:
  STEW:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "time passes."
"#,
            &dict,
        )
        .unwrap();
        let c_state = clone_state(&c.state);
        let before = snap(&c_state);
        let r = run(&c.content, &dict, c.state, "0:0 @ ANA STEW", 1);
        // MOVE cannot proceed with empty bindings (§9.2)
        assert!(r.rejected.as_deref().is_some_and(|m| regex::Regex::new(
            r"unresolvable destination"
        )
        .unwrap()
        .is_match(m)));
        assert_eq!(r.records.len(), 0);
        assert_eq!(snap(&c_state), before); // nothing applied, clock unmoved
    }

    #[test]
    fn progress_in_a_body_load_error_from_a_trigger_fine_from_a_recipe() {
        let dict = stew_dict();
        let tick = fdef("tick", &[], "PROGRESS(30m, \"tick\")");
        // the trigger guard walks EXPANDED bodies — the call site is guilty
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["tick()"]),
                    triggers: Some(vec![TriggerDef::Slot(
                        chorus::content::content::SlotTriggerDef {
                            id: "tock".to_string(),
                            slot: 2,
                            once: Some(true),
                            out: vec!["tick()".to_string()],
                        },
                    )]),
                    functions: Some(vec![tick.clone()]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"trigger tock: PROGRESS belongs to verb recipes, not events",
        );

        // the same body from a recipe is ordinary content
        let c = compile_content(
            &ContentRaw {
                verbs: rig_verb(&["tick()"]),
                functions: Some(vec![tick]),
                ..ContentRaw::default()
            },
            &dict,
            &[],
        )
        .unwrap();
        let r = run(&c, &dict, world(), "0:0 @ ANA STEW", 1);
        assert_eq!(r.rejected, None);
        let first = &act(&r).unwrap().outcomes[0];
        assert_eq!(first.op, "PROGRESS");
        assert_eq!(
            serde_json::to_value(&first.args).unwrap(),
            serde_json::json!([
                { "t": "dur", "v": 30 },
                { "t": "str", "v": "tick" },
            ])
        );
    }

    #[test]
    fn an_undeclared_ending_id_inside_a_body_is_a_load_error_through_the_expansion() {
        let dict = stew_dict();
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["doom()"]),
                    functions: Some(vec![fdef("doom", &[], "ENDING NOT_DECLARED")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"ENDING 'NOT_DECLARED' is not in the endings table",
        );
        // the same through the TRIGGER path — expansion precedes
        // check_ending_refs there too
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["NARRATE \"idle.\""]),
                    triggers: Some(vec![TriggerDef::Slot(
                        chorus::content::content::SlotTriggerDef {
                            id: "doom_t".to_string(),
                            slot: 2,
                            once: Some(true),
                            out: vec!["doom()".to_string()],
                        },
                    )]),
                    functions: Some(vec![fdef("doom", &[], "ENDING NOT_DECLARED")]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"trigger doom_t: ENDING 'NOT_DECLARED' is not in the endings table",
        );
    }

    #[test]
    fn guards_see_into_a_nested_if_inside_a_body_progress_in_a_branch_is_still_a_trigger_load_error(
    ) {
        let dict = stew_dict();
        // expansion flattens the call, then the recursive guard walks the
        // branch — neither the FN boundary nor the IF boundary may hide a
        // PROGRESS
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["NARRATE \"idle.\""]),
                    triggers: Some(vec![TriggerDef::Slot(
                        chorus::content::content::SlotTriggerDef {
                            id: "sneak".to_string(),
                            slot: 2,
                            once: Some(true),
                            out: vec!["sneak()".to_string()],
                        },
                    )]),
                    functions: Some(vec![fdef(
                        "sneak",
                        &[],
                        "IF exists(ANA) -> (PROGRESS(30m, \"tick\")) ELSE -> (NARRATE \"no\")",
                    )]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"trigger sneak: PROGRESS belongs to verb recipes, not events",
        );
    }

    #[test]
    fn the_count_typo_lint_applies_to_an_expanded_body_condition_on_the_trigger_path_too() {
        let dict = stew_dict();
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["NARRATE \"idle.\""]),
                    triggers: Some(vec![TriggerDef::Slot(
                        chorus::content::content::SlotTriggerDef {
                            id: "linted".to_string(),
                            slot: 2,
                            once: Some(true),
                            out: vec!["watch(3)".to_string()],
                        },
                    )]),
                    functions: Some(vec![fdef(
                        "watch",
                        &["n"],
                        "IF count(DREAD) >= n -> (NARRATE \"raised\")",
                    )]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"trigger linted: count\(\) argument 'DREAD' is neither a role",
        );
        // and the role-operand lint: count(actors) over a trigger-called body
        // condition
        assert_err_msg(
            compile_content(
                &ContentRaw {
                    verbs: rig_verb(&["NARRATE \"idle.\""]),
                    triggers: Some(vec![TriggerDef::Slot(
                        chorus::content::content::SlotTriggerDef {
                            id: "roled".to_string(),
                            slot: 2,
                            once: Some(true),
                            out: vec!["watch(3)".to_string()],
                        },
                    )]),
                    functions: Some(vec![fdef(
                        "watch",
                        &["n"],
                        "IF count(actors) >= n -> (NARRATE \"raised\")",
                    )]),
                    ..ContentRaw::default()
                },
                &dict,
                &[],
            ),
            r"trigger roled: IF operand count\(actors\) reads a step role",
        );
    }
}

// ---------- IF in a body, end to end ---------------------------------------

mod if_inside_a_function_body {
    use super::*;

    #[test]
    fn three_stews_the_else_arm_twice_then_watch_3_arrests_through_its_param() {
        let dict = stew_dict();
        let c = compile_script(&rig_script(CALL_RECIPE, CALL_FUNCTIONS), &dict).unwrap();
        let c_state = clone_state(&c.state);
        let mut cur = c.state;
        let mut id = 1;
        let mut all: Vec<ActionRecord> = Vec::new();
        let mut told: Vec<Vec<String>> = Vec::new();
        for _ in 0..3 {
            let r = run(&c.content, &dict, cur, "0:0 @ ANA STEW", id);
            assert_eq!(r.rejected, None);
            told.push(said(&r));
            all.extend(r.records.clone());
            id += r.records.len() as i64;
            cur = r.state;
        }
        assert_eq!(
            told,
            vec![
                vec![
                    "The conductor's eyes linger on THE_GUARD.".to_string(),
                    "not yet.".to_string()
                ],
                vec![
                    "The conductor's eyes linger on THE_GUARD.".to_string(),
                    "not yet.".to_string()
                ],
                vec!["The conductor's eyes linger on THE_GUARD.".to_string()], // the third stew's own SPAWN flipped watch(3)
            ]
        );
        assert_eq!(cur.by_name.get("SUSPICION").unwrap().len(), 3);
        let ended = cur.ended.as_ref().unwrap();
        assert_eq!(ended.id, "ARRESTED");
        assert_eq!(ended.title, "Taken by the constabulary");
        assert_eq!(ended.flavour, "melancholy");
        let over = run(&c.content, &dict, cur, "0:0 @ ANA STEW", id);
        assert!(over.rejected.as_deref().is_some_and(|m| m.contains("the story has ended")));
        // and the whole run folds back identically
        assert_eq!(snap(&fold(&all, &c_state, &dict, &c.content).unwrap()), snap(&over.state));
    }
}

// ---------- round-trip: the log never sees a call --------------------------

mod canonical_equals_expansion {
    use super::*;

    #[test]
    fn expanded_outcomes_round_trip_through_jsonl_and_the_fold_no_fn_op_or_call_text_anywhere() {
        let dict = stew_dict();
        let c = compile_script(&rig_script(CALL_RECIPE, CALL_FUNCTIONS), &dict).unwrap();
        let c_state = clone_state(&c.state);
        let mut cur = c.state;
        let mut id = 1;
        let mut all: Vec<ActionRecord> = Vec::new();
        for _ in 0..3 {
            let r = run(&c.content, &dict, cur, "0:0 @ ANA STEW", id);
            all.extend(r.records.clone());
            id += r.records.len() as i64;
            cur = r.state;
        }
        let dump = serde_json::to_string(&all).unwrap();
        let call_text = regex::Regex::new(r"\b(sus|watch)\(").unwrap();
        assert!(!call_text.is_match(&dump), "no call text anywhere: {dump}");
        assert!(!dump.contains("\"FN\""), "the authored op never reaches a record");
        for rec in &all {
            // canonical sentences re-parse to themselves (every outcome is concrete)
            let parsed = parse_sentence(&rec.sentence, &dict).unwrap();
            assert_eq!(
                format_sentence(&parsed, Some(&dict)).unwrap(),
                rec.sentence,
                "sentence must round-trip"
            );
        }
        let dir = std::env::temp_dir().join(format!("chorus-fn-{}-jsonl", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("log.jsonl");
        save_log(&path, &all).unwrap();
        let replayed = fold(&load_log(&path).unwrap(), &c_state, &dict, &c.content).unwrap();
        assert_eq!(snap(&replayed), snap(&cur));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_formatter_refuses_authored_form_a_call_node_and_a_parameter_both_throw() {
        let dict = stew_dict();
        let table = raw_table(&dict, &[fdef("one", &["x"], "NARRATE \"x is {x}\"")]);
        let node = &parse_outcomes("one(THE_GUARD)", &dict, Some(&table)).unwrap()[0];
        let sentence = Sentence {
            kind: SentenceKind::Action,
            clock: None,
            trigger: None,
            clauses: vec![],
            outcomes: vec![node.clone()],
            span_min: None,
        };
        assert_err_msg(
            format_sentence(&sentence, Some(&dict)),
            r"a function call is authored form only",
        );
        let sentence = Sentence {
            kind: SentenceKind::Action,
            clock: None,
            trigger: None,
            clauses: vec![],
            outcomes: vec![Outcome {
                op: "SPAWN".to_string(),
                args: vec![Arg::Param { name: "x".to_string(), sign: None }],
                arms: None,
                fallback: None,
                call: None,
            }],
            span_min: None,
        };
        assert_err_msg(
            format_sentence(&sentence, Some(&dict)),
            r"a function parameter is authored form only",
        );
    }
}
