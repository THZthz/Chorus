//! The held slot (§6.7, v0.9.3): the §6.4 deferred selection (a held
//! binding can flip which recipe matches), the HOLD row (play == fold ==
//! undo — a logged step that replays), the lock for the window's remainder,
//! the at-most-once discipline, and the §6.4 identity pin (a held slot whose
//! recipes never read held selects exactly as a held-less twin would).
//!
//! Every test builds a FRESH Dictionary; the fixture script declares its
//! verbs through the `verbs:` block (§10's registration). State equality
//! goes through the shared `snap()` (law 6).

use crate::common::harness::assert_rejected;
use crate::common::rig::{ops, tmp_log_path};
use crate::common::snap::snap;
use chorus::content::script::compile_script;
use chorus::content::CompiledContent;
use chorus::core::clock::Clock;
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::r#ref::name_ref;
use chorus::core::sentence::Sentence;
use chorus::core::types::{clone_state, State};
use chorus::engine::fold::{fold, fold_up_to};
use chorus::engine::store::{load_log, save_log};
use chorus::engine::{execute_hold, execute_step, ActionRecord, ExecOptions, ExecResult, Step};

/// The fixture: a held-capable STUDY station (object: a text, held: a quest
/// item) whose ladder reads the held scope two ways — `count(held) >= 1`
/// (study.quested) and `held.aspect.quest >= 2` (study.relic) — plus a
/// plain FORGE station taking any item, a zero-duration THINK station
/// whose body chains into STUDY's default (the §6.7 chain-pinned follow-up
/// the hold must refuse), a held-less SMELT station whose single held-less
/// recipe reads the object's heat (a candle never matches it — the
/// §6.4 commit-time rejection), and a zero-duration HEX station that
/// mints a GHOST (the mid-window world flip).
const SCRIPT: &str = "\
script: Held Probe
player: ANA
start: 0:0

entities:
  ANA is character
  BOOK is text
  CHALICE is item with quest:1
  RELIC is item with quest:2
  CANDLE is item
  INGOT is item with heat:1

prototypes:
  GLYPH is idea
  GHOST is idea stackable

verbs:
  STUDY:
    actors: 1 kind:character
    object: 1 kind:text
    held: 0..1 aspect:quest
    duration: 30m
    recipes:
      study.relic when held.aspect.quest >= 2 -> DESTROY held, SPAWN GLYPH, NARRATE \"the relic burns.\"
      study.quested when count(held) >= 1 -> SPAWN GLYPH, NARRATE \"the quest item glows.\"
      default -> NARRATE \"plain.\"

  FORGE:
    actors: 1 kind:character
    object: 1 kind:item
    duration: 10m
    recipes:
      default -> NARRATE \"forged.\"

  THINK:
    actors: 1 kind:character
    duration: 0m
    recipes:
      think.hand -> RUN \"study.default\"

  SMELT:
    actors: 1 kind:character
    object: 1 kind:item
    duration: 10m
    recipes:
      smelt.hot when object.aspect.heat >= 1 -> NARRATE \"smelted.\"

  HEX:
    actors: 1 kind:character
    duration: 0m
    recipes:
      default -> SPAWN GHOST

  STUDY2:
    actors: 1 kind:character
    object: 1 kind:text
    held: 0..1 aspect:quest
    duration: 30m
    recipes:
      study2.quested when count(held) >= 1 AND NOT exists(GHOST) -> DESTROY held, SPAWN GLYPH
";

/// A fresh fixture: dictionary, content, and the pristine pre-play state
/// (compile_script is pure, so a fresh compile IS the base state).
fn rig() -> (Dictionary, CompiledContent, State) {
    let dict = core_dictionary();
    let compiled = compile_script(SCRIPT, &dict).unwrap();
    (compiled.dict, compiled.content, compiled.state)
}

/// parse one canonical line.
fn parse(src: &str, dict: &Dictionary) -> Sentence {
    parse_sentence(src, dict).unwrap()
}

/// commit one step WITHOUT settling — the window (if any) stays open.
fn begin(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    line: &str,
    first_id: i64,
) -> ExecResult {
    let s = parse(line, dict);
    execute_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, tick: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        ExecOptions { first_record_id: first_id, id_gen: None },
    )
    .unwrap()
}

/// commit one step at an exact MINUTE (start_min — a clock token counts
/// ticks of ten, so mid-window commits go through the minute-precise field).
fn begin_min(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    line: &str,
    minute: i64,
    first_id: i64,
) -> ExecResult {
    let s = parse(line, dict);
    execute_step(
        content,
        dict,
        state,
        Step { start_min: Some(minute), clock: None, clauses: s.clauses, t_real: None },
        ExecOptions { first_record_id: first_id, id_gen: None },
    )
    .unwrap()
}

/// advance to `minute` with no clauses — the pure pass that resolves every
/// window due by then.
fn advance_to(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    minute: i64,
    first_id: i64,
) -> ExecResult {
    execute_step(
        content,
        dict,
        state,
        Step { start_min: Some(minute), clock: None, clauses: vec![], t_real: None },
        ExecOptions { first_record_id: first_id, id_gen: None },
    )
    .unwrap()
}

fn hold_chalice(
    content: &CompiledContent,
    dict: &Dictionary,
    state: State,
    first_id: i64,
) -> ExecResult {
    execute_hold(content, dict, state, "STUDY", name_ref("CHALICE", None), first_id).unwrap()
}

fn complete_of(r: &ExecResult) -> &ActionRecord {
    r.records.iter().find(|rec| rec.kind == "complete").unwrap()
}

fn begin_of(r: &ExecResult) -> &ActionRecord {
    r.records.iter().find(|rec| rec.kind == "action").unwrap()
}

// ------------------------------------------------------------- deferral ----

#[test]
fn a_held_capable_verb_defers_selection_the_begin_row_records_the_deferral() {
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    assert_eq!(r.rejected, None);
    // the deferral mark: the empty recipe entry, the verb-default window
    assert_eq!(begin_of(&r).recipes, [""]);
    assert_eq!(begin_of(&r).advanced_min, 30);
    let w = r.state.windows.get("1:0").unwrap();
    assert_eq!(w.recipe, "");
}

#[test]
fn commit_without_a_hold_the_default_completes() {
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    let r2 = advance_to(&content, &_dict, r.state, 30, 2);
    // the COMPLETE row carries the id SELECTED at completion
    assert_eq!(complete_of(&r2).recipes, ["study.default"]);
    assert_eq!(ops(complete_of(&r2)), ["NARRATE"]);
    assert!(!r2.state.by_name.contains_key("GLYPH"));
    assert_eq!(r2.state.windows.len(), 0);
}

#[test]
fn commit_then_hold_the_held_recipe_completes_and_consumes() {
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    // the hold: one logged row, the window key riding it
    let h = hold_chalice(&content, &_dict, r.state, 2);
    assert_eq!(h.rejected, None);
    assert_eq!(h.records.len(), 1);
    let hold = &h.records[0];
    assert_eq!(hold.kind, "hold");
    assert_eq!(hold.verb, "STUDY");
    assert_eq!(hold.window.as_deref(), Some("1:0"));
    assert_eq!(hold.sentence, "0:0 @ HOLD STUDY CHALICE");
    // the lock rides the window
    let w = h.state.windows.get("1:0").unwrap();
    assert!(w.locked.iter().any(|id| id == "s_chalice"));
    let adv = advance_to(&content, &_dict, h.state, 30, 3);
    let complete = complete_of(&adv);
    assert_eq!(complete.recipes, ["study.quested"]);
    assert_eq!(ops(complete), ["SPAWN", "NARRATE"]);
    assert!(complete.held.is_some(), "the COMPLETE row carries the held card");
    // study.quested does not consume: the card rides out of the slot and
    // unlocks back to the table (the same discipline as any bound card);
    // the glyph minted
    assert!(adv.state.by_name.contains_key("CHALICE"));
    assert!(adv.state.by_name.contains_key("GLYPH"));
}

#[test]
fn the_aspect_face_flips_to_the_matching_recipe() {
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    let h = execute_hold(&content, &_dict, r.state, "STUDY", name_ref("RELIC", None), 2).unwrap();
    assert_eq!(h.rejected, None);
    let adv = advance_to(&content, &_dict, h.state, 30, 3);
    // held.aspect.quest >= 2: RELIC carries quest:2, CHALICE only 1 — the
    // same held scope, two different recipes
    assert_eq!(complete_of(&adv).recipes, ["study.relic"]);
}

// ------------------------------------------------- play == fold == undo ----

#[test]
fn the_hold_row_replays_the_fold_matches_play() {
    let (dict, content, base) = rig();
    let r = begin(&content, &dict, base.clone(), "0:0 @ ANA STUDY BOOK", 1);
    let h = hold_chalice(&content, &dict, r.state, 2);
    let adv = advance_to(&content, &dict, h.state, 30, 3);
    let begin_len = r.records.len() as i64;
    let records: Vec<ActionRecord> = [r.records, h.records, adv.records].concat();
    // the whole log folds to the played state (law: state = fold(log))
    let folded = fold(&records, &base, &dict, &content).unwrap();
    assert_eq!(snap(&folded), snap(&adv.state));
    // the undo truncation point: without the hold (and its completion), the
    // prefix folds to the open window — the card back on the table, unlocked
    let prefix = fold_up_to(&records, &base, &dict, &content, begin_len + 1).unwrap();
    assert_eq!(prefix.windows.len(), 1);
    let w = prefix.windows.values().next().unwrap();
    assert_eq!(w.locked, ["s_book"], "no hold — only the committed object locks");
    assert!(!w.locked.iter().any(|id| id == "s_chalice"), "the held card is free again");
    assert!(prefix.by_name.contains_key("CHALICE"));
}

#[test]
fn the_same_seed_double_run_is_byte_identical() {
    let play = || {
        let (dict, content, state) = rig();
        let r = begin(&content, &dict, state, "0:0 @ ANA STUDY BOOK", 1);
        let h = hold_chalice(&content, &dict, r.state, 2);
        let adv = advance_to(&content, &dict, h.state, 30, 3);
        [r.records, h.records, adv.records].concat()
    };
    let a = play();
    let b = play();
    let to_lines = |rs: &[ActionRecord]| -> Vec<String> {
        rs.iter().map(|r| serde_json::to_string(r).unwrap()).collect()
    };
    assert_eq!(to_lines(&a), to_lines(&b), "two identical plays, identical bytes (law 1)");
    // save → load → save stays byte-stable with HOLD rows in the log
    let path = tmp_log_path("held-law1");
    save_log(&path, &a).unwrap();
    let loaded = load_log(&path).unwrap();
    assert_eq!(to_lines(&loaded), to_lines(&a), "load round-trips every HOLD-row field");
    save_log(&path, &loaded).unwrap();
    let second = std::fs::read(&path).unwrap();
    let first = {
        let p2 = tmp_log_path("held-law1-first");
        save_log(&p2, &a).unwrap();
        std::fs::read(&p2).unwrap()
    };
    assert_eq!(first, second, "save(load(save(log))) is byte-stable with a HOLD row");
}

// ----------------------------------------------------------- the lock ------

#[test]
fn the_held_card_is_busy_for_the_windows_remainder() {
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    let h = hold_chalice(&content, &_dict, r.state, 2);
    // a second station cannot take the held card while STUDY processes
    // minute-precise mid-window commit (a clock token counts in ticks)
    let other = begin_min(&content, &_dict, h.state.clone(), "0:0 @ ANA FORGE CHALICE", 5, 3);
    assert_rejected(&other, r"CHALICE is busy with STUDY — it frees at 0:3");
    // completion releases it: the same commit lands once the window closed
    let adv = advance_to(&content, &_dict, h.state, 30, 3);
    assert!(adv.state.by_name.contains_key("CHALICE"), "study.quested does not consume");
    // an unconsumed hold releases: the card commits again on another station
    let again = begin_min(&content, &_dict, adv.state, "0:0 @ ANA FORGE CHALICE", 30, 3);
    assert_eq!(again.rejected, None, "the released card commits again");
    // a DESTROY-held outcome consumes instead — RELIC through study.relic
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    let h = execute_hold(&content, &_dict, r.state, "STUDY", name_ref("RELIC", None), 2).unwrap();
    let adv = advance_to(&content, &_dict, h.state, 30, 3);
    assert!(!adv.state.by_name.contains_key("RELIC"), "DESTROY held consumed it");
}

#[test]
fn a_card_locked_by_another_window_cannot_be_held() {
    let (_dict, content, state) = rig();
    // CHALICE locks into FORGE's window
    let r = begin(&content, &_dict, state, "0:0 @ ANA FORGE CHALICE", 1);
    assert_eq!(r.rejected, None);
    // the study station opens mid-window (minute 5 of FORGE's 0..10) and
    // reaches for the busy card
    let s = begin_min(&content, &_dict, r.state, "0:0 @ ANA STUDY BOOK", 5, 2);
    let h = execute_hold(&content, &_dict, s.state, "STUDY", name_ref("CHALICE", None), 3).unwrap();
    assert_rejected(&h, r"CHALICE is busy with FORGE — it frees at 0:1");
}

// ------------------------------------------------- the hold's own rules ----

#[test]
fn hold_rejects_non_held_verbs_unknown_verbs_and_empty_stations() {
    let (_dict, content, state) = rig();
    // no held slot on FORGE
    let h = execute_hold(&content, &_dict, state.clone(), "FORGE", name_ref("CHALICE", None), 1)
        .unwrap();
    assert_rejected(&h, r"FORGE has no held slot");
    // not a content verb
    let h = execute_hold(&content, &_dict, state.clone(), "CURSE", name_ref("CHALICE", None), 1)
        .unwrap();
    assert_rejected(&h, r"CURSE is not a content verb");
    // no open station
    let h = execute_hold(&content, &_dict, state, "STUDY", name_ref("CHALICE", None), 1).unwrap();
    assert_rejected(&h, r"STUDY is not processing");
}

#[test]
fn hold_rejects_a_card_that_does_not_fit_the_query() {
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    // CANDLE carries no quest aspect — the held slot wants aspect:quest
    let h = execute_hold(&content, &_dict, r.state, "STUDY", name_ref("CANDLE", None), 2).unwrap();
    assert_rejected(&h, r"CANDLE does not fit slot 'held' of STUDY");
}

#[test]
fn a_window_holds_at_most_once() {
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    let h = hold_chalice(&content, &_dict, r.state, 2);
    // no mid-window eviction: a second hold — even of a different fitting
    // card — is the at-most-once refusal
    let h2 = execute_hold(&content, &_dict, h.state, "STUDY", name_ref("RELIC", None), 3).unwrap();
    assert_rejected(&h2, r"STUDY already holds a card");
}

#[test]
fn a_chain_pinned_window_refuses_the_hold() {
    let (_dict, content, state) = rig();
    // THINK completes at once and chains into study.default — the follow-up
    // window on STUDY opens PINNED (the §6.7 pin fixed the id)
    let r = begin(&content, &_dict, state, "0:0 @ ANA THINK", 1);
    assert_eq!(r.rejected, None);
    let adv = advance_to(&content, &_dict, r.state, 0, 2);
    let followup = adv.records.iter().find(|rec| rec.kind == "action").unwrap();
    assert_eq!(followup.verb, "STUDY");
    assert_eq!(followup.recipes, ["study.default"], "the chain pin carries its id");
    let w = adv.state.windows.values().find(|w| w.verb == "STUDY").unwrap();
    assert_eq!(w.recipe, "study.default");
    // the hold cannot flip a pinned selection
    let h =
        execute_hold(&content, &_dict, adv.state, "STUDY", name_ref("CHALICE", None), 9).unwrap();
    assert_rejected(&h, r"STUDY is pinned to recipe study\.default");
}

// ----------------------------------------------- the completion's faces ----

#[test]
fn a_deferred_window_that_finds_no_recipe_refuses_the_window() {
    // the ladder's held-reading recipes all require the hold: with an empty
    // held scope the completion analog of "no recipe matched" is the
    // window-refuse — marker on the COMPLETE row, state untouched
    let (dict, content, base) = rig();
    let r = begin(&content, &dict, base.clone(), "0:0 @ ANA STUDY2 BOOK", 1);
    let adv = advance_to(&content, &dict, r.state, 30, 2);
    let complete = complete_of(&adv);
    assert!(
        complete.outcomes.iter().any(|o| o.op == "REFUSE"),
        "the refused deferred completion journals its marker"
    );
    assert!(
        complete.outcomes.iter().any(|o| o.op == "REFUSE"
            && o.args.iter().any(|a| matches!(a, chorus::core::sentence::Arg::Str { v }
                if v.contains("no recipe matched")))),
        "the marker names the deferred miss"
    );
    assert_eq!(complete.recipes, Vec::<String>::new(), "nothing was selected");
    assert!(!adv.state.by_name.contains_key("GLYPH"));
    assert_eq!(adv.state.windows.len(), 0);
    // the budget: a refused completion counts nothing — `executed` stays
    // untouched (the held-reading recipe matched nothing, no default exists)
    assert!(adv.state.executed.is_empty(), "{:?}", adv.state.executed);
    // the fold replays the refused row as the no-op it is
    let records: Vec<ActionRecord> = [r.records, adv.records].concat();
    let folded = fold(&records, &base, &dict, &content).unwrap();
    assert_eq!(snap(&folded), snap(&adv.state));
}

#[test]
fn the_deferred_count_lands_at_the_completion() {
    let (_dict, content, state) = rig();
    // the no-hold run counts ITS selection — the unselected recipe counts
    // nothing (the empty deferral entry never enters the map)
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    let adv = advance_to(&content, &_dict, r.state, 30, 2);
    assert!(!adv.state.executed.contains_key("study.quested"), "unselected → no count");
    assert_eq!(adv.state.executed.get("study.default"), Some(&1));
    // a held completion counts ITS recipe; the consumed card refuses the
    // next hold (the count is not the only thing that landed)
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY BOOK", 1);
    let h = hold_chalice(&content, &_dict, r.state, 2);
    let adv = advance_to(&content, &_dict, h.state, 30, 3);
    assert_eq!(adv.state.executed.get("study.quested"), Some(&1));
    assert!(!adv.state.executed.contains_key("study.default"), "the hold flipped the count");
    // the unheld card is back on the table: a later window holds it again —
    // the at-most-once rule is per window, the release discipline per hold
    let r = begin_min(&content, &_dict, adv.state, "0:0 @ ANA STUDY BOOK", 30, 9);
    let h = hold_chalice(&content, &_dict, r.state, 10);
    assert_eq!(h.rejected, None, "the released card holds again");
}

// ------------------------------------- the commit-time rejection (§6.4) ----

#[test]
fn a_held_less_verb_with_no_matching_recipe_rejects_at_commit() {
    // §6.4: a held-less verb selects NOW — no recipe matched is the
    // commit-time rejection, never a deferred window that re-selects over
    // a drifted world at completion
    let (_dict, content, state) = rig();
    let base = clone_state(&state);
    // CANDLE carries no heat, SMELT has no default: the commit is rejected
    // with the exact line the prose layer documents
    let r = begin(&content, &_dict, state, "0:0 @ ANA SMELT CANDLE", 1);
    assert_eq!(r.rejected.as_deref(), Some("SMELT: no recipe matched"));
    // the rejection discipline: nothing logged, no window opened, the
    // input state rides back untouched
    assert!(r.records.is_empty());
    assert!(r.state.windows.is_empty());
    assert_eq!(snap(&r.state), snap(&base), "the input state rides back untouched");
    // the card never locked — CANDLE commits freely on the next step
    let forge = begin(&content, &_dict, r.state, "0:0 @ ANA FORGE CANDLE", 1);
    assert_eq!(forge.rejected, None);
    // the positive control: a fitting object selects, commits and locks
    let (_dict, content, state) = rig();
    let ok = begin(&content, &_dict, state, "0:0 @ ANA SMELT INGOT", 1);
    assert_eq!(ok.rejected, None);
    assert_eq!(begin_of(&ok).recipes, ["smelt.hot"]);
    let w = ok.state.windows.values().next().unwrap();
    assert!(w.locked.contains(&"s_ingot".to_string()), "the committed object locks");
}

#[test]
fn a_mid_window_flip_does_not_rescue_a_held_reading_recipe() {
    // §6.4's deferral: the window selects AT COMPLETION, over the drifted
    // world — but the held-reading recipe still needs the held scope. A
    // ghost minting mid-window must not rescue study2.quested: without the
    // hold the completion refuses, the exact analog of the held-less
    // commit-time rejection.
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY2 BOOK", 1);
    // the flip: HEX's window (0..0) rides the same advance pass — its ghost
    // lands at minute 0, inside STUDY2's open window
    let hx = begin(&content, &_dict, r.state, "0:0 @ ANA HEX", 2);
    assert_eq!(hx.rejected, None);
    let adv = advance_to(&content, &_dict, hx.state, 30, 3);
    assert!(adv.state.by_name.contains_key("GHOST"), "the flip landed");
    let study2_done = adv
        .records
        .iter()
        .find(|rec| rec.kind == "complete" && rec.verb == "STUDY2")
        .expect("the STUDY2 window closed");
    assert!(study2_done.outcomes.iter().any(|o| o.op == "REFUSE"), "the flip did not rescue it");
    assert_eq!(study2_done.recipes, Vec::<String>::new(), "nothing fired");
    assert!(!adv.state.by_name.contains_key("GLYPH"));
    assert_eq!(adv.state.windows.len(), 0);
    // the held counterpart of the same flip: even WITH the hold the when
    // reads the COMPLETION world fresh — the deferral takes no commit
    // snapshot — so the ghost minted after the commit still defeats the
    // recipe. The hold moved no clock, but time had passed since the
    // window began: the row logs the world clock at the hold (§6.7).
    let (_dict, content, state) = rig();
    let r = begin(&content, &_dict, state, "0:0 @ ANA STUDY2 BOOK", 1);
    let mid = advance_to(&content, &_dict, r.state, 10, 2);
    let hx = begin_min(&content, &_dict, mid.state, "0:0 @ ANA HEX", 10, 3);
    assert_eq!(hx.rejected, None);
    let h =
        execute_hold(&content, &_dict, hx.state, "STUDY2", name_ref("CHALICE", None), 4).unwrap();
    assert_eq!(h.rejected, None);
    assert_eq!(h.records[0].t_game, "0:1", "the HOLD row logs the world clock at the hold");
    let adv = advance_to(&content, &_dict, h.state, 30, 5);
    // HEX's window (10..10) closes first — the ghost mints; STUDY2 then
    // re-selects at its end: count(held)=1 but the ghost exists, no match
    assert!(adv.state.by_name.contains_key("GHOST"));
    let study2_done = adv
        .records
        .iter()
        .find(|rec| rec.kind == "complete" && rec.verb == "STUDY2")
        .expect("the STUDY2 window closed");
    assert!(study2_done.outcomes.iter().any(|o| o.op == "REFUSE"), "the fresh evaluation fails");
    assert_eq!(study2_done.recipes, Vec::<String>::new());
    assert_eq!(adv.state.executed.get("study2.quested"), None, "a refusal counts nothing");
    assert!(adv.state.by_name.contains_key("CHALICE"), "DESTROY held never applied");
}

// ------------------------------------------------------ the identity pin ----

#[test]
fn a_held_slot_with_no_held_reading_recipes_selects_exactly_as_a_held_less_twin() {
    // §6.4's identity pin: the same ladder with and without the held slot —
    // played identically and WITHOUT any hold, the same recipes fire, the
    // states agree, and the prose agrees (only the row carrying the id moves)
    let twin = |held: bool| {
        let held_line = if held { "    held: 0..1 aspect:quest\n" } else { "" };
        let text = format!(
            "\
script: Twin Probe
player: ANA
start: 0:0

entities:
  ANA is character
  BOOK is text
  CHALICE is item with quest:1

verbs:
  STUDY:
    actors: 1 kind:character
    object: 1 kind:text
{held_line}    duration: 30m
    recipes:
      study.magick when object.aspect.magick >= 99 -> NARRATE \"never.\"
      default -> NARRATE \"plain.\"
"
        );
        let dict = core_dictionary();
        let compiled = compile_script(&text, &dict).unwrap();
        (compiled.dict, compiled.content, compiled.state)
    };
    let (dict_h, content_held, state_held) = twin(true);
    let (dict_p, content_plain, state_plain) = twin(false);
    let rh = begin(&content_held, &dict_h, state_held, "0:0 @ ANA STUDY BOOK", 1);
    let rp = begin(&content_plain, &dict_p, state_plain, "0:0 @ ANA STUDY BOOK", 1);
    // the begin rows differ ONLY in the deferral mark (empty vs the id —
    // the row the id rides moved; the sentence and window are identical)
    let bh = begin_of(&rh).clone();
    let bp = begin_of(&rp).clone();
    let ah = advance_to(&content_held, &dict_h, rh.state, 30, 2);
    let ap = advance_to(&content_plain, &dict_p, rp.state, 30, 2);
    // the same recipe decided both runs, the same outcomes applied
    assert_eq!(complete_of(&ah).recipes, complete_of(&ap).recipes);
    assert_eq!(complete_of(&ah).recipes, ["study.default"]);
    assert_eq!(complete_of(&ah).outcomes, complete_of(&ap).outcomes);
    assert_eq!(snap(&ah.state), snap(&ap.state));
    assert_eq!(bh.recipes, [""]);
    assert_eq!(bp.recipes, ["study.default"]);
    assert_eq!(bh.sentence, bp.sentence);
    assert_eq!(bh.advanced_min, bp.advanced_min);
}
