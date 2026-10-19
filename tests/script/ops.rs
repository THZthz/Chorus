//! Script-defined outcome operators (§10's `ops:`): a declared op is
//! a named, parameterized body of outcomes callable in ANY outcome position
//! exactly like a built-in op. The mechanism is the functions mechanism — a
//! call expands into its body AT LOAD, so the engine and the log never see a
//! call and every existing outcome guard stays sound by construction — while
//! the namespace is the OP namespace: UPPER_SNAKE names, arity-checked call
//! sites, op-grade composition (ops may call ops and functions; a function
//! may never call an op). Error-message patterns are pinned CHARACTER FOR
//! CHARACTER.

use chorus::content::script::{compile_script, lint_script_findings};
use chorus::core::dict::core_dictionary;
use chorus::core::types::clone_state;
use chorus::engine::fold::fold;
use chorus::engine::store::{load_log, save_log};
use chorus::session::game::{Game, TurnKind};

use crate::common::harness::assert_err_msg;
use crate::common::rig::auto_settle_line;
use crate::common::snap::snap;

/// The acceptance rig: two verbs (TOUCH — recipe-side op calls; SIMMER —
/// composition), a slot trigger and an `on complete` clause that call ops,
/// one function for the op→fn direction.
const OP_SCRIPT: &str = r#"
script: The Op Proof
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE
  THE_GUARD is character with duty:2 at HOUSE on complete -> ( MINT_SUS() )
  THE_COOK is character with duty:0 at HOUSE

prototypes:
  SUSPICION is state stackable

functions:
  sus who -> SPAWN SUSPICION, NARRATE "The conductor's eyes linger on {who}."

ops:
  PRESS(who, n) -> SET(who, aspect:pressure, +n), NARRATE "{who} feels it: +{n}."
  BUMP -> NARRATE "bump."
  MINT_SUS -> SUSPICION
  MAYBE_BREAK(who) -> IF object.aspect.duty >= 2 -> ( SET(who, aspect:broken, 1), NARRATE "{who} breaks." ) ELSE -> ( NARRATE "{who} holds." )
  CHAIN(who) -> MAYBE_BREAK(who), BUMP()
  VIA_FN(who) -> sus(who), BUMP()
  LUCKY(who) -> NARRATE "{who} eyes the dice ({roll}).", ROLL(1, 6), NARRATE "{who} rolled {roll}."
  ABORT -> REFUSE("the op refuses")

triggers:
  dusk at tick 3 -> PRESS(THE_GUARD, 3)

verbs:
  TOUCH:
    actors: 1 kind:character
    object: 1 kind:character
    duration: 30m
    recipes:
      default -> PRESS(OBJECT, 1)
  SIMMER:
    actors: 1 kind:character
    object: 1 kind:character
    duration: 0m
    recipes:
      default -> CHAIN(OBJECT)
  SNIFF:
    actors: 1 kind:character
    object: 1 kind:character
    duration: 0m
    recipes:
      default -> VIA_FN(THE_COOK)
  DICE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> LUCKY(ACTORS)
  FOLD_HANDS:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ABORT()
"#;

fn guard_pressure(game: &Game) -> i64 {
    let id = &game.state.by_name["THE_GUARD"][0];
    match game.state.entities[id.as_str()].aspects.get("pressure") {
        Some(chorus::core::types::AspectVal::Num(v)) => *v,
        other => panic!("expected a numeric pressure aspect, got {other:?}"),
    }
}

#[test]
fn an_op_declares_expands_and_plays_in_a_recipe() {
    let mut game = Game::from_script(OP_SCRIPT, core_dictionary()).unwrap();
    // args ride the call: a role ref (OBJECT), a literal int, and {param}
    // interpolation in the body's strings — canonical equals expansion. A
    // role-ref arg BINDS like the same text authored at the call site (the
    // SET below hits the bound guard) and its {param} spelling is the role
    // word — the arg_spelling rule the functions mechanism freezes
    let t = auto_settle_line(&mut game, "touch the guard").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l.contains("OBJECT feels it: +1.")), "{:?}", t.lines);
    // the recipe op's SET applied to the bound role — and the 30m window
    // completing at 0:30 crosses tick 3 (minute 30) in the SAME turn, so the slot
    // trigger's op adds its +3 before the turn ends
    assert_eq!(guard_pressure(&game), 4, "recipe op + trigger op both applied");
    // the log rows are built-in-shaped: no PRESS, no call — the recipe's
    // BEGIN row carries the concrete SET+NARRATE the expansion produced
    for rec in &game.log.records {
        for o in &rec.outcomes {
            assert_ne!(o.op, "PRESS", "an op name reached the log: {:?}", rec.outcomes);
            assert_ne!(o.op, "FN", "a call node reached the log: {:?}", rec.outcomes);
        }
    }
}

#[test]
fn an_op_plays_from_a_slot_trigger_and_an_on_complete_body() {
    let mut game = Game::from_script(OP_SCRIPT, core_dictionary()).unwrap();
    // TOUCH (30m) completes at 0:30: the recipe's op sets +1, the on
    // complete clause mints through MINT_SUS, and the advance pass crosses
    // tick 3 (minute 30) where the slot trigger's op sets +3 — every position is
    // just another outcome list, and the deltas commute
    let t = auto_settle_line(&mut game, "touch the guard").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert_eq!(guard_pressure(&game), 4, "recipe op + trigger op both applied");
    assert_eq!(game.state.by_name["SUSPICION"].len(), 1, "the on complete body's op minted");
}

#[test]
fn an_op_plays_from_an_on_expire_trigger() {
    // the third trigger kind rides the same compile_trigger expansion walk
    // as the slot and completion kinds above — the pin is coverage, not a
    // new mechanism: an authored `on expire of NAME` clause calls an op and
    // the crossing fires the body, expanded
    let script = r#"
script: The Expire Op Proof
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE
  THE_ICE is item at HOUSE

ops:
  MELT -> DESTROY THE_ICE, NARRATE "the ice melts away."

triggers:
  melt_ice on expire of THE_ICE -> MELT()

verbs:
  PRIME:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> SET(THE_ICE, expires, 30m)
  WATCH:
    actors: 1 kind:character
    duration: 1h
    recipes:
      default -> NARRATE "the watch passes."
"#;
    let mut game = Game::from_script(script, core_dictionary()).unwrap();
    // prime (0:00→1:00) SETs expires 30m from the completion's now — 1:30,
    // beyond the pass that turn already walked. watch (1:00→2:00) crosses
    // 1:30: the authored trigger replaces the built-in destroy with the
    // op's body
    auto_settle_line(&mut game, "prime").unwrap();
    assert!(game.state.by_name.contains_key("THE_ICE"), "no crossing yet");
    auto_settle_line(&mut game, "watch").unwrap();
    let event = game
        .log
        .records
        .iter()
        .find(|r| r.trigger.as_deref() == Some("melt_ice"))
        .expect("the expiry crossing fired the authored trigger");
    let ops: Vec<_> = event.outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["DESTROY", "NARRATE"], "the op body, expanded: {:?}", event.outcomes);
    assert!(!game.state.by_name.contains_key("THE_ICE"), "the op's DESTROY applied");
    assert!(game.journal().join("\n").contains("the ice melts away."));
}

#[test]
fn ops_compose_with_ops_and_functions() {
    let mut game = Game::from_script(OP_SCRIPT, core_dictionary()).unwrap();
    // SIMMER calls CHAIN, which calls MAYBE_BREAK (an IF inside an op body)
    // and BUMP: op→op, three bodies spliced in order. {who} spells as the
    // call site's arg (OBJECT — the arg_spelling rule), while the SET hits
    // the bound card
    let t = auto_settle_line(&mut game, "simmer the guard").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l.contains("OBJECT breaks.")), "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l.contains("bump.")), "{:?}", t.lines);
    let guard_id = &game.state.by_name["THE_GUARD"][0];
    assert!(game.state.entities[guard_id.as_str()].aspects.get("broken").is_some());
    // and the op→fn direction: VIA_FN calls the `sus` function with a name
    // arg, whose own {who} interpolation reads the op call's argument (a
    // ROLE arg would spell the role word here — the frozen arg_spelling)
    let t2 = auto_settle_line(&mut game, "sniff the cook").unwrap();
    assert_eq!(t2.kind, TurnKind::Action, "{:?}", t2.lines);
    assert!(
        t2.lines.iter().any(|l| l.contains("The conductor's eyes linger on THE_COOK.")),
        "{:?}",
        t2.lines
    );
    assert!(t2.lines.iter().any(|l| l.contains("bump.")), "{:?}", t2.lines);
}

#[test]
fn ops_resolve_forward_references_regardless_of_section_order() {
    // ops: and functions: declare ONE flat namespace and the combined table
    // builds names first, bodies second (functions.rs build_call_tables), so
    // a body's references resolve regardless of section order: the ops:
    // section here comes FIRST and calls into a later functions: section,
    // and LOUD calls LOUDER — an op declared after its caller
    let script = r#"
script: The Forward Proof
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE
  THE_COOK is character at HOUSE

ops:
  LOUD(who) -> whisper(who), LOUDER(who)
  LOUDER(who) -> NARRATE "{who} boils over."

functions:
  whisper who -> NARRATE "{who} mutters."

verbs:
  PROD:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> LOUD(THE_COOK)
"#;
    let mut game = Game::from_script(script, core_dictionary()).unwrap();
    let t = auto_settle_line(&mut game, "prod").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    // both forward references expanded: the later-declared function and the
    // later-declared op spliced in call order
    assert!(t.lines.iter().any(|l| l.contains("THE_COOK mutters.")), "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l.contains("THE_COOK boils over.")), "{:?}", t.lines);
}

#[test]
fn an_if_inside_an_op_body_takes_different_arms_at_play() {
    let mut game = Game::from_script(OP_SCRIPT, core_dictionary()).unwrap();
    // the SAME op body, two call sites, two arms — the IF survives expansion
    // intact and evaluates at play against the call site's bindings
    let t = auto_settle_line(&mut game, "simmer the guard").unwrap();
    assert!(t.lines.iter().any(|l| l.contains("OBJECT breaks.")), "{:?}", t.lines);
    let t2 = auto_settle_line(&mut game, "simmer the cook").unwrap();
    assert!(t2.lines.iter().any(|l| l.contains("OBJECT holds.")), "{:?}", t2.lines);
    let cook_id = &game.state.by_name["THE_COOK"][0];
    assert!(game.state.entities[cook_id.as_str()].aspects.get("broken").is_none());
}

#[test]
fn a_roll_inside_an_op_body_binds_roll_for_the_rest_of_the_expanded_list() {
    let mut game = Game::from_script(OP_SCRIPT, core_dictionary()).unwrap();
    let base = game.log.records.len();
    let t = auto_settle_line(&mut game, "dice").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    // expansion is compile-time, so the {roll} slot survives into the
    // concrete rows: the NARRATE BEFORE the op body's ROLL keeps the literal
    // token, the one after carries the rolled number — the binding covers
    // the REST of the expanded list, exactly like an authored ROLL
    let journal = game.journal().join("\n");
    assert!(journal.contains("eyes the dice ({roll})"), "{journal}");
    let rolled: Vec<_> = journal.match_indices("rolled ").collect();
    assert_eq!(rolled.len(), 1, "{journal}");
    let after = &journal[rolled[0].0..];
    let num: String = after["rolled ".len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
    let v: i64 = num.parse().unwrap_or(-1);
    assert!((1..=6).contains(&v), "expected one d6 in '{after}'");
    assert_eq!(game.state.rng, 1, "the roll consumed exactly one counter");
    // a REFUSE-less op step still undoes cleanly past its roll
    while game.log.records.len() > base {
        game.undo().unwrap();
    }
    assert_eq!(game.state.rng, 0, "undo rewinds the counter the op's roll consumed");
}

#[test]
fn a_refuse_inside_an_op_body_aborts_the_step_with_the_attempt_logged() {
    let mut game = Game::from_script(OP_SCRIPT, core_dictionary()).unwrap();
    let t = auto_settle_line(&mut game, "fold hands").unwrap();
    // §6.5: draft discarded, attempt still logged — an op's REFUSE is the
    // recipe's own refusal once expanded
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert!(t.lines.iter().any(|l| l.contains("the op refuses")), "{:?}", t.lines);
    assert!(
        game.log.records.iter().any(|r| r.outcomes.iter().any(|o| o.op == "REFUSE")),
        "the attempt is logged: {:?}",
        game.log.records
    );
}

#[test]
fn an_op_named_like_a_builtin_op_or_a_reserved_word_is_a_load_error() {
    let dict = core_dictionary();
    // ROLL is a built-in op first — the op-collision message wins over the
    // reserved message (the collision check runs first)
    assert_err_msg(
        compile_script("ops:\n  ROLL(n) -> NARRATE \"x\"\n", &dict),
        r"op 'ROLL' collides with an outcome op — the built-in vocabulary is closed; rename the op",
    );
    // a reserved NON-op: the §5.1 message
    assert_err_msg(
        compile_script("ops:\n  EVENT -> NARRATE \"x\"\n", &dict),
        r"op 'EVENT' is a reserved word \(§5\.1\)",
    );
    // a core verb: reserved (by being a verb), same message face
    assert_err_msg(
        compile_script("ops:\n  READ -> NARRATE \"x\"\n", &dict),
        r"op 'READ' is a reserved word \(§5\.1\)",
    );
    // a DECLARED verb: the ops table builds after the load's verb
    // registration, so the script's own lexemes are as taken as built-ins
    assert_err_msg(
        compile_script(
            "verbs:\n  SHOUT:\n    recipes:\n      default -> NARRATE \"ho.\"\nops:\n  SHOUT -> NARRATE \"x\"\n",
            &dict,
        ),
        r"op 'SHOUT' is a reserved word \(§5\.1\)",
    );
    // ... in either section order (registration precedes the whole compile)
    assert_err_msg(
        compile_script(
            "ops:\n  SHOUT -> NARRATE \"x\"\nverbs:\n  SHOUT:\n    recipes:\n      default -> NARRATE \"ho.\"\n",
            &dict,
        ),
        r"op 'SHOUT' is a reserved word \(§5\.1\)",
    );
    // and the shape rule: a call is written and read like a built-in op call,
    // so the name must look like one
    assert_err_msg(
        compile_script("ops:\n  ench(x) -> NARRATE \"x\"\n", &dict),
        "op name 'ench' must be UPPER_SNAKE",
    );
}

#[test]
fn duplicate_ops_and_op_fn_name_collisions_are_load_errors() {
    let dict = core_dictionary();
    assert_err_msg(
        compile_script("ops:\n  BUMP -> NARRATE \"a\"\n  BUMP -> NARRATE \"b\"\n", &dict),
        "duplicate op 'BUMP'",
    );
    // the fn-name ident rule admits UPPER names, so the two namespaces are
    // NOT case-disjoint by construction — the collision check earns its keep
    assert_err_msg(
        compile_script(
            "ops:\n  ENCHANT(who) -> NARRATE \"{who}\"\nfunctions:\n  ENCHANT(x) -> NARRATE \"x\"\n",
            &dict,
        ),
        "function 'ENCHANT' collides with a declared op — calls resolve to one table; rename one",
    );
}

#[test]
fn op_recursion_names_the_cycle_and_depth_reuses_the_fn_cap() {
    let dict = core_dictionary();
    assert_err_msg(
        compile_script(
            "ops:\n  A(x) -> B(x)\n  B(x) -> A(x)\n",
            &dict,
        ),
        "ops recurse: A -> B -> A — calls expand at load time, so a recursion cycle never terminates; rewrite without the cycle",
    );
    // nine ops in a chain: depth 9 is one past the shared cap
    let mut src = String::from("ops:\n");
    for i in 1..=8 {
        src.push_str(&format!("  A{i}(x) -> A{}(x)\n", i + 1));
    }
    src.push_str("  A9(x) -> NARRATE \"{x}\"\n");
    assert_err_msg(compile_script(&src, &dict), "op 'A1' sits 9 calls deep — the nesting cap is 8");
}

#[test]
fn op_call_arity_is_checked_at_every_site() {
    let dict = core_dictionary();
    // the authored call site
    assert_err_msg(
        compile_script(
            "entities:\n  X is item\nops:\n  PRESS(who, n) -> NARRATE \"{who} {n}\"\nverbs:\n  TOUCH:\n    actors: 1 kind:character\n    object: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> PRESS(OBJECT)\n",
            &dict,
        ),
        "op 'PRESS' expects 2 argument\\(s\\) \\(who, n\\), got 1",
    );
    // ... and a nested site inside another op body (the wrap names the body)
    assert_err_msg(
        compile_script("ops:\n  INNER(x) -> NARRATE \"{x}\"\n  OUTER -> INNER(1, 2)\n", &dict),
        "op 'OUTER': op 'INNER' expects 1 argument\\(s\\) \\(x\\), got 2",
    );
}

#[test]
fn an_op_call_from_a_function_body_is_the_specific_ban_error() {
    let dict = core_dictionary();
    // fn bodies parse with op calls BANNED — a specific error, not the
    // generic unknown-call (the ops table was deliberately visible to the
    // parse so the message can name the op)
    assert_err_msg(
        compile_script(
            "functions:\n  f(x) -> ENCHANT(x)\nops:\n  ENCHANT(x) -> NARRATE \"{x}\"\n",
            &dict,
        ),
        "function 'f': op 'ENCHANT' cannot be called from a function body — functions expand at load before ops exist",
    );
}

#[test]
fn a_braced_typo_in_a_never_called_op_body_is_a_load_error() {
    let dict = core_dictionary();
    // the v0.7.2 rule reaches op bodies: every declared body's tokens are
    // checked at load, called or not
    assert_err_msg(
        compile_script("ops:\n  NEVER -> NARRATE \"{typo}\"\n", &dict),
        r"op 'NEVER': '\{typo\}' in a body string names neither a parameter of 'NEVER' nor a reserved token \(\{actors\} \{object\} \{with\} \{at\} \{about\} \{to\} \{roll\}\) — undeclared interpolation is a load error, not a silent literal \(braced tokens must be spelled exactly, without inner spaces\)",
    );
}

#[test]
fn the_authored_dice_ban_reaches_through_op_bodies() {
    let dict = core_dictionary();
    // ROLLED/DRAWN are engine-written only — a body carrying one is the
    // recipe's own load error once the call expands into it
    assert_err_msg(
        compile_script(
            "ops:\n  CHEAT -> ROLLED(3, 1)\nverbs:\n  TOUCH:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> CHEAT()\n",
            &dict,
        ),
        r"recipe touch\.default: ROLLED is engine-written — author ROLL and the log records the concrete result \(§4\.4\)",
    );
    assert_err_msg(
        compile_script(
            "ops:\n  STEAL -> DRAWN(RUMOURS, HEARSAY)\nverbs:\n  TOUCH:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> STEAL()\n",
            &dict,
        ),
        r"recipe touch\.default: DRAWN is engine-written — author DRAW and the log records the concrete DRAWN\+SPAWN rows \(§4\.4\)",
    );
}

#[test]
fn the_event_trigger_ban_reaches_through_op_bodies() {
    let dict = core_dictionary();
    // expansion splices the op body into the trigger's list BEFORE the guard
    // sweep, so the ADVANCE ban sees through the call (a call is the
    // paren'd spelling — a bare name is SPAWN sugar and never a call)
    assert_err_msg(
        compile_script(
            "triggers:\n  dusk at tick 3 -> GO_ADV()\nops:\n  GO_ADV -> ADVANCE(1h)\n",
            &dict,
        ),
        "trigger dusk: ADVANCE belongs to verb recipes — events own no advance pass to stretch",
    );
}

#[test]
fn ending_and_run_checks_see_through_op_bodies() {
    let dict = core_dictionary();
    // ENDING membership: an undeclared id inside a body is the recipe's own
    // load error
    assert_err_msg(
        compile_script(
            "ops:\n  END_IT -> ENDING NOPE\nverbs:\n  TOUCH:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> END_IT()\n",
            &dict,
        ),
        r"recipe touch\.default: ENDING 'NOPE' is not in the endings table",
    );
    // RUN's parent-verb rule: the spliced body's candidates resolve against
    // the CALL SITE's verb — another verb's recipe is a load error ...
    assert_err_msg(
        compile_script(
            "verbs:\n  TOUCH:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> CHAIN()\n  READ:\n    actors: 1 kind:character\n    object: 1 kind:text\n    duration: 0m\n    recipes:\n      read.default -> NARRATE \"read.\"\nops:\n  CHAIN -> RUN \"read.default\"\n",
            &dict,
        ),
        r"recipe touch\.default: RUN 'read\.default' does not name a recipe of verb TOUCH — unknown and cross-verb ids are load errors \(§6\.7\)",
    );
    // ... while the same op, called from an `on complete` body, resolves
    // against the GLOBAL recipe table and loads fine (§6.7's exempt kind)
    compile_script(
        "entities:\n  HOUSE is place\n  ROWAN is character at HOUSE on complete -> ( CHAIN() )\nverbs:\n  READ:\n    actors: 1 kind:character\n    object: 1 kind:text\n    duration: 0m\n    recipes:\n      read.default -> NARRATE \"read.\"\nops:\n  CHAIN -> RUN \"read.default\"\n",
        &dict,
    )
    .unwrap();
}

#[test]
fn the_deck_check_sees_through_op_bodies() {
    let dict = core_dictionary();
    // DRAW's deck membership (§4.4): an undeclared deck id inside a body is
    // the call site's own load error — the recipe face ...
    assert_err_msg(
        compile_script(
            "ops:\n  DEAL -> DRAW NOWHERE\nverbs:\n  TOUCH:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DEAL()\n",
            &dict,
        ),
        r"recipe touch\.default: DRAW 'NOWHERE' is not a declared deck \(decks:\)",
    );
    // ... and the trigger face — the same post-expansion walk over both
    // homes (named-ref membership checks run on the expanded lists)
    assert_err_msg(
        compile_script(
            "triggers:\n  dusk at tick 3 -> DEAL()\nops:\n  DEAL -> DRAW NOWHERE\n",
            &dict,
        ),
        r"trigger dusk: DRAW 'NOWHERE' is not a declared deck \(decks:\)",
    );
}

#[test]
fn the_trigger_roll_lint_sees_through_op_bodies() {
    let dict = core_dictionary();
    // the §4.4 role-free ROLL rule for event triggers: the expanded list is
    // walked, so a slot trigger reaches a role-dependent ROLL through an op
    // body exactly as if authored at the trigger (the direct form is pinned
    // in tests/dice/dice.rs)
    assert_err_msg(
        compile_script(
            "ops:\n  GAMBLE -> ROLL(count(actors), 6)\ntriggers:\n  dusk at tick 3 -> GAMBLE()\n",
            &dict,
        ),
        r"trigger dusk: ROLL argument count\(actors\) reads a step role — trigger bindings are empty, it would always be 0; use count\(<query>\) \(e\.g\. count\(name:actors\)\)",
    );
}

#[test]
fn an_unknown_call_lists_declared_ops_in_the_vocabulary() {
    let dict = core_dictionary();
    assert_err_msg(
        compile_script(
            "ops:\n  PRESS(who) -> NARRATE \"{who}\"\n  BUMP -> NARRATE \"b\"\nverbs:\n  TOUCH:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> MYSTERY(1)\n",
            &dict,
        ),
        r"unknown 'MYSTERY\(\.\.\.\)' in outcome position — the outcome vocabulary is: SPAWN, DESTROY, REPLACE, SET, MOVE, REVEAL, ADVANCE, NARRATE, PROGRESS, RELATE, UNRELATE, REFUSE, ENDING, ROLL, ROLLED, DRAW, DRAWN, RUN; declared ops: PRESS, BUMP; no functions declared \(bare names spawn — an op's parens are part of its grammar; calls need a declared function or op\)",
    );
}

#[test]
fn an_entity_may_be_named_like_an_op_bare_stays_spawn_sugar() {
    // op names are deliberately NOT reserved: a bare `ENCHANT` in outcome
    // position is still SPAWN sugar — a PROTOTYPE legitimately bears the
    // name and the bare line mints from it; only `ENCHANT(x)` — name
    // followed by `(` — is the call
    let script = r#"
script: The Enchant Proof
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE

prototypes:
  ENCHANT is item stackable

ops:
  ENCHANT(who) -> NARRATE "{who} glows."

verbs:
  MINT:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ENCHANT
  GLOW:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> ENCHANT(ROWAN)
"#;
    let mut game = Game::from_script(script, core_dictionary()).unwrap();
    let t = auto_settle_line(&mut game, "mint").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    // the bare line minted from the prototype (the op would have narrated
    // instead — the pool would be empty and "glows." in the lines)
    assert_eq!(
        game.state.by_name["ENCHANT"].len(),
        1,
        "the bare name spawned, the op was not called"
    );
    assert!(!t.lines.iter().any(|l| l.contains("glows")), "{:?}", t.lines);
    let t2 = auto_settle_line(&mut game, "glow").unwrap();
    assert!(t2.lines.iter().any(|l| l.contains("ROWAN glows.")), "{:?}", t2.lines);
}

#[test]
fn a_zero_arg_op_referenced_bare_is_spawn_sugar_and_fails_like_spawn() {
    // the closing pin of the bare-sugar rule, for a ZERO-arg op: bare
    // `BUMP` is never a call — it is SPAWN sugar even with the op declared,
    // and with no entity or prototype named BUMP the sugar fails the way
    // any SPAWN of an unknown name does: the load is clean (mint names
    // resolve at runtime, §4.4) and the step refuses loudly at play. Only
    // `BUMP()` is the call.
    let script = r#"
script: The Bare Zero Proof
player: ROWAN
start: 0:0

entities:
  HOUSE is place
  ROWAN is character at HOUSE

ops:
  BUMP -> NARRATE "bump."

verbs:
  KNOCK:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> BUMP
  THUMP:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> BUMP()
"#;
    let mut game = Game::from_script(script, core_dictionary()).unwrap();
    // the bare face: SPAWN sugar of an undeclared name — the load is clean
    // and the sugar fails loudly at play, the SPAWN-shaped way: the window
    // refuses (§6.5's attempt-logged face), the op never ran
    let t = auto_settle_line(&mut game, "knock").unwrap();
    assert_eq!(t.kind, TurnKind::Action, "{:?}", t.lines);
    assert!(
        t.lines.iter().any(|l| l.contains("SPAWN BUMP: no kind (prototype or kind:NAME required)")),
        "{:?}",
        t.lines
    );
    assert!(!t.lines.iter().any(|l| l.contains("bump.")), "{:?}", t.lines);
    assert!(
        game.log.records.iter().all(|r| r.outcomes.iter().all(|o| o.op != "SPAWN")),
        "the sugar's spawn never landed: {:?}",
        game.log.records
    );
    // the paren'd face is the call
    let t2 = auto_settle_line(&mut game, "thump").unwrap();
    assert_eq!(t2.kind, TurnKind::Action, "{:?}", t2.lines);
    assert!(t2.lines.iter().any(|l| l.contains("bump.")), "{:?}", t2.lines);
}

#[test]
fn the_log_never_carries_an_op_name_and_the_fold_round_trips() {
    let mut game = Game::from_script(OP_SCRIPT, core_dictionary()).unwrap();
    // a step whose recipe calls an op with a ROLL inside: the richest
    // expansion the rig has — SET + two NARRATEs + ROLLED, all concrete rows
    auto_settle_line(&mut game, "dice").unwrap();
    let path = crate::common::rig::tmp_log_path("ops-jsonl");
    save_log(&path, &game.log.records).unwrap();
    let records = load_log(&path).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    let _ = std::fs::remove_file(&path);
    for name in ["LUCKY", "PRESS", "BUMP", "CHAIN", "MAYBE_BREAK", "FN("] {
        assert!(!raw.contains(name), "the log carries '{name}': {raw}");
    }
    // the fold replays the recorded rows against the same content and
    // dictionary and lands byte-identical — play ≡ fold by construction,
    // since the fold never sees a call either
    let replayed =
        fold(&records, &clone_state(&game.log.initial_state), &game.dict, &game.content).unwrap();
    assert_eq!(snap(&replayed), snap(&game.state));
    // and the session's own turn-boundary undo (truncate + re-fold) lands
    // back pristine — the settled turn may hold several engine rows
    while !game.log.records.is_empty() {
        game.undo().unwrap();
    }
    assert_eq!(game.log.records.len(), 0);
}

#[test]
fn compiling_the_same_ops_script_twice_is_idempotent_and_leaves_no_dictionary_residue() {
    let dict = core_dictionary();
    let first = compile_script(OP_SCRIPT, &dict).unwrap();
    let second = compile_script(OP_SCRIPT, &dict).unwrap();
    // ops live in the load-time tables only — nothing registers into the
    // dictionary, so both loads succeed and the base stays verbatim
    assert!(!dict.reserved.contains("PRESS"));
    assert!(!dict.reserved.contains("BUMP"));
    let third = compile_script(OP_SCRIPT, &first.dict).unwrap();
    assert!(!third.dict.reserved.contains("PRESS"));
    let _ = second;
}

#[test]
fn the_lint_sees_an_op_bodys_writes_and_lints_a_clean_ops_script_clean() {
    let dict = core_dictionary();
    // the aspect-read lint: the slot query reads aspect:pressure, the only
    // writer is the raw SET inside PRESS's declared body — counted, so no
    // "never declared" finding (the lint scans op bodies like fn bodies)
    let findings = lint_script_findings(OP_SCRIPT, &dict).unwrap();
    assert!(!findings.iter().any(|f| f.message.contains("aspect 'pressure'")), "{findings:?}");
    // and a script that only declares ops lints entirely clean
    let findings = lint_script_findings("ops:\n  BUMP -> NARRATE \"bump.\"\n", &dict).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn the_hour_alias_compiles_to_six_ticks_per_hour_and_tick_is_verbatim() {
    // §10's trigger grammar: `at hour N` is the wall-clock alias (slot 6N
    // under 144 ticks/day), `at tick N` carries its index verbatim
    let dict = core_dictionary();
    let compiled = compile_script(
        "script: Probe\nplayer: ROWAN\nstart: 0:0\n\nentities:\n  HOUSE is place\n  ROWAN is character at HOUSE\n\ntriggers:\n  dawn once at hour 4 -> NARRATE \"dawn.\"\n  tick16 once at tick 16 -> NARRATE \"tick.\"\n",
        &dict,
    )
    .unwrap();
    let slots: Vec<(&str, i64)> = compiled
        .content
        .triggers
        .iter()
        .filter_map(|t| match t {
            chorus::content::CompiledTrigger::Slot { id, slot, .. } => Some((id.as_str(), *slot)),
            _ => None,
        })
        .collect();
    assert_eq!(slots, [("dawn", 24), ("tick16", 16)]);
}
