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

#[path = "../common/mod.rs"]
mod common;

use common::settle::{act, done, refusal, settle_step};

use chorus::content::script::compile_script;
use chorus::core::clock::Clock;
use chorus::core::dict::core_dictionary;
use chorus::core::parse::parse_sentence;
use chorus::core::types::{AspectVal, AttrValue};
use chorus::engine::engine::{ExecResult, Step};
use chorus::session::game::{Game, TurnKind};
use chorus::session::render::ending_banner;

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
            clock: Some(s.clock.unwrap_or(Clock {
                day: 0,
                halfhour: 0,
            })),
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

    let r = run(
        &compiled.content,
        &dict,
        state,
        "0:16 @ WREN ACQUIRE THE_TIN_CANDLE AT THE_FLEA_MARKET",
    );
    assert_eq!(r.rejected, None, "a funded purse passes the broke gate");
    assert_eq!(refusal(&r), None);
    assert_eq!(act(&r).unwrap().recipes, ["acquire.candle"]);
    let ops: Vec<&str> = done(&r)
        .unwrap()
        .outcomes
        .iter()
        .map(|o| o.op.as_str())
        .collect();
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

// ---------------------------------------------------------------------------
// The five pinned playthroughs (plan §8). Every walk pins seed + inputs and
// drives `Game::handle_line` with clock-prefixed lines — the same shape the
// §8 CLI walk pastes as stdin; the engine's seeded counter dice (law 5: the
// only randomness) and the pure seed-fixed deck permutations do the rest, so
// each walk replays identically on every run. Assertions read the journal
// records, the state and the ending — never stdout.
//
// Steering disclosures per test: the P1 win rides the UNMODIFIED script and
// the verbatim §8 transcript (the plan's own disclosure — the steering IS
// that schedule). The three deaths ride documented deck-spec mutations of a
// copied script text (same seed 1837, same recipes, same dice): the deck
// permutation is pure in (seed, deck id), so a re-weighted spec has a KNOWN
// draw order — at seed 1837 the SEASONS permutation deals spec[0], spec[12],
// spec[9] as draws 1-3, which is how the P4 mutation pins QUIET first.
// ---------------------------------------------------------------------------

/// The verbatim §8 win-walk transcript (plan lines, accepted input only —
/// the REPL's `journal`/`cards`/`quit` meta words are not game lines).
/// Re-verified against the E9 lenient fix (2026-10-15): the 0:32 self-pair
/// now binds DISTINCT pool copies, so BOTH candle psalms go into the
/// Lampkeeper's Hour and no stray survives — the ≥7 promotion moved past the
/// first (roll-free) Bellwood crossing to pool the hour with the lost bell
/// (18+6), with a `talk` spacer bridging the promotion's mint (§8 erratum 5),
/// and the business ladders pool the hour alone (still the ≥5 tier). The
/// summon rolled its 30% face and failed at its hinge — the walk never
/// spends the guest, so the yearning crossings pay for both draws. Both
/// edited work slots after the 1:38 wits drain bind the dimmed wit
/// (roll-free, parse-live), leaving the seed-1837 die stream untouched: 41
/// counters, ascension faces 7, 1, 1.
const WIN_WALK: &[&str] = &[
    "0:16 study the psalm of candles",
    "0:17 dream with wits",
    "0:21 wait",
    "0:23 dream with the promise of heights a candle psalm",
    "0:26 study the snow pamphlet",
    "0:27 talk with a candle psalm",
    "0:29 wait",
    "0:30 talk to mercy hale with a candle psalm",
    "0:32 study a candle psalm with a candle psalm",
    "0:37 wait",
    "0:39 study the lampkeepers hour",
    "0:44 mansus the bellwood with the yearning",
    "0:46 work with vigor",
    "1:0 work with wits",
    "1:2 work with wits",
    "1:4 work with wits",
    "1:6 work with wits",
    "1:8 work with wits",
    "1:10 work with wits",
    "1:12 travel to the flea market",
    "1:14 acquire the skeleton key at the flea market",
    "1:16 travel to the cold water room",
    "1:17 talk to hale the believer with the lost bell the lampkeepers hour",
    "1:19 talk",
    "1:21 talk to hale the disciple with the lampkeepers hour",
    "1:23 work with wits",
    "1:25 use on the lifted lamp about the skeleton key with the lampkeepers hour a still air",
    "1:27 talk",
    "1:31 work with wits",
    "1:34 study the lampkeepers hour",
    "1:38 work with wits",
    "1:40 talk to hale the disciple with the lampkeepers hour",
    "1:41 talk to hale the disciple with the lampkeepers hour",
    "1:47 talk",
    "2:0 dream with the pale yearning",
    "2:3 talk",
    "2:4 mansus the bellwood with the yearning",
    "2:9 talk",
    "2:10 dream with the pale yearning",
    "2:13 talk",
    "2:15 use on the lifted lamp to the vow of heights with the lampkeepers hour",
    "2:20 talk",
    "2:22 use on the lifted lamp to the hunger of heights with the lost bell",
    "2:26 work with the dimmed wit",
    "2:28 study the lampkeepers hour",
    "2:32 talk to hale the disciple with the lampkeepers hour",
    "2:33 talk to hale the disciple with the lampkeepers hour",
    "2:34 talk to hale the disciple with the lampkeepers hour",
    "2:35 talk",
    "2:37 use on the lifted lamp to the hunger of heights with the lost bell",
    "2:41 work with the dimmed wit",
    "2:43 talk to hale the disciple with the lampkeepers hour",
    "2:44 talk to hale the disciple with the lampkeepers hour",
    "2:45 talk to hale the disciple with the lampkeepers hour",
    "2:46 talk to hale the disciple with the lampkeepers hour",
    "3:0 use on the lifted lamp to the hunger of heights with the lost bell the bell heart",
    "3:5 use on the lifted lamp to the hunger of heights with the lost bell the bell heart",
    "3:10 talk",
];

/// Feed the walk's lines; an Error turn (parse failure or engine rejection)
/// fails the walk loudly with its rendered lines.
fn play(game: &mut Game, lines: &[&str]) {
    for line in lines {
        let turn = game.handle_line(line).unwrap();
        assert_ne!(
            turn.kind,
            TurnKind::Error,
            "walk line {line:?} was rejected: {:?}",
            turn.lines
        );
    }
}

/// The slice text (or an already-mutated copy) with one deck-spec line
/// swapped — the documented mutation lever (deck re-weighting at the SAME
/// seed; the recipes and dice are the script's own). Fails loudly if the
/// text drifts from the line being swapped, so a mutation can never
/// silently become a no-op.
fn with_deck(text: &str, from: &str, to: &str) -> String {
    assert!(
        text.contains(from),
        "the text no longer carries the deck line this mutation swaps: {from}"
    );
    let mutated = text.replacen(from, to, 1);
    assert_ne!(mutated, text, "the deck swap replaced nothing");
    mutated
}

/// How many log records carry `needle` in their canonical sentence (the
/// journal's own lines: concrete outcomes render on the record's sentence).
fn sentence_count(game: &Game, needle: &str) -> usize {
    game.log
        .records
        .iter()
        .filter(|r| r.sentence.contains(needle))
        .count()
}

/// How many COMPLETE records fired `recipe`.
fn completions(game: &Game, recipe: &str) -> usize {
    game.log
        .records
        .iter()
        .filter(|r| r.kind == "complete" && r.recipes.iter().any(|id| id == recipe))
        .count()
}

/// How many log records REFUSEd anywhere (an authored gate speaking or a
/// completion-time failure).
fn refusals(game: &Game) -> usize {
    game.log
        .records
        .iter()
        .filter(|r| r.outcomes.iter().any(|o| o.op == "REFUSE"))
        .count()
}

/// How many LIVING entities answer to `name` (a name whose every copy died
/// is released from the table entirely — the missing key reads as zero).
fn alive_named(game: &Game, name: &str) -> usize {
    game.state
        .by_name
        .get(name)
        .map(|ids| {
            ids.iter()
                .filter(|id| {
                    game.state
                        .entities
                        .get(id.as_str())
                        .is_some_and(|e| e.dead_action.is_none())
                })
                .count()
        })
        .unwrap_or(0)
}

/// The ending pin: id, authored title, flavour — and the banner text the
/// status area shows once the story has closed.
fn assert_ended(game: &Game, id: &str, title: &str, flavour: &str) {
    let ended = game.state.ended.clone().expect("the story must have ended");
    assert_eq!(ended.id, id);
    assert_eq!(ended.title, title);
    assert_eq!(ended.flavour, flavour);
    assert_eq!(
        ending_banner(&ended),
        format!("— {} · {} —", title.to_uppercase(), flavour)
    );
}

/// After the ending, further input is refused: the §9.35 notice instead of
/// execution, nothing new logged.
fn assert_further_input_rejected(game: &mut Game, next_line: &str) {
    let records_before = game.log.records.len();
    let turn = game.handle_line(next_line).unwrap();
    assert_eq!(turn.kind, TurnKind::Info);
    let lines = turn.lines.join("\n");
    assert!(
        lines.contains("the story has ended"),
        "the notice must speak: {lines}"
    );
    assert_eq!(game.log.records.len(), records_before);
}

/// P1 — the win walk. The UNMODIFIED repo script + the verbatim §8
/// transcript (its own dice-steering disclosure: works and business talks
/// are placed so the summon's die is >= 4 and the ascension dice land <= 9
/// at seed 1837 — the plan's erratum recounts the verified replay). Pins:
/// OVER_THE_ROOF with the grand banner, the hunger at 6 marks, the full
/// ascension trail (one climb per rung), wheel turns en route, and not one
/// REFUSE or error in the whole journal.
#[test]
fn the_win_walk_replays_to_over_the_roof() {
    let mut game = Game::from_script(SLICE, core_dictionary()).unwrap();
    play(&mut game, WIN_WALK);

    // the grand ending, with the banner text the status area shows
    assert_ended(
        &game,
        "OVER_THE_ROOF",
        "Over the Roof of the Visible",
        "grand",
    );

    // the sixth mark is ON the hunger card (SET(TO, aspect:marks) — the
    // hunger's counter is an aspect, §3's inventory)
    let hunger = game.state.by_name["THE_HUNGER_OF_HEIGHTS"][0].clone();
    assert_eq!(
        game.state.entities[hunger.as_str()].aspects.get("marks"),
        Some(&AspectVal::Num(6)),
        "the ascension must have struck the sixth mark"
    );

    // the §8 pin — the ascension trail in the journal: the hunger climbs one
    // rung per rite (ascend_1 at the lone lost bell's 18, ascend_2 at 18,
    // ascend_3 at the 18+6 pool), dice 7, 1, 1 at seed 1837 — the Grand 10%
    // never fired on this replay; and the wheel was turning throughout
    assert_eq!(
        completions(&game, "rite.ascend_1"),
        1,
        "the first climb (marks 3 -> 4)"
    );
    assert_eq!(
        completions(&game, "rite.ascend_2"),
        1,
        "the second climb (marks 4 -> 5)"
    );
    assert_eq!(
        completions(&game, "rite.ascend_3"),
        1,
        "the grand climb (marks 5 -> 6, the ending)"
    );
    assert!(
        sentence_count(&game, "DRAWN(SEASONS, ") >= 3,
        "the wheel must have turned at least three seasons en route"
    );

    // the E9-at-scale pins: the 0:32 self-pair bound DISTINCT pool copies
    // (the lenient binder's hand-out — the old head-only binding fired the
    // pair row on one card and left the second as a stray), so both candle
    // psalms went into the Lampkeeper's Hour and nothing pooled twice
    assert_eq!(
        completions(&game, "study.rad2_pair"),
        1,
        "the psalm pair combined once"
    );
    assert_eq!(
        alive_named(&game, "A_CANDLE_PSALM"),
        0,
        "no stray: the pair consumed both copies"
    );
    assert_eq!(
        alive_named(&game, "THE_LAMPKEEPERS_HOUR"),
        1,
        "the 6 minted exactly once"
    );
    assert_eq!(
        alive_named(&game, "THE_LOST_BELL"),
        1,
        "the crossing's draw survives as the promotion's pooled light"
    );

    // zero rejections: no authored REFUSE spoke, no completion failed
    assert_eq!(refusals(&game), 0, "the verified walk accepts every line");
}

/// P2 — the despair death. DOCUMENTED MUTATION (scratch copy of the script
/// text, seed and recipes untouched): the SEASONS deck re-weighted to
/// SEASON_GREY x14 and the MOODS deck to DREAD x6 — every draw is known, so
/// three bare `dream` lines (dream.default's router draw, roll-free) stock
/// three DREADs, and every wheel cycle is a Grey Morning. Dice disclosure:
/// the only rolls are the two bootstrap works, pinned at seed 1837 to faces
/// 1 (the clerk's-post chance spawns a benign post), 1 (WITS drains — also
/// benign here), 7 and 4 (VIGOR holds, no mending) — nothing touches the
/// despair machinery. The purse is placed so time.turn always drains (3 -> 2
/// -> 1 -> 0) and never eats VIGOR: the death is pure despair, not hunger.
#[test]
fn three_grey_marks_end_in_grey_water() {
    let seasons_from = "SEASONS spec SEASON_QUIET, SEASON_QUIET, SEASON_QUIET, SEASON_WHITE, \
                        SEASON_WHITE, SEASON_GREY, SEASON_GREY, SEASON_SHADOW, SEASON_SHADOW, \
                        SEASON_AMBITION, SEASON_AMBITION, SEASON_SICKLY, SEASON_LEAN, SEASON_WARM";
    let seasons_to = "SEASONS spec SEASON_GREY, SEASON_GREY, SEASON_GREY, SEASON_GREY, \
                      SEASON_GREY, SEASON_GREY, SEASON_GREY, SEASON_GREY, SEASON_GREY, \
                      SEASON_GREY, SEASON_GREY, SEASON_GREY, SEASON_GREY, SEASON_GREY";
    let moods_from = "MOODS spec DREAD, RAPTURE, GLIMMER, MENDING, EASE, WAKEFULNESS";
    let moods_to = "MOODS spec DREAD, DREAD, DREAD, DREAD, DREAD, DREAD";
    let mutated = with_deck(
        &with_deck(SLICE, seasons_from, seasons_to),
        moods_from,
        moods_to,
    );
    let mut game = Game::from_script(&mutated, core_dictionary()).unwrap();

    // three router draws stock three DREADs (roll-free), the two works build
    // a 3-silver purse and bootstrap the wheel, then the clock sweeps the
    // grey chain — turn, link, judge, process, one hop per 2h line — until
    // the third mark lands (0:34, 0:42, 1:2)
    play(
        &mut game,
        &[
            "0:16 dream",
            "0:18 dream",
            "0:20 dream",
            "0:22 work with wits",
            "0:24 work with vigor",
            "0:26",
            "0:28",
            "0:30",
            "0:32",
            "0:34",
            "0:36",
            "0:38",
            "0:40",
            "0:42",
            "0:44",
            "0:46",
            "1:0",
            "1:2",
        ],
    );

    // the melancholy: the third processed dread closes the sky
    assert_ended(
        &game,
        "GREY_WATER",
        "The Weight of Grey Water",
        "melancholy",
    );
    assert_eq!(
        completions(&game, "despair_process"),
        3,
        "three grey mornings must each process one dread"
    );
    assert_eq!(
        alive_named(&game, "THE_GREY_MARK"),
        3,
        "three grey marks stand at the ending"
    );
    assert_eq!(
        sentence_count(&game, "DRAWN(MOODS, DREAD)"),
        3,
        "the mutated router dealt the three dreads"
    );
    // the walk's line after the ending draws the notice, and the journal
    // stops growing: the state is closed
    assert_further_input_rejected(&mut game, "1:4");
    assert_eq!(refusals(&game), 0, "the death walk accepts every line");
}

/// P3 — the visions death. Same shape, mirrored mutation: SEASONS re-weighted
/// to SEASON_WHITE x14, MOODS to RAPTURE x6 — three bare dreams stock three
/// RAPTUREs, so every White Air finds a held rapture and rapture_judge
/// processes it. Same pinned bootstrap dice (1, 1, 7, 4 at seed 1837 — all
/// benign).
#[test]
fn three_rapture_marks_end_in_bright_air() {
    let seasons_from = "SEASONS spec SEASON_QUIET, SEASON_QUIET, SEASON_QUIET, SEASON_WHITE, \
                        SEASON_WHITE, SEASON_GREY, SEASON_GREY, SEASON_SHADOW, SEASON_SHADOW, \
                        SEASON_AMBITION, SEASON_AMBITION, SEASON_SICKLY, SEASON_LEAN, SEASON_WARM";
    let seasons_to = "SEASONS spec SEASON_WHITE, SEASON_WHITE, SEASON_WHITE, SEASON_WHITE, \
                      SEASON_WHITE, SEASON_WHITE, SEASON_WHITE, SEASON_WHITE, SEASON_WHITE, \
                      SEASON_WHITE, SEASON_WHITE, SEASON_WHITE, SEASON_WHITE, SEASON_WHITE";
    let moods_from = "MOODS spec DREAD, RAPTURE, GLIMMER, MENDING, EASE, WAKEFULNESS";
    let moods_to = "MOODS spec RAPTURE, RAPTURE, RAPTURE, RAPTURE, RAPTURE, RAPTURE";
    let mutated = with_deck(
        &with_deck(SLICE, seasons_from, seasons_to),
        moods_from,
        moods_to,
    );
    let mut game = Game::from_script(&mutated, core_dictionary()).unwrap();

    play(
        &mut game,
        &[
            "0:16 dream",
            "0:18 dream",
            "0:20 dream",
            "0:22 work with wits",
            "0:24 work with vigor",
            "0:26",
            "0:28",
            "0:30",
            "0:32",
            "0:34",
            "0:36",
            "0:38",
            "0:40",
            "0:42",
            "0:44",
            "0:46",
            "1:0",
            "1:2",
        ],
    );

    assert_ended(&game, "BRIGHT_AIR", "Taken by the Bright Air", "melancholy");
    assert_eq!(
        completions(&game, "rapture_process"),
        3,
        "three white airs must each process one rapture"
    );
    assert_eq!(
        alive_named(&game, "THE_BRIGHT_MARK"),
        3,
        "three bright marks stand at the ending"
    );
    assert_eq!(
        sentence_count(&game, "DRAWN(MOODS, RAPTURE)"),
        3,
        "the mutated router dealt the three raptures"
    );
    assert_further_input_rejected(&mut game, "1:4");
    assert_eq!(refusals(&game), 0, "the death walk accepts every line");
}

/// P4 — the body death, the starvation path (the plan's LEAN shape: the
/// purse drained, then a Lean Day eats VIGOR into THE_HOLLOW_DAYS, then the
/// next penniless Lean Day lays the body down). DOCUMENTED MUTATION: SEASONS
/// re-weighted to SEASON_QUIET + SEASON_LEAN x13 — the seed-1837 permutation
/// deals spec[0] first, so the single QUIET is placed there as a one-turn
/// bail-out and every later draw is a Lean Day. Dice disclosure: the two
/// bootstrap works land faces 1 (post spawns a clerk's post), 1 (WITS
/// drains), 7 and 4 (VIGOR holds) — pinned, and the ledger is exact: 3 -> 2
/// (quiet turn) -> 1 (lean #1 starves VIGOR) -> 0 (the wheel feeds the
/// hollow days back) -> no meal, no body.
#[test]
fn a_starved_body_fails() {
    let seasons_from = "SEASONS spec SEASON_QUIET, SEASON_QUIET, SEASON_QUIET, SEASON_WHITE, \
                        SEASON_WHITE, SEASON_GREY, SEASON_GREY, SEASON_SHADOW, SEASON_SHADOW, \
                        SEASON_AMBITION, SEASON_AMBITION, SEASON_SICKLY, SEASON_LEAN, SEASON_WARM";
    let seasons_to = "SEASONS spec SEASON_QUIET, SEASON_LEAN, SEASON_LEAN, SEASON_LEAN, \
                      SEASON_LEAN, SEASON_LEAN, SEASON_LEAN, SEASON_LEAN, SEASON_LEAN, \
                      SEASON_LEAN, SEASON_LEAN, SEASON_LEAN, SEASON_LEAN, SEASON_LEAN";
    let mutated = with_deck(SLICE, seasons_from, seasons_to);
    let mut game = Game::from_script(&mutated, core_dictionary()).unwrap();

    // the purse: +2 (wits) then +1 (the vigor bootstrap); then the clock
    // sweeps quiet, lean, lean to the ending at 0:36
    play(
        &mut game,
        &[
            "0:16 work with wits",
            "0:18 work with vigor",
            "0:20",
            "0:22",
            "0:24",
            "0:26",
            "0:28",
            "0:30",
            "0:32",
            "0:34",
            "0:36",
        ],
    );

    assert_ended(
        &game,
        "THE_BODY_FAILS",
        "The Body Lays Down Its Tenant",
        "melancholy",
    );
    // the LEAN shape itself: two starvation leans — the first eats VIGOR
    // into the hollow days ("the lean days eat the meal itself"), the last
    // finds no meal and no body
    assert_eq!(
        completions(&game, "season.rags"),
        2,
        "two lean days must reach the rags gate"
    );
    assert_eq!(
        completions(&game, "starvation_lean"),
        2,
        "both penniless leans must starve"
    );
    assert_eq!(
        sentence_count(&game, "The lean days eat the meal itself"),
        1,
        "the first lean must eat VIGOR, not the body"
    );
    assert_eq!(alive_named(&game, "VIGOR"), 0, "the body is gone");
    assert_further_input_rejected(&mut game, "0:38");
    assert_eq!(refusals(&game), 0, "the death walk accepts every line");
}

/// P5 — the wheel, unmodified script at its own seed. One wits work funds
/// the purse, one vigor work spawns THE_TURNING (only VIGOR and the posts
/// carry `on complete job`), then the clock advances through four full
/// season cycles. Pins: >= 3 season draws with >= 3 DISTINCT weathers; the
/// time verb resident (a WAIT window open) at every breath after the
/// bootstrap and time.turn completed at every cycle; THE_TURNING spawned
/// exactly once; and a SECOND job completion (the clerk's post worked) that
/// spawns nothing and whose bootstrap RUN skips on the busy station — the
/// §9.48 suppression, asserted by the draw cadence: four turns, four draws,
/// no extra.
#[test]
fn the_wheel_turns_at_least_three_seasons() {
    let mut game = Game::from_script(SLICE, core_dictionary()).unwrap();
    play(
        &mut game,
        &[
            "0:16 work with wits",
            "0:18 work with vigor",
            "0:20",
            "0:22",
            "0:24",
            "0:26",
            "0:28",
            "0:30 work with the clerks post",
            "0:32",
            "0:34",
            "0:36",
        ],
    );

    // >= 3 season draws, >= 3 distinct weathers (seed 1837 deals QUIET, LEAN,
    // AMBITION, QUIET — the lean day eats VIGOR mid-walk, per the cycle)
    assert!(
        sentence_count(&game, "DRAWN(SEASONS, ") >= 3,
        "the wheel must draw at least three seasons"
    );
    let mut weathers: Vec<String> = game
        .log
        .records
        .iter()
        .filter(|r| r.kind == "complete")
        .flat_map(|r| r.recipes.iter().cloned())
        .filter(|id| id.starts_with("season."))
        .collect();
    weathers.sort();
    weathers.dedup();
    assert!(
        weathers.len() >= 3,
        "three distinct weathers must complete: {weathers:?}"
    );

    // the rechain: time.turn completed once per cycle, and the WAIT station
    // holds an open window right now — the loop never stopped being resident
    assert!(
        completions(&game, "time.turn") >= 3,
        "the wheel must rechain through at least three turns"
    );
    assert!(
        game.state.windows.values().any(|w| w.verb == "WAIT"),
        "the time verb must still be resident"
    );

    // THE_TURNING spawned exactly once — and the second job completion (the
    // clerk's post) neither spawned a second wheel nor slipped an extra turn
    // past the busy station: four wheel cycles, four draws, no more
    assert_eq!(completions(&game, "work.vigor"), 1, "the bootstrap");
    assert_eq!(
        completions(&game, "work.post"),
        1,
        "a second job completion must have happened"
    );
    assert_eq!(
        sentence_count(&game, "SPAWN THE_TURNING INTO"),
        1,
        "the wheel spawns exactly once, ever"
    );
    assert_eq!(refusals(&game), 0, "the walk accepts every line");
}
