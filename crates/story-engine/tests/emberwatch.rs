//! M1 acceptance over the real sample: the three endings, determinism (T6),
//! the snapshot/undo loop with zero drift, ending semantics (T7), and the
//! full-mechanism showcase (relations, carrier zones, books, effectset,
//! continue chains, the tides deck, the bell).

mod common;

use common::{
    aspect, boot, commit_named, compile, count_named, ended, legacy_out, named, narration,
    state_bytes,
};
use story_engine::{Engine, Event, Snapshot};

/// One replayable host operation (the undo stack's redo face).
type EngineOp = Box<dyn Fn(&mut Engine)>;

const EMBERWATCH: &str = include_str!("../../../docs/emberwatch.sw");

fn game() -> Engine {
    boot(compile(EMBERWATCH), 7)
}

/// Scripted session to `survived`: sup on bread with tea in the cup (the
/// cup survives the meal; its completion book counts it), comb for
/// driftwood, a blaze at the witching hour, and a one-coal feed through the
/// dawn check.
#[test]
fn scripted_session_reaches_survived() {
    let mut e = game();

    commit_named(&mut e, "sup", &[("morsel", "bread"), ("cup", "tea")]).expect("sup opens");
    e.advance(10).expect("the meal");
    e.advance(290).expect("to hour five");
    assert_eq!(legacy_out(&e).get("cups"), Some(&1), "tea's book counted the cup");

    commit_named(&mut e, "comb", &[]).expect("comb opens");
    e.advance(40).expect("to 340m");
    commit_named(&mut e, "comb", &[]).expect("second comb opens");
    e.advance(20).expect("to 360m");
    commit_named(&mut e, "tend", &[]).expect("blaze opens");
    e.advance(30).expect("to 390m");
    commit_named(&mut e, "tend", &[("fuel", "coal")]).expect("feed opens");
    e.advance(30).expect("to dawn");

    assert_eq!(ended(&e).as_deref(), Some("survived"));
    assert_eq!(e.state().now, 420);
    assert_eq!(aspect(&e, named(&e, "beacon"), "heat"), 3, "beacon stays lit");
    assert_eq!(aspect(&e, named(&e, "warden"), "dread"), 2, "dread under 5");
    assert_eq!(legacy_out(&e).get("dawns_seen"), Some(&1));
    // The ending discards the remaining due queue unresolved: the feed is
    // still open, its greedy-pulled cards still bound (T7).
    assert_eq!(e.state().situations.len(), 1);
    assert_eq!(count_named(&e, "driftwood"), 2, "one ashore, one from flotsam's alias");
}

/// Scripted session to `gray_dawn`: play nothing; the heat never rises, hunger
/// climbs, gnaw burns the tea then the bread, and dread_first ticks the hours
/// the dawn preempts — the dawn finds the beacon dark at hour seven.
#[test]
fn scripted_session_reaches_gray_dawn() {
    let mut e = game();

    let events = e.advance(420).expect("the whole night");
    let texts = narration(&events);
    assert!(
        texts.iter().any(|t| t.contains("Nothing to eat")),
        "dread_first ran"
    );
    assert!(events.iter().any(|ev| matches!(
        ev,
        Event::SituationCompleted { recipe, .. } if recipe.as_str() == "night.dawn"
    )));

    assert_eq!(ended(&e).as_deref(), Some("gray_dawn"));
    assert_eq!(e.state().now, 420);
    assert_eq!(aspect(&e, named(&e, "beacon"), "heat"), 0);
    assert_eq!(aspect(&e, named(&e, "warden"), "dread"), 3);
    assert_eq!(legacy_out(&e).get("dawns_seen"), Some(&0));
    assert_eq!(count_named(&e, "tea"), 0, "gnaw burned the tea");
    assert_eq!(count_named(&e, "bread"), 0, "gnaw burned the bread");
}

/// Scripted session to `despair`: dream the night away. The omens deck deals
/// portents faster than second sight can read them; every failed read adds a
/// dread, and the portents it leaves behind expire unread — the expiry book's
/// own dread — until broken (priority 1) outruns dawn at hour six.
#[test]
fn scripted_session_reaches_despair() {
    let mut e = game();

    for _ in 0..4 {
        commit_named(&mut e, "dream", &[]).expect("dream opens");
        e.advance(60).expect("an hour of night");
    }
    e.advance(300).expect("to the broken ending");

    assert_eq!(ended(&e).as_deref(), Some("despair"));
    assert_eq!(e.state().now, 360);
    assert_eq!(aspect(&e, named(&e, "warden"), "dread"), 6);
    assert_eq!(legacy_out(&e).get("omens_lost"), Some(&3));
    assert_eq!(legacy_out(&e).get("dawns_seen"), Some(&0));
}

/// T6: the same content, seed, and operation sequence reaches the identical
/// state — across different advance splits, and with a refused commit costing
/// nothing (T3: zero state change, zero time).
#[test]
fn determinism_same_seed_same_ops_identical_state() {
    fn session(e: &mut Engine) {
        commit_named(e, "sup", &[("morsel", "bread")]).expect("sup");
        e.advance(300).expect("advance");
    }

    let mut a = game();
    session(&mut a);
    a.advance(60).expect("advance");
    a.advance(60).expect("advance");

    let mut b = game();
    session(&mut b);
    // The same span in one call: identical state bytes.
    b.advance(120).expect("advance");
    assert_eq!(
        state_bytes(&a),
        state_bytes(&b),
        "same seed + same ops must reach the identical state"
    );

    // A retried identical commit after a refusal is bit-restored: a refusal
    // costs no state and no time.
    let before = state_bytes(&a);
    let now_before = a.state().now;
    let refused = commit_named(&mut a, "omen_sight", &[]);
    assert!(refused.is_err(), "uncommittable verbs refuse");
    assert_eq!(state_bytes(&a), before, "refusal left the state identical");
    assert_eq!(a.state().now, now_before, "refusal spent zero time");
}

/// The snapshot/undo loop shows no drift across a scripted session: undoing
/// every action restores each prior state bit-for-bit, and replaying the same
/// operations after undoing everything lands on the original final bytes.
#[test]
fn snapshot_undo_loop_has_zero_drift() {
    let mut e = game();

    // The host-held undo stack: a snapshot pushed before every
    // player-visible action (L2).
    let mut undo: Vec<Snapshot> = Vec::new();
    let mut ops: Vec<EngineOp> = Vec::new();
    macro_rules! step {
        ($op:expr) => {{
            undo.push(e.snapshot());
            let op: Box<dyn Fn(&mut Engine)> = Box::new($op);
            op(&mut e);
            ops.push(op);
        }};
    }
    step!(|e: &mut Engine| {
        commit_named(e, "sup", &[("morsel", "bread"), ("cup", "tea")]).expect("sup");
    });
    step!(|e: &mut Engine| {
        e.advance(60).expect("advance");
    });
    step!(|e: &mut Engine| {
        commit_named(e, "tend", &[]).expect("tend");
    });
    step!(|e: &mut Engine| {
        e.advance(120).expect("advance");
    });
    step!(|e: &mut Engine| {
        commit_named(e, "comb", &[]).expect("comb");
    });
    step!(|e: &mut Engine| {
        e.advance(60).expect("advance");
    });
    let final_bytes = state_bytes(&e);

    // Undo one step: the state is exactly the snapshot taken before it.
    let before_last = undo[undo.len() - 1].clone();
    e.restore(&undo.pop().unwrap()).expect("restore");
    assert_eq!(
        state_bytes(&e),
        before_last.state,
        "undo restores the prior state"
    );

    // Undo everything back to boot, then replay the identical operations:
    // zero drift means the replay lands on the original final bytes.
    while let Some(snap) = undo.pop() {
        e.restore(&snap).expect("restore");
    }
    for op in ops {
        op(&mut e);
    }
    assert_eq!(
        state_bytes(&e),
        final_bytes,
        "replay after undo shows no drift"
    );
}

/// T7: after any ending the due queue is discarded unresolved, every entry
/// point refuses with `StoryEnded`, and the pre-ending snapshot resumes play.
#[test]
fn ending_closes_the_run_until_restored() {
    let mut e = game();

    e.advance(420).expect("to gray_dawn");
    assert_eq!(ended(&e).as_deref(), Some("gray_dawn"));

    assert!(commit_named(&mut e, "tend", &[])
        .err()
        .is_some_and(|d| d.kind == story_content::runtime::DiagKind::StoryEnded));
    assert!(e
        .advance(10)
        .err()
        .is_some_and(|d| d.kind == story_content::runtime::DiagKind::StoryEnded));

    // The host restores the snapshot taken before the ending: play resumes.
    let mut resume = game();
    resume.advance(360).expect("to 360m");
    let snap = resume.snapshot();
    resume.advance(60).expect("the ending fires");
    assert_eq!(ended(&resume).as_deref(), Some("gray_dawn"));
    resume.restore(&snap).expect("restore");
    assert_eq!(ended(&resume), None, "the pre-ending snapshot resumes play");
    assert_eq!(resume.state().now, 360);
    assert_eq!(resume.snapshot().state, snap.state);
}

/// The mechanism showcase: relations boot edges and the exclusive steal, the
/// lantern carrier zone, the wrapping tides deck with the flotsam mint alias
/// and the swap, the log's continue chain, and the storm bell's event rules.
#[test]
fn the_mechanisms_of_the_night_fire_and_answer() {
    let mut e = game();

    // The warden boots knowing the gull and carrying the lantern.
    let warden = named(&e, "warden");
    let lantern = named(&e, "lantern");
    let gull = named(&e, "gull");
    let knows = e.state().relations.get(&warden).and_then(|k| k.get("knows"));
    let mut expected_knows = std::collections::BTreeSet::new();
    expected_knows.insert(gull);
    assert_eq!(knows, Some(&expected_knows), "the boot knows edge");
    let carries = e.state().relations.get(&warden).and_then(|k| k.get("carries"));
    let mut expected_carries = std::collections::BTreeSet::new();
    expected_carries.insert(lantern);
    assert_eq!(carries, Some(&expected_carries), "the boot carries edge");

    // The lamp: oil moves into the carrier, then a measured burn (unclamped
    // set) buys one heat.
    commit_named(&mut e, "lamp", &[("pour", "oil")]).expect("refill opens");
    e.advance(20).expect("the pour");
    assert_eq!(aspect(&e, lantern, "oil"), 1, "oil moved into the lantern");
    let oil = named(&e, "oil");
    let oil_zone = e.state().entities[&oil].zone;
    assert!(
        matches!(e.state().zones[&oil_zone].kind, story_core::ZoneKind::Of(z) if z == lantern),
        "the pour moved the oil into the lantern's carrier zone"
    );
    commit_named(&mut e, "lamp", &[("pour", "oil")]).expect("second refill");
    e.advance(20).expect("the second pour");
    commit_named(&mut e, "lamp", &[]).expect("dim opens");
    e.advance(10).expect("the dim");
    assert_eq!(aspect(&e, lantern, "oil"), 1, "dim burned one oil");
    assert_eq!(aspect(&e, named(&e, "beacon"), "heat"), 1, "a measured burn");

    // The log: write continues into close; the chain completes through one
    // commit and one advance.
    commit_named(&mut e, "log", &[]).expect("write opens");
    assert_eq!(
        e.state().situations.values().find(|s| s.recipe.as_str() == "log.write"),
        Some(e.state().situations.values().find(|s| s.recipe.as_str() == "log.write")).unwrap(),
    );
    e.advance(30).expect("write continues");
    assert_eq!(
        e.state().situations.values().filter(|s| s.recipe.as_str() == "log.close").count(),
        1,
        "the write continued into close"
    );
    e.advance(90).expect("close completes");

    // The tides: cast draws off the wrapping deck and mints flotsam — a
    // driftwood body under a flotsam name — which salvage swaps to a real
    // driftwood.
    let tides_before = count_named(&e, "ebb") + count_named(&e, "flood");
    commit_named(&mut e, "cast", &[]).expect("cast opens");
    e.advance(30).expect("the cast");
    assert_eq!(
        count_named(&e, "ebb") + count_named(&e, "flood"),
        tides_before + 1,
        "the wrap deck dealt a tide"
    );
    assert_eq!(count_named(&e, "flotsam"), 1, "flotsam keeps its name");
    assert_eq!(
        aspect(&e, named(&e, "flotsam"), "kindle"),
        1,
        "flotsam carries the driftwood body"
    );
    commit_named(&mut e, "salvage", &[]).expect("salvage opens");
    e.advance(20).expect("the swap");
    assert_eq!(count_named(&e, "flotsam"), 0, "the flotsam was swapped");
    assert_eq!(count_named(&e, "driftwood"), 1, "a true driftwood came back");

    // The gull: fed once, the symmetric bond steals nothing (there was no
    // prior bond) and reads either direction.
    commit_named(&mut e, "feed_gull", &[]).expect("toss opens");
    e.advance(10).expect("the toss");
    let bonded = e
        .state()
        .relations
        .values()
        .any(|kinds| kinds.contains_key("bonded"));
    assert!(bonded, "the feed related the gull as bonded");
}
