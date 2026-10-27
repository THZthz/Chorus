use std::collections::BTreeMap;
use std::fs;
use story_content::ir::{CardDecl, EndingDecl, RecipeDecl, VerbDecl, VerbMode};
use story_content::Content;
use story_core::{CardId, EndingId, MintPolicy, RecipeId, VerbId};
use story_present::catalog::{
    CardPresentation, EndingPresentation, PresentationCatalog, RecipePresentation,
    SlotPresentation, VerbPresentation,
};

#[test]
fn test_json_deserialization_and_file_roundtrip() {
    let narrative_json = r#"{
  "cards": {
    "grimoire": {
      "title": "Tome of Whispers",
      "description": "Bound in black leather"
    }
  },
  "verbs": {
    "study": {
      "title": "Study Lore",
      "description": "Peruse the esoteric writings",
      "slots": {
        "focus": {
          "label": "Focus Material",
          "description": "An object of contemplation"
        }
      }
    }
  },
  "recipes": {
    "study.read": {
      "label": "Deep Reading",
      "start": "You open the pages...",
      "completion": "The secrets unravel.",
      "slots": {
        "book": {
          "label": "The Grimoire",
          "description": "The exact tome to read"
        }
      }
    }
  },
  "cues": {
    "night_falls": "The shadows stretch and darkness settles."
  },
  "endings": {
    "ascension": {
      "title": "The Higher Sphere",
      "tier": "radiant",
      "text": "You transcend the mortal coils."
    }
  }
}"#;

    let catalog = PresentationCatalog::from_json(narrative_json).expect("valid json deserializes");
    assert_eq!(
        catalog
            .cards
            .get("grimoire")
            .and_then(|c| c.title.as_deref()),
        Some("Tome of Whispers")
    );
    assert_eq!(
        catalog.verbs.get("study").and_then(|v| v.title.as_deref()),
        Some("Study Lore")
    );
    assert_eq!(
        catalog
            .recipes
            .get("study.read")
            .and_then(|r| r.label.as_deref()),
        Some("Deep Reading")
    );
    assert_eq!(
        catalog.cues.get("night_falls").map(|s| s.as_str()),
        Some("The shadows stretch and darkness settles.")
    );
    assert_eq!(
        catalog
            .endings
            .get("ascension")
            .and_then(|e| e.tier.as_deref()),
        Some("radiant")
    );

    // Test file save and load roundtrip
    let tmp_path = std::env::temp_dir().join("test_companion.narrative.json");
    catalog.save_to_file(&tmp_path).expect("save to file ok");
    let loaded = PresentationCatalog::from_file(&tmp_path).expect("load from file ok");
    let _ = fs::remove_file(tmp_path);

    assert_eq!(
        loaded
            .cards
            .get("grimoire")
            .and_then(|c| c.title.as_deref()),
        Some("Tome of Whispers")
    );
}

#[test]
fn test_card_resolution_cascade() {
    let mut catalog = PresentationCatalog::default();
    catalog.cards.insert(
        "card_companion".to_string(),
        CardPresentation {
            title: Some("Companion Title".to_string()),
            description: Some("Companion Desc".to_string()),
        },
    );

    let mut content = Content::default();
    content.cards.insert(
        CardId::from("card_companion"),
        CardDecl {
            id: CardId::from("card_companion"),
            kind: None,
            mint: MintPolicy::SingleCopy,
            aspects: BTreeMap::new(),
            lifetime: None,
            boot: None,
            relates: Vec::new(),
            desc: Some("DSL Desc".to_string()),
            mint_alias: None,
            on_complete: Vec::new(),
            on_expire: None,
            meta: BTreeMap::from([("title".to_string(), "DSL Title".to_string())]),
            decl_index: 0,
        },
    );
    content.cards.insert(
        CardId::from("card_dsl_only"),
        CardDecl {
            id: CardId::from("card_dsl_only"),
            kind: None,
            mint: MintPolicy::SingleCopy,
            aspects: BTreeMap::new(),
            lifetime: None,
            boot: None,
            relates: Vec::new(),
            desc: Some("DSL Desc Only".to_string()),
            mint_alias: None,
            on_complete: Vec::new(),
            on_expire: None,
            meta: BTreeMap::from([("title".to_string(), "DSL Title Only".to_string())]),
            decl_index: 1,
        },
    );

    // Companion overrides DSL
    assert_eq!(
        catalog.card_title("card_companion", &content),
        "Companion Title"
    );
    assert_eq!(
        catalog.card_desc("card_companion", &content),
        Some("Companion Desc")
    );

    // DSL overrides default
    assert_eq!(
        catalog.card_title("card_dsl_only", &content),
        "DSL Title Only"
    );
    assert_eq!(
        catalog.card_desc("card_dsl_only", &content),
        Some("DSL Desc Only")
    );

    // Default fallback when not declared
    assert_eq!(
        catalog.card_title("card_unregistered", &content),
        "card_unregistered"
    );
    assert_eq!(catalog.card_desc("card_unregistered", &content), None);
}

#[test]
fn test_verb_and_recipe_resolution_cascade() {
    let mut catalog = PresentationCatalog::default();
    catalog.verbs.insert(
        "work".to_string(),
        VerbPresentation {
            title: Some("Honest Labor".to_string()),
            description: Some("Companion Work Desc".to_string()),
            slots: BTreeMap::new(),
        },
    );
    catalog.recipes.insert(
        "work.overtime".to_string(),
        RecipePresentation {
            label: Some("Burn the Midnight Oil".to_string()),
            start: Some("Starting late shift...".to_string()),
            completion: Some("Shift completed in exhaustion.".to_string()),
            slots: BTreeMap::new(),
        },
    );

    let mut content = Content::default();
    let mut verb_work = VerbDecl {
        id: VerbId::from("work"),
        mode: VerbMode::Committable,
        instances: story_content::ir::InstanceCap::Any,
        title: Some("DSL Work".to_string()),
        releases: None,
        recipes: BTreeMap::new(),
        recipe_order: Vec::new(),
        meta: BTreeMap::from([("description".to_string(), "DSL Work Desc".to_string())]),
        decl_index: 0,
    };

    let recipe_overtime = RecipeDecl {
        verb: VerbId::from("work"),
        name: "overtime".to_string(),
        id: RecipeId::from("work.overtime"),
        label: Some("DSL Overtime".to_string()),
        priority: 10,
        craftable: true,
        chain_only: false,
        max: None,
        duration: 5,
        declares: Vec::new(),
        when: None,
        guards: Vec::new(),
        slots: Vec::new(),
        effects: Vec::new(),
        on_complete: None,
        allow_shadow: false,
        meta: BTreeMap::from([
            ("start".to_string(), "DSL start...".to_string()),
            ("completion".to_string(), "DSL completion...".to_string()),
        ]),
        decl_index: 0,
    };

    let recipe_standard = RecipeDecl {
        verb: VerbId::from("work"),
        name: "standard".to_string(),
        id: RecipeId::from("work.standard"),
        label: Some("DSL Standard Labor".to_string()),
        priority: 5,
        craftable: true,
        chain_only: false,
        max: None,
        duration: 3,
        declares: Vec::new(),
        when: None,
        guards: Vec::new(),
        slots: Vec::new(),
        effects: Vec::new(),
        on_complete: None,
        allow_shadow: false,
        meta: BTreeMap::from([
            ("start".to_string(), "DSL Standard Start".to_string()),
            ("completion".to_string(), "DSL Standard Comp".to_string()),
        ]),
        decl_index: 1,
    };

    verb_work
        .recipes
        .insert("overtime".to_string(), recipe_overtime.clone());
    verb_work
        .recipes
        .insert("standard".to_string(), recipe_standard.clone());
    content.verbs.insert("work".to_string(), verb_work);

    // Verb cascade
    assert_eq!(catalog.verb_title("work", &content), "Honest Labor");
    assert_eq!(
        catalog.verb_desc("work", &content),
        Some("Companion Work Desc")
    );
    assert_eq!(
        catalog.verb_title("unregistered_verb", &content),
        "unregistered_verb"
    );
    assert_eq!(catalog.verb_desc("unregistered_verb", &content), None);

    // Recipe cascade: Companion overrides DSL
    assert_eq!(
        catalog.recipe_label("work.overtime", &recipe_overtime),
        "Burn the Midnight Oil"
    );
    assert_eq!(
        catalog.recipe_start("work.overtime", &recipe_overtime),
        Some("Starting late shift...")
    );
    assert_eq!(
        catalog.recipe_completion("work.overtime", &recipe_overtime),
        Some("Shift completed in exhaustion.")
    );

    // Recipe cascade: DSL overrides default
    assert_eq!(
        catalog.recipe_label("work.standard", &recipe_standard),
        "DSL Standard Labor"
    );
    assert_eq!(
        catalog.recipe_start("work.standard", &recipe_standard),
        Some("DSL Standard Start")
    );
    assert_eq!(
        catalog.recipe_completion("work.standard", &recipe_standard),
        Some("DSL Standard Comp")
    );
}

#[test]
fn test_slot_prompt_resolution_cascade() {
    let mut catalog = PresentationCatalog::default();

    // Verb slot prompt for "verb_slot" and "shared_slot"
    catalog.verbs.insert(
        "study".to_string(),
        VerbPresentation {
            title: None,
            description: None,
            slots: BTreeMap::from([
                (
                    "shared_slot".to_string(),
                    SlotPresentation {
                        label: Some("Verb Shared Label".to_string()),
                        description: Some("Verb Shared Desc".to_string()),
                    },
                ),
                (
                    "verb_only_slot".to_string(),
                    SlotPresentation {
                        label: Some("Verb Only Label".to_string()),
                        description: Some("Verb Only Desc".to_string()),
                    },
                ),
            ]),
        },
    );

    // Recipe slot prompt overrides shared_slot
    catalog.recipes.insert(
        "study.read".to_string(),
        RecipePresentation {
            label: None,
            start: None,
            completion: None,
            slots: BTreeMap::from([(
                "shared_slot".to_string(),
                SlotPresentation {
                    label: Some("Recipe Specific Label".to_string()),
                    description: Some("Recipe Specific Desc".to_string()),
                },
            )]),
        },
    );

    // 1. Recipe slot prompt takes highest precedence
    assert_eq!(
        catalog.slot_label("study.read", "shared_slot"),
        "Recipe Specific Label"
    );
    assert_eq!(
        catalog.slot_desc("study.read", "shared_slot"),
        Some("Recipe Specific Desc")
    );

    // 2. Verb slot prompt takes precedence when recipe slot prompt is absent
    assert_eq!(
        catalog.slot_label("study.read", "verb_only_slot"),
        "Verb Only Label"
    );
    assert_eq!(
        catalog.slot_desc("study.read", "verb_only_slot"),
        Some("Verb Only Desc")
    );

    // 3. Default prompt takes precedence when neither is provided
    assert_eq!(
        catalog.slot_label("study.read", "unconfigured_slot"),
        "unconfigured_slot"
    );
    assert_eq!(catalog.slot_desc("study.read", "unconfigured_slot"), None);
}

#[test]
fn test_ending_resolution_cascade() {
    let mut catalog = PresentationCatalog::default();
    catalog.endings.insert(
        "triumph".to_string(),
        EndingPresentation {
            title: Some("Catalog Triumph".to_string()),
            tier: Some("glorious".to_string()),
            text: Some("Catalog Epilogue".to_string()),
        },
    );
    catalog.endings.insert(
        "partial_override".to_string(),
        EndingPresentation {
            title: None,
            tier: None,
            text: Some("Overridden Epilogue".to_string()),
        },
    );

    let dsl_triumph = EndingDecl {
        id: EndingId::from("triumph"),
        title: "DSL Triumph".to_string(),
        tier: "victorious".to_string(),
        text: Some("DSL Epilogue".to_string()),
        meta: Default::default(),
        decl_index: 0,
    };

    let dsl_defeat = EndingDecl {
        id: EndingId::from("defeat"),
        title: "Bitter Defeat".to_string(),
        tier: "tragic".to_string(),
        text: Some("Everything fell into ruin.".to_string()),
        meta: Default::default(),
        decl_index: 1,
    };

    // Companion overrides DSL
    let (title, tier, text) = catalog.ending_prose("triumph", Some(&dsl_triumph));
    assert_eq!(title, "Catalog Triumph");
    assert_eq!(tier, "glorious");
    assert_eq!(text, Some("Catalog Epilogue"));

    // Partial override retains DSL title and tier
    let (title, tier, text) = catalog.ending_prose("partial_override", Some(&dsl_defeat));
    assert_eq!(title, "Bitter Defeat");
    assert_eq!(tier, "tragic");
    assert_eq!(text, Some("Overridden Epilogue"));

    // DSL overrides defaults
    let (title, tier, text) = catalog.ending_prose("defeat", Some(&dsl_defeat));
    assert_eq!(title, "Bitter Defeat");
    assert_eq!(tier, "tragic");
    assert_eq!(text, Some("Everything fell into ruin."));

    // Absent declaration falls back to defaults ("somber", ending id, None)
    let (title, tier, text) = catalog.ending_prose("unknown_ending", None);
    assert_eq!(title, "unknown_ending");
    assert_eq!(tier, "somber");
    assert_eq!(text, None);
}
