//! §3.14 ordering/atomicity matrix — dedicated ordering tests targeting
//! the ordering semantics DIRECTLY.
//!
//! Every expectation below is a FROZEN EXPECTATION — the literal values are
//! this repository's frozen expectations; each test names its scenario
//! (family `anchor-*`) at those values, and the annotations stay
//! as the provenance record.
//!
//! The fold-equality assertions in this file compare through the shared
//! `snap()` (tests/common/snap.rs — same field set and order disciplines).

use chorus::content::content::{
    compile_content, CompiledContent, ContentRaw, EndingDef, PrototypeDef, RecipeDef, SlotDef, TriggerDef, VerbContent,
};
use chorus::core::clock::{parse_clock, Clock};
use chorus::core::dict::{core_dictionary, Dictionary};
use chorus::core::parse::parse_sentence;
use chorus::core::r#ref::Ref;
use chorus::core::sentence::Arg;
use chorus::core::types::{add_entity, clone_state, new_state, AttrValue, Attrs, Entity, State};
use chorus::engine::engine::{
    apply_record, execute_step, ActionRecord, ExecOptions, Step,
};
use chorus::engine::fold::{fold, fold_up_to};
use chorus::error::Result;

mod common;
use common::snap::snap;

// ---------------------------------------------------------------- rig ------

fn verb_of(name: &str, slots: Vec<SlotDef>, duration: i64, out: &[&str]) -> (String, VerbContent) {
    (
        name.to_string(),
        VerbContent {
            slots,
            duration,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                out: out.iter().map(|s| s.to_string()).collect(),
            }],
        },
    )
}

fn actor_slot() -> SlotDef {
    SlotDef {
        role: "actors".into(),
        min: 1,
        max: Some(1),
        query: Some("kind:character".into()),
        shown: None,
    }
}

fn object_slot(query: Option<&str>) -> SlotDef {
    SlotDef {
        role: "object".into(),
        min: 0,
        max: Some(1),
        query: query.map(|s| s.to_string()),
        shown: None,
    }
}

/// RigContent — rig verbs are code-registered (§10).
/// Each test builds a FRESH Dictionary (per-test isolation affects
/// reserved-word collisions).
fn rig(verbs: Vec<(String, VerbContent)>, extra: impl FnOnce(&mut ContentRaw)) -> (Dictionary, CompiledContent) {
    let mut dict = core_dictionary();
    let mut map = indexmap::IndexMap::new();
    for (name, v) in verbs {
        if !dict.is_verb(&name) {
            dict.register_verb(&name).unwrap();
        }
        map.insert(name.to_string(), v);
    }
    let mut raw = ContentRaw {
        verbs: map,
        ..ContentRaw::default()
    };
    extra(&mut raw);
    let content = compile_content(&raw, &dict, &[]).unwrap();
    (dict, content)
}

/// A lean seed world (the
/// ordering rig needs only characters + items with insert-order variance).
fn seed() -> State {
    let mut s = new_state(0);
    let card = |id: &str, kind: &str, name: &str, attrs: Attrs| Entity {
        id: id.to_string(),
        kind: kind.to_string(),
        name: name.to_string(),
        aspects: indexmap::IndexMap::new(),
        attrs,
        born_action: None,
        dead_action: None,
        stackable: false,
    };
    let base = |container: &str| {
        let mut a = Attrs::new();
        a.insert("container".to_string(), AttrValue::Str(container.to_string()));
        a
    };
    add_entity(&mut s, card("house", "place", "HOUSE", Attrs::new())).unwrap();
    add_entity(&mut s, card("ana", "character", "ANA", base("house"))).unwrap();
    add_entity(&mut s, card("bertha", "character", "BERTHA", base("house"))).unwrap();
    add_entity(&mut s, card("book", "text", "MAGICK_BOOK", base("house"))).unwrap();
    s
}

/// An item inserted with an `expiresAt` attr (the §9.36 materialized form —
/// tests write the engine fact directly).
fn expiring_item(s: &mut State, id: &str, name: &str, at: i64) {
    let mut attrs = Attrs::new();
    attrs.insert("container".to_string(), AttrValue::Str("house".to_string()));
    attrs.insert("expiresAt".to_string(), AttrValue::Num(at));
    add_entity(
        s,
        Entity {
            id: id.to_string(),
            kind: "item".to_string(),
            name: name.to_string(),
            aspects: indexmap::IndexMap::new(),
            attrs,
            born_action: None,
            dead_action: None,
            stackable: false,
        },
    )
    .unwrap();
}

fn run(content: &CompiledContent, dict: &Dictionary, state: State, src: &str, first: i64) -> chorus::engine::engine::ExecResult {
    let s = parse_sentence(src, dict).unwrap();
    execute_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(s.clock.unwrap_or(Clock { day: 0, halfhour: 0 })),
            clauses: s.clauses,
            t_real: None,
        },
        ExecOptions {
            first_record_id: first,
            id_gen: None,
        },
    )
    .unwrap()
}

/// A lone clock token is a pure advance — no sentence to parse (§9.28).
fn run_clock(content: &CompiledContent, dict: &Dictionary, state: State, clock: &str, first: i64) -> chorus::engine::engine::ExecResult {
    execute_step(
        content,
        dict,
        state,
        Step {
            start_min: None,
            clock: Some(parse_clock(clock).unwrap()),
            clauses: vec![],
            t_real: None,
        },
        ExecOptions {
            first_record_id: first,
            id_gen: None,
        },
    )
    .unwrap()
}

/// fold a full log over the SAME base the play started from — the law-1
/// helper (no tests/common yet).
fn replay(records: &[ActionRecord], base: &State, dict: &Dictionary, content: &CompiledContent) -> Result<State> {
    fold(records, base, dict, content)
}

/// The whole-history replay: folding a mid-window slice (advance rows only)
/// skips the BEGINs whose registration the completions presuppose — §9.50
/// gives the BEGIN a durable effect (the execution count rides registration),
/// so a slice must carry its own BEGINs for the fold to land the same state.
fn replay_all(
    slices: &[&[ActionRecord]],
    base: &State,
    dict: &Dictionary,
    content: &CompiledContent,
) -> Result<State> {
    let flat: Vec<ActionRecord> = slices.iter().flat_map(|s| s.iter()).cloned().collect();
    fold(&flat, base, dict, content)
}

fn kinds(records: &[ActionRecord]) -> Vec<&str> {
    records.iter().map(|r| r.kind.as_str()).collect()
}

fn triggers(records: &[ActionRecord]) -> Vec<Option<&str>> {
    records.iter().map(|r| r.trigger.as_deref()).collect()
}

fn outcome_ops(r: &ActionRecord) -> Vec<&str> {
    r.outcomes.iter().map(|o| o.op.as_str()).collect()
}

// ------------------------------------------------------------- matrix ------

/// §3.14 row 1a — advance pass: due windows in (end, beginId, clause) order;
/// a completion at minute m fires BEFORE slot-m events.
/// scenario: anchor-complete-before-slot-event (frozen expectation):
/// [complete, event, wait] at minute 60.
#[test]
fn advance_pass_completion_precedes_same_minute_slot_events() {
    let (dict, content) = rig(
        vec![verb_of(
            "PACE_A",
            vec![actor_slot()],
            60,
            &["NARRATE \"a done.\""],
        )],
        |raw| {
            raw.triggers = Some(vec![TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                id: "the_chime".to_string(),
                slot: 2,
                once: None,
                out: vec!["NARRATE \"the halfhour chime sings.\"".to_string()],
            })]);
        },
    );
    let state = seed();
    let r1 = run(&content, &dict, state, "0:0 @ ANA PACE_A", 1); // window [0,60]
    assert_eq!(r1.rejected, None);
    assert_eq!(r1.state.windows.len(), 1);
    assert_eq!(r1.state.now, 0, "beginning moves nothing");
    // pure advance to minute 60: completion at 60 BEFORE the slot-60 event
    let adv = run_clock(&content, &dict, r1.state, "0:2", r1.records.len() as i64 + 1);
    assert_eq!(adv.rejected, None);
    // scenario: anchor-complete-before-slot-event (frozen expectation)
    assert_eq!(kinds(&adv.records), ["complete", "event", "wait"]);
    assert_eq!(adv.records[0].trigger.as_deref(), Some("PACE_A"));
    assert_eq!(adv.records[1].trigger.as_deref(), Some("the_chime"));
    assert_eq!(adv.state.now, 60);
    // the slot cursor did NOT jump over the window's span: the halfhour the
    // window covered still struck — this ordering IS that pin (the chime at
    // 60 fires after the completion though the cursor conceptually lagged).
    // the fold agrees on the whole shape:
    let replayed = replay_all(&[&r1.records, &adv.records], &seed(), &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

/// §3.14 row 1b — chronological pass: earlier completion first, each
/// same-minute slot event follows its completion.
/// scenario: anchor-advance-chronological (frozen expectation): [complete FAST, event
/// mark, complete SLOW, event ninety, wait].
#[test]
fn advance_pass_is_chronological_across_windows_and_slots() {
    let (dict, content) = rig(
        vec![
            verb_of("SLOW", vec![actor_slot()], 90, &["NARRATE \"slow.\""]),
            verb_of("FAST", vec![actor_slot()], 30, &["NARRATE \"fast.\""]),
        ],
        |raw| {
            raw.triggers = Some(vec![
                TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                    id: "halfhour_mark".to_string(),
                    slot: 1,
                    once: None,
                    out: vec!["NARRATE \"mark.\"".to_string()],
                }),
                TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                    id: "ninety_mark".to_string(),
                    slot: 3,
                    once: None,
                    out: vec!["NARRATE \"ninety.\"".to_string()],
                }),
            ]);
        },
    );
    let state = seed();
    let r1 = run(&content, &dict, state, "0:0 @ ANA SLOW", 1); // [0,90]
    let r2 = run(&content, &dict, r1.state, "0:0 @ BERTHA FAST", r1.records.len() as i64 + 1); // [0,30]
    let adv = run_clock(
        &content,
        &dict,
        r2.state,
        "0:3",
        (r1.records.len() + r2.records.len()) as i64 + 1,
    );
    assert_eq!(adv.rejected, None);
    // scenario: anchor-advance-chronological (frozen expectation) — FAST closes at
    // 30 (before the halfhour mark), SLOW at 90 (before the ninety mark) —
    // completions lead, same-minute events follow.
    let shapes: Vec<(Option<&str>, Option<&str>)> = adv
        .records
        .iter()
        .map(|r| (Some(r.kind.as_str()), r.trigger.as_deref()))
        .collect();
    assert_eq!(
        shapes,
        vec![
            (Some("complete"), Some("FAST")),
            (Some("event"), Some("halfhour_mark")),
            (Some("complete"), Some("SLOW")),
            (Some("event"), Some("ninety_mark")),
            (Some("wait"), None),
        ]
    );
    assert_eq!(adv.state.now, 90);
    // ids minted in record order across the pass
    let ids: Vec<i64> = adv.records.iter().map(|r| r.id).collect();
    let first = (r1.records.len() + r2.records.len()) as i64 + 1;
    assert_eq!(ids, vec![first, first + 1, first + 2, first + 3, first + 4]);
    let replayed = replay_all(
        &[&r1.records, &r2.records, &adv.records],
        &seed(),
        &dict,
        &content,
    )
    .unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

/// §3.14 row 1c — due-tie broken by (beginId, clause): two same-end windows
/// from DIFFERENT begins close in begin order; two same-end windows from ONE
/// begin (clause offsets landing on the same minute) close in clause order.
/// . scenario: anchor-due-tie / anchor-due-tie-clause
/// (frozen expectation).
#[test]
fn due_window_ties_break_by_begin_id_then_clause() {
    // (a) beginId tie-break: two 30-minute windows both [0,30]
    let (dict, content) = rig(
        vec![
            verb_of("PACE_A", vec![actor_slot()], 30, &["NARRATE \"a.\""]),
            verb_of("PACE_B", vec![actor_slot()], 30, &["NARRATE \"b.\""]),
        ],
        |_| {},
    );
    let r1 = run(&content, &dict, seed(), "0:0 @ ANA PACE_A", 1); // beginId 1
    let r2 = run(&content, &dict, r1.state, "0:0 @ ANA PACE_B", 2); // beginId 2
    let adv = run_clock(&content, &dict, r2.state, "0:1", 3);
    assert_eq!(adv.rejected, None);
    let closes: Vec<Option<&str>> = adv
        .records
        .iter()
        .filter(|r| r.kind == "complete")
        .map(|r| r.trigger.as_deref())
        .collect();
    assert_eq!(closes, vec![Some("PACE_A"), Some("PACE_B")], "beginId 1 before 2");
    // (b) clause tie-break: ONE step, durations 30 + 0 → windows [0,30] and
    // [30,30]; same end, same beginId → clause 0 before clause 1
    let (dict, content) = rig(
        vec![
            verb_of("PACE_A", vec![actor_slot()], 30, &["NARRATE \"a.\""]),
            verb_of("PACE_Z", vec![actor_slot()], 0, &["NARRATE \"z.\""]),
        ],
        |_| {},
    );
    let s = parse_sentence("0:0 @ ANA PACE_A; ANA PACE_Z", &dict).unwrap();
    assert_eq!(s.clauses.len(), 2);
    let r = execute_step(
        &content,
        &dict,
        seed(),
        Step {
            start_min: None,
            clock: Some(Clock { day: 0, halfhour: 0 }),
            clauses: s.clauses,
            t_real: None,
        },
        ExecOptions {
            first_record_id: 1,
            id_gen: None,
        },
    )
    .unwrap();
    assert_eq!(r.rejected, None);
    let windows: Vec<(i64, i64)> = r.state.windows.values().map(|w| (w.start, w.end)).collect();
    // scenario: anchor-due-tie-clause (frozen expectation) — clause 0 spans [0,30],
    // clause 1 (offset 30, duration 0) [30,30]
    assert_eq!(windows, vec![(0, 30), (30, 30)]);
    let adv = run_clock(&content, &dict, r.state, "0:1", r.records.len() as i64 + 1);
    assert_eq!(adv.rejected, None);
    let closes: Vec<Option<&str>> = adv
        .records
        .iter()
        .filter(|r| r.kind == "complete")
        .map(|r| r.trigger.as_deref())
        .collect();
    assert_eq!(closes, vec![Some("PACE_A"), Some("PACE_Z")], "clause 0 before 1");
    let replayed = replay_all(&[&r.records, &adv.records], &seed(), &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

/// §3.14 row 2a — same-halfhour expiries fire in ENTITIES INSERTION order
/// (the hoisted snapshot preserves State.entities insert order).
/// scenario: anchor-expiry-insertion-order (frozen expectation).
#[test]
fn same_halfhour_expiries_fire_in_entity_insertion_order() {
    let (dict, content) = rig(vec![], |_| {});
    let mut state = seed();
    // insertion order ZOMBIE then APPLE — deliberately NOT name order
    expiring_item(&mut state, "z_card", "ZOMBIE_CARD", 30);
    expiring_item(&mut state, "a_card", "APPLE_CARD", 30);
    let base = clone_state(&state);
    let adv = run_clock(&content, &dict, state, "0:1", 1);
    assert_eq!(adv.rejected, None);
    // scenario: anchor-expiry-insertion-order (frozen expectation) — insertion
    // order, NOT alphabetical
    assert_eq!(kinds(&adv.records), ["event", "event", "wait"]);
    assert_eq!(triggers(&adv.records), [Some("expire:ZOMBIE_CARD"), Some("expire:APPLE_CARD"), None]);
    assert!(adv.state.entities["z_card"].dead_action.is_some());
    assert!(adv.state.entities["a_card"].dead_action.is_some());
    // the fold replays both destroys through the DESTROY #id rows
    let replayed = replay(&adv.records, &base, &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

/// §3.14 row 2b — authored `on expire of NAME`: FIRST match in declaration
/// order wins; a name with no authored trigger takes the built-in destroy.
/// . scenario: anchor-on-expire-first-match (frozen expectation).
#[test]
fn authored_on_expire_first_match_in_declaration_order() {
    let (dict, content) = rig(vec![], |raw| {
        raw.triggers = Some(vec![
            TriggerDef::Expire(chorus::content::content::ExpireTriggerDef {
                id: "decay_first".to_string(),
                on_expire_of: "INFLUENCE_MOTH_G".to_string(),
                out: vec!["NARRATE \"first wins.\"".to_string()],
            }),
            TriggerDef::Expire(chorus::content::content::ExpireTriggerDef {
                id: "decay_second".to_string(),
                on_expire_of: "INFLUENCE_MOTH_G".to_string(),
                out: vec!["NARRATE \"never fires.\"".to_string()],
            }),
        ]);
        raw.prototypes = Some(
            [(
                "PARANOIA".to_string(),
                PrototypeDef {
                    kind: "state".to_string(),
                    ..PrototypeDef::default()
                },
            )]
            .into_iter()
            .collect(),
        );
    });
    let mut state = seed();
    expiring_item(&mut state, "moth_g", "INFLUENCE_MOTH_G", 30);
    expiring_item(&mut state, "dust", "DUST", 30);
    let base = clone_state(&state);
    let adv = run_clock(&content, &dict, state, "0:1", 1);
    assert_eq!(adv.rejected, None);
    // scenario: anchor-on-expire-first-match (frozen expectation) — the first
    // authored match only, the un-authored name takes the built-in
    assert_eq!(
        triggers(&adv.records),
        [Some("decay_first"), Some("expire:DUST"), None]
    );
    // the first-match event row carries the AUTHORED id as its trigger and
    // its outcomes (the built-in row carries the DESTROY #id)
    let decay = &adv.records[0];
    assert_eq!(outcome_ops(decay), ["NARRATE"]);
    assert!(matches!(&decay.outcomes[0].args[0], Arg::Str { v } if v == "first wins."));
    let builtin = &adv.records[1];
    assert_eq!(outcome_ops(builtin), ["DESTROY"]);
    assert!(matches!(&builtin.outcomes[0].args[0], Arg::Ref { r#ref: Ref::Id { id } } if id == "dust"));
    // fired carries exactly the two event triggers (§9.34)
    let mut fired: Vec<&String> = adv.state.fired.iter().collect();
    fired.sort();
    let mut expect: Vec<&str> = vec!["decay_first", "expire:DUST"];
    expect.sort();
    assert_eq!(fired.iter().map(|s| s.as_str()).collect::<Vec<_>>(), expect);
    let replayed = replay(&adv.records, &base, &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

/// §3.14 row 2c — the expiry snapshot is hoisted ONCE per pass: expiries
/// CREATED by this pass's outcomes do not cascade; they resolve at the NEXT
/// advance. Also pins the §6.7 anchoring: SET expires computes from the
/// window's START clock (fireWindow now = w.start).
/// scenario: anchor-expiry-hoist-no-cascade
/// (frozen expectation: no expiry event in pass 1; EGG expires at minute
/// 60 = window START + 1h, not 90; the frozen egg sentinel is 2^53-1,
/// where this rig uses i64::MAX-1).
#[test]
fn expiry_snapshot_is_hoisted_no_cascade_next_pass_resolves() {
    let (dict, content) = rig(
        vec![verb_of(
            "INCUBATE",
            vec![actor_slot(), object_slot(Some("kind:item"))],
            30,
            &["SET(OBJECT, expires, 1h)"],
        )],
        |_| {},
    );
    let mut state = seed();
    expiring_item(&mut state, "egg", "EGG", i64::MAX - 1); // no expiry of its own yet
    let base = clone_state(&state);
    let r = run(&content, &dict, state, "0:0 @ ANA INCUBATE EGG", 1);
    assert_eq!(r.rejected, None);
    let post_begin = clone_state(&r.state);
    let adv1 = run_clock(&content, &dict, r.state, "0:1", r.records.len() as i64 + 1);
    assert_eq!(adv1.rejected, None);
    // pass 1: [complete, wait] — the SET's fresh expiresAt=60 does NOT fire
    // within this pass (the snapshot predates it), even though slot 1 (minute
    // 30) was crossed
    assert_eq!(kinds(&adv1.records), ["complete", "wait"]);
    let pre_adv1 = clone_state(&adv1.state);
    let replayed1 = replay(&adv1.records, &post_begin, &dict, &content).unwrap();
    assert_eq!(snap(&replayed1), snap(&pre_adv1));
    let _ = &base;
    let complete = &adv1.records[0];
    assert_eq!(outcome_ops(complete), ["SET"]);
    // scenario: anchor-expiry-hoist-no-cascade (frozen expectation) — anchored to
    // the window START (minute 0): 0 + 60 = 60
    let egg = &adv1.state.entities["egg"];
    assert_eq!(egg.attrs.get("expiresAt"), Some(&AttrValue::Num(60)));
    assert!(egg.dead_action.is_none());
    // pass 2: the snapshot now carries EGG@60; the pass walks 30 → 60 and the
    // slot-2 crossing (minute 60) fires it — at the END of the walk, after
    // any due windows (none here)
    let pre_adv2 = clone_state(&adv1.state);
    let adv2 = run_clock(&content, &dict, adv1.state, "0:2", adv1.records.len() as i64 + 1 + 10);
    assert_eq!(adv2.rejected, None);
    assert_eq!(kinds(&adv2.records), ["event", "wait"]);
    assert_eq!(adv2.records[0].trigger.as_deref(), Some("expire:EGG"));
    assert_eq!(adv2.state.now, 60);
    assert!(adv2.state.entities["egg"].dead_action.is_some());
    let replayed = replay(&adv2.records, &pre_adv2, &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv2.state));
}

/// §3.14 row 3 — REFUSE discards the whole advance pass (records AND clock
/// AND the would-have-closed window) and the id is RECYCLED: the refusal row
/// takes firstRecordId and the next attempt mints the same id again (the
/// fold_up_to `<` filter depends on it).
/// scenario: anchor-refuse-id-recycle (frozen expectation: refusal row id = 2 twice;
/// the real advance closes with ids 2,3).
#[test]
fn refuse_discards_advance_and_recycles_ids() {
    let (dict, content) = rig(
        vec![
            verb_of("SEEDV", vec![actor_slot()], 30, &["NARRATE \"planted.\""]),
            verb_of("GATE", vec![actor_slot()], 30, &["REFUSE(\"not without a lantern.\")"]),
        ],
        |_| {},
    );
    let base = seed();
    let r1 = run(&content, &dict, clone_state(&base), "0:0 @ ANA SEEDV", 1);
    assert_eq!(r1.records.len(), 1, "begin only — nothing due yet");
    assert_eq!(r1.state.windows.len(), 1);
    // the refused step at 0:1 WOULD have closed the window first (complete +
    // wait), but the whole pass dies with the refusal
    let r2 = run(&content, &dict, clone_state(&r1.state), "0:1 @ ANA GATE", 2);
    assert_eq!(r2.rejected, None, "a refusal is a logged result, not a rejection");
    assert_eq!(r2.records.len(), 1);
    assert_eq!(r2.records[0].id, 2, "a refusal row reuses the pass's firstRecordId");
    assert_eq!(r2.records[0].advanced_min, 0, "§6.5: no window elapses");
    assert_eq!(outcome_ops(&r2.records[0]), ["REFUSE"]);
    assert!(matches!(&r2.records[0].outcomes[0].args[0], Arg::Str { v } if v == "not without a lantern."));
    // state untouched: clock unmoved, window still open
    assert_eq!(r2.state.now, 0);
    assert_eq!(r2.state.windows.len(), 1);
    assert_eq!(snap(&r2.state), snap(&r1.state));
    // id recycle: the SAME attempt again mints the SAME id
    let r3 = run(&content, &dict, clone_state(&r1.state), "0:1 @ ANA GATE", 2);
    assert_eq!(r3.records[0].id, 2, "a recycled id mints identically on replay");
    // a real advance now closes the still-open window with ids 2,3
    let begin_only_state = clone_state(&r1.state);
    let adv = run_clock(&content, &dict, r1.state, "0:1", 2);
    assert_eq!(adv.records.iter().map(|r| r.id).collect::<Vec<_>>(), vec![2, 3]);
    assert_eq!(adv.state.windows.len(), 0);
    assert_eq!(adv.state.now, 30);
    // law 1 + fold_up_to: folding everything matches the played state; folding
    // ONLY the begin row (< 2) matches the pre-advance state — the `<`
    // filter is what makes undo-past-a-refusal work
    let mut full = r1.records.clone();
    full.extend(adv.records.clone());
    let played = {
        let mut st = begin_only_state;
        for rec in &adv.records {
            apply_record(&rec.clone(), &mut st, &dict, &content).unwrap();
        }
        st
    };
    assert_eq!(snap(&replay(&full, &base, &dict, &content).unwrap()), snap(&played));
    let undone = fold_up_to(&full, &new_state(0), &dict, &content, 2).unwrap();
    assert_eq!(undone.windows.len(), 1);
    assert_eq!(undone.now, 0);
}

/// §3.14 row 4 — the wait row is the ONLY record that moves `now`; wait
/// spans must partition time (fold assertion); the completion's tGame NEVER
/// feeds mechanics when its window is in the table (anchored to w.start) but
/// the bare-row tGame fallback exists for fold probes — keep it).
/// scenario: anchor-wait-partition — the fold
/// values below are the fold of that scenario's log (frozen).
#[test]
fn wait_is_the_only_clock_mover_and_must_partition_time() {
    let (dict, content) = rig(
        vec![verb_of("PACE", vec![actor_slot()], 30, &["NARRATE \"d.\""])],
        |_| {},
    );
    let r = run(&content, &dict, seed(), "0:0 @ ANA PACE", 1);
    let adv = run_clock(&content, &dict, r.state, "0:1", 2);
    assert_eq!(kinds(&adv.records), ["complete", "wait"]);
    // fold the BEGIN only: windows rebuild, clock does not move
    let begin_only = vec![r.records[0].clone()];
    let st = fold(&begin_only, &new_state(0), &dict, &content).unwrap();
    assert_eq!(st.now, 0, "BEGIN never moves the clock");
    assert_eq!(st.windows.len(), 1);
    // + the COMPLETE row: window closes, clock STILL does not move
    let begin_complete = vec![r.records[0].clone(), adv.records[0].clone()];
    let st = fold(&begin_complete, &new_state(0), &dict, &content).unwrap();
    assert_eq!(st.now, 0, "COMPLETE never moves the clock");
    assert_eq!(st.windows.len(), 0);
    // + the WAIT row: now lands exactly (the full log = begin + complete + wait)
    let mut full = r.records.clone();
    full.extend(adv.records.clone());
    let st = fold(&full, &seed(), &dict, &content).unwrap();
    assert_eq!(st.now, 30);
    assert_eq!(snap(&st), snap(&adv.state));
    // wait spans must partition time: a wait row whose start disagrees with
    // the folded clock is a loud fold error
    let mut drifted_wait = adv.records[1].clone();
    drifted_wait.start_min = Some(10);
    let mut st = fold(&begin_complete, &new_state(0), &dict, &content).unwrap();
    let err = apply_record(&drifted_wait, &mut st, &dict, &content).unwrap_err();
    assert!(
        err.msg.contains("wait row 3 starts at 0:0 but the folded clock reads 0"),
        "{err}"
    );
    // the tGame fallback for window-less COMPLETE rows (fold probes):
    // startMin anchors to absMin(parseClock(tGame)) when no window exists
    let bare_complete = ActionRecord {
        id: 9,
        kind: "complete".to_string(),
        trigger: Some("PROBE".to_string()),
        t_game: "0:5".to_string(),
        t_real: None,
        advanced_min: 0,
        start_min: None,
        window: None,
        actors: vec![],
        verb: "PROBE".to_string(),
        objects: vec![],
        instruments: vec![],
        at: None,
        about: None,
        to: None,
        outcomes: vec![chorus::core::sentence::Outcome {
            op: "SET".to_string(),
            args: vec![
                Arg::Ref {
                    r#ref: chorus::core::r#ref::name_ref("MAGICK_BOOK", None),
                },
                Arg::Key { v: "expires".to_string() },
                Arg::Dur { v: 30 },
            ],
            arms: None,
            fallback: None,
            call: None,
        }],
        recipes: vec![],
        sentence: String::new(),
    };
    let mut st = seed();
    apply_record(&bare_complete, &mut st, &dict, &content).unwrap();
    // scenario: anchor-wait-partition (frozen expectation, folded) — 0:5 = day 0,
    // halfhour 5 = minute 150; 150 + 30 = 180
    assert_eq!(st.entities["book"].attrs.get("expiresAt"), Some(&AttrValue::Num(180)));
}

/// §3.14 row 5 — a completion that cannot apply refuses ONLY that window:
/// the partial effects are discarded, the attempt is logged with a REFUSE
/// marker, the window still closes, and the pass does NOT roll back (no
/// livelock). scenario: anchor-window-completion-
/// failure (frozen expectation: [complete(REFUSE), wait]).
#[test]
fn window_completion_failure_refuses_only_that_window() {
    let (dict, content) = rig(
        vec![verb_of("BAD", vec![actor_slot()], 30, &["SPAWN NOSUCH_PROTOTYPE"])],
        |_| {},
    );
    let r = run(&content, &dict, seed(), "0:0 @ ANA BAD", 1);
    assert_eq!(r.rejected, None, "the BEGIN succeeds — the failure is deferred");
    assert_eq!(r.state.windows.len(), 1);
    let adv = run_clock(&content, &dict, r.state, "0:1", 2);
    assert_eq!(adv.rejected, None, "the pass did NOT roll back");
    assert_eq!(kinds(&adv.records), ["complete", "wait"]);
    let complete = &adv.records[0];
    assert_eq!(outcome_ops(complete), ["REFUSE"]);
    assert!(matches!(&complete.outcomes[0].args[0], Arg::Str { v } if v.contains("NOSUCH_PROTOTYPE")));
    assert_eq!(adv.state.windows.len(), 0, "the window is gone — no retry loop");
    assert_eq!(adv.state.now, 30);
    // a second advance is healthy
    let adv_state = clone_state(&adv.state);
    let adv2 = run_clock(&content, &dict, adv.state, "0:2", 10);
    assert_eq!(adv2.rejected, None);
    assert_eq!(kinds(&adv2.records), ["wait"]);
    // the fold agrees: the REFUSE complete row is a no-op that closed its window
    let replayed = replay_all(&[&r.records, &adv.records], &seed(), &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv_state));
}

/// §3.14 row 6 — the first ENDING wins; later ENDINGs are logged no-ops; the
/// ended state gates every later step with the story-over message (§9.35).
/// . scenario: anchor-first-ending (frozen expectation:
/// ended.id = QUIET_DUSK).
#[test]
fn first_ending_wins_and_gates_the_story() {
    let (dict, content) = rig(
        vec![verb_of(
            "FINISH",
            vec![actor_slot()],
            30,
            &["ENDING QUIET_DUSK", "ENDING OTHER_END"],
        )],
        |raw| {
            raw.endings = Some(
                [
                    (
                        "QUIET_DUSK".to_string(),
                        EndingDef {
                            title: "A quiet dusk.".to_string(),
                            flavour: Some("calm".to_string()),
                        },
                    ),
                    (
                        "OTHER_END".to_string(),
                        EndingDef {
                            title: "Another end.".to_string(),
                            flavour: None,
                        },
                    ),
                ]
                .into_iter()
                .collect(),
            );
        },
    );
    let r = run(&content, &dict, seed(), "0:0 @ ANA FINISH", 1);
    assert_eq!(r.rejected, None);
    assert_eq!(r.state.ended, None, "the ending rides the completion");
    let adv = run_clock(&content, &dict, r.state, "0:1", 2);
    assert_eq!(adv.rejected, None);
    let complete = &adv.records[0];
    assert_eq!(outcome_ops(complete), ["ENDING", "ENDING"], "both logged");
    assert_eq!(
        adv.state.ended.as_ref().map(|e| e.id.as_str()),
        Some("QUIET_DUSK"),
        "first ENDING to APPLY wins"
    );
    assert_eq!(adv.state.ended.as_ref().unwrap().title, "A quiet dusk.");
    let adv_state = clone_state(&adv.state);
    // the story-over gate: nothing further executes, nothing logged
    let gated = run(&content, &dict, adv.state, "0:2 @ ANA FINISH", 10);
    assert_eq!(
        gated.rejected.as_deref(),
        Some("the story has ended — A quiet dusk.")
    );
    assert!(gated.records.is_empty());
    // the fold projects the same ending from the log alone
    let replayed = replay_all(&[&r.records, &adv.records], &seed(), &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv_state));
}

/// §3.14 row 7a — ROLL last-wins inside one outcome list: the concrete rows
/// carry both ROLLED results and the NARRATE prose carries the SECOND total.
/// . scenario: anchor-roll-last-wins (frozen expectation: the
/// ROLLED (total, ctrAfter) pairs and the second-total prose ride the
/// frozen complete row verbatim).
#[test]
fn roll_binds_last_wins_within_a_list() {
    let (dict, content) = rig(
        vec![verb_of(
            "CAST",
            vec![actor_slot()],
            0,
            &["ROLL(1, 6)", "ROLL(1, 6)", "NARRATE \"r={roll}.\""],
        )],
        |_| {},
    );
    let dice_seed = content.seed;
    let r = run(&content, &dict, seed(), "0:0 @ ANA CAST", 1);
    assert_eq!(r.rejected, None);
    let adv = run_clock(&content, &dict, r.state, "0:0", 2); // zero-duration windows are due now
    assert_eq!(adv.rejected, None);
    let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
    let ops = outcome_ops(complete);
    assert_eq!(ops, ["ROLLED", "ROLLED", "NARRATE"]);
    let (Arg::Int { v: t1 }, Arg::Int { v: c1 }) = (&complete.outcomes[0].args[0], &complete.outcomes[0].args[1])
    else {
        panic!("ROLLED(total, ctrAfter)");
    };
    let (Arg::Int { v: t2 }, Arg::Int { v: c2 }) = (&complete.outcomes[1].args[0], &complete.outcomes[1].args[1])
    else {
        panic!("ROLLED(total, ctrAfter)");
    };
    // scenario: anchor-roll-last-wins (frozen expectation) — counters 0 and 1
    // consumed in order
    assert_eq!((*t1, *c1, *t2, *c2),
        (chorus::core::dice::roll_die(dice_seed, 0, 6), 1, chorus::core::dice::roll_die(dice_seed, 1, 6), 2));
    assert_eq!(adv.state.rng, 2);
    // the NARRATE carries the SECOND total (last-wins)
    assert!(matches!(&complete.outcomes[2].args[0], Arg::Str { v } if *v == format!("r={t2}.")));
    // the fold lands the counter from the ROLLED rows alone (law 3)
    let replayed = replay_all(&[&r.records, &adv.records], &seed(), &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

/// §3.14 row 7b — the dice binding is PER CLAUSE: a
/// ROLL in clause one is NOT visible to clause two's IF — its unbound
/// `roll` refuses clause two's window at completion (the IF is opaque to
/// the gate, so the step itself commits).
/// scenario: anchor-roll-per-clause (frozen expectation: complete rows
/// [CAST: ROLLED+NARRATE, BARE: ROLLED+NARRATE on its own roll]).
#[test]
fn roll_binding_resets_per_clause() {
    let cast = VerbContent {
        slots: vec![actor_slot()],
        duration: 0,
        recipes: vec![RecipeDef {
            id: None,
            when: None,
            shown: None,
            hint: None,
            max: None,
            out: vec![
                "ROLL(1, 6)".to_string(),
                "IF roll >= 0 -> (NARRATE \"bound.\")".to_string(),
            ],
        }],
    };
    let bare = VerbContent {
        slots: vec![actor_slot()],
        duration: 0,
        recipes: vec![RecipeDef {
            id: None,
            when: None,
            shown: None,
            hint: None,
            max: None,
            out: vec![
                "ROLL(1, 6)".to_string(),
                "IF roll >= 0 -> (NARRATE \"bare {roll}.\")".to_string(),
            ],
        }],
    };
    let (dict, content) = rig(vec![("CAST".to_string(), cast), ("BARE".to_string(), bare)], |_| {});
    let dice_seed = content.seed;
    let s = parse_sentence("0:0 @ ANA CAST; ANA BARE", &dict).unwrap();
    let r = execute_step(
        &content,
        &dict,
        seed(),
        Step {
            start_min: None,
            clock: Some(Clock { day: 0, halfhour: 0 }),
            clauses: s.clauses,
            t_real: None,
        },
        ExecOptions {
            first_record_id: 1,
            id_gen: None,
        },
    )
    .unwrap();
    assert_eq!(r.rejected, None, "the IF is opaque to the gate — the step commits");
    assert_eq!(r.state.windows.len(), 2);
    // both zero-duration windows are due at the same minute; clause order
    // settles them (row 1c)
    let adv = run_clock(&content, &dict, r.state, "0:0", 2);
    assert_eq!(adv.rejected, None);
    let completes: Vec<&ActionRecord> = adv
        .records
        .iter()
        .filter(|x| x.kind == "complete")
        .collect();
    assert_eq!(completes.len(), 2);
    assert_eq!(completes[0].trigger.as_deref(), Some("CAST"));
    assert_eq!(outcome_ops(completes[0]), ["ROLLED", "NARRATE"], "clause one saw its roll");
    assert_eq!(outcome_ops(completes[1]), ["ROLLED", "NARRATE"], "clause two rolled on its own");
    // scenario: anchor-roll-per-clause (frozen expectation) — CAST consumes counter
    // 0 (one die); BARE consumes counter 1 — if the binding leaked, clause
    // two's prose would carry clause one's total
    let cast_total = chorus::core::dice::roll_die(dice_seed, 0, 6);
    let bare_total = chorus::core::dice::roll_die(dice_seed, 1, 6);
    assert!(matches!(&completes[0].outcomes[1].args[0], Arg::Str { v } if v == "bound."));
    assert!(matches!(&completes[1].outcomes[1].args[0], Arg::Str { v } if *v == format!("bare {bare_total}.")),
        "clause two must see ITS OWN roll, not {:?}", completes[1].outcomes[1].args[0]);
    assert_eq!(adv.state.rng, 2);
    let _ = cast_total;
    let replayed = replay_all(&[&r.records, &adv.records], &seed(), &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

/// §3.14 row 8 — ADVANCE extends the pass target and the extension rides the
/// wait row's span (replay-safe: the fold never re-derives it). Also pins
/// the zero-duration-window due-at-begin minute and the busy/station gate.
/// . scenario: anchor-advance-extension /
/// anchor-flash-zero-duration (frozen expectation: wait span 90 = 30 (window) +
/// 60 (ADVANCE)).
#[test]
fn advance_extension_lands_in_the_wait_span() {
    let (dict, content) = rig(
        vec![verb_of(
            "HATCH",
            vec![actor_slot()],
            30,
            &["ADVANCE(1h)", "NARRATE \"warm.\""],
        )],
        |_| {},
    );
    let r = run(&content, &dict, seed(), "0:0 @ ANA HATCH", 1);
    assert_eq!(r.records[0].advanced_min, 30, "advancedMin is the window length, not the advance");
    let adv = run_clock(&content, &dict, r.state, "0:1", 2);
    assert_eq!(adv.rejected, None);
    assert_eq!(kinds(&adv.records), ["complete", "wait"]);
    let wait = &adv.records[1];
    // scenario: anchor-advance-extension (frozen expectation) — the wait row spans
    // [0, 90] — the ADVANCE extended the target
    assert_eq!((wait.advanced_min, wait.start_min), (90, Some(0)));
    assert_eq!(adv.state.now, 90);
    // zero-duration verb: due exactly at its begin minute (the complement of
    // the branch where a window's end minute is still in the future)
    let (dict, content) = rig(vec![verb_of("FLASH", vec![actor_slot()], 0, &["NARRATE \"zap.\""])], |_| {});
    let r = run(&content, &dict, seed(), "0:0 @ ANA FLASH", 1);
    let adv = run_clock(&content, &dict, r.state, "0:0", 2);
    assert_eq!(adv.rejected, None, "a due window makes the zero-span pass meaningful");
    assert_eq!(kinds(&adv.records), ["complete", "wait"]);
    assert_eq!(adv.records[1].advanced_min, 0);
    assert_eq!(adv.state.now, 0);
}

/// §3.14 row 9 — the station/lock gates (§6.7): one window per verb station;
/// committed cards busy in ANY committed role while actors stay free; locks
/// release exactly when the pass resolves their window.
/// scenario: anchor-station-locks (frozen expectation — the two rejection
/// messages are frozen byte-for-byte).
#[test]
fn stations_and_committed_locks_gate_phase_two() {
    let seize = VerbContent {
        slots: vec![
            SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            },
            object_slot(None),
        ],
        duration: 60,
        recipes: vec![RecipeDef {
            id: None,
            when: None,
            shown: None,
            hint: None,
            max: None,
            out: vec!["NARRATE \"seized.\"".to_string()],
        }],
    };
    let touch = VerbContent {
        slots: vec![actor_slot(), object_slot(None)],
        duration: 30,
        recipes: vec![RecipeDef {
            id: None,
            when: None,
            shown: None,
            hint: None,
            max: None,
            out: vec!["NARRATE \"touched.\"".to_string()],
        }],
    };
    let (dict, content) = rig(vec![("SEIZE".to_string(), seize), ("TOUCH".to_string(), touch)], |_| {});
    let r1 = run(&content, &dict, seed(), "0:0 @ ANA SEIZE MAGICK_BOOK", 1);
    assert_eq!(r1.state.windows.len(), 1);
    // station exclusivity — same verb cannot open a second window
    let r2 = run(&content, &dict, clone_state(&r1.state), "0:0 @ BERTHA SEIZE", 3);
    assert_eq!(
        r2.rejected.as_deref(),
        Some("SEIZE is already processing — its station frees at 0:2")
    );
    assert!(r2.records.is_empty());
    // committed card busy (as object of another verb); actors stay free
    let r3 = run(&content, &dict, clone_state(&r1.state), "0:0 @ ANA TOUCH MAGICK_BOOK", 3);
    assert_eq!(
        r3.rejected.as_deref(),
        Some("MAGICK BOOK is busy with SEIZE — it frees at 0:2")
    );
    let r4 = run(&content, &dict, clone_state(&r1.state), "0:0 @ BERTHA TOUCH", 3);
    assert_eq!(r4.rejected, None, "the actor is never locked");
    assert_eq!(r4.state.windows.len(), 2, "two stations run in parallel");
    // locks release the moment a pass resolves their window: the explicit
    // future clock advances first (closing SEIZE), then begins TOUCH
    let r5 = run(&content, &dict, r1.state, "1:0 @ ANA TOUCH MAGICK_BOOK", 3);
    assert_eq!(r5.rejected, None);
    assert_eq!(r5.records[0].kind, "complete", "the SEIZE window closed en route");
}

/// §3.14 row 10 — interleaved windows fold byte-for-byte: two overlapping
/// stations + a JSONL save/load/fold round trip over a log that ENDS with
/// open windows; fold_up_to drops the second turn whole.
/// scenario: anchor-overlapping-windows (frozen expectation ids/windows).
#[test]
fn overlapping_windows_fold_and_survive_a_jsonl_roundtrip() {
    let (dict, content) = rig(
        vec![
            verb_of("PACE_A", vec![actor_slot()], 60, &["NARRATE \"a.\""]),
            verb_of("PACE_B", vec![actor_slot()], 30, &["NARRATE \"b.\""]),
        ],
        |_| {},
    );
    let r1 = run(&content, &dict, seed(), "0:0 @ ANA PACE_A", 1); // [0,60]
    let r1_state = clone_state(&r1.state);
    let r2 = run(&content, &dict, r1.state, "0:0 @ ANA PACE_B", r1.records.len() as i64 + 1); // [0,30]
    assert_eq!(r2.rejected, None);
    assert_eq!(r2.state.windows.len(), 2);
    assert_eq!(r2.state.now, 0);
    let mut all = r1.records.clone();
    all.extend(r2.records.clone());
    // JSONL round trip with BOTH windows open: the fold rebuilds them
    let dir = std::env::temp_dir().join(format!("chorus-ordering-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("log.jsonl");
    chorus::engine::store::save_log(&path, &all).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let loaded = chorus::engine::store::load_log(&path).unwrap();
    assert_eq!(loaded, all);
    // byte identity: re-saving the loaded log reproduces the same bytes
    let path2 = dir.join("log2.jsonl");
    chorus::engine::store::save_log(&path2, &loaded).unwrap();
    assert_eq!(bytes, std::fs::read(&path2).unwrap(), "law 1 within one implementation");
    let replayed = fold(&loaded, &seed(), &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&r2.state));
    assert_eq!(replayed.windows.len(), 2);
    // window table shape (scenario: anchor-overlapping-windows, frozen expectation):
    // PACE_A [0,60] key "1:0", PACE_B [0,30] key "2:0"
    let wa = r2.state.windows.get("1:0").unwrap();
    let wb = r2.state.windows.get("2:0").unwrap();
    assert_eq!((wa.verb.as_str(), wa.start, wa.end), ("PACE_A", 0, 60));
    assert_eq!((wb.verb.as_str(), wb.start, wb.end), ("PACE_B", 0, 30));
    // undo turn 2 (fold_up_to < 3): only the first begin remains
    let undone = fold_up_to(&all, &seed(), &dict, &content, r1.records.len() as i64 + 1).unwrap();
    assert_eq!(undone.windows.len(), 1);
    assert_eq!(snap(&undone), snap(&r1_state));
    let _ = std::fs::remove_dir_all(&dir);
}

/// §3.14 row 11 — events do not respect locks: an expiry destroys a locked
/// card mid-window and the completion DROPS the dead ref (the SET finds no
/// targets; the DESTROY concretizes empty; dead
/// refs DROP) — scenario: anchor-expiry-locked-drop (frozen expectation: complete
/// outcomes = [DESTROY] with [] args).
#[test]
fn expiry_of_a_locked_card_drops_the_completion_refs() {
    let (dict, content) = rig(
        vec![verb_of(
            "SEIZE",
            vec![actor_slot(), object_slot(None)],
            60,
            &["SET(OBJECT, aspect:touched, 1), DESTROY OBJECT"],
        )],
        |_| {},
    );
    let mut state = seed();
    state.entities.get_mut("book").unwrap().attrs.insert(
        "expiresAt".to_string(),
        AttrValue::Num(30),
    );
    let base = clone_state(&state);
    let r1 = run(&content, &dict, state, "0:0 @ ANA SEIZE MAGICK_BOOK", 1);
    assert_eq!(r1.state.windows.len(), 1);
    assert!(r1.state.windows.values().any(|w| w.locked.iter().any(|id| id == "book")));
    let adv = run_clock(&content, &dict, r1.state, "1:0", 2);
    assert_eq!(adv.rejected, None);
    // the expiry event fired at slot 1 (minute 30) while the card was locked
    assert_eq!(adv.records[0].trigger.as_deref(), Some("expire:MAGICK_BOOK"));
    assert!(adv.state.entities["book"].dead_action.is_some());
    // the completion dropped the dead ref: SET found no targets (concretized
    // per-card → none), DESTROY concretized to an empty row
    let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
    assert_eq!(outcome_ops(complete), ["DESTROY"]);
    assert!(complete.outcomes[0].args.is_empty());
    assert!(!adv.state.by_name.contains_key("MAGICK_BOOK"));
    let replayed = replay_all(&[&r1.records, &adv.records], &base, &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv.state));
}

/// §3.14 row 12 — slot cadence: a trigger fires once per crossed halfhour in
/// declaration order across MULTIPLE slots, and `once` fires only the first
/// crossing. scenario: anchor-slot-cadence-once
/// (frozen expectation: [mark@30, once_dusk@30, mark@60, wait]).
#[test]
fn slot_triggers_fire_per_crossing_in_declaration_order_and_once_stays_once() {
    let (dict, content) = rig(
        vec![],
        |raw| {
            raw.triggers = Some(vec![
                TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                    id: "mark".to_string(),
                    slot: 1,
                    once: None,
                    out: vec!["NARRATE \"mark.\"".to_string()],
                }),
                TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                    id: "mark".to_string(), // same id, later declaration — first match wins per slot
                    slot: 2,
                    once: None,
                    out: vec!["NARRATE \"mark again.\"".to_string()],
                }),
                TriggerDef::Slot(chorus::content::content::SlotTriggerDef {
                    id: "once_dusk".to_string(),
                    slot: 1,
                    once: Some(true),
                    out: vec!["NARRATE \"dusk.\"".to_string()],
                }),
            ]);
        },
    );
    let adv = run_clock(&content, &dict, seed(), "0:2", 1); // walk 0 → 60
    assert_eq!(adv.rejected, None);
    // scenario: anchor-slot-cadence-once (frozen expectation) — slot 1 (minute 30):
    // mark then once_dusk (declaration order); slot 2 (minute 60): the
    // second mark declaration
    assert_eq!(
        triggers(&adv.records),
        [
            Some("mark"),
            Some("once_dusk"),
            Some("mark"), // the slot-2 declaration REUSES the id — fired only gates `once`
            None,
        ]
    );
    assert_eq!(adv.state.now, 60);
    // once: dusk fired exactly once — the NEXT slot-1 crossing (abs slot 49,
    // minute 1470, day 1) refires `mark` (slot 2 declaration at abs 50,
    // minute 1500) but NOT `once_dusk`
    let pre = clone_state(&adv.state);
    let adv2 = run_clock(&content, &dict, adv.state, "1:4", 10); // walk 60 → 1560
    let t2: Vec<Option<&str>> = adv2.records.iter().map(|r| r.trigger.as_deref()).collect();
    assert!(t2.contains(&Some("mark")), "non-once refires per crossing: {t2:?}");
    assert!(!t2.contains(&Some("once_dusk")), "once means once (§9.29)");
    assert_eq!(adv2.state.now, 1560);
    let replayed = replay(&adv2.records, &pre, &dict, &content).unwrap();
    assert_eq!(snap(&replayed), snap(&adv2.state));
}
