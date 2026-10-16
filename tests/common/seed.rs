//! The seed world + seed content — the spec's flagship examples as data
//! (§6.4). A test fixture, not engine machinery.

//! (Shared test lib: every helper is the common discipline, not per-file
//! dead code — each mounting test crate uses a different subset, exactly like
//! harness.rs. The `allow` lives in this one place.)
#![allow(dead_code)]

use chorus::content::content::{
    ContentRaw, DeriveAspect, PrototypeDef, PrototypeDerive, RecipeDef, SlotDef, SlotTriggerDef,
    TriggerDef, VerbContent,
};
use chorus::core::types::{
    add_entity, new_state, relation_key, AspectVal, Aspects, AttrValue, Attrs, Entity, Relation,
    State,
};

/// The seed content: the spec's flagship examples as data (§6.4).
#[must_use]
pub fn seed_content() -> ContentRaw {
    let verb = |slots: Vec<SlotDef>, duration: i64, recipes: Vec<RecipeDef>| VerbContent {
        slots,
        duration,
        recipes,
    };
    let slot = |role: &str, min: i64, max: Option<i64>, query: Option<&str>| SlotDef {
        role: role.to_string(),
        min,
        max,
        query: query.map(str::to_string),
        shown: None,
    };
    let recipe = |id: Option<&str>, when: Option<&str>, out: &[&str]| RecipeDef {
        id: id.map(str::to_string),
        when: when.map(str::to_string),
        shown: None,
        hint: None,
        max: None,
        aspects: Vec::new(),
        out: out.iter().map(|s| (*s).to_string()).collect(),
    };
    ContentRaw {
        seed: None,
        verbs: [
            (
                "READ",
                verb(
                    vec![
                        slot("actors", 1, Some(2), Some("kind:character")),
                        slot("object", 1, Some(1), Some("kind:text")),
                        slot("at", 0, Some(1), Some("kind:place")),
                    ],
                    60,
                    vec![
                        recipe(
                            Some("read.magick_gate"),
                            Some("object.aspect.magick >= 1 AND NOT any(actors, aspect:occult >= 2)"),
                            &["REFUSE(\"The letters swim like minnows.\")"],
                        ),
                        recipe(
                            Some("read.parent_child"),
                            Some("count(actors) = 2 AND rel(parent_child, actors)"),
                            &[
                                "SPAWN BONDING_MEMORY",
                                "NARRATE(\"{actors} read aloud; the child sleeps wiser.\")",
                            ],
                        ),
                        recipe(
                            Some("read.unrelated_pair"),
                            Some("count(actors) = 2"),
                            &["NARRATE(\"{actors} trade the book back and forth and lose the thread.\")"],
                        ),
                        recipe(
                            Some("read.secret"),
                            Some("object.aspect.secret >= 3"),
                            &[
                                "SPAWN CLUE",
                                "NARRATE(\"{actors} tease out a hidden thread.\")",
                            ],
                        ),
                        recipe(None, None, &["NARRATE(\"{actors} read {object}.\")"]),
                    ],
                ),
            ),
            (
                // v0.3: mirrors READ's role grammar exactly (§4.3) —
                // cardinalities and queries stay content; the CS study station
                // is authored on top
                "STUDY",
                verb(
                    vec![
                        slot("actors", 1, Some(2), Some("kind:character")),
                        slot("object", 1, Some(1), Some("kind:text")),
                        slot("at", 0, Some(1), Some("kind:place")),
                    ],
                    60,
                    vec![recipe(None, None, &["NARRATE(\"{actors} study {object}.\")"])],
                ),
            ),
            (
                "TALK",
                verb(
                    vec![
                        slot("actors", 1, Some(3), Some("kind:character")),
                        slot("about", 0, Some(1), Some("kind:idea")),
                        slot("at", 0, Some(1), Some("kind:place")),
                    ],
                    60,
                    vec![
                        recipe(
                            Some("talk.plan"),
                            Some("about.aspect.secret >= 1"),
                            &["SPAWN PLAN_INFILTRATION"],
                        ),
                        recipe(None, None, &["NARRATE(\"{actors} talk.\")"]),
                    ],
                ),
            ),
            (
                "TRAVEL_TO",
                verb(
                    vec![
                        slot("actors", 1, Some(3), Some("kind:character")),
                        slot("object", 1, Some(1), Some("kind:place")),
                    ],
                    60,
                    vec![recipe(
                        None,
                        None,
                        &[
                            "MOVE ACTORS INTO OBJECT",
                            "NARRATE(\"{actors} travel to {object}.\")",
                        ],
                    )],
                ),
            ),
            (
                "USE_ON",
                verb(
                    vec![
                        slot("actors", 1, Some(5), Some("kind:character")),
                        slot("object", 1, Some(1), Some("kind:rite")),
                        slot("with", 0, Some(2), Some("kind:item")),
                        slot("at", 0, Some(1), Some("kind:place")),
                    ],
                    180,
                    vec![
                        RecipeDef {
                            id: Some("rite.lantern_gate".to_string()),
                            when: Some("total(actors, aspect:lantern) < object.aspect.demand".to_string()),
                            shown: Some(true),
                            hint: Some(
                                "The rite wants pooled lantern — it will starve.".to_string(),
                            ),
                            max: None,
                            aspects: Vec::new(),
                            out: vec!["REFUSE(\"The rite starves for light.\")".to_string()],
                        },
                        recipe(
                            None,
                            None,
                            &[
                                "PROGRESS(1h, \"The circle begins to hum against your palms.\")",
                                "PROGRESS(2h, \"Light leaks between the chalk lines.\")",
                                "DESTROY OBJECT",
                                "DESTROY WITH",
                                "SPAWN MARK_OF_ASCENSION INTO at",
                            ],
                        ),
                    ],
                ),
            ),
            (
                "WAIT",
                verb(
                    vec![slot("actors", 1, Some(1), Some("kind:character"))],
                    60,
                    vec![recipe(None, None, &["NARRATE(\"{actors} wait.\")"])],
                ),
            ),
            (
                "WORK",
                verb(
                    vec![
                        slot("actors", 1, Some(1), Some("kind:character")),
                        slot("at", 0, Some(1), Some("kind:place")),
                    ],
                    120,
                    vec![recipe(None, None, &["NARRATE(\"{actors} work at {at}.\")"])],
                ),
            ),
            (
                "EXPLORE",
                verb(
                    vec![
                        slot("actors", 1, Some(1), Some("kind:character")),
                        slot("object", 1, Some(1), Some("kind:place")),
                    ],
                    60,
                    vec![recipe(None, None, &["NARRATE(\"{actors} explore {object}.\")"])],
                ),
            ),
            (
                // v0.3: mirrors EXPLORE's role grammar (§4.3) — the
                // enter-a-place shape
                "MANSUS",
                verb(
                    vec![
                        slot("actors", 1, Some(1), Some("kind:character")),
                        slot("object", 1, Some(1), Some("kind:place")),
                    ],
                    60,
                    vec![recipe(
                        None,
                        None,
                        &["NARRATE(\"{actors} walk the Mansus to {object}.\")"],
                    )],
                ),
            ),
            (
                "DREAM",
                verb(
                    vec![
                        slot("actors", 1, Some(1), Some("kind:character")),
                        slot("with", 0, Some(1), Some("kind:idea")),
                    ],
                    60,
                    vec![recipe(None, None, &["NARRATE(\"{actors} dream.\")"])],
                ),
            ),
            (
                "LOCATE_AT",
                verb(
                    vec![
                        slot("actors", 1, Some(3), Some("kind:character")),
                        slot("object", 1, Some(1), Some("kind:place")),
                    ],
                    0,
                    vec![recipe(None, None, &["MOVE ACTORS INTO OBJECT"])],
                ),
            ),
            (
                "ACQUIRE",
                verb(
                    vec![
                        slot("actors", 1, Some(1), Some("kind:character")),
                        slot("object", 1, Some(1), None),
                    ],
                    0,
                    vec![recipe(None, None, &["NARRATE(\"{actors} acquire {object}.\")"])],
                ),
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect(),
        triggers: Some(vec![TriggerDef::Slot(SlotTriggerDef {
            id: "dusk_falls".to_string(),
            slot: 32, // 16:00
            once: None,
            out: vec![
                "SPAWN DREAD".to_string(),
                "NARRATE \"The lamps come on.\"".to_string(),
            ],
        })]),
        prototypes: Some([
            (
                "CLUE_SECRET_HIDEOUT",
                PrototypeDef {
                    kind: "idea".into(),
                    stackable: false,
                    aspects: Some([("secret", AspectVal::Num(1))].into_iter().map(|(k, v)| (k.to_string(), v)).collect()),
                    attrs: None,
                    derive: None,
                },
            ),
            ("PLAN_INFILTRATION", PrototypeDef { kind: "idea".into(), ..Default::default() }),
            ("BONDING_MEMORY", PrototypeDef { kind: "idea".into(), ..Default::default() }),
            ("MARK_OF_ASCENSION", PrototypeDef { kind: "idea".into(), ..Default::default() }),
            ("DREAD", PrototypeDef { kind: "state".into(), ..Default::default() }),
            (
                "CLUE",
                PrototypeDef {
                    kind: "idea".into(),
                    stackable: false,
                    aspects: None,
                    attrs: None,
                    derive: Some(PrototypeDerive {
                        name_from: Some("object".to_string()),
                        aspects: Some([(
                            "secret".to_string(),
                            DeriveAspect {
                                from_object_aspect: "secret".to_string(),
                                minus: 2,
                            },
                        )]
                        .into_iter()
                        .collect()),
                    }),
                },
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()),
        templates: Some(
            [
                ("READ", "{actors} read {object} at {at}."),
                ("STUDY", "{actors} study {object} at {at}."),
                ("TALK", "{actors} talk about {about} at {at}."),
                ("TRAVEL_TO", "{actors} travel to {object}."),
                ("USE_ON", "{actors} use {object} at {at}."),
                ("WAIT", "{actors} wait."),
                ("WORK", "{actors} work at {at}."),
                ("EXPLORE", "{actors} explore {object}."),
                ("MANSUS", "{actors} walk the Mansus to {object}."),
                ("DREAM", "{actors} dream."),
                ("LOCATE_AT", "{actors} are at {object}."),
                ("ACQUIRE", "{actors} acquire {object}."),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ),
        endings: None,
        functions: None,
        decks: None,
    }
}

/// The seed world. (Test-fixture path: `add_entity` cannot fail on
/// this literal world, so the `expect`s are the fixture's proof, not engine
/// panics — test code may panic on its own fixtures.)
#[must_use]
pub fn seed_state() -> State {
    let mut s = new_state(0);

    fn place(s: &mut State, id: &str, name: &str, container: Option<&str>) {
        let mut attrs = Attrs::new();
        if let Some(c) = container {
            attrs.insert("container".to_string(), AttrValue::Str(c.to_string()));
        }
        add_entity(
            s,
            Entity {
                id: id.to_string(),
                kind: "place".to_string(),
                name: name.to_string(),
                aspects: Aspects::new(),
                attrs,
                born_action: None,
                dead_action: None,
                stackable: false,
            },
        )
        .expect("seed fixture place");
    }

    place(&mut s, "house", "HOUSE", None);
    // v0.3: not 'STUDY' — the verb lexeme is reserved (§9.14)
    place(&mut s, "study", "THE_STUDY", Some("house"));
    s.entities
        .get_mut("study")
        .expect("seed fixture study")
        .aspects
        .insert("study".to_string(), AspectVal::Num(1));
    place(&mut s, "dining", "DINING_HALL", Some("house"));
    place(&mut s, "villa", "VILLA", None);
    place(&mut s, "greenhouse", "GREENHOUSE", Some("villa"));

    fn character(s: &mut State, id: &str, name: &str, aspects: Aspects, container: &str) {
        let mut attrs = Attrs::new();
        attrs.insert("container".to_string(), AttrValue::Str(container.to_string()));
        add_entity(
            s,
            Entity {
                id: id.to_string(),
                kind: "character".to_string(),
                name: name.to_string(),
                aspects,
                attrs,
                born_action: None,
                dead_action: None,
                stackable: false,
            },
        )
        .expect("seed fixture character");
    }

    character(
        &mut s,
        "ana",
        "ANA",
        [("occult", AspectVal::Num(2)), ("lantern", AspectVal::Num(1))]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        "house",
    );
    character(&mut s, "bertha", "BERTHA", Aspects::new(), "house");
    character(
        &mut s,
        "follower",
        "MY_FOLLOWER",
        [("lantern", AspectVal::Num(2))].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        "house",
    );

    fn text(s: &mut State, id: &str, name: &str, aspects: Aspects, container: &str) {
        let mut attrs = Attrs::new();
        attrs.insert("container".to_string(), AttrValue::Str(container.to_string()));
        add_entity(
            s,
            Entity {
                id: id.to_string(),
                kind: "text".to_string(),
                name: name.to_string(),
                aspects,
                attrs,
                born_action: None,
                dead_action: None,
                stackable: false,
            },
        )
        .expect("seed fixture text");
    }

    text(
        &mut s,
        "forgetting",
        "THE_FORGETTING",
        [("secret", AspectVal::Num(4))].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        "house",
    );
    text(
        &mut s,
        "pale",
        "THE_PALE_LADY",
        [
            ("secret", AspectVal::Num(3)),
            (
                "language",
                AspectVal::Sub(
                    [("latin", 2i64)].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
                ),
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect(),
        "study",
    );
    text(
        &mut s,
        "fairy",
        "FAIRY_TALES",
        [("tender", AspectVal::Num(1))].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        "house",
    );
    text(
        &mut s,
        "magick",
        "MAGICK_BOOK",
        [("magick", AspectVal::Num(1))].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        "study",
    );

    fn plain(s: &mut State, id: &str, kind: &str, name: &str, aspects: Aspects, container: &str) {
        let mut attrs = Attrs::new();
        attrs.insert("container".to_string(), AttrValue::Str(container.to_string()));
        add_entity(
            s,
            Entity {
                id: id.to_string(),
                kind: kind.to_string(),
                name: name.to_string(),
                aspects,
                attrs,
                born_action: None,
                dead_action: None,
                stackable: false,
            },
        )
        .expect("seed fixture entity");
    }

    plain(
        &mut s,
        "rite",
        "rite",
        "RITE_OF_ASCENSION",
        [("demand", AspectVal::Num(3))].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        "house",
    );
    plain(&mut s, "chalice", "item", "CHALICE", Aspects::new(), "house");

    // a clue already in play — the TALK ABOUT topic
    plain(
        &mut s,
        "clue_hideout",
        "idea",
        "CLUE_SECRET_HIDEOUT",
        [("secret", AspectVal::Num(1))].into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        "house",
    );

    // ANA is BERTHA's parent — the seed world's one social edge
    let key = relation_key("ana", "bertha", "parent_child", true);
    s.relations.insert(
        key,
        Relation {
            a: "ana".to_string(),
            b: "bertha".to_string(),
            type_: "parent_child".to_string(),
            value: 1,
            born_action: 0,
            dead_action: None,
        },
    );
    s
}
