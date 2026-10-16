//! The Bellwood Aspirant (scripts/cultist-slice.txt) — script-driven
//! acceptance for the v0.6 CS slice. The script text is DATA: embedded with
//! include_str! and compiled fresh per test (compile_script is pure, so a
//! fresh compile IS the pristine pre-play state).
//!
//! E1 lands here: the slice's `acquire.broke_gate`
//! (`actors.attr.silver < object.aspect.cost`) is the finding's casualty —
//! WREN mints with an EMPTY attr map (script entities mint no attrs), so
//! before the fix the missing purse read false and the candle recipe sold
//! her one silver into debt.

mod common;

use common::settle::{act, done, refusal, settle_step};

use chorus::content::script::compile_script;
use chorus::core::clock::Clock;
use chorus::core::dict::core_dictionary;
use chorus::core::parse::parse_sentence;
use chorus::core::types::AttrValue;
use chorus::engine::engine::{ExecResult, Step};
use chorus::session::game::{Game, TurnKind};

/// The bundled v0.6 slice, read as text.
const SLICE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/scripts/cultist-slice.txt"
));

/// settle_step with the engine's id-generator shape (fresh `x{n}` ids).
fn run(
    content: &chorus::content::content::CompiledContent,
    dict: &chorus::core::dict::Dictionary,
    state: chorus::core::types::State,
    src: &str,
) -> ExecResult {
    let s = parse_sentence(src, dict).unwrap();
    let mut n: i64 = 0;
    let mut id_gen = || {
        n += 1;
        format!("x{n}")
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
        1,
        Some(&mut id_gen),
    )
    .unwrap()
}

/// A purse-less first-time buyer is REFUSED at the trestles, not sold into
/// debt: the missing silver attr reads 0 against the candle's cost-1, the
/// gate speaks, the purse never grows a negative entry, and the candle
/// stays on the stall.
#[test]
fn the_broke_gate_refuses_a_first_time_buyer_instead_of_selling_her_into_debt() {
    let mut game = Game::from_script(SLICE, core_dictionary()).unwrap();
    let wren = &game.state.by_name["WREN"][0].clone();
    let candle = &game.state.by_name["THE_TIN_CANDLE"][0].clone();
    // the finding's exact shape: the entry is ABSENT, not zeroed
    assert_eq!(game.state.entities[wren.as_str()].attrs.get("silver"), None);

    let turn = game
        .handle_line("acquire the tin candle at the flea market")
        .unwrap();
    assert_eq!(turn.kind, TurnKind::Action);
    let lines = turn.lines.join("\n");
    assert!(
        lines.contains("Silver first"),
        "the broke gate must speak for an empty purse: {lines}"
    );

    // the refusal, not a negative-silver purchase: no purse entry was
    // minted and the stock never moved
    assert_eq!(game.state.entities[wren.as_str()].attrs.get("silver"), None);
    assert_eq!(
        game.state.entities[candle.as_str()].attrs.get("container"),
        Some(&AttrValue::Str("s_the_flea_market".into()))
    );
}

/// The same gate with a PRESENT purse behaves exactly as authored: one
/// silver buys the candle down to 0 and the stock moves into the buyer's
/// pocket (the fix is corrective for `<` only).
#[test]
fn a_present_silver_purse_still_buys_the_candle() {
    let dict = core_dictionary();
    let compiled = compile_script(SLICE, &dict).unwrap();
    let mut state = compiled.state;
    // fund the purse: a PRESENT entry, the pre-fix and post-fix reads equal
    let wren = state.by_name["WREN"][0].clone();
    state
        .entities
        .get_mut(wren.as_str())
        .unwrap()
        .attrs
        .insert("silver".to_string(), AttrValue::Num(1));

    let r = run(&compiled.content, &dict, state, "0:16 @ WREN ACQUIRE THE_TIN_CANDLE AT THE_FLEA_MARKET");
    assert_eq!(r.rejected, None, "a funded purse passes the broke gate");
    assert_eq!(refusal(&r), None);
    assert_eq!(act(&r).unwrap().recipes, ["acquire.candle"]);
    let ops: Vec<&str> = done(&r).unwrap().outcomes.iter().map(|o| o.op.as_str()).collect();
    assert_eq!(ops, ["SET", "MOVE", "NARRATE"]);

    let wren = r.state.by_name["WREN"][0].clone();
    let candle = r.state.by_name["THE_TIN_CANDLE"][0].clone();
    assert_eq!(
        r.state.entities[wren.as_str()].attrs.get("silver"),
        Some(&AttrValue::Num(0))
    );
    assert_eq!(
        r.state.entities[candle.as_str()].attrs.get("container"),
        Some(&AttrValue::Str("s_wren".into()))
    );
}
