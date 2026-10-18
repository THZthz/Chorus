//! The lint bridge (tests/reach/reach_bridge.rs) — the §13.1 → §13.2 seam:
//! the reach analyzer's global-dynamic findings surfaced as WARNING-class
//! lint findings. Byte-exact class coverage (a fixture tripping dead verb,
//! self-gated list, risky owner, frozen world, deck-cap cursor freeze and
//! REFUSE-doomed sites in one script), the determinism contract, the clean
//! face, and the `lint-scripts --reach` bin surface (the `[reach]` stream
//! plus the default invocation's unchanged bytes).

use std::process::Command;

use chorus::content::script::{compile_script, LintSeverity};
use chorus::core::dict::core_dictionary;
use chorus::reach::reach_findings;

/// A fixture tripping ALL SIX bridge classes at once (modeled on
/// world_report's RICH fixture, whose risky/freezing shape is pinned):
///
/// - `READ`'s object slot reads an aspect nothing writes — the verb is dead
///   (class 1) while `WORK` stays committable;
/// - `work.loop` spawns THE_CHARM and gates a sibling mint on
///   `exists(THE_CHARM)` — gated on a card the list itself mints (class 2);
/// - `t_freeze_a`/`t_freeze_b` mint the same non-stackable name on the same
///   slot — both risky (class 3) and the world freezes at their crossing
///   (class 5, minute 240);
/// - `t_cursor` (repeatable) draws POUCH beside a THE_CHARM co-mint — the
///   census freezes the deck cursor at 1 (class 4); `t_draw`'s once draw is
///   its design, not a smell;
/// - `work.moonlight`'s ELSE arm carries a REFUSE — its GIFT_C site is
///   refused (class 6).
const FIXTURE: &str = r#"
script: Reach Bridge Fixture
player: MARCH
goal: exercise every lint-bridge class
seed: 7
start: 0:00

entities:
  HALL is place
  MARCH is character at HALL
  THE_RELIC is item at HALL

prototypes:
  THE_CHARM is item
  GIFT_B is item
  GIFT_C is item
  GIFT_D is item
  COIN is resource stackable
  TALISMAN is resource stackable

triggers:
  t_kill once at halfhour 2 -> DESTROY THE_RELIC
  t_draw once at halfhour 3 -> DRAW RELIC_DRAW
  t_cursor at halfhour 4 -> SPAWN THE_CHARM, DRAW POUCH
  t_freeze_a at halfhour 8 -> SPAWN THE_CHARM
  t_freeze_b at halfhour 8 -> SPAWN THE_CHARM

decks:
  POUCH spec COIN defaultcard COIN reset
  RELIC_DRAW spec TALISMAN

verbs:
  WORK:
    actors: 1 kind:character
    duration: 0m
    recipes:
      work.moonlight -> SPAWN GIFT_B, IF exists(THE_RELIC) -> (NARRATE "fine") ELSE -> (SPAWN GIFT_C, REFUSE("the hour is wrong"))
      work.loop -> SPAWN THE_CHARM, IF exists(THE_CHARM) -> (SPAWN GIFT_D)
  READ:
    actors: 1 kind:character
    object: 1 aspect:unwritable>=1
    duration: 1h
    recipes:
      read.never -> NARRATE "never"
"#;

/// A bare, clean script: no verbs, no triggers, no sites — no findings of
/// either layer.
const CLEAN: &str = "
script: Bare Room
player: SOLO
goal: nothing to reach

entities:
  SOLO is character at ROOM
  ROOM is place
";

fn compile(text: &str) -> chorus::content::script::CompiledScript {
    let dict = core_dictionary();
    compile_script(text, &dict).expect("fixture compiles")
}

/// Every class, byte-exact, in the bridge's deterministic order (class
/// order: dead verbs, self-gated lists, risky owners, deck-cap freezes, the
/// frozen world, REFUSE-doomed sites), every severity WARNING.
#[test]
fn bridge_classes_are_byte_exact() {
    let cs = compile(FIXTURE);
    let findings = reach_findings(&cs.content, &cs.state, &cs.dict);
    let messages: Vec<&str> = findings.iter().map(|f| f.message.as_str()).collect();
    assert_eq!(
        messages,
        vec![
            "verb READ never commits — its object slot's query (aspect:unwritable>=1) can never match anything the world can produce (§13.1)",
            "recipe work.loop of WORK: gated on (or filled by) a card the list itself mints (§13.1)",
            "trigger t_freeze_a: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)",
            "trigger t_freeze_b: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)",
            "trigger t_cursor: its DRAW from deck POUCH freezes the cursor at 1 served position(s) — a non-stackable co-mint in the same list pins the draw (§13.1)",
            "the world is frozen — the first risky crossing lands at minute 240 (trigger t_freeze_a, trigger t_freeze_b): every draft completing there or later refuses (§13.1)",
            "1 producer site(s) sit behind an authored REFUSE — they can never apply: recipe work.moonlight of WORK (§13.1)",
        ],
        "the bridge classes drifted"
    );
    assert!(
        findings.iter().all(|f| f.severity == LintSeverity::Warning),
        "every reach finding is WARNING-class (the §13.2 discipline)"
    );
}

/// The determinism contract: two runs over the same (content, initial)
/// render byte-identical findings.
#[test]
fn two_runs_are_byte_identical() {
    let cs = compile(FIXTURE);
    assert_eq!(
        reach_findings(&cs.content, &cs.state, &cs.dict),
        reach_findings(&cs.content, &cs.state, &cs.dict)
    );
}

/// A clean world yields no findings — the bridge only speaks when the
/// settled facts carry a smell.
#[test]
fn a_clean_world_yields_no_findings() {
    let cs = compile(CLEAN);
    assert!(reach_findings(&cs.content, &cs.state, &cs.dict).is_empty());
}

// ---------------------------------------------------------------------------
// the bin surface — lint-scripts --reach
// ---------------------------------------------------------------------------

/// Write `text` to a temp file and run the real `lint-scripts` bin
/// (CARGO_BIN_EXE) from the manifest dir.
fn run_bin(text: &str, args: &[&str]) -> (Option<i32>, String, String) {
    let path = std::env::temp_dir().join(format!("chorus-reach-bridge-{}.txt", std::process::id()));
    std::fs::write(&path, text).expect("fixture written");
    let path = path.to_str().expect("utf8 temp path").to_string();
    let mut owned: Vec<String> = vec![path];
    owned.extend(args.iter().map(|a| a.to_string()));
    let refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    let out = Command::new(env!("CARGO_BIN_EXE_lint-scripts"))
        .args(&refs)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the bin spawns");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

/// `--reach` appends the bridge's findings to the same stream, `[reach]`-
/// marked, and names them in the summary; the exit code stays 0 (the §13.2
/// rule). The fixture also trips two per-site lint ERRORs on purpose —
/// work.loop sits behind a when-less recipe and READ's aspect has no writer
/// — so the pin shows the two layers reading apart in one stream. The
/// default invocation on the CLEAN fixture stays byte-identical to the
/// pre-bridge bin, with and without the flag.
#[test]
fn lint_scripts_reach_flag_and_the_unchanged_default() {
    // --reach over the smell-fest: the two lint ERRORs, then the seven
    // [reach] findings, then the three-way summary
    let (code, out, err) = run_bin(FIXTURE, &["--reach"]);
    assert_eq!(code, Some(0), "{err}");
    let path = out.lines().next().expect("a file header").trim_end_matches(':');
    let expected = format!(
        "\
{path}:
  - recipe work.loop (WORK): unreachable — work.moonlight already matches everything (first match wins, §6.4)
  - aspect 'unwritable' is queried but never declared on any entity or prototype — check the spelling
  - [reach] verb READ never commits — its object slot's query (aspect:unwritable>=1) can never match anything the world can produce (§13.1)
  - [reach] recipe work.loop of WORK: gated on (or filled by) a card the list itself mints (§13.1)
  - [reach] trigger t_freeze_a: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  - [reach] trigger t_freeze_b: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  - [reach] trigger t_cursor: its DRAW from deck POUCH freezes the cursor at 1 served position(s) — a non-stackable co-mint in the same list pins the draw (§13.1)
  - [reach] the world is frozen — the first risky crossing lands at minute 240 (trigger t_freeze_a, trigger t_freeze_b): every draft completing there or later refuses (§13.1)
  - [reach] 1 producer site(s) sit behind an authored REFUSE — they can never apply: recipe work.moonlight of WORK (§13.1)

1 file(s), 2 finding(s), 0 warning(s), 7 reach finding(s) — warnings and reach findings advise, the exit code stays 0
"
    );
    assert_eq!(out, expected, "the --reach stream drifted");

    // the same file WITHOUT --reach: the pre-bridge bytes, byte-pinned (the
    // two lint ERRORs only — the bridge layer is absent)
    let (code, out, err) = run_bin(FIXTURE, &[]);
    assert_eq!(code, Some(0), "{err}");
    let path = out.lines().next().expect("a file header").trim_end_matches(':');
    assert_eq!(
        out,
        format!(
            "\
{path}:
  - recipe work.loop (WORK): unreachable — work.moonlight already matches everything (first match wins, §6.4)
  - aspect 'unwritable' is queried but never declared on any entity or prototype — check the spelling

1 file(s), 2 finding(s)
"
        ),
        "the default invocation changed"
    );

    // determinism at the bin face: a second --reach run is byte-identical
    let (code_a, run_a, _) = run_bin(FIXTURE, &["--reach"]);
    assert_eq!(code_a, Some(0));
    let (code_b, run_b, _) = run_bin(FIXTURE, &["--reach"]);
    assert_eq!(code_b, Some(0));
    assert_eq!(run_a, run_b, "two --reach runs differ");

    // the CLEAN fixture: byte-identical with and without --reach (the flag
    // only speaks when the settled facts carry a smell)
    let clean_text = format!(
        "{}: clean\n\n1 file(s), 0 finding(s)\n",
        std::env::temp_dir()
            .join(format!("chorus-reach-bridge-{}.txt", std::process::id()))
            .to_str()
            .expect("utf8 temp path")
    );
    let (code, out, err) = run_bin(CLEAN, &[]);
    assert_eq!(code, Some(0), "{err}");
    assert_eq!(out, clean_text, "the default invocation on a clean file changed");
    let (code, out, err) = run_bin(CLEAN, &["--reach"]);
    assert_eq!(code, Some(0), "{err}");
    assert_eq!(out, clean_text, "--reach spoke on a clean file");

    // `--reach` with NO named files still reads the default scripts/ list
    // relative to the cwd (the flag never shadows the no-argument face)
    let scratch =
        std::env::temp_dir().join(format!("chorus-reach-bridge-dir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(scratch.join("scripts")).expect("the scratch scripts dir");
    std::fs::write(scratch.join("scripts").join("bridge-fixture.txt"), CLEAN).expect("fixture");
    let out = Command::new(env!("CARGO_BIN_EXE_lint-scripts"))
        .arg("--reach")
        .current_dir(&scratch)
        .output()
        .expect("the bin spawns");
    let _ = std::fs::remove_dir_all(&scratch);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        "scripts/bridge-fixture.txt: clean\n\n1 file(s), 0 finding(s)\n",
        "--reach with no named files lost the default list"
    );

    // the bundled slice is lint-clean but whole-world-risky: the pinned
    // --reach stream (five DREAM/USE_ON recipes minting living non-stackable
    // cards — VIGOR and its foundation kin — plus mansus.enter's BELLWOOD
    // draw pinned at 1 by the THE_PALE_YEARNING co-mint)
    let slice = format!("{}/scripts/cultist-slice.txt", env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(env!("CARGO_BIN_EXE_lint-scripts"))
        .args(["--reach", &slice])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the bin spawns");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let expected = format!(
        "\
{slice}:
  - [reach] recipe dream.rest_vigor of DREAM: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  - [reach] recipe dream.rest_wits of DREAM: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  - [reach] recipe dream.rest_yearning of DREAM: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  - [reach] recipe dream.mend of DREAM: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  - [reach] recipe dream.fever of DREAM: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  - [reach] recipe rite.salve of USE_ON: its list's non-stackable mints can collide with a living card or another owner's mint at a crossing — the collision refuses the whole draft (§13.1)
  - [reach] recipe mansus.enter of MANSUS: its DRAW from deck BELLWOOD freezes the cursor at 1 served position(s) — a non-stackable co-mint in the same list pins the draw (§13.1)

1 file(s), 0 finding(s), 0 warning(s), 7 reach finding(s) — warnings and reach findings advise, the exit code stays 0
"
    );
    assert_eq!(stdout, expected, "the bundled slice's --reach stream drifted");

    // the bundled flyer: one reach finding (talk.meet_spiritualist's RUMOURS
    // draw pinned by its co-mint), exit 0
    let flyer = format!("{}/scripts/the-nightjar-flyer.txt", env!("CARGO_MANIFEST_DIR"));
    let out = Command::new(env!("CARGO_BIN_EXE_lint-scripts"))
        .args(["--reach", &flyer])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the bin spawns");
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let expected = format!(
        "\
{flyer}:
  - [reach] recipe talk.meet_spiritualist of TALK: its DRAW from deck RUMOURS freezes the cursor at 1 served position(s) — a non-stackable co-mint in the same list pins the draw (§13.1)

1 file(s), 0 finding(s), 0 warning(s), 1 reach finding(s) — warnings and reach findings advise, the exit code stays 0
"
    );
    assert_eq!(stdout, expected, "the bundled flyer's --reach stream drifted");
}
