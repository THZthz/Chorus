//! Per-recipe duration through the session face (§6.4): a Game played on
//! `dur`-overridden recipes is deterministic across independent runs — the
//! rendered transcripts and the JSONL logs match byte for byte.

use chorus::content::script::compile_script;
use chorus::core::dict::core_dictionary;
use chorus::session::game::Game;

/// One verb, two recipes: the dossier text runs under a `dur 10m` window,
/// the plain manifest under the verb's 1h default.
const DUR_SESSION: &str = "
script: Dur Session
player: ANA
start: 0:0

entities:
  HOUSE is place
  ANA is character at HOUSE
  THE_LEDGER is text with dossier:2 at HOUSE
  THE_MANIFEST is text at HOUSE

verbs:
  READ:
    actors: 1 kind:character
    object: 1 kind:text
    duration: 1h
    recipes:
      read.dossier dur 10m when object.aspect.dossier >= 1 -> NARRATE \"the dossier reads fast.\"
      default -> NARRATE \"the manifest reads slow.\"
";

/// One full session: the short recipe, the long one, an undo past the second
/// turn and a replay of it. The transcript is the rendered turn lines, the
/// log the canonical JSONL rows — both surfaces byte-exact.
fn play_session() -> (String, String) {
    let mut game = Game::from_script(DUR_SESSION, core_dictionary()).unwrap();
    let mut out = String::new();
    let push = |out: &mut String, lines: &[String]| {
        for l in lines {
            out.push_str(l);
            out.push('\n');
        }
    };
    let t1 = game.handle_line("read the ledger").unwrap();
    push(&mut out, &t1.lines);
    push(&mut out, &game.wait().unwrap().lines);
    let t2 = game.handle_line("read the manifest").unwrap();
    push(&mut out, &t2.lines);
    push(&mut out, &game.wait().unwrap().lines);
    push(&mut out, &game.undo().unwrap().lines);
    push(&mut out, &game.wait().unwrap().lines);
    let t3 = game.handle_line("read the manifest").unwrap();
    push(&mut out, &t3.lines);
    push(&mut out, &game.wait().unwrap().lines);
    let log = game
        .log
        .records
        .iter()
        .map(|r| serde_json::to_string(r).expect("the record serializes"))
        .collect::<Vec<String>>()
        .join("\n");
    (out, log)
}

#[test]
fn a_recipe_dur_session_replays_byte_identically() {
    // the short recipe's turn renders the recipe's OWN window: (+10m), not
    // the verb's (+1h)
    let (transcript, log) = play_session();
    assert!(
        transcript.contains("(+10m) time passes."),
        "the dur-10m recipe drives the rendered window: {transcript}"
    );
    assert!(
        transcript.contains("(+1h) time passes."),
        "the default recipe keeps the verb's window: {transcript}"
    );
    // two independent runs — same bytes, both surfaces
    let (transcript2, log2) = play_session();
    assert_eq!(transcript2, transcript, "the transcripts differ between runs");
    assert_eq!(log2, log, "the JSONL logs differ between runs");
    // and the compiled content carries exactly the authored override
    let compiled = compile_script(DUR_SESSION, &core_dictionary()).unwrap();
    let read = compiled.content.verbs.get("READ").unwrap();
    assert_eq!(read.duration, 60);
    assert_eq!(read.recipes[0].dur, Some(10));
    assert_eq!(read.recipes[1].dur, None);
}
