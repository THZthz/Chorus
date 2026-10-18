//! The machine-readable exports — `--json` (reach::export's report
//! envelope) and `--graph` (the whole-content mint dependency graph as
//! Graphviz DOT). Everything is pinned BYTE EXACT over one hand-written
//! fixture whose shapes are the simple ones (an initial card, one proven
//! recipe site with a slot filler, one fallback-blocked trigger site), the
//! CLI faces run as a subprocess (the array face, the --why merge, the
//! demand face, the priority), and the scale/determinism guard runs the
//! graph over the bundled cultist-slice world.

use std::process::Command;

use chorus::content::script::{compile_script, CompiledScript};
use chorus::content::CompiledContent;
use chorus::core::dict::core_dictionary;
use chorus::core::types::State;
use chorus::reach::export::{card_export, graph_dot, render_json, render_json_array};
use chorus::reach::{Analyzer, CardOptions};

/// The export fixture: COIN is the initial card (the simplest proven
/// report — initial_decl_line chain, no deck, no guards), GEM is minted by
/// a PROVEN recipe site (WORK commits with SOLO, so the graph grows the
/// `GEM -> SOLO requires` filler edge) and by a fallback-blocked trigger
/// site (the `--why --json` merge's one blocker). Nothing destroys or
/// replaces, so the graph carries no kill edges.
const EXPORT_SCRIPT: &str = "\
script: Export Fixture
player: SOLO
goal: pin the export faces
seed: 3
start: 0:00

entities:
  ROOM is place
  SOLO is character at ROOM
  COIN is resource at ROOM

prototypes:
  GEM is item

triggers:
  t_fallback at halfhour 1 -> IF exists(COIN) -> (NARRATE \"coin lives\") ELSE -> (SPAWN GEM)

verbs:
  WORK:
    actors: 1 kind:character
    duration: 0m
    recipes:
      grab -> SPAWN GEM
";

/// The fixture triple: the analyzer now carries the load-effective
/// dictionary beside (content, initial) — the fixtures declare no
/// vocabulary, so the core seed is the effective one.
fn fixture() -> (CompiledContent, State, chorus::core::dict::Dictionary) {
    let dict = core_dictionary();
    let cs = compile_script(EXPORT_SCRIPT, &dict).expect("export fixture compiles");
    let CompiledScript { content, state, dict, .. } = cs;
    (content, state, dict)
}

/// The `--json` face over one card (the CLI's exact bytes: the envelope
/// rendered by `render_json`).
fn card_json(card: &str) -> String {
    let (content, state, dict) = fixture();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let r = analyzer
        .analyze_card(card, CardOptions::default())
        .expect("export fixture analyzes cleanly");
    render_json(&card_export("export.txt", None, false, &r, None))
}

// ---------------------------------------------------------------------------
// --json
// ---------------------------------------------------------------------------

/// The full envelope, byte for byte: the meta (file / demand / usable —
/// the text head's fields) around the report, whose keys are the
/// `CardReport` fields in declaration order and whose enums ride as their
/// `as_str` strings (`obtainable`, `proven`, `initial`).
#[test]
fn json_pins_the_initial_card_envelope_byte_for_byte() {
    assert_eq!(
        card_json("COIN"),
        r#"{
  "file": "export.txt",
  "demand": null,
  "usable": false,
  "report": {
    "target": "COIN",
    "verdict": "obtainable",
    "existence": "proven",
    "chain": [
      "COIN is declared in the initial entities (resource)"
    ],
    "paths": [
      {
        "via": "initial entity COIN (resource)",
        "op": "initial",
        "mints": "COIN",
        "derived_name": false,
        "deck": null,
        "guards": [],
        "supports": [
          "COIN is declared in the initial entities (resource)"
        ],
        "requires": []
      }
    ],
    "candidates": [],
    "usable_via": [],
    "notes": []
  }
}
"#
    );
}

/// The JSON is well-formed and reads back with the expected values — the
/// proven recipe site renders the full tree (op as `as_str`, the deck
/// `null`, the support lines as strings).
#[test]
fn json_round_trips_through_serde_json() {
    let (content, state, dict) = fixture();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let r = analyzer
        .analyze_card("GEM", CardOptions::default())
        .expect("export fixture analyzes cleanly");
    let out = render_json(&card_export("export.txt", None, false, &r, None));
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["file"], "export.txt");
    assert_eq!(v["demand"], serde_json::Value::Null);
    assert_eq!(v["usable"], false);
    assert_eq!(v["report"]["target"], "GEM");
    assert_eq!(v["report"]["verdict"], "obtainable");
    assert_eq!(v["report"]["existence"], "proven");
    let path = &v["report"]["paths"][0];
    assert_eq!(path["op"], "SPAWN");
    assert_eq!(path["mints"], "GEM");
    assert_eq!(path["via"], "recipe grab of WORK");
    assert_eq!(path["deck"], serde_json::Value::Null);
    assert_eq!(path["derived_name"], false);
    assert_eq!(path["guards"], serde_json::json!([]));
    assert!(!path["supports"].as_array().expect("supports").is_empty());
    assert_eq!(v["report"]["candidates"], serde_json::json!([]));
    // the proven recipe's verb is committable with SOLO in actors — the
    // usable_via line rides the report like its text face
    assert!(
        v["report"]["usable_via"].as_array().expect("usable_via").is_empty(),
        "COIN-less GEM fits no slot: {v}"
    );
}

/// The demand face: the envelope carries the query in `demand` (the text
/// head's `matching '<demand>'`), the target is the analyzer's (ignored
/// under demand), and the chain head names the satisfying descriptor.
#[test]
fn json_demand_mode_carries_the_query() {
    let (content, state, dict) = fixture();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let r = analyzer
        .analyze_card("", CardOptions { usable: false, demand: Some("kind:resource".into()) })
        .expect("demand analyzes cleanly");
    let out = render_json(&card_export("export.txt", Some("kind:resource"), false, &r, None));
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["demand"], "kind:resource");
    assert_eq!(v["report"]["verdict"], "obtainable");
    assert_eq!(v["report"]["existence"], "proven");
    assert_eq!(v["report"]["chain"][0], "COIN (resource) satisfies the demand 'kind:resource'");
    // `--demand --json` rides the ARRAY face too (one envelope per name)
    let mut reports = Vec::new();
    for name in ["COIN", "GEM"] {
        let r = analyzer
            .analyze_card(name, CardOptions { usable: false, demand: Some("kind:resource".into()) })
            .expect("demand analyzes cleanly");
        reports.push(r);
    }
    let envs: Vec<_> = reports
        .iter()
        .map(|r| card_export("export.txt", Some("kind:resource"), false, r, None))
        .collect();
    let arr = render_json_array(&envs);
    let v: serde_json::Value = serde_json::from_str(&arr).expect("valid JSON");
    assert_eq!(v.as_array().expect("array").len(), 2);
    assert_eq!(v[0]["report"]["target"], "COIN");
}

/// `--why --json` MERGES the blocker table into the envelope as `why` —
/// each row's blocker is `CardBlocker::as_str`'s own text (the very string
/// `render_why` prints); without the pass the key is absent entirely.
#[test]
fn json_why_merges_the_blocker_table() {
    let (content, state, dict) = fixture();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let r = analyzer
        .analyze_card("GEM", CardOptions::default())
        .expect("export fixture analyzes cleanly");
    let with = render_json(&card_export(
        "export.txt",
        None,
        false,
        &r,
        Some(&analyzer.card_blockers("GEM")),
    ));
    let v: serde_json::Value = serde_json::from_str(&with).expect("valid JSON");
    let why = v["why"].as_array().expect("why array");
    assert_eq!(why.len(), 1, "the fallback trigger is GEM's one blocked server: {v}");
    assert_eq!(why[0]["owner"], "trigger t_fallback");
    assert_eq!(why[0]["op"], "SPAWN");
    assert_eq!(why[0]["name"], "GEM");
    assert_eq!(
        why[0]["blocker"],
        "ELSE fallback body — runs only when earlier arms are false — unprovable"
    );
    // without the pass the key is absent (the text face renders nothing)
    let without = render_json(&card_export("export.txt", None, false, &r, None));
    assert!(!without.contains("\"why\""), "{without}");
}

/// `--all --json`: one envelope per name, in the --all walk order
/// (prototypes ∪ live initial ∪ deck order, UTF-16 sorted).
#[test]
fn json_all_is_the_sorted_array() {
    let (content, state, dict) = fixture();
    let analyzer = Analyzer::new(&content, &state, &dict);
    let mut names: indexmap::IndexSet<String> = indexmap::IndexSet::new();
    names.extend(content.prototypes.keys().cloned());
    for e in state.entities.values() {
        if e.dead_action.is_none() {
            names.insert(e.name.clone());
        }
    }
    for d in content.decks.values() {
        for c in &d.order {
            names.insert(c.name.clone());
        }
    }
    let mut sorted: Vec<String> = names.into_iter().collect();
    sorted.sort_by(|a, b| chorus::core::text::cmp_utf16(a, b));
    assert_eq!(sorted, ["COIN", "GEM", "ROOM", "SOLO"]);
    let mut reports = Vec::new();
    for name in &sorted {
        reports.push(
            analyzer
                .analyze_card(name, CardOptions::default())
                .expect("export fixture analyzes cleanly"),
        );
    }
    let envs: Vec<_> =
        reports.iter().map(|r| card_export("export.txt", None, false, r, None)).collect();
    let arr = render_json_array(&envs);
    let v: serde_json::Value = serde_json::from_str(&arr).expect("valid JSON");
    let arr = v.as_array().expect("the array face");
    assert_eq!(arr.len(), 4);
    let targets: Vec<&str> =
        arr.iter().map(|e| e["report"]["target"].as_str().expect("target")).collect();
    assert_eq!(targets, ["COIN", "GEM", "ROOM", "SOLO"]);
    for e in arr {
        assert_eq!(e["file"], "export.txt");
        assert_eq!(e["demand"], serde_json::Value::Null);
        assert_eq!(e["usable"], false);
    }
}

// ---------------------------------------------------------------------------
// --graph
// ---------------------------------------------------------------------------

/// The small world's DOT, byte for byte: kind-annotated card nodes and the
/// one proven site's owner node; the edges are the proven SPAWN and its
/// verb's slot filler. The fallback trigger's site is UNPROVEN — it mints
/// no edge and no owner node.
#[test]
fn graph_pins_the_small_world_byte_for_byte() {
    let (content, state, dict) = fixture();
    assert_eq!(
        graph_dot(&content, &state, &dict),
        r#"digraph reach {
  rankdir=LR;
  "COIN" [label="COIN\nresource", shape=ellipse];
  "GEM" [label="GEM\nitem", shape=ellipse];
  "ROOM" [label="ROOM\nplace", shape=ellipse];
  "SOLO" [label="SOLO\ncharacter", shape=ellipse];
  "recipe grab of WORK" [shape=box];
  "GEM" -> "SOLO" [label="requires"];
  "recipe grab of WORK" -> "GEM" [label="SPAWN"];
}
"#
    );
}

/// A recurring (non-once) draw whose trigger crosses BEFORE the freeze: the
/// site's raw cap is the MANY sentinel, but the freeze model rewrites the
/// deck's cap to 1 in `build_post_fixpoint`, so the report side (seed
/// 81401's shape) serves order[0] and denies order[1] — and the graph must
/// agree: exactly the DAWN edge, never a DUSK edge. The risky SPAWN ECHO
/// (a living initial name) sets the freeze boundary after `d0`'s crossing
/// and its own demoted site draws nothing.
#[test]
fn graph_respects_the_freeze_deck_cap_on_proven_draws() {
    let frozen_script = "\
script: Freeze Fixture
player: SOLO
goal: cap the draw past the freeze
seed: 3
start: 0:00

entities:
  ROOM is place
  SOLO is character at ROOM
  ECHO is item at ROOM

prototypes:
  DAWN is item
  DUSK is item

decks:
  CYCLE spec DAWN, DUSK

triggers:
  d0 at halfhour 1 -> DRAW CYCLE
  t_risky at halfhour 2 -> SPAWN ECHO
";
    let dict = core_dictionary();
    let cs = compile_script(frozen_script, &dict).expect("freeze fixture compiles");
    let CompiledScript { content, state, .. } = cs;
    // the analyzer's own face: the frozen cap lets order[0] through and
    // denies order[1] — the graph may not draw an edge the analyzer denies
    let analyzer = Analyzer::new(&content, &state, &dict);
    let dawn = analyzer.analyze_card("DAWN", CardOptions::default()).expect("analyzes");
    assert_eq!(dawn.verdict, chorus::reach::CardVerdict::Obtainable);
    let dusk = analyzer.analyze_card("DUSK", CardOptions::default()).expect("analyzes");
    assert_eq!(dusk.verdict, chorus::reach::CardVerdict::Unproven);
    // the graph, byte for byte: the one cap-served draw edge, no DUSK, and
    // the risky SPAWN's demoted site minting nothing
    let g = graph_dot(&content, &state, &dict);
    assert_eq!(
        g,
        r#"digraph reach {
  rankdir=LR;
  "DAWN" [label="DAWN\nitem", shape=ellipse];
  "DUSK" [label="DUSK\nitem", shape=ellipse];
  "ECHO" [label="ECHO\nitem", shape=ellipse];
  "ROOM" [label="ROOM\nplace", shape=ellipse];
  "SOLO" [label="SOLO\ncharacter", shape=ellipse];
  "trigger d0" [shape=box];
  "trigger d0" -> "DAWN" [label="DRAW deck CYCLE"];
}
"#
    );
    assert_eq!(g, graph_dot(&content, &state, &dict), "byte-identical across runs");
}

/// A proven SPAWN of a `deriving name from object` prototype behind an
/// object-mandatory verb never mints the BARE name (12320) — the analyzer
/// keeps CLUE at candidate and the graph draws no bare-name edge (the
/// blinded site contributes nothing at all: no mint edge, no requires
/// edges). The sentinel node still rides the universe, as it rides the
/// text reports' universe.
#[test]
fn graph_drops_the_derive_blinded_bare_name() {
    let derive_script = "\
script: Derive Fixture
player: SOLO
goal: blind the bare name
seed: 3
start: 0:00

entities:
  ROOM is place
  SOLO is character at ROOM
  MANUAL is text at ROOM

prototypes:
  CLUE is idea deriving name from object

verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 0m
    recipes:
      skim -> SPAWN CLUE
";
    let dict = core_dictionary();
    let cs = compile_script(derive_script, &dict).expect("derive fixture compiles");
    let CompiledScript { content, state, .. } = cs;
    let analyzer = Analyzer::new(&content, &state, &dict);
    let clue = analyzer.analyze_card("CLUE", CardOptions::default()).expect("analyzes");
    assert_eq!(clue.verdict, chorus::reach::CardVerdict::Unproven);
    assert_eq!(clue.existence, chorus::reach::Existence::Candidate);
    let g = graph_dot(&content, &state, &dict);
    assert!(!g.contains(" -> "), "the blinded site mints no edge at all: {g}");
    assert!(!g.contains("recipe skim of READ"), "no owner node without an edge: {g}");
    // the universe's nodes still ride (the bare name and the derive
    // sentinel are mint-universe names — the graph annotates, never hides)
    assert!(g.contains("\"CLUE\" [label=\"CLUE\\nidea\", shape=ellipse];"), "{g}");
    assert_eq!(g, graph_dot(&content, &state, &dict), "byte-identical across runs");
}

/// The determinism contract at scale: the graph over the bundled
/// cultist-slice world (86 universe/initial nodes — the slice's whole mint
/// universe) is byte-identical across runs, its node ids ascend under
/// cmp_utf16, its edge lines (src, dst, label) ascend under the same order
/// after the node block, and the spawn + dashed-kill edge families ride
/// (the slice has no PROVEN expire site — the requires family is pinned on
/// the small fixture above and on nightjar below). (cultist-full is out
/// of scope — unverified.)
#[test]
fn graph_is_deterministic_and_scales_to_cultist_slice() {
    let text = std::fs::read_to_string("scripts/cultist-slice.txt").expect("bundled script");
    let dict = core_dictionary();
    let cs = compile_script(&text, &dict).expect("cultist-slice compiles");
    let CompiledScript { content, state, .. } = cs;
    let g1 = graph_dot(&content, &state, &dict);
    let g2 = graph_dot(&content, &state, &dict);
    assert_eq!(g1, g2, "two runs over the same inputs must be byte-identical");
    assert!(g1.starts_with("digraph reach {\n  rankdir=LR;\n"));
    assert!(g1.ends_with("}\n"));
    let utf16 = |s: &str| s.encode_utf16().collect::<Vec<_>>();
    let mut nodes = 0usize;
    let mut edges: Vec<(String, String, String)> = Vec::new();
    let mut prev_node: Option<String> = None;
    let mut node_block = true;
    for line in g1.lines().skip(2) {
        if line == "}" {
            break;
        }
        if let Some((src, dst, label)) = edge_of(line) {
            node_block = false;
            edges.push((src, dst, label));
        } else {
            assert!(node_block, "no node line may follow an edge line: {line}");
            let id = line.trim_start().split('"').nth(1).expect("quoted id").to_string();
            if let Some(p) = &prev_node {
                assert!(utf16(p) <= utf16(&id), "node ids ascend under cmp_utf16: {p} then {id}");
            }
            prev_node = Some(id);
            nodes += 1;
        }
    }
    let mut sorted_edges = edges.clone();
    sorted_edges.sort_by(|a, b| {
        utf16(&a.0)
            .cmp(&utf16(&b.0))
            .then(utf16(&a.1).cmp(&utf16(&b.1)))
            .then(utf16(&a.2).cmp(&utf16(&b.2)))
    });
    assert_eq!(edges, sorted_edges, "edge lines ascend by (src, dst, label)");
    assert!(nodes >= 80, "the slice's whole-universe node set: {nodes}");
    assert!(edges.len() >= 25, "the slice's proven spawn + kill edges: {}", edges.len());
    assert!(g1.contains("[label=\"SPAWN\"]"), "the proven completion spawn rides");
    assert!(g1.contains("[label=\"destroys\", style=dashed]"), "kill edges ride dashed");
    // nightjar: the expiry trigger's proven REPLACE site grows the
    // `THE_TENDED_LORD -> THE_LATE_LORD requires` edge (the third family)
    let text = std::fs::read_to_string("scripts/the-nightjar-flyer.txt").expect("bundled script");
    let cs = compile_script(&text, &dict).expect("nightjar compiles");
    let CompiledScript { content, state, .. } = cs;
    let g = graph_dot(&content, &state, &dict);
    assert_eq!(g, graph_dot(&content, &state, &dict), "nightjar: byte-identical across runs");
    assert!(g.contains("\"THE_TENDED_LORD\" -> \"THE_LATE_LORD\" [label=\"requires\"];"), "{g}");
}

/// Split one DOT edge line back into (src, dst, label); `None` for a node
/// line. The pinned emission shape is `  "SRC" -> "DST" [label="L"...];`.
fn edge_of(line: &str) -> Option<(String, String, String)> {
    let (src, rest) = line.trim_start().split_once("\" -> \"")?;
    let (dst, rest) = rest.split_once("\" [label=\"")?;
    let (label, _) = rest.split_once('"')?;
    Some((src.to_string(), dst.to_string(), label.to_string()))
}

// ---------------------------------------------------------------------------
// the CLI faces (subprocess)
// ---------------------------------------------------------------------------

mod cli {
    use super::Command;

    const EXPORT_SCRIPT: &str = super::EXPORT_SCRIPT;

    /// One fixture file per run set, a fresh dir per test process.
    fn write_fixture() -> String {
        let dir = std::env::temp_dir().join(format!("chorus-reach-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("export.txt");
        std::fs::write(&path, EXPORT_SCRIPT).expect("fixture written");
        path.to_str().expect("utf8 temp path").to_string()
    }

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

    /// Card mode: the envelope parses, the meta mirrors the text head, and
    /// the `--why` merge adds exactly the `why` key.
    #[test]
    fn card_json_and_the_why_merge_over_the_cli() {
        let script = write_fixture();
        let (code, out, err) = run(&[&script, "COIN", "--json"]);
        assert_eq!(code, Some(0), "{err}");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["file"], script);
        assert_eq!(v["demand"], serde_json::Value::Null);
        assert_eq!(v["usable"], false);
        assert_eq!(v["report"]["target"], "COIN");
        assert!(v.get("why").is_none(), "no --why, no why key: {v}");
        let (code, out, err) = run(&[&script, "GEM", "--why", "--json"]);
        assert_eq!(code, Some(0), "{err}");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["why"][0]["owner"], "trigger t_fallback");
    }

    /// `--all --json` is one JSON array in the walk order; the demand face
    /// carries the query; `--usable --json` flips the flag.
    #[test]
    fn all_demand_usable_faces_over_the_cli() {
        let script = write_fixture();
        let (code, out, err) = run(&[&script, "--all", "--json"]);
        assert_eq!(code, Some(0), "{err}");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        let arr = v.as_array().expect("the array face");
        assert_eq!(arr.len(), 4);
        let targets: Vec<&str> =
            arr.iter().map(|e| e["report"]["target"].as_str().expect("target")).collect();
        assert_eq!(targets, ["COIN", "GEM", "ROOM", "SOLO"]);
        let (code, out, err) = run(&[&script, "--demand", "kind:resource", "--json"]);
        assert_eq!(code, Some(0), "{err}");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["demand"], "kind:resource");
        assert_eq!(v["report"]["verdict"], "obtainable");
        let (code, out, err) = run(&[&script, "COIN", "--usable", "--json"]);
        assert_eq!(code, Some(0), "{err}");
        let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
        assert_eq!(v["usable"], true);
    }

    /// `--graph` needs only the script and loses only to `--world`: it wins
    /// over --all, an explicit card and a demand; two runs are
    /// byte-identical; `--json` is ignored beside it (the DOT face).
    #[test]
    fn graph_face_priority_and_determinism_over_the_cli() {
        let script = write_fixture();
        let (code, out, err) = run(&[&script, "--graph"]);
        assert_eq!(code, Some(0), "{err}");
        assert!(out.starts_with("digraph reach {\n  rankdir=LR;\n"), "{out}");
        assert!(out.contains("\"GEM\" -> \"SOLO\" [label=\"requires\"];"), "{out}");
        let (_, out2, _) = run(&[&script, "--graph"]);
        assert_eq!(out, out2, "two runs must be byte-identical");
        // --graph wins over --all / card / demand (the DOT face each time)
        for args in [
            vec![script.as_str(), "--all", "--graph"],
            vec![script.as_str(), "COIN", "--graph"],
            vec![script.as_str(), "--demand", "kind:resource", "--graph"],
        ] {
            let (code, out, err) = run(&args);
            assert_eq!(code, Some(0), "{err}");
            assert!(out.starts_with("digraph reach {\n"), "{out}");
        }
        // --world still wins over --graph (the dashboard's text face)
        let (code, out, err) = run(&[&script, "--world", "--graph"]);
        assert_eq!(code, Some(0), "{err}");
        assert!(out.contains(" · world\n"), "{out}");
        assert!(!out.contains("digraph"), "{out}");
        // --json is a card-face modifier: beside --graph it is ignored
        let (code, out, err) = run(&[&script, "--graph", "--json"]);
        assert_eq!(code, Some(0), "{err}");
        assert!(out.starts_with("digraph reach {\n"), "{out}");
    }

    /// The usage gate unchanged: a lone `--json` (no card, no face) and a
    /// lone `--graph` with no script are the argv error (exit 2, the
    /// byte-pinned two-line usage).
    #[test]
    fn argv_gate_unchanged() {
        let script = write_fixture();
        let (code, stdout, stderr) = run(&[&script, "--json"]);
        assert_eq!(code, Some(2));
        assert_eq!(stdout, "");
        assert!(
            stderr.starts_with("usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"] [--why] [--verb \"<NAME>\"] [--world]\n       reach <script.txt> --all\n"),
            "{stderr}"
        );
        let (code, stdout, stderr) = run(&["--graph"]);
        assert_eq!(code, Some(2));
        assert_eq!(stdout, "");
        assert!(stderr.starts_with("usage: reach <script.txt>"), "{stderr}");
    }
}
