use std::sync::Arc;
use story_content::runtime::DiagKind;
use story_core::{EntityId, RecipeId, SlotName};
use story_engine::{CommitAction, EffectRegistry, Engine, Event};
use story_present::{BeatKind, PresentationCatalog, Presenter};

const GAME_DSL: &str = r#"
game "PresentationTest"
  seed host
  turn 1m
  home table

zones
  table

aspects
  lore: 0
  fuel: 0
  dream: 0

relations
  mentor

cards
  card master
    unique
    start in table
    relates mentor apprentice

  card apprentice
    unique
    start in table
    aspects lore 3

  card coal
    stackable
    start in table count 2
    aspects fuel 1

verbs
  verb study committable instances 1
    recipe read_tome
      priority 10
      duration 10m
      slots
        reader: 1..1 aspect:lore
        item: 1..1 consuming aspect:fuel
      effects
        ending ascension

    recipe explore_lore
      priority 20
      duration 5m
      craftable
      slots
        investigator: 1..1 aspect:lore

  verb dream autonomous instances 1
    recipe slumber
      duration 20m
      slots
        sleeper: 1..1 aspect:dream

endings
  ending ascension "Transcendence" victorious
    text "Default epilogue"
"#;

const NARRATIVE_JSON: &str = r#"{
  "cards": {
    "master": {
      "title": "Grandmaster",
      "description": "Elder sage of the archives"
    },
    "apprentice": {
      "title": "Eager Novice",
      "description": "A keen student of mysteries"
    },
    "coal": {
      "title": "Brimstone Fuel",
      "description": "Burns with intense heat"
    }
  },
  "verbs": {
    "study": {
      "title": "Esoteric Study",
      "description": "Probe hidden truths",
      "slots": {
        "reader": {
          "label": "The Scholar",
          "description": "Who conducts the study"
        },
        "item": {
          "label": "Incense or Fuel",
          "description": "Material consumed in the ritual"
        }
      }
    }
  },
  "recipes": {
    "study.read_tome": {
      "label": "Rite of Awakening",
      "start": "{reader} ignites {item} and begins the rite...",
      "completion": "The rite concludes. {reader} has consumed {item} and ascended!"
    }
  },
  "endings": {
    "ascension": {
      "title": "Eternal Dawn",
      "tier": "radiant",
      "text": "The mortal realm fades. The ascent into radiant light is complete."
    }
  }
}"#;

fn setup_presenter() -> Presenter {
    let out = story_lang::compile_source("test_game", GAME_DSL);
    assert!(
        out.diagnostics.iter().all(|d| !d.is_error()),
        "DSL compilation failed: {:?}",
        out.diagnostics
    );
    let content = Arc::new(out.content.expect("compiled content"));
    let (engine, diags) = Engine::boot(content, EffectRegistry::new(), 42, None);
    assert!(
        diags.iter().all(|d| !d.is_error()),
        "Engine boot failed: {:?}",
        diags
    );
    let catalog = PresentationCatalog::from_json(NARRATIVE_JSON).expect("valid narrative json");
    Presenter::new(engine, catalog)
}

fn find_entity(presenter: &Presenter, proto: &str) -> EntityId {
    presenter
        .state()
        .entities
        .values()
        .find(|e| e.prototype.as_str() == proto)
        .map(|e| e.id)
        .unwrap_or_else(|| panic!("expected entity of prototype `{proto}`"))
}

#[test]
fn test_commit_lifecycle_hook_emits_start_beat() {
    let mut presenter = setup_presenter();
    let reader_id = find_entity(&presenter, "apprentice");
    let fuel_id = find_entity(&presenter, "coal");

    let action = CommitAction {
        verb: "study".into(),
        bindings: vec![
            (SlotName::from("reader"), reader_id),
            (SlotName::from("item"), fuel_id),
        ],
    };

    let output = presenter.commit(action).expect("commit succeeds");

    // Lifecycle hook: SituationStarted emits ActionStart beat
    assert_eq!(output.beats.len(), 1);
    let start_beat = &output.beats[0];
    assert_eq!(start_beat.kind, BeatKind::ActionStart);
    assert_eq!(
        start_beat.text,
        "Eager Novice ignites Brimstone Fuel and begins the rite..."
    );
    assert_eq!(start_beat.clock, Some(0));
}

#[test]
fn test_advance_lifecycle_hook_completion_and_ending_beats() {
    let mut presenter = setup_presenter();
    let reader_id = find_entity(&presenter, "apprentice");
    let fuel_id = find_entity(&presenter, "coal");

    presenter
        .commit(CommitAction {
            verb: "study".into(),
            bindings: vec![
                (SlotName::from("reader"), reader_id),
                (SlotName::from("item"), fuel_id),
            ],
        })
        .expect("commit succeeds");

    // Advance halfway (5 minutes) - no beats emitted
    let mid_output = presenter.advance(5).expect("advance 5m succeeds");
    assert!(mid_output.beats.is_empty());

    // Advance remaining 5 minutes - situation finishes at 10m
    let end_output = presenter.advance(5).expect("advance to 10m succeeds");

    // Verify ActionComplete beat with {slot} interpolation (even consumed fuel!)
    let complete_beat = end_output
        .beats
        .iter()
        .find(|b| b.kind == BeatKind::ActionComplete)
        .expect("expected ActionComplete beat");
    assert_eq!(
        complete_beat.text,
        "The rite concludes. Eager Novice has consumed Brimstone Fuel and ascended!"
    );
    assert_eq!(complete_beat.clock, Some(10));

    // Verify Ended beat
    let ending_beat = end_output
        .beats
        .iter()
        .find(|b| b.kind == BeatKind::Ending)
        .expect("expected Ending beat");
    assert!(ending_beat
        .text
        .contains("Ending: \"Eternal Dawn\" (radiant)"));
    assert!(ending_beat
        .text
        .contains("The mortal realm fades. The ascent into radiant light is complete."));
    assert_eq!(ending_beat.clock, Some(10));

    // Verify fuel was consumed from state
    let remaining_coal: Vec<_> = presenter
        .state()
        .entities
        .values()
        .filter(|e| e.prototype.as_str() == "coal")
        .collect();
    assert_eq!(remaining_coal.len(), 1, "one coal was consumed");
}

#[test]
fn test_board_view() {
    let mut presenter = setup_presenter();
    let reader_id = find_entity(&presenter, "apprentice");
    let fuel_id = find_entity(&presenter, "coal");

    // Initial board view
    let initial_board = presenter.board_view();
    assert_eq!(initial_board.clock, 0);
    assert!(initial_board.ended.is_none());

    let table_zone = initial_board
        .zones
        .iter()
        .find(|z| z.kind == "table")
        .expect("table zone exists");
    assert!(table_zone
        .cards
        .iter()
        .any(|c| c.prototype == "master" && c.count == 1));
    assert!(table_zone
        .cards
        .iter()
        .any(|c| c.prototype == "coal" && c.count == 2));

    // Commit situation
    presenter
        .commit(CommitAction {
            verb: "study".into(),
            bindings: vec![
                (SlotName::from("reader"), reader_id),
                (SlotName::from("item"), fuel_id),
            ],
        })
        .expect("commit succeeds");

    // Board view with in-flight situation
    let inflight_board = presenter.board_view();
    assert_eq!(inflight_board.situations.len(), 1);
    let sit_view = &inflight_board.situations[0];
    assert_eq!(sit_view.verb, "study");
    assert_eq!(sit_view.verb_title, "Esoteric Study");
    assert_eq!(sit_view.recipe, "study.read_tome");
    assert_eq!(sit_view.recipe_label, "Rite of Awakening");
    assert_eq!(sit_view.started_at, 0);
    assert_eq!(sit_view.ends_at, 10);
    assert_eq!(sit_view.remaining_time, 10);
    assert_eq!(sit_view.total_duration, 10);

    // Verify slotted cards
    assert_eq!(sit_view.slotted.len(), 2);
    let slotted_reader = sit_view
        .slotted
        .iter()
        .find(|s| s.slot == "reader")
        .unwrap();
    assert_eq!(slotted_reader.title, "Eager Novice");
    assert_eq!(slotted_reader.prototype, "apprentice");

    let slotted_fuel = sit_view.slotted.iter().find(|s| s.slot == "item").unwrap();
    assert_eq!(slotted_fuel.title, "Brimstone Fuel");
    assert_eq!(slotted_fuel.prototype, "coal");

    // Advance 4 minutes and verify countdown
    presenter.advance(4).expect("advance succeeds");
    let advanced_board = presenter.board_view();
    assert_eq!(advanced_board.clock, 4);
    assert_eq!(advanced_board.situations[0].remaining_time, 6);

    // Complete situation and verify ended view
    presenter.advance(6).expect("advance to 10m succeeds");
    let ended_board = presenter.board_view();
    assert!(ended_board.situations.is_empty());
    let ended_view = ended_board.ended.as_ref().expect("ended view present");
    assert_eq!(ended_view.id, "ascension");
    assert_eq!(ended_view.title, "Eternal Dawn");
    assert_eq!(ended_view.tier, "radiant");
}

#[test]
fn test_verbs_view() {
    let presenter = setup_presenter();

    // Verbs view (unfiltered)
    let all_verbs = presenter.verbs_view(false);
    assert!(all_verbs
        .iter()
        .any(|v| v.id == "study" && v.mode == "committable"));
    assert!(all_verbs
        .iter()
        .any(|v| v.id == "dream" && v.mode == "autonomous"));

    let study_verb = all_verbs.iter().find(|v| v.id == "study").unwrap();
    assert_eq!(study_verb.title, "Esoteric Study");
    assert_eq!(
        study_verb.description,
        Some("Probe hidden truths".to_string())
    );

    let read_tome = study_verb
        .recipes
        .iter()
        .find(|r| r.name == "read_tome")
        .expect("read_tome recipe exists");
    assert_eq!(read_tome.label, "Rite of Awakening");
    assert_eq!(read_tome.duration, 10);
    assert!(read_tome.runnable);

    // Verify slot prompts
    let reader_slot = read_tome.slots.iter().find(|s| s.name == "reader").unwrap();
    assert_eq!(reader_slot.label, "The Scholar");
    assert_eq!(
        reader_slot.description,
        Some("Who conducts the study".to_string())
    );
    assert_eq!(reader_slot.bounds, "1..1");
    assert_eq!(reader_slot.criteria, "aspect:lore");

    let item_slot = read_tome.slots.iter().find(|s| s.name == "item").unwrap();
    assert_eq!(item_slot.label, "Incense or Fuel");
    assert!(item_slot.flags.contains("consuming"));

    // Filter runnable: only runnable recipes are returned
    let runnable_verbs = presenter.verbs_view(true);
    let runnable_study = runnable_verbs.iter().find(|v| v.id == "study").unwrap();
    assert!(runnable_study.recipes.iter().any(|r| r.name == "read_tome"));
}

#[test]
fn test_inspect_card() {
    let presenter = setup_presenter();
    let master_id = find_entity(&presenter, "master");

    // Inspect by prototype name
    let lore_by_proto = presenter.inspect_card("master").expect("card found");
    assert_eq!(lore_by_proto.title, "Grandmaster");
    assert_eq!(
        lore_by_proto.description,
        Some("Elder sage of the archives".to_string())
    );
    assert_eq!(lore_by_proto.entity_id, Some(master_id));

    // Verify relation edges: master relates mentor -> apprentice
    assert_eq!(lore_by_proto.relations.len(), 1);
    let (kind, target_title, target_id) = &lore_by_proto.relations[0];
    assert_eq!(kind, "mentor");
    assert_eq!(target_title, "Eager Novice");
    assert!(target_id.is_some());

    // Inspect by #<entity_id>
    let lore_by_id = presenter
        .inspect_card(&format!("#{}", master_id.0))
        .expect("card found by id");
    assert_eq!(lore_by_id.prototype, "master");
    assert_eq!(lore_by_id.title, "Grandmaster");

    // Inspect card with aspects
    let apprentice_lore = presenter
        .inspect_card("apprentice")
        .expect("apprentice found");
    assert_eq!(apprentice_lore.aspects.get("lore"), Some(&3));

    // Inspect non-existent card
    assert!(presenter.inspect_card("mythical_beast").is_none());
}

#[test]
fn test_snapshot_and_restore() {
    let mut presenter = setup_presenter();
    let reader_id = find_entity(&presenter, "apprentice");
    let fuel_id = find_entity(&presenter, "coal");

    // Take snapshot at t=0 before commit
    let snapshot = presenter.snapshot();

    // Commit and advance to completion
    presenter
        .commit(CommitAction {
            verb: "study".into(),
            bindings: vec![
                (SlotName::from("reader"), reader_id),
                (SlotName::from("item"), fuel_id),
            ],
        })
        .expect("commit succeeds");
    presenter.advance(10).expect("advance succeeds");

    // Verify state changed
    assert_eq!(presenter.board_view().clock, 10);
    assert!(presenter.board_view().ended.is_some());

    // Restore to initial snapshot
    presenter.restore(&snapshot).expect("restore succeeds");

    // Verify restored state matches initial state
    let restored_board = presenter.board_view();
    assert_eq!(restored_board.clock, 0);
    assert!(restored_board.situations.is_empty());
    assert!(restored_board.ended.is_none());

    let table_zone = restored_board
        .zones
        .iter()
        .find(|z| z.kind == "table")
        .expect("table zone exists");
    let coal_card = table_zone
        .cards
        .iter()
        .find(|c| c.prototype == "coal")
        .expect("coal exists");
    assert_eq!(coal_card.count, 2, "both coal cards restored");
}

/// A fixture with `shown when` and `internal` slots. `with_charge_cards`
/// controls whether any card matching the internal `cost` slot exists.
fn presence_dsl(with_charge_cards: bool) -> String {
    let rod = if with_charge_cards {
        "\n  card rod\n    stackable\n    start in table count 2\n    aspects charge 1\n"
    } else {
        ""
    };
    format!(
        r#"
game "PresenceTest"
  seed host
  turn 1m
  home table

zones
  table

aspects
  spark: 0
  charge: 0

legacies
  favor: 0

cards
  card cell
    stackable
    start in table count 3
    aspects spark 1
{rod}
verbs
  verb tinker committable instances 1
    recipe assemble
      priority 10
      duration 5m
      slots
        main: 1..1 open aspect:spark
        echo: 0..1 open aspect:spark
          shown when count(slot:main) == 1
        gated: 0..1 open aspect:spark
          shown when legacy.favor >= 1
        cost: 1..1 consuming internal aspect:charge
      effects
        narrate "assembled"
"#
    )
}

fn setup_presence(with_charge_cards: bool) -> Presenter {
    let out = story_lang::compile_source("presence_game", &presence_dsl(with_charge_cards));
    assert!(
        out.diagnostics.iter().all(|d| !d.is_error()),
        "DSL compilation failed: {:?}",
        out.diagnostics
    );
    let content = Arc::new(out.content.expect("compiled content"));
    let (engine, diags) = Engine::boot(content, EffectRegistry::new(), 7, None);
    assert!(
        diags.iter().all(|d| !d.is_error()),
        "Engine boot failed: {:?}",
        diags
    );
    Presenter::new(engine, PresentationCatalog::default())
}

fn entities_of(presenter: &Presenter, proto: &str) -> Vec<EntityId> {
    presenter
        .state()
        .entities
        .values()
        .filter(|e| e.prototype.as_str() == proto)
        .map(|e| e.id)
        .collect()
}

#[test]
fn test_presence_gates_open_slots_and_flags_internal() {
    let mut presenter = setup_presence(true);
    let cells = entities_of(&presenter, "cell");
    let rods = entities_of(&presenter, "rod");

    // Internal slots are ordinary commit data: the host assembles them.
    presenter
        .commit(CommitAction {
            verb: "tinker".into(),
            bindings: vec![
                (SlotName::from("main"), cells[0]),
                (SlotName::from("cost"), rods[0]),
            ],
        })
        .expect("commit with an internal-slot binding opens");

    let sit = &presenter.board_view().situations[0];
    let open_names: Vec<&str> = sit.open_slots.iter().map(|s| s.name.as_str()).collect();
    assert!(open_names.contains(&"main"));
    assert!(
        open_names.contains(&"echo"),
        "count(slot:main) == 1 holds, so echo is shown"
    );
    assert!(
        !open_names.contains(&"gated"),
        "a condition-false slot is gated out"
    );
    assert!(
        !open_names.contains(&"cost"),
        "an internal slot is never prompted"
    );
    // Bound cards of hidden slots still appear in `slotted`.
    assert!(sit
        .slotted
        .iter()
        .any(|sc| sc.slot == "cost" && sc.entity_id == rods[0]));

    // The flag word lands last in the slot prompt flags.
    let tinker = presenter
        .verbs_view(false)
        .into_iter()
        .find(|v| v.id == "tinker")
        .expect("tinker listed");
    let cost = tinker.recipes[0]
        .slots
        .iter()
        .find(|s| s.name == "cost")
        .expect("cost prompt");
    assert_eq!(cost.flags, "consuming internal");
}

#[test]
fn test_staging_view() {
    let presenter = setup_presence(true);
    let cells = entities_of(&presenter, "cell");

    let view = presenter.staging_view("tinker", &[(SlotName::from("main"), cells[0])]);
    assert_eq!(view.verb, "tinker");
    assert_eq!(view.matched, Some(RecipeId::from("tinker.assemble")));
    assert_eq!(view.recipe_label.as_deref(), Some("assemble"));
    let names: Vec<&str> = view.slots.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["main", "echo"],
        "the internal cost slot and the condition-false gated slot are excluded"
    );
    let main = &view.slots[0];
    assert_eq!(main.bounds, "1..1");
    assert_eq!(main.candidates.len(), 3, "every cell is a candidate");
    assert!(main.candidates.iter().any(|(e, _)| *e == cells[0]));

    // Nothing staged: main's minimum is unmet, so nothing matches.
    let none = presenter.staging_view("tinker", &[]);
    assert_eq!(none.matched, None);
    assert_eq!(none.recipe_label, None);
    assert!(none.slots.is_empty());
}

#[test]
fn test_commit_staged() {
    let mut presenter = setup_presence(true);
    let cells = entities_of(&presenter, "cell");

    let output = presenter
        .commit_staged("tinker", &[(SlotName::from("main"), cells[0])])
        .expect("the internal cost slot auto-stages and the commit opens");
    assert!(output
        .raw_events
        .iter()
        .any(|e| matches!(e, Event::SituationStarted { .. })));
    let sit = presenter
        .state()
        .situations
        .values()
        .next()
        .expect("situation open");
    assert_eq!(sit.recipe.as_str(), "tinker.assemble");
    assert_eq!(
        sit.bindings.get(&SlotName::from("cost")).map(|b| b.len()),
        Some(1),
        "one rod auto-staged into the internal slot"
    );

    // No charge card in the world: the kernel's real refusal surfaces.
    let mut presenter = setup_presence(false);
    let cells = entities_of(&presenter, "cell");
    let err = presenter
        .commit_staged("tinker", &[(SlotName::from("main"), cells[0])])
        .expect_err("the kernel refuses without a cost candidate");
    assert_eq!(err.kind, DiagKind::NoRecipe);
}
