//! M1 acceptance over the real sample: the three endings, determinism (T6),
//! the snapshot/undo loop with zero drift, and ending semantics (T7).

mod common;

use common::{
    aspect, boot, commit_named, compile, count_named, ended, named, narration, state_bytes,
};
use story_engine::{Engine, Event, Snapshot};

/// One replayable host operation (the undo stack's redo face).
type EngineOp = Box<dyn Fn(&mut Engine)>;

const EMBERWATCH: &str = include_str!("../../../docs/emberwatch.sw");

fn game() -> Engine {
    boot(compile(EMBERWATCH), 7)
}

/// Scripted session to `survived`: tend (blaze) for heat, sup against hunger,
/// comb to wash driftwood ashore, a second blaze from hour six carries the
/// dawn check; the ending discards the still-open feed with its greedy pulls.
#[test]
fn scripted_session_reaches_survived() {
    let mut e = game();

    commit_named(&mut e, "sup", &[("morsel", "tea")]).expect("sup opens");
    e.advance(300).expect("to hour five");
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
    assert_eq!(
        aspect(&e, named(&e, "beacon"), "heat"),
        3,
        "beacon stays lit"
    );
    assert_eq!(aspect(&e, named(&e, "warden"), "dread"), 3, "dread under 5");
    assert_eq!(e.legacy_out().get("dawns_seen"), Some(&1));
    // The ending discards the remaining due queue unresolved: the feed is
    // still open, its greedy-pulled cards still bound (T7).
    assert_eq!(e.state().situations.len(), 1);
    assert_eq!(count_named(&e, "driftwood"), 2);
}

/// Scripted session to `gray_dawn`: play nothing; the heat never rises, hunger
/// climbs, the tea burns at hour two and dread_first takes over — the dawn
/// finds the beacon dark at hour seven.
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
    assert_eq!(aspect(&e, named(&e, "warden"), "dread"), 4);
    assert_eq!(e.legacy_out().get("dawns_seen"), Some(&0));
    assert_eq!(count_named(&e, "tea"), 0, "gnaw burned the tea");
}

/// Scripted session to `despair`: dream early (the omens deck deals calm,
/// gull, portent, portent under seed 7), keep a comb roll between dreams so
/// both portent depth rolls land under 4 — each adds a dread — and let the
/// starving night do the rest: dread hits 5 at hour six, broken outranks dawn,
/// and despair closes the run.
#[test]
fn scripted_session_reaches_despair() {
    let mut e = game();

    commit_named(&mut e, "dream", &[]).expect("dream opens");
    commit_named(&mut e, "comb", &[]).expect("comb opens");
    e.advance(100).expect("to 100m");
    for _ in 0..3 {
        commit_named(&mut e, "dream", &[]).expect("dream opens");
        e.advance(60).expect("an hour of night");
    }
    e.advance(360).expect("to the broken ending");

    assert_eq!(ended(&e).as_deref(), Some("despair"));
    assert_eq!(e.state().now, 420);
    assert_eq!(aspect(&e, named(&e, "warden"), "dread"), 5);
    assert_eq!(e.legacy_out().get("dawns_seen"), Some(&0));
}

/// T6: the same content, seed, and operation sequence reaches the identical
/// state — across different advance splits, and with a refused commit costing
/// nothing (T3: zero state change, zero time).
#[test]
fn determinism_same_seed_same_ops_identical_state() {
    fn session(e: &mut Engine) {
        commit_named(e, "sup", &[("morsel", "tea")]).expect("sup");
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
        commit_named(e, "sup", &[("morsel", "tea")]).expect("sup");
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
