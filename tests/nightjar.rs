//! The Nightjar Flyer (21 tests) — story acceptance: the mystery compiles, plays, and
//! is winnable; the timetable (triggers) is a real adversary; PROGRESS ticks
//! (§6.7) render as the action's read-bar. Since v0.4 it is also the
//! showcase: the scripted win path must fire every new mechanic (the
//! SUSPICION pool, patrol_check's roll and tiers, the RUMOURS deck, the
//! exists() branch in the talk recipe) without ever tripping the patrol's
//! escalation ending — seed 1899 is calibrated for exactly that.
//!
//! P0 anchors (the golden diff reuses these): the win path's dice order
//! [[5,2],[8,4],[14,7]], the deck order ['SCANDAL','RUMOUR'], and
//! PORTENT×2/VOID×1 — asserted value-for-value (:153/:163/:347-348).
//!
//! The v0.5 test adapter auto-settles: each committed action's open windows
//! complete before the next input ([`auto_settle_line`]; there is no
//! implicit settling to lean on, so the adapter is explicit here).
//! Object-identity assertions have no Rust observable; snap /
//! field equality is used at those sites (never weakened).

mod common;

use common::explorer_lib::SCRIPT_TEXT;

use chorus::content::script::compile_script;
use chorus::core::dict::core_dictionary;
use chorus::engine::fold::fold;
use chorus::js::cmp_utf16;
use chorus::session::game::{Game, Turn, TurnKind};
use chorus::session::lenient::{parse_player_input, LenientCtx, ParsedInput};

/// Game + the auto-settle adapter.
fn fresh() -> Game {
    Game::from_script(SCRIPT_TEXT, core_dictionary()).unwrap()
}

/// the auto-settle adapter: after a committed action with open windows, the
/// settle lines append to the SAME action turn's lines.
fn auto_settle_line(game: &mut Game, line: &str) -> chorus::js::Result<Turn> {
    let t = game.handle_line(line)?;
    let t = if t.kind == TurnKind::Action && !game.state.windows.is_empty() {
        let settle = game.wait()?;
        Turn {
            kind: TurnKind::Action,
            lines: [t.lines, settle.lines].concat(),
        }
    } else {
        t
    };
    Ok(t)
}

fn play(game: &mut Game, lines: &[&str]) -> chorus::js::Result<Vec<Turn>> {
    let mut turns = Vec::new();
    for l in lines {
        turns.push(auto_settle_line(game, l)?);
    }
    Ok(turns)
}

/// (`WIN_PATH` — the test-local copy; the
/// explorer lib's is the sweep/walk's own)
const WIN_PATH: [&str; 9] = [
    "wait",                          // 22:00 — the body is found
    "explore the first cabin",       // 23:00 — the solicitor's letter
    "read the solicitor letter",     // 0:00  — border stop; the telegram lands
    "explore the firebox",           // 1:00  — the wax cylinder
    "read the vane ledger",          // 2:00  — read in the same hour it burns
    "talk with the spiritualist",    // meet Seraphine (the name is learned by talking)
    "seraphine read the wax cylinder",
    "seraphine read the telegram",
    "use on the inquest with the embezzlement the stolen voice the vienna warning",
];

use chorus::core::sentence::Arg;
use chorus::engine::engine::ActionRecord;

/// The ROLLED (total, counter-after) pair.
fn ints(o: &chorus::core::sentence::Outcome) -> Option<(i64, i64)> {
    match (o.args.first(), o.args.get(1)) {
        (Some(Arg::Int { v: a }), Some(Arg::Int { v: b })) => Some((*a, *b)),
        _ => None,
    }
}

/// The DRAWN name (arg 1's name ref).
fn drawn_name(o: &chorus::core::sentence::Outcome) -> String {
    match o.args.get(1) {
        Some(Arg::Ref {
            r#ref: chorus::core::r#ref::Ref::Name { name, .. },
        }) => name.clone(),
        _ => "(?)".to_string(),
    }
}

fn pool_len(game: &Game, name: &str) -> usize {
    game.state.by_name.get(name).map_or(0, Vec::len)
}

/// The game's base state — the pristine pre-play state: compile_script is
/// pure, so a fresh compile IS the same base state.
fn base_of(game: &Game) -> chorus::core::types::State {
    compile_script(SCRIPT_TEXT, &game.dict).unwrap().state
}

// ------------------------------- tests -------------------------------------

/// The single top-level section.
mod the_nightjar_flyer {
    use super::*;

    #[test]
    fn lenient_talk_with_x_maps_onto_talks_to_slot() {
        let game = fresh();
        let r = parse_player_input(
            "talk with the guard",
            &LenientCtx {
                dict: &game.dict,
                content: &game.content,
                state: &game.state,
                player: Some("MARCH"),
            },
        );
        assert!(r.is_ok());
        let ParsedInput::Ok { step, .. } = r else {
            unreachable!()
        };
        let c = &step.clauses[0];
        assert_eq!(c.verb, "TALK");
        assert!(
            matches!(&c.to, Some(chorus::core::r#ref::Ref::Name { name, .. }) if name == "THE_GUARD")
        );
        assert_eq!(c.with, Vec::<chorus::core::r#ref::Ref>::new());
    }

    #[test]
    fn compiles_the_train_the_masked_suspects_and_the_twelve_verb_core_set() {
        let game = fresh();
        assert_eq!(game.title(), "Murder on the Nightjar Flyer");
        for name in [
            "MARCH",
            "THE_PATENTEE",
            "THE_WIDOW",
            "THE_SECRETARY",
            "THE_STOKER",
            "THE_SPIRITUALIST",
            "THE_GUARD",
            "THE_INQUEST",
            "THE_MANIFEST",
        ] {
            assert!(game.state.by_name.contains_key(name), "{name}");
        }
        // real names mint into being only through the talk.meet_* reveals
        for name in ["LORD_VANE", "VERITY", "QUILL", "COLE", "SERAPHINE", "BRACK"] {
            assert!(!game.state.by_name.contains_key(name), "{name}");
        }
        let mut verbs: Vec<&str> = game.content.verbs.keys().map(String::as_str).collect();
        verbs.sort_unstable(); // JS `.sort()` — ASCII keys, code-unit order
        assert_eq!(
            verbs,
            [
                "ACQUIRE", "DREAM", "EXPLORE", "LOCATE_AT", "MANSUS", "READ", "STUDY", "TALK",
                "TRAVEL_TO", "USE_ON", "WAIT", "WORK",
            ]
        );
    }

    #[test]
    fn the_train_is_a_zone_not_a_card_her_cars_carry_it_as_an_attribute() {
        let game = fresh();
        assert!(!game.state.by_name.contains_key("NIGHTJAR_FLYER"));
        assert_eq!(
            game.state.entities["s_guard_van"].attrs.get("zone"),
            Some(&chorus::core::types::AttrValue::Str("NIGHTJAR_FLYER".into()))
        );
    }

    #[test]
    fn travel_follows_the_corridor_adjacent_cars_only() {
        let mut game = fresh();
        let refused = auto_settle_line(&mut game, "travel to the dining car").unwrap();
        assert_eq!(refused.kind, TurnKind::Action);
        assert!(refused.lines.join("\n").contains("does not bend"));
        let march = game.state.by_name["MARCH"][0].clone();
        assert_eq!(
            game.state.entities[&march].attrs.get("container"),
            Some(&chorus::core::types::AttrValue::Str("s_guard_van".into()))
        );
        let ok = auto_settle_line(&mut game, "travel to the luggage van").unwrap();
        assert_eq!(ok.kind, TurnKind::Action);
        assert_eq!(
            game.state.entities[&march].attrs.get("container"),
            Some(&chorus::core::types::AttrValue::Str("s_luggage_van".into()))
        );
    }

    #[test]
    fn the_timetable_discovers_the_body_at_22_00() {
        let mut game = fresh();
        let turn = auto_settle_line(&mut game, "wait").unwrap();
        assert_eq!(turn.kind, TurnKind::Action);
        assert!(game
            .log
            .records
            .iter()
            .any(|r| r.kind == "event" && r.trigger.as_deref() == Some("body_found")));
        assert!(!game.state.by_name.contains_key("LORD_VANE"));
        assert!(game.state.by_name.contains_key("THE_LATE_LORD"));
        assert!(game.state.by_name.contains_key("THE_AETHER_SEAL"));
        assert!(turn.lines.join("\n").contains("entirely dead"));
    }

    #[test]
    fn progress_ticks_render_as_the_actions_read_bar() {
        // (§6.7)
        let mut game = fresh();
        auto_settle_line(&mut game, "wait").unwrap();
        let turn = auto_settle_line(&mut game, "explore the first cabin").unwrap();
        assert_eq!(turn.kind, TurnKind::Action);
        let joined = turn.lines.join("\n");
        assert!(joined.contains("(+1halfhour)"), "{joined}"); // live tick line
        assert!(joined.contains("solicitor's letter"));
        let canonical = turn
            .lines
            .iter()
            .filter(|l| l.starts_with('«'))
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        assert!(canonical.contains("PROGRESS(1halfhour,")); // the tick rides the BEGIN row
        assert!(canonical.contains("SPAWN THE_SOLICITOR_LETTER INTO #s_first_cabin")); // the mint the completion
    }

    #[test]
    fn full_win_path_motive_means_opportunity_confession() {
        let mut game = fresh();
        let turns = play(&mut game, &WIN_PATH).unwrap();
        assert!(turns.iter().all(|t| t.kind == TurnKind::Action));
        for clue in ["THE_EMBEZZLEMENT", "THE_STOLEN_VOICE", "THE_VIENNA_WARNING"] {
            assert!(game.state.by_name.contains_key(clue), "{clue}");
        }
        assert!(game.state.by_name.contains_key("CONFESSION_OF_QUILL"));
        let inquest = game
            .log
            .records
            .iter()
            .find(|r| r.verb == "USE_ON")
            .expect("the inquest ran");
        assert_eq!(inquest.recipes, ["inquest.win"]);
        assert!(game
            .log
            .records
            .iter()
            .any(|r| r.trigger.as_deref() == Some("border_stop")));
        assert!(game
            .log
            .records
            .iter()
            .any(|r| r.trigger.as_deref() == Some("the_furnace_takes_it")));
    }

    #[test]
    fn every_v0_4_mechanic_fires_on_the_scripted_path_v0_5_timeline() {
        // The P0 anchor battery
        let mut game = fresh();
        play(&mut game, &WIN_PATH).unwrap();
        // F1 — the SUSPICION pool is present: the 22:30 rounds landed the middle
        // tier under seed 1899; the cabin's own patrol roll now resolves at
        // the window's close — after the rounds — and lands the pass-by tier
        assert_eq!(pool_len(&game, "SUSPICION"), 1);
        // F4 — three ROLLED rows (conductor's rounds, cabin patrol, firebox
        // find — v0.5 completion order), the counter landing where play left
        // it. P0 ANCHOR: value-for-value, the golden reuses it.
        let rolled: Vec<Option<(i64, i64)>> = game
            .log
            .records
            .iter()
            .flat_map(|r| r.outcomes.iter().filter(|o| o.op == "ROLLED").map(ints))
            .collect();
        assert_eq!(
            rolled,
            vec![
                Some((5, 2)),
                Some((8, 4)),
                Some((14, 7))
            ]
        ); // calibrated against seed 1899
        assert_eq!(game.state.rng, 7);
        // F5 — the RUMOURS deck drew twice: once from the patrol trigger
        // (into the guard van), once from the talk recipe (into the dining
        // car). P0 ANCHOR (nightjar.test:163).
        let drawn: Vec<String> = game
            .log
            .records
            .iter()
            .flat_map(|r| {
                r.outcomes
                    .iter()
                    .filter(|o| o.op == "DRAWN")
                    .map(drawn_name)
            })
            .collect();
        assert_eq!(drawn, ["SCANDAL", "RUMOUR"]); // the seed-fixed permutation, in order
        assert!(game.state.by_name.contains_key("RUMOUR"));
        assert!(game.state.by_name.contains_key("SCANDAL"));
        // F3 — the conductor's rounds: the story's recurring patrol (the dawn
        // watch is the other recurring slot trigger, off-screen tonight), and
        // its event row carries the expanded patrol_check outcomes (ROLLED
        // row)
        let rounds: Vec<&ActionRecord> = game
            .log
            .records
            .iter()
            .filter(|r| r.trigger.as_deref() == Some("conductor_rounds"))
            .collect();
        assert_eq!(rounds.len(), 1);
        assert!(rounds[0].outcomes.iter().any(|o| o.op == "ROLLED"));
        // F2 — the talk recipe's exists() branch took the evidence arm (the
        // seal idea is on the table by 2:00)
        let meet_begin = game
            .log
            .records
            .iter()
            .find(|r| r.recipes.iter().any(|x| x == "talk.meet_spiritualist"))
            .expect("the spiritualist meeting began");
        let meet_done = game
            .log
            .records
            .iter()
            .find(|r| r.kind == "complete" && r.window == Some(format!("{}:0", meet_begin.id)))
            .expect("the meeting completed");
        let narrate = meet_done.outcomes.iter().find(|o| o.op == "NARRATE");
        let said = narrate.map(|o| &o.args[0]);
        let ok = matches!(said, Some(Arg::Str { v }) if v.contains("we all read the same card: the lord is dead"));
        assert!(ok, "said = {said:?}");
        // the escalation never tripped — but the inquest's grand ending now
        // lands at the window's close, one crossing after the vienna quiet
        // night: first ENDING wins (§9.35), and the train gets there first
        let ended = game.state.ended.as_ref().expect("the story ended");
        assert_eq!(ended.id, "QUIET_NIGHT");
        assert_eq!(ended.flavour, "melancholy");
        // the inquest still won the case
        assert!(game.state.by_name.contains_key("CONFESSION_OF_QUILL"));
    }

    #[test]
    fn a_careless_search_spawns_suspicion_the_pool_is_real_not_flavor() {
        // (F1)
        let mut game = fresh();
        play(&mut game, &["wait", "explore the first cabin"]).unwrap();
        let before = pool_len(&game, "SUSPICION");
        // rummaging again
        let turn = auto_settle_line(&mut game, "explore the first cabin").unwrap();
        assert_eq!(turn.kind, TurnKind::Action);
        assert_eq!(pool_len(&game, "SUSPICION"), before + 1);
        assert!(turn.lines.join("\n").contains("A steward will remember MARCH"));
    }

    #[test]
    fn careless_searches_feed_the_pool_until_watch_3_ends_the_case_escalation() {
        let mut game = fresh();
        play(&mut game, &["wait", "explore the first cabin"]).unwrap();
        // the 22:30 rounds fed the pool once; the cabin's own patrol roll now
        // resolves at the window's close — under seed 1899 it lands the
        // pass-by tier, so the pool sits at one until the searches pile on
        assert_eq!(pool_len(&game, "SUSPICION"), 1);
        assert!(game.state.ended.is_none());
        // the searched-again recipe spawns a copy AND calls watch(3) — the
        // pool climbs one copy per careless search until the third ends the
        // story
        let turn3 = auto_settle_line(&mut game, "explore the first cabin").unwrap();
        assert_eq!(turn3.kind, TurnKind::Action);
        assert_eq!(pool_len(&game, "SUSPICION"), 2);
        assert!(game.state.ended.is_none());
        let turn4 = auto_settle_line(&mut game, "explore the first cabin").unwrap();
        assert_eq!(turn4.kind, TurnKind::Action);
        let ended = game.state.ended.as_ref().expect("watch(3) ended the story");
        assert_eq!(ended.id, "OFF_THE_FLYER");
        assert_eq!(ended.flavour, "melancholy");
        let actions: Vec<&ActionRecord> = game
            .log
            .records
            .iter()
            .filter(|r| r.kind == "action")
            .collect();
        let last = actions[actions.len() - 1];
        assert!(last.recipes.iter().any(|r| r == "explore.searched"));
        // v0.5: the ENDING lands at the completion
        let last_complete = game
            .log
            .records
            .iter()
            .rev()
            .find(|r| r.kind == "complete")
            .unwrap();
        assert!(last_complete.outcomes.iter().any(|o| o.op == "ENDING"));
        // undo reopens the story — `ended` derives from the fold (§9.35)
        assert_eq!(game.undo().unwrap().kind, TurnKind::Info);
        assert!(game.state.ended.is_none());
    }

    #[test]
    fn the_french_gate_refuses_an_anglophone_reader_and_speaks_in_prose() {
        // (§6.2)
        let mut game = fresh();
        play(&mut game, &["wait", "wait", "wait"]).unwrap(); // to 0:00 — the telegram lands
        let turn = auto_settle_line(&mut game, "read the telegram").unwrap();
        assert_eq!(turn.kind, TurnKind::Action);
        assert!(turn.lines.iter().any(|l| l.starts_with("The words are French")));
        assert!(!game.state.by_name.contains_key("THE_VIENNA_WARNING"));
    }

    #[test]
    fn searching_the_sealed_cabin_before_the_murder_neither_spoils_nor_breaks_the_search() {
        let mut game = fresh();
        let pre = auto_settle_line(&mut game, "explore the first cabin").unwrap();
        assert_eq!(pre.kind, TurnKind::Action);
        assert!(pre.lines.join("\n").contains("does not open for a piece of paper"));
        assert!(!game.state.by_name.contains_key("THE_SOLICITOR_LETTER"));
        play(&mut game, &["wait"]).unwrap(); // the body is found; the seal comes down
        let post = auto_settle_line(&mut game, "explore the first cabin").unwrap();
        assert_eq!(post.kind, TurnKind::Action);
        assert!(game.state.by_name.contains_key("THE_SOLICITOR_LETTER"));
    }

    #[test]
    fn unknown_topics_and_places_fail_loudly_lenient_about_at() {
        let mut game = fresh();
        let turn = auto_settle_line(&mut game, "talk to seraphine about the lost tracklist")
            .unwrap();
        assert_eq!(turn.kind, TurnKind::Error);
        assert!(turn.lines.join("\n").contains("I don't know"));
    }

    #[test]
    fn an_inquest_on_a_hunch_or_on_empty_hands_refuses() {
        let mut game = fresh();
        play(
            &mut game,
            &["wait", "explore the first cabin", "read the solicitor letter"],
        )
        .unwrap();
        let thin = auto_settle_line(&mut game, "use on the inquest with the disinheritance")
            .unwrap();
        assert_eq!(thin.kind, TurnKind::Action);
        assert!(thin.lines.join("\n").contains("wobbles"));
        assert!(!game.state.by_name.contains_key("CONFESSION_OF_QUILL"));
        let empty = auto_settle_line(&mut game, "use on the inquest").unwrap();
        assert_eq!(empty.kind, TurnKind::Action);
        assert!(empty.lines.join("\n").contains("empty hands"));
    }

    #[test]
    fn halfhour_actions_land_back_on_the_ticks_and_the_timetable_fires_as_the_night_passes() {
        let mut game = fresh();
        // 21:00 → 21:30 — meet Cole
        assert_eq!(
            auto_settle_line(&mut game, "talk to the stoker").unwrap().kind,
            TurnKind::Action
        );
        assert!(game.state.by_name.contains_key("COLE"));
        // 21:30 → 22:00 — meet Brack; the body is found
        assert_eq!(
            auto_settle_line(&mut game, "talk to the guard").unwrap().kind,
            TurnKind::Action
        );
        assert!(game
            .log
            .records
            .iter()
            .any(|r| r.trigger.as_deref() == Some("body_found")));
        assert!(game.state.by_name.contains_key("THE_LATE_LORD"));
        // 22:00 → 22:30 — meet Seraphine
        assert_eq!(
            auto_settle_line(&mut game, "talk to the spiritualist")
                .unwrap()
                .kind,
            TurnKind::Action
        );
        // 22:30 → 23:30
        assert_eq!(
            auto_settle_line(&mut game, "wait").unwrap().kind,
            TurnKind::Action
        );
        assert!(game
            .log
            .records
            .iter()
            .any(|r| r.trigger.as_deref() == Some("night_porter")));
    }

    #[test]
    fn once_triggers_fire_exactly_once_and_the_lord_expires_into_his_successor() {
        // (§9.29, §9.32)
        let mut game = fresh();
        play(&mut game, &["wait"]).unwrap(); // day 0, hour 22 — body_found fires
        assert_eq!(
            game.log
                .records
                .iter()
                .filter(|r| r.trigger.as_deref() == Some("body_found"))
                .count(),
            1
        );
        // jump a day ahead in ONE step: the once-trigger must neither re-fire
        // nor reject — and the lord's 1-day `expires` crosses on the way, so
        // the authored `on expire` trigger replaces him (REPLACE, §9.32-§9.33).
        // The span also crosses the vienna ending (§9.35) — the ending rides
        // along: the pass that fired it runs to its full target, and play
        // stops after.
        let jump = auto_settle_line(&mut game, "2:0 @ MARCH WAIT").unwrap();
        assert_eq!(jump.kind, TurnKind::Action);
        // once means once
        assert_eq!(
            game.log
                .records
                .iter()
                .filter(|r| r.trigger.as_deref() == Some("body_found"))
                .count(),
            1
        );
        assert_eq!(
            game.log
                .records
                .iter()
                .filter(|r| r.trigger.as_deref() == Some("lord_tended"))
                .count(),
            1
        );
        assert!(!game.state.by_name.contains_key("THE_LATE_LORD"));
        assert!(game.state.by_name.contains_key("THE_TENDED_LORD"));
        // the flyer reached vienna en route
        assert!(game.state.ended.is_some());
    }

    #[test]
    fn the_ledger_burns_at_2_00_the_timetable_is_the_culprit_too() {
        let mut game = fresh();
        play(&mut game, &["wait", "wait", "wait", "wait", "wait"]).unwrap(); // 21:00 → 2:00
        assert!(!game.state.by_name.contains_key("THE_VANE_LEDGER"));
        let turn = auto_settle_line(&mut game, "read the vane ledger").unwrap();
        assert_eq!(turn.kind, TurnKind::Error); // the card is gone; the step is rejected
    }

    #[test]
    fn interrogation_exercises_the_to_role_end_to_end() {
        let mut game = fresh();
        play(&mut game, &["wait", "talk to the stoker"]).unwrap(); // the seal idea spawns; meet Cole
        let turn = auto_settle_line(&mut game, "talk to cole about the aether seal").unwrap();
        assert_eq!(turn.kind, TurnKind::Action);
        assert!(turn.lines.join("\n").contains("rake the firebox hot at midnight"));
        let rec = game
            .log
            .records
            .iter()
            .filter(|r| r.kind == "action" && r.verb == "TALK")
            .nth(1)
            .expect("the second TALK action row");
        assert!(
            matches!(&rec.to, Some(chorus::core::r#ref::Ref::Name { name, .. }) if name == "COLE")
        );
        assert!(rec
            .sentence
            .contains("MARCH TALK TO COLE ABOUT THE_AETHER_SEAL"));
    }

    #[test]
    fn the_showcase_verbs_play_wages_the_tunnel_closure_and_a_ticket_with_one_price() {
        let mut game = fresh();
        // meet Brack — RELATE(MARCH, BRACK, neighbor)
        assert_eq!(
            auto_settle_line(&mut game, "talk to the guard").unwrap().kind,
            TurnKind::Action
        );
        assert_eq!(
            auto_settle_line(&mut game, "march, brack work at the firebox")
                .unwrap()
                .kind,
            TurnKind::Action
        );
        // the load copy + the shift's wage
        assert_eq!(pool_len(&game, "FUNDS"), 2);
        let march_id = game.state.by_name["MARCH"][0].clone();
        assert_eq!(
            game.state.entities[&march_id].attrs.get("coin"),
            Some(&chorus::core::types::AttrValue::Num(2))
        );
        assert_eq!(
            auto_settle_line(&mut game, "locate_at the firebox")
                .unwrap()
                .kind,
            TurnKind::Action
        );
        // to 1:00 — the tunnel severs the crew door
        auto_settle_line(&mut game, "wait").unwrap();
        assert!(game.state.relations["s_firebox|s_guard_van|south"]
            .dead_action
            .is_some());
        let blocked = auto_settle_line(&mut game, "travel to the guard van").unwrap();
        assert_eq!(blocked.kind, TurnKind::Action);
        assert!(blocked.lines.join("\n").contains("does not bend"));
        auto_settle_line(&mut game, "wait").unwrap();
        auto_settle_line(&mut game, "wait").unwrap();
        // to 4:30 — dawn_watch relinks the corridor
        auto_settle_line(&mut game, "wait").unwrap();
        assert!(game.state.relations["s_firebox|s_guard_van|south"]
            .dead_action
            .is_none());
        assert_eq!(
            auto_settle_line(&mut game, "travel to the guard van")
                .unwrap()
                .kind,
            TurnKind::Action
        );
        // ACQUIRE: a cost that can go below zero — exactly once per ticket (§9.31)
        assert_eq!(
            auto_settle_line(&mut game, "acquire the claim ticket")
                .unwrap()
                .kind,
            TurnKind::Action
        );
        assert_eq!(
            game.state.entities[&march_id].attrs.get("coin"),
            Some(&chorus::core::types::AttrValue::Num(1))
        );
        let again = auto_settle_line(&mut game, "acquire the claim ticket").unwrap();
        assert_eq!(again.kind, TurnKind::Action);
        assert!(again.lines.join("\n").contains("Nobody aboard is selling"));
    }

    #[test]
    fn dreaming_draws_the_dreams_deck_until_it_runs_dry() {
        // (§9.42, §9.23)
        let mut game = fresh();
        play(
            &mut game,
            &[
                "wait",
                "explore the firebox",
                "read the wax cylinder",
                "read the vane ledger",
            ],
        )
        .unwrap();
        // the derive prototype minted
        assert!(game.state.by_name.contains_key("HUNCH_THE_VANE_LEDGER"));
        // playing the cylinder unsealed the seal idea, so it fits the dream slot
        assert_eq!(
            auto_settle_line(&mut game, "dream with the aether seal")
                .unwrap()
                .kind,
            TurnKind::Action
        );
        assert_eq!(
            auto_settle_line(&mut game, "dream with the aether seal")
                .unwrap()
                .kind,
            TurnKind::Action
        );
        assert_eq!(
            auto_settle_line(&mut game, "dream with the aether seal")
                .unwrap()
                .kind,
            TurnKind::Action
        );
        // the seed-fixed permutation served VOID, PORTENT, PORTENT —
        // stackable pools. P0 ANCHOR, the golden reuses it.
        assert_eq!(pool_len(&game, "PORTENT"), 2);
        assert_eq!(pool_len(&game, "VOID"), 1);
        // v0.5: the draw decides at the completion…
        let dry = auto_settle_line(&mut game, "dream with the aether seal").unwrap();
        assert_eq!(dry.kind, TurnKind::Action);
        // …where the exhausted deck fails loud — the completion row refuses
        // (§9.19 analog)
        let last_complete = game
            .log
            .records
            .iter()
            .rev()
            .find(|r| r.kind == "complete")
            .unwrap();
        assert!(last_complete.outcomes.iter().any(|o| o.op == "REFUSE"));
        assert!(regex::Regex::new(r"exhausted")
            .unwrap()
            .is_match(&game.journal().join("\n")));
    }

    #[test]
    fn the_whole_night_replays_from_the_log_state_equals_fold_of_log() {
        let mut game = fresh();
        play(&mut game, &WIN_PATH).unwrap();
        let base = base_of(&game);
        let replayed =
            fold(&game.log.records, &base, &game.dict, &game.content).unwrap();
        let mut replayed_names: Vec<&String> = replayed.by_name.keys().collect();
        let mut played_names: Vec<&String> = game.state.by_name.keys().collect();
        // JS `.sort()` = default string sort = UTF-16 code-unit order
        replayed_names.sort_by(|a, b| cmp_utf16(a, b));
        played_names.sort_by(|a, b| cmp_utf16(a, b));
        assert_eq!(replayed_names, played_names);
        assert_eq!(replayed.now, game.state.now);
    }
}
