//! Smoke tests: minimal per-op / per-phase proof-of-life.
#[cfg(test)]
mod smoke {

    use indexmap::IndexMap;

    use crate::content::{
        compile_content, CompiledContent, ContentRaw, PrototypeDef, RecipeDef, SlotDef, VerbContent,
    };
    use crate::core::clock::{parse_clock, Clock};
    use crate::core::dice::roll_die;
    use crate::core::dict::{core_dictionary, Dictionary};
    use crate::core::parse::parse_sentence;
    use crate::core::r#ref::Ref;
    use crate::core::sentence::Arg;
    use crate::core::types::{
        add_entity, clone_state, new_state, AspectVal, Aspects, AttrValue, Attrs, Ending, Entity,
        State,
    };
    use crate::engine::exec::register_windows;
    use crate::engine::{apply_record, execute_step, ActionRecord, ExecOptions, ExecResult, Step};

    fn pace(duration: i64, out: &[&str]) -> VerbContent {
        VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            }],
            duration,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: out.iter().map(|s| s.to_string()).collect(),
            }],
        }
    }

    /// Test rig — code-registered verbs (§10).
    fn rig(
        verbs: Vec<(&str, VerbContent)>,
        extra: impl FnOnce(&mut ContentRaw),
    ) -> (Dictionary, CompiledContent) {
        let mut dict = core_dictionary();
        let mut map: IndexMap<String, VerbContent> = IndexMap::new();
        for (name, v) in verbs {
            if !dict.is_verb(name) {
                dict.register_verb(name).unwrap();
            }
            map.insert(name.to_string(), v);
        }
        let mut raw = ContentRaw { verbs: map, ..ContentRaw::default() };
        extra(&mut raw);
        let content = compile_content(&raw, &dict, &[]).unwrap();
        (dict, content)
    }

    fn seed_state() -> State {
        let mut s = new_state(0);
        let card = |id: &str, kind: &str, name: &str, container: Option<&str>| Entity {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects: Aspects::new(),
            attrs: match container {
                Some(c) => Attrs::from([("container".to_string(), AttrValue::Str(c.to_string()))]),
                None => Attrs::new(),
            },
            born_action: None,
            dead_action: None,
            stackable: false,
        };
        add_entity(&mut s, card("house", "place", "HOUSE", None)).unwrap();
        add_entity(&mut s, card("ana", "character", "ANA", Some("house"))).unwrap();
        add_entity(&mut s, card("book", "text", "MAGICK_BOOK", Some("house"))).unwrap();
        s
    }

    fn run(
        content: &CompiledContent,
        dict: &Dictionary,
        state: State,
        src: &str,
        first: i64,
    ) -> ExecResult {
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
            ExecOptions { first_record_id: first, id_gen: None },
        )
        .unwrap()
    }

    fn advance(
        content: &CompiledContent,
        dict: &Dictionary,
        state: State,
        clock: &str,
        first: i64,
    ) -> ExecResult {
        execute_step(
            content,
            dict,
            state,
            Step {
                start_min: None,
                clock: Some(parse_clock(clock).unwrap()),
                clauses: Vec::new(),
                t_real: None,
            },
            ExecOptions { first_record_id: first, id_gen: None },
        )
        .unwrap()
    }

    #[test]
    fn begin_advance_complete_walks_the_four_phases() {
        let (dict, content) = rig(vec![("PACE", pace(30, &["NARRATE \"done.\""]))], |_| {});
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA PACE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(r.records.len(), 1);
        assert_eq!(r.records[0].kind, "action");
        assert_eq!(r.records[0].recipes, ["pace.default"]);
        assert_eq!(r.records[0].advanced_min, 30);
        assert_eq!(r.state.now, 0, "beginning moves nothing");
        assert_eq!(r.state.windows.len(), 1);
        // the pass: completion then the wait row
        let adv = advance(&content, &dict, r.state, "0:1", 2);
        assert_eq!(
            adv.records.iter().map(|x| x.kind.as_str()).collect::<Vec<_>>(),
            ["complete", "wait"]
        );
        assert_eq!(adv.state.now, 30);
        assert_eq!(adv.state.windows.len(), 0);
    }

    #[test]
    fn all_outcome_ops_smoke() {
        // one recipe exercising every op; ENDING first-wins, REFUSE checked
        // separately (it refuses the step), ROLLED/DRAWN engine-written.
        let out = [
            "RELATE(ANA, MAGICK_BOOK, parent_child)",
            "SPAWN CLUE_SECRET_HIDEOUT INTO at AS #e_minted",
            "SET(MAGICK_BOOK, aspect:secret, 3)",
            "SET(MAGICK_BOOK, curse, +2)",
            "SET(MAGICK_BOOK, note, \"scribbled\")",
            "REVEAL MAGICK_BOOK",
            "MOVE MAGICK_BOOK INTO ANA",
            "ADVANCE(1h)",
            "NARRATE \"bump.\"",
            "PROGRESS(30m, \"ticking.\")",
            "SPAWN CLUE_SECRET_HIDEOUT AS #e_extra",
            "DESTROY OBJECT",
            "ENDING QUIET_DUSK",
            "ENDING OTHER_END",
        ];
        let mut verbs: Vec<(&str, VerbContent)> = vec![(
            "STUDY",
            VerbContent {
                slots: vec![
                    SlotDef {
                        role: "actors".into(),
                        min: 1,
                        max: Some(1),
                        query: Some("kind:character".into()),
                        shown: None,
                    },
                    SlotDef {
                        role: "object".into(),
                        min: 1,
                        max: Some(1),
                        query: None,
                        shown: None,
                    },
                    SlotDef {
                        role: "at".into(),
                        min: 0,
                        max: Some(1),
                        query: Some("kind:place".into()),
                        shown: None,
                    },
                ],
                duration: 30,
                recipes: vec![RecipeDef {
                    id: None,
                    when: None,
                    shown: None,
                    hint: None,
                    max: None,
                    aspects: Vec::new(),
                    out: out.iter().map(|s| s.to_string()).collect(),
                }],
            },
        )];
        verbs.push(("REL", pace(0, &["UNRELATE(ANA, MAGICK_BOOK, parent_child)"])));
        let (dict, content) = rig(verbs, |raw| {
            raw.endings = Some(
                [
                    (
                        "QUIET_DUSK".to_string(),
                        crate::content::EndingDef {
                            title: "A quiet dusk.".to_string(),
                            flavour: Some("calm".to_string()),
                        },
                    ),
                    (
                        "OTHER_END".to_string(),
                        crate::content::EndingDef { title: "Another.".to_string(), flavour: None },
                    ),
                ]
                .into_iter()
                .collect(),
            );
            raw.prototypes = Some(
                [(
                    "CLUE_SECRET_HIDEOUT".to_string(),
                    PrototypeDef {
                        kind: "idea".to_string(),
                        stackable: true,
                        aspects: Some(
                            Attrs::from([("secret".to_string(), AttrValue::Num(1))])
                                .into_iter()
                                .map(|(k, v)| {
                                    (
                                        k,
                                        AspectVal::Num(match v {
                                            AttrValue::Num(n) => n,
                                            _ => 0,
                                        }),
                                    )
                                })
                                .collect(),
                        ),
                        attrs: None,
                        derive: None,
                    },
                )]
                .into_iter()
                .collect(),
            );
        });
        // the study turn: BEGIN carries only PROGRESS
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA STUDY MAGICK_BOOK AT HOUSE", 1);
        assert_eq!(r.rejected, None, "rejected: {:?}", r.rejected);
        assert!(
            r.records[0].outcomes.iter().all(|o| o.op == "PROGRESS"),
            "begin carries only the PROGRESS tick"
        );
        assert_eq!(r.state.ended, None, "the ending rides the completion");
        // the completion: the recipe's real outcomes land there
        let adv = advance(&content, &dict, r.state, "0:1", 2);
        assert_eq!(adv.rejected, None, "rejected: {:?}", adv.rejected);
        let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
        let mut ops: Vec<&str> = complete.outcomes.iter().map(|o| o.op.as_str()).collect();
        // IF/ROLL/DRAW/ROLLED/DRAWN are exercised by their own smokes below;
        // REFUSE cannot co-apply (it discards the draft) — its smoke is
        // separate. PROGRESS rode the BEGIN row (skipped at completion).
        // ENDING x2: the second is a logged no-op.
        for op in [
            "RELATE", "SPAWN", "SET", "SET", "SET", "REVEAL", "MOVE", "ADVANCE", "NARRATE",
            "SPAWN", "DESTROY", "ENDING", "ENDING",
        ] {
            assert_eq!(ops.first().copied(), Some(op), "op order at {ops:?}");
            ops.remove(0);
        }
        let st = &adv.state;
        assert_eq!(st.ended.as_ref().unwrap().id, "QUIET_DUSK", "first ENDING wins");
        // SET aspect + delta + str all landed
        let book = st.entities.get("book").unwrap();
        assert_eq!(book.aspects.get("secret"), Some(&AspectVal::Num(3)));
        assert_eq!(book.attrs.get("curse"), Some(&AttrValue::Num(2)));
        assert_eq!(book.attrs.get("note"), Some(&AttrValue::Str("scribbled".into())));
        assert_eq!(book.attrs.get("state.hidden"), Some(&AttrValue::Bool(false)));
        assert_eq!(book.attrs.get("container"), Some(&AttrValue::Str("ana".into())));
        // RELATE landed, UNRELATE (second verb) will stamp it
        assert!(st.relations.contains_key("ana|book|parent_child"));
        // both spawns share the stackable name pool
        assert_eq!(st.by_name.get("CLUE_SECRET_HIDEOUT").map(|v| v.len()), Some(2));
        // ADVANCE extended the pass target: 30 (window) + 60 (ADVANCE)
        assert_eq!(adv.state.now, 90);
    }

    #[test]
    fn unrelate_stamps_the_relation_dead() {
        let rel_verb = VerbContent {
            slots: vec![
                SlotDef {
                    role: "actors".into(),
                    min: 1,
                    max: Some(1),
                    query: Some("kind:character".into()),
                    shown: None,
                },
                SlotDef { role: "object".into(), min: 1, max: Some(1), query: None, shown: None },
            ],
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: vec!["RELATE(ANA, MAGICK_BOOK, parent_child)".to_string()],
            }],
        };
        let unrel_verb = VerbContent {
            slots: rel_verb.slots.clone(),
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: vec!["UNRELATE(ANA, MAGICK_BOOK, parent_child)".to_string()],
            }],
        };
        let (dict, content) = rig(vec![("LINK", rel_verb), ("UNLINK", unrel_verb)], |_| {});
        let s1 = run(&content, &dict, seed_state(), "0:0 @ ANA LINK MAGICK_BOOK", 1);
        assert_eq!(s1.rejected, None, "{:?}", s1.rejected);
        let c1 = advance(&content, &dict, s1.state, "0:0", 2);
        assert!(c1.state.relations.contains_key("ana|book|parent_child"));
        let s2 = run(&content, &dict, c1.state, "0:0 @ ANA UNLINK MAGICK_BOOK", 5);
        assert_eq!(s2.rejected, None, "{:?}", s2.rejected);
        let c2 = advance(&content, &dict, s2.state, "0:0", 6);
        let rel = c2.state.relations.get("ana|book|parent_child").unwrap();
        assert!(rel.dead_action.is_some(), "UNRELATE stamps, never erases (§7)");
    }

    #[test]
    fn refuse_gate_rejects_at_the_table_with_id_recycle() {
        let (dict, content) = rig(
            vec![
                ("PACE", pace(30, &["NARRATE \"done.\""])),
                ("GATE", pace(30, &["REFUSE(\"no.\")"])),
            ],
            |_| {},
        );
        let state = seed_state();
        let r1 = run(&content, &dict, state, "0:0 @ ANA PACE", 1);
        assert_eq!(r1.state.windows.len(), 1);
        // the GATE step is refused: the advance (complete+wait) is discarded,
        // the attempt row takes firstRecordId = 2, state untouched
        let r2 = run(&content, &dict, clone_state(&r1.state), "0:1 @ ANA GATE", 2);
        assert_eq!(r2.rejected, None);
        assert_eq!(r2.records.len(), 1);
        assert_eq!(r2.records[0].id, 2, "the refusal row reuses firstRecordId");
        assert_eq!(r2.records[0].advanced_min, 0);
        assert!(r2.records[0].outcomes.iter().any(|o| o.op == "REFUSE"));
        assert_eq!(r2.state.now, 0, "clock unmoved");
        assert_eq!(
            r2.state.windows.len(),
            1,
            "the window the pass would have closed is still open"
        );
        // same inputs again → the SAME id is minted again (id recycle)
        let r3 = run(&content, &dict, r1.state, "0:1 @ ANA GATE", 2);
        assert_eq!(r3.records[0].id, 2);
    }

    #[test]
    fn roll_binds_per_clause_and_rewrites_roll_prose() {
        let roll_verb = VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            }],
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: vec!["ROLL(2, 6)".to_string(), "NARRATE \"rolled {roll}.\"".to_string()],
            }],
        };
        let (dict, content) = rig(vec![("CAST", roll_verb)], |_| {});
        let seed = content.seed;
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA CAST", 1);
        assert_eq!(r.rejected, None);
        assert!(r.records[0].outcomes.is_empty(), "ROLL is not PROGRESS — BEGIN carries nothing");
        let adv = advance(&content, &dict, r.state, "0:0", 2);
        assert_eq!(adv.rejected, None);
        let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
        let outcomes = &complete.outcomes;
        assert_eq!(outcomes[0].op, "ROLLED");
        let (Arg::Int { v: total }, Arg::Int { v: ctr_after }) =
            (&outcomes[0].args[0], &outcomes[0].args[1])
        else {
            panic!("ROLLED(total, ctrAfter)");
        };
        let expect: i64 = (0..2).map(|i| roll_die(seed, i, 6)).sum();
        assert_eq!(*total, expect);
        assert_eq!(*ctr_after, 2);
        assert_eq!(adv.state.rng, 2);
        // the concrete NARRATE carries the NUMBER, not {roll}
        let Arg::Str { v } = &outcomes[1].args[0] else {
            panic!("NARRATE str");
        };
        assert_eq!(v, &format!("rolled {expect}."));
        // the fold lands the same counter from the ROLLED row (law 3)
        let mut folded = new_state(0);
        for rec in &adv.records {
            apply_record(rec, &mut folded, &dict, &content).unwrap();
        }
        assert_eq!(folded.rng, 2);
    }

    #[test]
    fn draw_concretizes_to_drawn_plus_spawn_and_fold_asserts() {
        let draw_verb = VerbContent {
            slots: vec![SlotDef {
                role: "actors".into(),
                min: 1,
                max: Some(1),
                query: Some("kind:character".into()),
                shown: None,
            }],
            duration: 0,
            recipes: vec![RecipeDef {
                id: None,
                when: None,
                shown: None,
                hint: None,
                max: None,
                aspects: Vec::new(),
                out: vec!["DRAW RUMOURS".to_string()],
            }],
        };
        let (dict, content) = rig(vec![("DRAWV", draw_verb)], |raw| {
            raw.decks = Some(vec![crate::content::DeckRaw {
                id: "RUMOURS".to_string(),
                spec: vec!["HEARSAY".to_string(), "SCANDAL".to_string()],
                defaultcard: None,
                reset: false,
            }]);
            raw.prototypes = Some(
                [("HEARSAY", "idea"), ("SCANDAL", "idea")]
                    .into_iter()
                    .map(|(n, k)| {
                        (
                            n.to_string(),
                            PrototypeDef { kind: k.to_string(), ..PrototypeDef::default() },
                        )
                    })
                    .collect(),
            );
        });
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA DRAWV", 1);
        assert_eq!(r.rejected, None, "{:?}", r.rejected);
        let adv = advance(&content, &dict, r.state, "0:0", 2);
        assert_eq!(adv.rejected, None, "{:?}", adv.rejected);
        let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
        let ops: Vec<&str> = complete.outcomes.iter().map(|o| o.op.as_str()).collect();
        assert_eq!(ops, ["DRAWN", "SPAWN"], "DRAW concretizes to DRAWN+SPAWN");
        assert_eq!(adv.state.decks.get("RUMOURS"), Some(&1));
        // fold: the DRAWN row advances the cursor; the fold state matches play
        let mut folded = new_state(0);
        for rec in &adv.records {
            apply_record(rec, &mut folded, &dict, &content).unwrap();
        }
        assert_eq!(folded.decks, adv.state.decks);
        // drift turns loud: a DRAWN row naming a card the deck never serves
        let mut drifted = adv.records.clone();
        for o in &mut drifted[0].outcomes {
            if o.op == "DRAWN" {
                if let Arg::Ref { r#ref: Ref::Name { name, .. } } = &mut o.args[1] {
                    *name = "NOSUCH".to_string();
                }
            }
        }
        let mut bad = new_state(0);
        let err = apply_record(&drifted[0], &mut bad, &dict, &content).unwrap_err();
        assert!(err.msg.contains("content drift between play and fold"), "{err}");
    }

    #[test]
    fn if_flattens_the_taken_branch_and_defers_to_completion() {
        let late = VerbContent {
        slots: vec![SlotDef {
            role: "actors".into(),
            min: 1,
            max: Some(1),
            query: Some("kind:character".into()),
            shown: None,
        }],
        duration: 30,
        recipes: vec![RecipeDef {
            id: None,
            when: None,
            shown: None,
            hint: None,
            max: None,
            aspects: Vec::new(),
            out: vec![
                "IF exists(HOUSE) -> (SPAWN PARANOIA, REFUSE(\"too late.\")) ELSE -> (NARRATE \"calm.\")".to_string(),
            ],
        }],
    };
        let (dict, content) = rig(vec![("LATE", late)], |raw| {
            raw.prototypes = Some(
                [(
                    "PARANOIA".to_string(),
                    PrototypeDef { kind: "state".to_string(), ..PrototypeDef::default() },
                )]
                .into_iter()
                .collect(),
            );
        });
        // the IF is opaque to the gate: the window OPENS
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA LATE", 1);
        assert_eq!(r.rejected, None);
        assert_eq!(r.state.windows.len(), 1);
        assert!(!r.state.by_name.contains_key("PARANOIA"));
        // the completion evaluates the IF against the completion-time world:
        // HOUSE exists → SPAWN then REFUSE; the spawn dies with the scratch…
        let begin_state = clone_state(&r.state);
        let adv = advance(&content, &dict, r.state, "0:31", 2);
        assert_eq!(adv.rejected, None);
        let complete = adv.records.iter().find(|x| x.kind == "complete").unwrap();
        let ops: Vec<&str> = complete.outcomes.iter().map(|o| o.op.as_str()).collect();
        assert_eq!(ops, ["SPAWN", "REFUSE"], "only the TAKEN branch is recorded");
        assert!(!adv.state.by_name.contains_key("PARANOIA"), "the scratch died with the refusal");
        assert_eq!(adv.state.windows.len(), 0, "…and the window still closed");
        // the fold replays the SPAWN (the refused COMPLETION is a no-op, but
        // the SPAWN is ON the refused row) — so the fold must NOT spawn either
        let mut folded = begin_state;
        for rec in &adv.records {
            apply_record(rec, &mut folded, &dict, &content).unwrap();
        }
        assert!(!folded.by_name.contains_key("PARANOIA"));
        assert_eq!(folded.windows.len(), 0);
    }

    #[test]
    fn register_windows_roundtrips_the_sentence_and_errors_on_degenerate_rows() {
        // Q6 (fixed): a BEGIN row whose sentence fails to re-parse is a
        // loud drift error — it used to silently open no window
        let (dict, content) = rig(vec![("PACE", pace(30, &["NARRATE \"d.\""]))], |_| {});
        let rec = ActionRecord {
            id: 7,
            kind: "action".to_string(),
            trigger: None,
            t_game: "0:0".to_string(),
            t_real: None,
            advanced_min: 30,
            start_min: Some(0),
            window: None,
            actors: vec![],
            verb: String::new(),
            objects: vec![],
            instruments: vec![],
            at: None,
            about: None,
            to: None,
            outcomes: vec![],
            recipes: vec!["pace.default".to_string()],
            // a degenerate sentence: quotes never close — unparseable
            sentence: "0:0 @ \"unclosed PACE".to_string(),
        };
        let mut st = seed_state();
        let e = register_windows(&rec, &mut st, &dict, &content).unwrap_err();
        assert_eq!(
            e.msg,
            "begin row 7: the recorded sentence does not re-parse \
         ('0:0 @ \"unclosed PACE': unterminated string literal) — drift"
        );
        assert_eq!(st.windows.len(), 0, "the error left no window behind");
        // the healthy row: the SAME code path re-parses the canonical sentence
        let healthy = ActionRecord { sentence: "0:0 @ ANA PACE".to_string(), ..rec };
        register_windows(&healthy, &mut st, &dict, &content).unwrap();
        assert_eq!(st.windows.len(), 1);
        let w = st.windows.get("7:0").unwrap();
        assert_eq!((w.start, w.end, w.verb.as_str()), (0, 30, "PACE"));
        assert!(w.locked.is_empty(), "actors are never locked (COMMITTED_ROLES exclude actors)");
    }

    #[test]
    fn records_serialize_in_the_jsonl_wire_field_order() {
        // the JSONL wire order is the pinned record field order.
        let (dict, content) = rig(vec![("PACE", pace(30, &["NARRATE \"d.\""]))], |_| {});
        let r = run(&content, &dict, seed_state(), "0:0 @ ANA PACE", 1);
        let line = serde_json::to_string(&r.records[0]).unwrap();
        // hand-written canonical JSON shape for a BEGIN row
        let expected = concat!(
            r#"{"id":1,"kind":"action","trigger":null,"tGame":"0:0","tReal":null,"advancedMin":30,"startMin":0,"#,
            r#""window":null,"actors":[{"form":"name","name":"ANA"}],"verb":"PACE","objects":[],"instruments":[],"#,
            r#""at":null,"about":null,"to":null,"outcomes":[],"recipes":["pace.default"],"sentence":"0:0 @ ANA PACE"}"#
        );
        assert_eq!(line, expected);
        // parse-back is lossless
        let back: ActionRecord = serde_json::from_str(&line).unwrap();
        assert_eq!(back, r.records[0]);
    }

    #[test]
    fn rejected_messages_are_byte_exact() {
        let (dict, content) = rig(vec![("PACE", pace(30, &["NARRATE \"d.\""]))], |_| {});
        // before-now
        let r = execute_step(
            &content,
            &dict,
            {
                let mut s = seed_state();
                s.now = 60;
                s
            },
            Step {
                start_min: None,
                clock: Some(Clock { day: 0, halfhour: 1 }),
                clauses: vec![],
                t_real: None,
            },
            ExecOptions { first_record_id: 1, id_gen: None },
        )
        .unwrap();
        assert_eq!(r.rejected.as_deref(), Some("clock 0:1 is before state.now (0:2)"));
        // nothing to do (the live to_min == now && !has_due branch)
        let r = advance(&content, &dict, seed_state(), "0:0", 1);
        assert!(
            r.rejected
                .as_deref()
                .is_some_and(|m| m.starts_with("nothing to do — the clock already reads 0:0")),
            "{:?}",
            r.rejected
        );
        // story ended
        let mut ended = seed_state();
        ended.ended =
            Some(Ending { id: "E".into(), title: "It is over.".into(), flavour: "calm".into() });
        let r = run(&content, &dict, ended, "0:1 @ ANA PACE", 1);
        assert_eq!(r.rejected.as_deref(), Some("the story has ended — It is over."));
        // wait-partition fold assertion
        let mut st = new_state(5);
        let err = apply_record(
            &ActionRecord {
                id: 1,
                kind: "wait".to_string(),
                trigger: None,
                t_game: "0:0".to_string(),
                t_real: None,
                advanced_min: 30,
                start_min: Some(0),
                window: None,
                actors: vec![],
                verb: String::new(),
                objects: vec![],
                instruments: vec![],
                at: None,
                about: None,
                to: None,
                outcomes: vec![],
                recipes: vec![],
                sentence: String::new(),
            },
            &mut st,
            &dict,
            &content,
        )
        .unwrap_err();
        assert!(err.msg.contains("wait spans must partition time"), "{err}");
    }
}
