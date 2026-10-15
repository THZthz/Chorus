//! `max N` — recipe execution limits (spec §9.50): the
//! positional script token and its keyword exclusion, the
//! `State.executed` map maintained by the ONE window-registration path
//! play and fold share, the selection gate (an exhausted recipe is simply
//! non-matching — the walk continues), table-refusal vs window-refusal
//! budgets, load errors, the fold rebuild and the suggestion gate.
//!
//! Conventions follow tests/decks.rs: a fresh dictionary per test, error
//! patterns copied CHARACTER FOR CHARACTER from the frozen literals, and
//! play/fold equality asserted through the shared `snap`.

mod common;

use common::harness::assert_err_msg;
use common::settle::{act, done, refusal, settle_step};
use common::snap::snap;

use chorus::content::content::{
    compile_content, CompiledContent, ContentRaw, RecipeDef, SlotDef, VerbContent,
};
use chorus::content::script::compile_script;
use chorus::core::clock::Clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::types::clone_state;
use chorus::engine::engine::{ExecResult, Step};
use chorus::engine::fold::fold;
use chorus::session::game::{Game, TurnKind};

// ---------------------------------------------------------------- rigs ------

/// The file-level dict registers CLEAR (the
/// budget-unlock verb; verbs are code-registered, §10). Per-test fresh.
fn max_dict() -> Dictionary {
    let mut d = core_dictionary();
    if !d.is_verb("CLEAR") {
        d.register_verb("CLEAR").unwrap();
    }
    d
}

/// The rig: ANA at HOUSE, a stackable TRINKET prototype, and the caller's
/// verbs block. TRINKET exists so a recipe can SPAWN and `exists()` it.
fn rig(verbs: &str, entities: Option<&str>) -> String {
    let entities = entities.unwrap_or("");
    format!(
        "
script: Max Rig
player: ANA
start: 0:0
seed: 0

entities:
  ANA is character at HOUSE
  HOUSE is place
{entities}

prototypes:
  TRINKET is idea stackable

verbs:
{verbs}
"
    )
}

fn compile(dict: &Dictionary, text: &str) -> (CompiledContent, chorus::core::types::State) {
    let compiled = compile_script(text, dict).unwrap();
    (compiled.content, compiled.state)
}

/// Parse + settle_step with the decks.rs
/// id-generator shape: `x{first_record_id}_{n}`.
fn run(
    content: &CompiledContent,
    dict: &Dictionary,
    state: chorus::core::types::State,
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

/// The BEGIN row's commit-time recipe ids, clause order.
fn begin_recipes(r: &ExecResult) -> Vec<String> {
    act(r)
        .map(|rec| rec.recipes.clone())
        .unwrap_or_default()
}

/// The execution map as entry pairs (insertion order observable).
fn executed_entries(s: &chorus::core::types::State) -> Vec<(String, i64)> {
    s.executed.iter().map(|(k, v)| (k.clone(), *v)).collect()
}

// ------------------------------------------------------------- the gate -----

/// A `max 1` recipe runs once; the second selection attempt skips it and
/// the walk continues into the next matching recipe (§9.50).
#[test]
fn max_one_runs_once_then_the_walk_skips_it() {
    let dict = max_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      first max 1 when count(actors) >= 1 -> NARRATE \"one\"\n      default -> NARRATE \"rest\"",
            None,
        ),
    );
    // first run: the budgeted recipe fires and its window registers
    let r1 = run(&c, &dict, s, "0:0 @ ANA WAIT", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(begin_recipes(&r1), ["first"]);
    assert_eq!(done(&r1).unwrap().outcomes[0].op, "NARRATE");
    assert_eq!(executed_entries(&r1.state), vec![("first".to_string(), 1)]);
    // second run: `first` is spent — the walk continues into the default
    let r2 = run(&c, &dict, r1.state, "0:0 @ ANA WAIT", 10);
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), ["wait.default"]);
    assert_eq!(
        executed_entries(&r2.state),
        vec![("first".to_string(), 1), ("wait.default".to_string(), 1)]
    );
}

/// A table-refused attempt opens no window and counts nothing (§9.50): the
/// gate recipe stays selectable, and once its `when` stops matching the
/// retry succeeds.
#[test]
fn a_table_refused_attempt_consumes_nothing() {
    let dict = max_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      gate max 1 when exists(LOCK) -> REFUSE(\"blocked\")\n      default -> NARRATE \"rest\"\n  CLEAR:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      default -> DESTROY LOCK, NARRATE \"cleared\"",
            Some("\n  LOCK is item at HOUSE"),
        ),
    );
    // attempt 1: the gate refuses AT THE TABLE (§6.5) — attempt logged,
    // zero time, no window, no count
    let r1 = run(&c, &dict, s, "0:0 @ ANA WAIT", 1);
    assert_eq!(r1.rejected, None);
    assert_eq!(refusal(&r1).as_deref(), Some("blocked"));
    assert_eq!(act(&r1).unwrap().advanced_min, 0); // §6.5: the refusal collapses the cycle
    assert_eq!(executed_entries(&r1.state), vec![]);
    // attempt 2: the budget was NOT spent — the gate is selected again
    // (a spent gate would have fallen through to the default instead)
    let r2 = run(&c, &dict, r1.state, "0:0 @ ANA WAIT", 10);
    assert_eq!(r2.rejected, None);
    assert_eq!(refusal(&r2).as_deref(), Some("blocked"));
    assert_eq!(executed_entries(&r2.state), vec![]);
    // clear the lock, then the retry succeeds through the default recipe
    let r3 = run(&c, &dict, r2.state, "0:0 @ ANA CLEAR", 20);
    assert_eq!(r3.rejected, None);
    let r4 = run(&c, &dict, r3.state, "0:0 @ ANA WAIT", 40);
    assert_eq!(r4.rejected, None);
    assert_eq!(refusal(&r4), None);
    assert_eq!(begin_recipes(&r4), ["wait.default"]);
    assert!(!r4.state.executed.contains_key("gate"));
    assert_eq!(r4.state.executed.get("wait.default"), Some(&1));
}

/// A window that begins then refuses at completion DOES consume the budget
/// — it registered at begin (§9.50). The REFUSE that only decides inside an
/// IF branch belongs to the completion (§6.7); the partial effects are
/// discarded, the window closes, the count stays.
#[test]
fn a_window_that_refuses_at_completion_still_counted() {
    let dict = max_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      default -> SPAWN TRINKET, IF exists(TRINKET) -> (REFUSE(\"too late\"))",
            None,
        ),
    );
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA WAIT", 1);
    assert_eq!(r.rejected, None, "the BEGIN succeeds — the refusal is deferred");
    let complete = done(&r).unwrap();
    let ops: Vec<&str> = complete.outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["SPAWN", "REFUSE"]);
    assert_eq!(refusal(&r).as_deref(), Some("too late"));
    // the window refused — no TRINKET survives the discarded draft…
    assert!(!r.state.entities.values().any(|e| e.name == "TRINKET"));
    // …but the registration at begin already counted
    assert_eq!(executed_entries(&r.state), vec![("wait.default".to_string(), 1)]);
    // play and fold agree through the same registration path
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
}

/// Fold replay of a budget-consuming log rebuilds `State.executed`
/// identically (the §9.29 fired precedent — selection reads only the state
/// map, never the log), and a subsequent play step FROM the folded state
/// gates the same way.
#[test]
fn fold_replay_rebuilds_executed_and_the_gate_still_gates() {
    let dict = max_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      first max 1 when count(actors) >= 1 -> NARRATE \"one\"\n      default -> NARRATE \"rest\"",
            None,
        ),
    );
    let base = clone_state(&s);
    let r = run(&c, &dict, s, "0:0 @ ANA WAIT", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(begin_recipes(&r), ["first"]);
    let replayed = fold(&r.records, &base, &dict, &c).unwrap();
    assert_eq!(snap(&replayed), snap(&r.state));
    assert_eq!(executed_entries(&replayed), vec![("first".to_string(), 1)]);
    // a subsequent play step from the FOLDED state gates identically
    let r2 = run(&c, &dict, replayed, "0:0 @ ANA WAIT", 10);
    assert_eq!(r2.rejected, None);
    assert_eq!(begin_recipes(&r2), ["wait.default"]);
}

/// `max N` parses positionally; `max` is a KEYWORD, never a leading recipe
/// id (§9.50 — §9.48 will add `aspects` to the same exclusion). N ≥ 1: a
/// `max 0` or negative is a load error, in the script and in hand-built
/// content alike.
#[test]
fn max_zero_and_negative_are_load_errors_and_max_is_a_keyword_not_an_id() {
    let dict = max_dict();
    let verbs_max = |recipes: &str| {
        rig(
            &format!(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n{recipes}"
            ),
            None,
        )
    };
    assert_err_msg(
        compile_script(&verbs_max("      first max 0 -> NARRATE \"x\""), &dict),
        "`max 0` — N must be an integer ≥ 1",
    );
    assert_err_msg(
        compile_script(&verbs_max("      first max -2 -> NARRATE \"x\""), &dict),
        "`max -2` — N must be an integer ≥ 1",
    );
    assert_err_msg(
        compile_script(&verbs_max("      first max -> NARRATE \"x\""), &dict),
        "`max` needs an integer N ≥ 1",
    );
    // the keyword: a leading `max N` parses as the limit, not an id — the
    // id-less line mints the default id
    let (c, _) = compile(
        &dict,
        &verbs_max("      watched hidden max 2 when count(actors) >= 1 -> NARRATE \"m\"\n      max 1 -> NARRATE \"once\""),
    );
    let recipes = &c.verbs.get("WAIT").unwrap().recipes;
    assert_eq!(recipes[0].id, "watched");
    assert_eq!(recipes[0].max, Some(2));
    assert!(!recipes[0].shown);
    assert_eq!(recipes[1].id, "wait.default"); // `max` was never an id candidate
    assert_eq!(recipes[1].max, Some(1));
    // the code-level face fails just as loud (the slot max<min precedent)
    let raw = ContentRaw {
        verbs: [(
            "WAIT".to_string(),
            VerbContent {
                slots: vec![SlotDef {
                    role: "actors".into(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".into()),
                    shown: None,
                }],
                duration: 0,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: Some(0),
                    aspects: Vec::new(),
                    out: vec!["NARRATE \"x\"".to_string()],
                }],
            },
        )]
        .into_iter()
        .collect(),
        ..ContentRaw::default()
    };
    assert_err_msg(
        compile_content(&raw, &dict, &[]),
        "verb WAIT: recipe wait.default has max < 1",
    );
}

/// The positional walk never drops a token: `hidden|shown` and `max N`
/// compose around the regex-extracted `hint "…"`/`when COND` clauses in any
/// order, and a stray the walk cannot own is a load error naming the line
/// (the review pin — `first max 2 hidden` used to discard the `hidden`).
#[test]
fn the_positional_walk_honors_every_token_and_errors_on_strays() {
    let dict = max_dict();
    let verbs_max = |recipes: &str| {
        rig(
            &format!(
                "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n{recipes}"
            ),
            None,
        )
    };
    // the reviewer's trap line, now parsed whole: max AND hidden both land
    let (c, _) = compile(
        &dict,
        &verbs_max("      first max 2 hidden when count(actors) >= 1 -> NARRATE \"x\""),
    );
    let rs = &c.verbs.get("WAIT").unwrap().recipes;
    assert_eq!(rs[0].id, "first");
    assert_eq!(rs[0].max, Some(2));
    assert!(!rs[0].shown);
    assert!(rs[0].when.is_some());
    // …and the same line with the hint clause interleaved
    let (c2, _) = compile(
        &dict,
        &verbs_max("      second hidden hint \"twice\" max 3 when count(actors) >= 1 -> NARRATE \"x\""),
    );
    let rs2 = &c2.verbs.get("WAIT").unwrap().recipes;
    assert_eq!(rs2[0].id, "second");
    assert_eq!(rs2[0].max, Some(3));
    assert!(!rs2[0].shown);
    assert_eq!(rs2[0].hint.as_deref(), Some("twice"));
    // a stray the walk cannot own is a load error naming the line
    assert_err_msg(
        compile_script(
            &verbs_max("      first max 2 junk when count(actors) >= 1 -> NARRATE \"x\""),
            &dict,
        ),
        "line 20: unexpected 'junk' before '->'",
    );
    // an unquoted hint never matches the hint clause — the dangling keyword
    // is a load error too
    assert_err_msg(
        compile_script(
            &verbs_max("      first hint x when count(actors) >= 1 -> NARRATE \"x\""),
            &dict,
        ),
        "line 20: unexpected 'hint' before '->'",
    );
}

/// The CS shape (25 × max 1, 1 × max 3): three runs fine, the fourth skipped.
#[test]
fn max_three_runs_three_then_skips() {
    let dict = max_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      first max 3 when count(actors) >= 1 -> NARRATE \"counting\"\n      default -> NARRATE \"rest\"",
            None,
        ),
    );
    let mut state = s;
    let mut id = 1i64;
    let mut begins: Vec<Vec<String>> = Vec::new();
    for _ in 0..4 {
        let r = run(&c, &dict, state, "0:0 @ ANA WAIT", id);
        assert_eq!(r.rejected, None);
        begins.push(begin_recipes(&r));
        id += r.records.len() as i64;
        state = r.state;
    }
    assert_eq!(begins[0], ["first"]);
    assert_eq!(begins[1], ["first"]);
    assert_eq!(begins[2], ["first"]);
    assert_eq!(begins[3], ["wait.default"]);
    assert_eq!(
        executed_entries(&state),
        vec![("first".to_string(), 3), ("wait.default".to_string(), 1)]
    );
}

/// A spent recipe stops being suggested (§9.50's hintonly/shown note): the
/// suggestion walk applies the same gate selection does, so the binding that
/// lit the spent recipe drops out of the story suggestions (a lit verb ranks
/// before a flavor-only one).
#[test]
fn a_spent_recipe_stops_being_suggested() {
    let mut dict = max_dict();
    if !dict.is_verb("SHOW") {
        dict.register_verb("SHOW").unwrap();
    }
    let mut game = Game::from_script(
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      hinted max 1 when count(actors) >= 1 -> NARRATE \"hint\"\n      default -> NARRATE \"rest\"\n  SHOW:\n    actors: 1 kind:character\n    duration: 0m\n    recipes:\n      always when count(actors) >= 1 -> NARRATE \"always\"",
            None,
        ),
        dict,
    )
    .unwrap();
    // both verbs light a non-default recipe — story rank, in verb order
    assert_eq!(game.suggestions().unwrap(), vec!["wait", "show"]);
    assert_eq!(game.handle_line("wait").unwrap().kind, TurnKind::Action);
    game.wait().unwrap(); // v0.5: the count lands when the window registers
    assert_eq!(game.state.executed.get("hinted"), Some(&1));
    // `hinted` is spent — WAIT falls back to flavor rank, below the lit SHOW
    assert_eq!(game.suggestions().unwrap(), vec!["show", "wait"]);
}

/// The count rides the ONE registration path: a BEGIN whose window is still
/// OPEN has already spent the budget — registration at begin, completion
/// later (§9.50: a window that later refuses still counted).
#[test]
fn registration_at_begin_counts_while_the_window_is_still_open() {
    use chorus::engine::engine::{execute_step, ExecOptions};
    let dict = max_dict();
    let (c, s) = compile(
        &dict,
        &rig(
            "  WAIT:\n    actors: 1 kind:character\n    duration: 1h\n    recipes:\n      first max 1 when count(actors) >= 1 -> NARRATE \"one\"",
            None,
        ),
    );
    let parsed = parse_sentence("0:0 @ ANA WAIT", &dict).unwrap();
    let mut n: i64 = 0;
    let mut id_gen = || {
        n += 1;
        format!("x1_{n}")
    };
    let r = execute_step(
        &c,
        &dict,
        s,
        Step {
            start_min: None,
            clock: Some(Clock { day: 0, halfhour: 0 }),
            clauses: parsed.clauses,
            t_real: None,
        },
        ExecOptions {
            first_record_id: 1,
            id_gen: Some(&mut id_gen),
        },
    )
    .unwrap();
    assert_eq!(r.rejected, None);
    // the window is OPEN (nothing settled) and the budget is already spent
    assert_eq!(r.state.windows.len(), 1);
    assert_eq!(r.state.executed.get("first"), Some(&1));
}
