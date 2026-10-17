//! The diagnostic faces — `--why` (the blocker table) and `--verb` (the verb
//! report) — plus the note-convergence rule. Everything is pinned BYTE
//! EXACT over one hand-written script whose sites fail distinct ladder rungs
//! (the blockers are distinguishable), and over the bundled nightjar script
//! (the real 44-card report the convergence rule was cut for). The CLI is
//! exercised as a subprocess for the argv faces (priority, the two argv
//! errors, exit codes).

use std::process::Command;

use indexmap::IndexMap;

use chorus::content::script::compile_script;
use chorus::content::{
    compile_content, CompleteTriggerDef, ContentRaw, PrototypeDef, RecipeDef, SlotDef, TriggerDef,
    VerbContent,
};
use chorus::core::dict::core_dictionary;
use chorus::core::types::{add_entity, new_state, Entity};
use chorus::reach::fixpoint::CardBlocker;
use chorus::reach::paths::SiteVerdict;
use chorus::reach::render::{render_verb, render_why};
use chorus::reach::{Analyzer, CardOptions};

/// The blocker fixture: one hand-written script whose unproven sites fail
/// DISTINCT rungs of the ladder — Fallback (t_fallback's ELSE arm), DupRisk
/// (make.dup mints the living initial SOLO), ThrowOps (make.throw's MOVE),
/// VerbNotCommittable (WORK needs two distinct characters, one exists),
/// SelfGated (gate.self gates on its own mint), PathClosed (seal.closed's
/// IF is statically false), GateNameDestroyed (gate.doomed's gate names the
/// t_doom-destroyed DOOMED_GEM, seed 6028), ShadowRisk (seal.shadowed is not
/// the first recipe), WhenNotStaticTrue (sing.when reads a role),
/// KilledInList (t_kill destroys its own mint), ExpiryUnsupported
/// (NO_SUCH_CARD has no descriptor) — and one REFUSE arm in READ feeding the
/// GLOBAL refused note (the convergence rule's name-free face). Every slot
/// trigger sits on the same clean boundary slot (recurring, disjoint mints)
/// so the risky-freeze model marks nothing — which is also why the four
/// TIMING rungs (GateTimingUnsatisfiable, FillerDoomed,
/// CrossListMintCollision, FrozenDuration) are absent: each needs a global
/// timing shape (a late producer, a proven filler source, a duration>0
/// recipe, a risky owner) that would disturb the fixture's clean boundary.
/// The twelfth exercised rung, CompleteHomeMissing, cannot be authored in
/// the script grammar (a declared entity is initial, hence obtainable) —
/// it has its own ContentRaw fixture below.
const WHY_SCRIPT: &str = "\
script: Why Fixture
player: SOLO
goal: exercise the blocker ladder
seed: 3
start: 0:00

entities:
  ROOM is place
  SOLO is character at ROOM
  THE_COIN is resource at ROOM

prototypes:
  FALLBACK_GEM is item
  MINTED_THING is item
  KILLED_GEM is item
  THROW_GEM is item
  DEAD_GEM is item
  CLOSED_GEM is item
  SHADOWED_GEM is item
  WHEN_GEM is item
  EXPIRY_GEM is item
  REFUSED_GEM is item
  DOOMED_GEM is item
  GATED_GEM is item

triggers:
  t_fallback at halfhour 1 -> IF exists(THE_COIN) -> (NARRATE \"coin lives\") ELSE -> (SPAWN FALLBACK_GEM)
  t_kill at halfhour 1 -> SPAWN KILLED_GEM, DESTROY KILLED_GEM
  t_expire_bad on expire of NO_SUCH_CARD -> SPAWN EXPIRY_GEM
  t_gate_src at halfhour 1 -> SPAWN DOOMED_GEM
  t_doom at halfhour 2 -> DESTROY DOOMED_GEM

verbs:
  WORK:
    actors: 2 kind:character
    duration: 0m
    recipes:
      make.dead -> SPAWN DEAD_GEM
      make.dup -> SPAWN SOLO
      make.throw -> SPAWN THROW_GEM, MOVE SOLO INTO NOWHERE
  DREAM:
    actors: 1 kind:character
    duration: 0m
    recipes:
      gate.self -> IF exists(MINTED_THING) -> (SPAWN MINTED_THING)
  EXPLORE:
    actors: 1 kind:character
    duration: 0m
    recipes:
      seal.closed -> IF exists(NO_SUCH_CARD) -> (SPAWN CLOSED_GEM)
      seal.shadowed -> SPAWN SHADOWED_GEM
  STUDY:
    actors: 1 kind:character
    duration: 0m
    recipes:
      sing.when when actors.aspect.occult >= 1 -> SPAWN WHEN_GEM
  ACQUIRE:
    actors: 2 kind:character
    duration: 0m
    recipes:
      gate.doomed -> IF exists(DOOMED_GEM) -> (SPAWN GATED_GEM)
  READ:
    actors: 1 kind:character
    duration: 0m
    recipes:
      break.free -> SPAWN FALLBACK_GEM
      read.refused -> IF exists(THE_COIN) -> (SPAWN REFUSED_GEM, REFUSE(\"sealed\"))
";

fn why_analyzer() -> (chorus::content::CompiledContent, chorus::core::types::State) {
    let dict = core_dictionary();
    let cs = compile_script(WHY_SCRIPT, &dict).expect("blocker fixture compiles");
    (cs.content, cs.state)
}

/// The --why table for `target`, rendered (the CLI's exact bytes).
fn why_section(target: &str) -> String {
    let (content, state) = why_analyzer();
    let analyzer = Analyzer::new(&content, &state);
    let mut out = String::new();
    render_why(&mut out, "why.txt", target, &analyzer.card_blockers(target));
    out
}

/// Twelve rungs, one site each — the distinguishable-output contract.
#[test]
fn why_table_names_the_first_blocker_of_each_ladder_rung() {
    // Fallback — the ELSE arm runs only when earlier arms are false.
    assert_eq!(
        why_section("FALLBACK_GEM"),
        "why.txt · card FALLBACK_GEM — why\n  · SPAWN 'FALLBACK_GEM' — trigger t_fallback: \
         ELSE fallback body — runs only when earlier arms are false — unprovable\n"
    );
    // DupRisk — the recipe mints the living initial non-stackable SOLO.
    assert_eq!(
        why_section("SOLO"),
        "why.txt · card SOLO — why\n  · SPAWN 'SOLO' — recipe make.dup of WORK: \
         this list mints a name that can collide with a living card (duplicate mint or an \
         initial copy) — the refusal would discard the draft\n"
    );
    // ThrowOps — MOVE in the list discards the draft.
    assert_eq!(
        why_section("THROW_GEM"),
        "why.txt · card THROW_GEM — why\n  · SPAWN 'THROW_GEM' — recipe make.throw of WORK: \
         a throwing op in this list may discard the whole draft at completion\n"
    );
    // VerbNotCommittable — MAKE needs two DISTINCT characters, one exists
    // (the slot query matches, so the verb is not dead).
    assert_eq!(
        why_section("DEAD_GEM"),
        "why.txt · card DEAD_GEM — why\n  · SPAWN 'DEAD_GEM' — recipe make.dead of WORK: \
         verb 'WORK' committability unknown — no proof fills its min>0 slot(s)\n"
    );
    // SelfGated — the arm gates on the very card it mints.
    assert_eq!(
        why_section("MINTED_THING"),
        "why.txt · card MINTED_THING — why\n  · SPAWN 'MINTED_THING' — recipe gate.self of DREAM: \
         gated on (or filled by) a card the list itself mints\n"
    );
    // PathClosed — the statically-false IF, named in the design-time spelling.
    assert_eq!(
        why_section("CLOSED_GEM"),
        "why.txt · card CLOSED_GEM — why\n  · SPAWN 'CLOSED_GEM' — recipe seal.closed of EXPLORE: \
         IF condition 'exists(NO_SUCH_CARD)' is not a static truth — the guarded arm never \
         applies (anything non-static demotes)\n"
    );
    // GateNameDestroyed — the gate names a card another trigger destroys
    // (seed 6028): the gate is only as durable as that name's aliveness.
    assert_eq!(
        why_section("GATED_GEM"),
        "why.txt · card GATED_GEM — why\n  · SPAWN 'GATED_GEM' — recipe gate.doomed of ACQUIRE: \
         a static gate names 'DOOMED_GEM', which another owner destroys/replaces — the gate is \
         only as durable as the aliveness of 'DOOMED_GEM'\n"
    );
    // ShadowRisk — the verb's second recipe (first-match).
    assert_eq!(
        why_section("SHADOWED_GEM"),
        "why.txt · card SHADOWED_GEM — why\n  · SPAWN 'SHADOWED_GEM' — recipe seal.shadowed of EXPLORE: \
         an earlier recipe of the same verb may shadow it (first-match)\n"
    );
    // WhenNotStaticTrue — the `when` reads a role.
    assert_eq!(
        why_section("WHEN_GEM"),
        "why.txt · card WHEN_GEM — why\n  · SPAWN 'WHEN_GEM' — recipe sing.when of STUDY: \
         the recipe `when` guard is not a static truth — it reads runtime state or evaluates false\n"
    );
    // KilledInList — the list destroys its own mint.
    assert_eq!(
        why_section("KILLED_GEM"),
        "why.txt · card KILLED_GEM — why\n  · SPAWN 'KILLED_GEM' — trigger t_kill: \
         the mint is destroyed/Replaced later in the same outcome list — it never survives a step\n"
    );
    // ExpiryUnsupported — the named card has no obtainable descriptor.
    assert_eq!(
        why_section("EXPIRY_GEM"),
        "why.txt · card EXPIRY_GEM — why\n  · SPAWN 'EXPIRY_GEM' — trigger t_expire_bad: \
         the expiry trigger fires only while a living 'NO_SUCH_CARD' can expire — no obtainable \
         'NO_SUCH_CARD' can be armed to expire\n"
    );
}

/// One fixture entity (id derived from the lowercased name).
fn ent(name: &str, kind: &str) -> Entity {
    Entity {
        id: format!("s_{}", name.to_lowercase()),
        kind: kind.to_string(),
        name: name.to_string(),
        aspects: IndexMap::new(),
        attrs: IndexMap::new(),
        born_action: Some(0),
        dead_action: None,
        stackable: false,
    }
}

/// The twelfth rung, CompleteHomeMissing, as its own ContentRaw fixture
/// (reach_complete's control shape): the script grammar cannot author an
/// unobtainable home — a declared entity is initial, hence obtainable.
#[test]
fn why_table_names_the_missing_completion_home() {
    let raw = ContentRaw {
        prototypes: Some(IndexMap::from([
            (
                "HOME_GEM".to_string(),
                PrototypeDef { kind: "item".to_string(), ..Default::default() },
            ),
            (
                "NO_SUCH_HOME".to_string(),
                PrototypeDef { kind: "item".to_string(), ..Default::default() },
            ),
        ])),
        triggers: Some(vec![TriggerDef::Complete(CompleteTriggerDef {
            id: "complete:NO_SUCH_HOME".to_string(),
            home: "NO_SUCH_HOME".to_string(),
            aspect: None,
            out: vec!["SPAWN HOME_GEM".to_string()],
        })]),
        ..ContentRaw::default()
    };
    let dict = core_dictionary();
    let content = compile_content(&raw, &dict, &[]).expect("fixture compiles");
    let mut state = new_state(0);
    add_entity(&mut state, ent("ANA", "character")).expect("fixture entity");
    let analyzer = Analyzer::new(&content, &state);
    let mut out = String::new();
    render_why(&mut out, "why.txt", "HOME_GEM", &analyzer.card_blockers("HOME_GEM"));
    assert_eq!(
        out,
        "why.txt · card HOME_GEM — why\n  · SPAWN 'HOME_GEM' — trigger complete:NO_SUCH_HOME: \
         the completion trigger fires only when a window completes binding 'NO_SUCH_HOME' — \
         'NO_SUCH_HOME' is not obtainable, so the trigger never fires\n"
    );
}

/// The Wave-3 completion rungs, as their own ContentRaw fixtures (the
/// ladder tail's three new demotions — each pinned byte-exact):
/// CompleteWindowUnbindable (a home no verb can lock), and
/// CompleteAspectUnreachable + CompleteDraftCollision on a binder fixture.
#[test]
fn why_table_names_the_wave3_completion_rungs() {
    use chorus::core::types::AspectVal;
    let dict = core_dictionary();
    let ent = |name: &str, kind: &str, aspects: &[(&str, i64)]| -> Entity {
        let mut a: IndexMap<String, AspectVal> = IndexMap::new();
        for (k, v) in aspects {
            a.insert((*k).to_string(), AspectVal::Num(*v));
        }
        Entity {
            id: format!("s_{}", name.to_lowercase()),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects: a,
            attrs: IndexMap::new(),
            born_action: Some(0),
            dead_action: None,
            stackable: false,
        }
    };
    let mut state = new_state(0);
    add_entity(&mut state, ent("ANA", "character", &[])).expect("fixture entity");
    add_entity(&mut state, ent("LOCKED_HOME", "item", &[])).expect("fixture entity");

    // ① the window rung: the home is obtainable, but the content has no
    // verb with a committed-role slot at all — no window can ever lock it.
    let unbindable = ContentRaw {
        prototypes: Some(IndexMap::from([
            (
                "HOME_GEM".to_string(),
                PrototypeDef { kind: "item".to_string(), ..Default::default() },
            ),
            ("X_GEM".to_string(), PrototypeDef { kind: "item".to_string(), ..Default::default() }),
        ])),
        triggers: Some(vec![TriggerDef::Complete(CompleteTriggerDef {
            id: "complete:LOCKED_HOME".to_string(),
            home: "LOCKED_HOME".to_string(),
            aspect: None,
            out: vec!["SPAWN X_GEM".to_string()],
        })]),
        ..ContentRaw::default()
    };
    let content = compile_content(&unbindable, &dict, &[]).expect("fixture compiles");
    let analyzer = Analyzer::new(&content, &state);
    let mut out = String::new();
    render_why(&mut out, "why.txt", "X_GEM", &analyzer.card_blockers("X_GEM"));
    assert_eq!(
        out,
        "why.txt · card X_GEM — why\n  · SPAWN 'X_GEM' — trigger complete:LOCKED_HOME: \
         no committable verb can lock 'LOCKED_HOME' under a completable recipe — no window \
         completing the binding exists, so the trigger never fires\n"
    );

    // ② the aspect rung + ③ the draft rung, on one binder fixture: WORK
    // locks LOCKED_HOME via its object slot (kind:item — ANA does not fit,
    // so the lock is forced) under completable work.r0, which declares no
    // aspect and mints the draft name the trigger body mints too.
    let binder = ContentRaw {
        prototypes: Some(IndexMap::from([
            (
                "HOME_GEM".to_string(),
                PrototypeDef { kind: "item".to_string(), ..Default::default() },
            ),
            ("X_GEM".to_string(), PrototypeDef { kind: "item".to_string(), ..Default::default() }),
        ])),
        verbs: IndexMap::from([(
            "WORK".to_string(),
            VerbContent {
                slots: vec![
                    SlotDef {
                        role: "actors".to_string(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".to_string()),
                        shown: None,
                    },
                    SlotDef {
                        role: "object".to_string(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:item".to_string()),
                        shown: None,
                    },
                ],
                duration: 30,
                recipes: vec![RecipeDef {
                    id: Some("work.r0".to_string()),
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: vec![],
                    out: vec!["SPAWN X_GEM".to_string()],
                }],
            },
        )]),
        triggers: Some(vec![TriggerDef::Complete(CompleteTriggerDef {
            id: "complete:LOCKED_HOME".to_string(),
            home: "LOCKED_HOME".to_string(),
            aspect: Some("labor".to_string()),
            out: vec!["SPAWN X_GEM".to_string()],
        })]),
        ..ContentRaw::default()
    };
    let content = compile_content(&binder, &dict, &[]).expect("fixture compiles");
    let analyzer = Analyzer::new(&content, &state);
    // ② the aspect rung: work.r0 is a selectable completable lock-binder,
    // but it declares no 'labor' — the filter never passes.
    let mut out = String::new();
    render_why(&mut out, "why.txt", "X_GEM", &analyzer.card_blockers("X_GEM"));
    assert_eq!(
        out,
        "why.txt · card X_GEM — why\n  · SPAWN 'X_GEM' — trigger complete:LOCKED_HOME: \
         the completed recipe must declare the 'labor' aspect, and no selectable completable \
         recipe of any verb that can lock 'LOCKED_HOME' declares it — the filter never passes\n"
    );

    // ③ the draft rung: drop the aspect gate — now the pair survives ① and
    // ② but the completion draft already holds X_GEM (work.r0's own mint),
    // so the body's X_GEM mint refuses the whole scratch. The recipe's site
    // demotes the same way (the completing recipe's own contamination).
    let binder_no_aspect = ContentRaw {
        prototypes: binder.prototypes.clone(),
        verbs: {
            let mut v = binder.verbs.clone();
            if let Some((_, verb)) = v.iter_mut().next() {
                verb.recipes[0].aspects = Vec::new();
            }
            v
        },
        triggers: Some(vec![TriggerDef::Complete(CompleteTriggerDef {
            id: "complete:LOCKED_HOME".to_string(),
            home: "LOCKED_HOME".to_string(),
            aspect: None,
            out: vec!["SPAWN X_GEM".to_string()],
        })]),
        ..ContentRaw::default()
    };
    let content = compile_content(&binder_no_aspect, &dict, &[]).expect("fixture compiles");
    let analyzer = Analyzer::new(&content, &state);
    let mut out = String::new();
    render_why(&mut out, "why.txt", "X_GEM", &analyzer.card_blockers("X_GEM"));
    assert_eq!(
        out,
        "why.txt · card X_GEM — why\n  · SPAWN 'X_GEM' — recipe work.r0 of WORK: \
         the completion draft already holds 'X_GEM' (the locked home card, the completing \
         recipe's own mints, or a sibling trigger's mint) — the body's mint refuses the draft \
         and the whole completion is discarded\n  · SPAWN 'X_GEM' — trigger complete:LOCKED_HOME: \
         the completion draft already holds 'X_GEM' (the locked home card, the completing \
         recipe's own mints, or a sibling trigger's mint) — the body's mint refuses the draft \
         and the whole completion is discarded\n"
    );
}

/// A proven card whose remaining unproven site still serves it lists only
/// that site (FALLBACK_GEM is proven via BREAK, its trigger site is not);
/// a name nothing unproven serves renders the empty face.
#[test]
fn why_table_omits_proven_sites_and_renders_the_empty_face() {
    // FALLBACK_GEM: proven via recipe break.free of BREAK — the trigger's
    // ELSE site is the only unproven server.
    let out = why_section("FALLBACK_GEM");
    assert!(out.contains("— trigger t_fallback: ELSE fallback body"), "{out}");
    assert_eq!(out.lines().count(), 2, "{out}");
    // THE_COIN: the refused arm never entered the live census and nothing
    // else serves it — the empty face.
    assert_eq!(
        why_section("THE_COIN"),
        "why.txt · card THE_COIN — why\n  (no blocked candidate sites)\n"
    );
}

/// The --verb report, byte-exact: duration, slots in the design-time query
/// spelling, the committability verdict with `slot_proof_lines`' own text,
/// and one line per recipe with the mint-site verdicts.
#[test]
fn verb_report_renders_the_settled_facts_byte_exact() {
    let (content, state) = why_analyzer();
    let analyzer = Analyzer::new(&content, &state);
    let mut out = String::new();
    let vr = analyzer.verb_report("WORK").expect("WORK exists");
    render_verb(&mut out, "why.txt", &vr);
    assert_eq!(
        out,
        "why.txt · verb WORK\n  duration: 0 min\n  slots:\n  · actors ×2 — kind:character\n  \
         committable: no\n  recipes:\n  \
         · make.dead (when: none) — SPAWN 'DEAD_GEM': unproven — verb 'WORK' committability \
         unknown — no proof fills its min>0 slot(s)\n  \
         · make.dup (when: none, shadow_risk) — SPAWN 'SOLO': unproven — this list mints a name \
         that can collide with a living card (duplicate mint or an initial copy) — the refusal \
         would discard the draft\n  \
         · make.throw (when: none, shadow_risk) — SPAWN 'THROW_GEM': unproven — a throwing op in \
         this list may discard the whole draft at completion\n\n"
    );

    // The committable verb: the proof lines are slot_proof_lines' OWN text,
    // and the recipe rows show proven verdicts and the shadow_risk flag.
    let mut out = String::new();
    let vr = analyzer.verb_report("EXPLORE").expect("EXPLORE exists");
    render_verb(&mut out, "why.txt", &vr);
    assert_eq!(
        out,
        "why.txt · verb EXPLORE\n  duration: 0 min\n  slots:\n  · actors ×1 — kind:character\n  \
         committable: yes\n  · EXPLORE committable: actors ×1 ← initial entity SOLO (character)\n  \
         recipes:\n  \
         · seal.closed (when: none) — SPAWN 'CLOSED_GEM': unproven — IF condition \
         'exists(NO_SUCH_CARD)' is not a static truth — the guarded arm never applies (anything \
         non-static demotes)\n  \
         · seal.shadowed (when: none, shadow_risk) — SPAWN 'SHADOWED_GEM': unproven — an earlier \
         recipe of the same verb may shadow it (first-match)\n\n"
    );

    // The structured verdicts agree with the --why table (one source).
    // STUDY the verb commits fine (the `when` is a recipe guard, not a slot
    // demand) — the SITE still fails the ladder on the role-read when.
    let vr = analyzer.verb_report("STUDY").expect("STUDY exists");
    assert_eq!(vr.recipes.len(), 1);
    assert!(vr.committable);
    assert!(!vr.dead, "the role-read when never kills the verb");
    match &vr.recipes[0].sites[0].verdict {
        SiteVerdict::Unproven(CardBlocker::WhenNotStaticTrue) => {}
        other => panic!("expected WhenNotStaticTrue, got {other:?}"),
    }
    assert_eq!(vr.recipes[0].when.as_deref(), Some("? >= 1"));
}

/// The note-convergence rule (§13.1 revision): a world note naming a card
/// rides ONLY on that name's report; name-free notes stay global.
#[test]
fn world_notes_converge_on_the_named_card() {
    let (content, state) = why_analyzer();
    let analyzer = Analyzer::new(&content, &state);
    // The GLOBAL refused note (no card name in the text) rides on every
    // report — the nightjar card and a name nothing mints alike.
    for target in ["THE_COIN", "WHEN_GEM", "NO_SUCH_NAME"] {
        let r = analyzer.analyze_card(target, CardOptions::default()).unwrap();
        assert!(
            r.notes.iter().any(|n| n.contains("producer site(s) sit behind an authored REFUSE")),
            "{target}: the global refused note must stay global: {:?}",
            r.notes
        );
    }
}

/// The nightjar end of the convergence rule: the `lord_tended` expiry note
/// names THE_LATE_LORD — it used to ride on all 44 --all reports; now only
/// THE_LATE_LORD's report carries it.
#[test]
fn nightjar_expiry_note_rides_only_on_the_late_lord() {
    let dict = core_dictionary();
    let text = std::fs::read_to_string("scripts/the-nightjar-flyer.txt").expect("bundled script");
    let cs = compile_script(&text, &dict).expect("the nightjar compiles");
    let analyzer = Analyzer::new(&cs.content, &cs.state);
    let note = "expiry trigger lord_tended reaches its body only while a living 'THE_LATE_LORD' can expire";
    let lord = analyzer.analyze_card("THE_LATE_LORD", CardOptions::default()).unwrap();
    assert!(lord.notes.iter().any(|n| n == note), "{:?}", lord.notes);
    for target in ["THE_WAX_CYLINDER", "MARCH", "ROOM"] {
        let r = analyzer.analyze_card(target, CardOptions::default()).unwrap();
        assert!(
            !r.notes.iter().any(|n| n.contains("expiry trigger")),
            "{target}: the expiry note must not ride here: {:?}",
            r.notes
        );
    }
}

/// The CLI faces as a subprocess: exit codes, the two argv errors, the
/// priority (--verb wins over --all), and the frozen face staying
/// byte-identical without --why.
mod cli {
    use super::Command;

    fn run(args: &[&str]) -> (Option<i32>, String, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_reach"))
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .expect("the bin spawns");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).to_string(),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    }

    const SCRIPT: &str = "scripts/the-nightjar-flyer.txt";

    /// `--why` appends the section after the frozen report; the proven
    /// initial card renders the empty face (no live unproven server).
    #[test]
    fn why_appends_the_section_and_the_proven_card_is_empty() {
        let (code, out, err) = run(&[SCRIPT, "THE_LATE_LORD", "--why"]);
        assert_eq!(code, Some(0), "{err}");
        assert!(out.contains("verdict: OBTAINABLE  (existence: proven)"), "{out}");
        assert!(
            out.contains("scripts/the-nightjar-flyer.txt · card THE_LATE_LORD — why\n  (no blocked candidate sites)\n"),
            "{out}"
        );
        // the frozen face is untouched without the flag
        let (code, plain, _) = run(&[SCRIPT, "THE_LATE_LORD"]);
        assert_eq!(code, Some(0));
        assert!(!plain.contains("— why"), "{plain}");
        assert!(out.starts_with(&plain), "the report must precede the why section byte-exactly");
    }

    /// `--verb <NAME>` renders the verb report; it WINS over --all (the
    /// --world precedence shape) and over an explicit card.
    #[test]
    fn verb_wins_over_all_and_card() {
        let (code, out, err) = run(&[SCRIPT, "--all", "--verb", "WORK"]);
        assert_eq!(code, Some(0), "{err}");
        assert!(out.starts_with("scripts/the-nightjar-flyer.txt · verb WORK\n"), "{out}");
        assert!(!out.contains("· card "), "--all must not render beside the verb report");
        let (code, out, err) = run(&[SCRIPT, "MARCH", "--verb", "WORK"]);
        assert_eq!(code, Some(0), "{err}");
        assert!(out.contains("  committable: yes"), "{out}");
    }

    /// A trailing `--verb` (no value slot) is the argv error naming the
    /// flag; an unknown verb name is the analyze error (exit 1).
    #[test]
    fn verb_argv_error_and_unknown_verb() {
        let (code, stdout, stderr) = run(&[SCRIPT, "--verb"]);
        assert_eq!(code, Some(2));
        assert_eq!(stdout, "");
        assert!(
            stderr.starts_with(
                "--verb needs a value: --verb \"<NAME>\"\nusage: reach <script.txt> <CARD_NAME>"
            ),
            "{stderr}"
        );
        let (code, stdout, stderr) = run(&[SCRIPT, "--verb", "NO_SUCH_VERB"]);
        assert_eq!(code, Some(1));
        assert_eq!(stdout, "");
        assert_eq!(stderr, "Error: no verb 'NO_SUCH_VERB'\n");
    }
}
