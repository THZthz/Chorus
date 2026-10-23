//! A thin dev driver (spec.md §3) and design analyzer front end: `play`
//! loads one root `.sw` file, boots the engine, and follows a dumb terminal
//! loop (mutating the state only through story-engine's API; the undo stack
//! is host-held, L2); `analyze` boots the same content and runs
//! story-analyze's design-time analyses, printing reports to stdout.

mod analyze;
mod profile;

use std::collections::BTreeMap;
use std::sync::Arc;

use story_content::ir::VerbMode;
use story_core::{EntityId, SituationId, SlotName, VerbId};
use story_engine::{CommitAction, EffectRegistry, Engine, Event};

const USAGE: &str = "\
story-cli — the Chorus² dev driver and design analyzer

USAGE:
    story-cli play <file.sw> [--seed N]
    story-cli analyze <file.sw> [options]
    story-cli <file.sw> [--seed N]            alias for play

PLAY — the interactive terminal driver; commands at the `>` prompt:
    verbs | commit <verb> [slot=card ...] | bind <sit> <slot> <card> |
    give <card> [n] | advance [n] | state | undo | quit
    --seed N    host seed when the content declares `seed host` (default 1)

ANALYZE — boot the content and run design analyses; reports go to stdout.
    story-cli analyze <file.sw> [options]
    --reach <goal> | --classify | --mdp [<goal>] | --audit | --pareto <objs>
    run `story-cli analyze --help` for the full option reference.
Notes:
    Analyses exit 0 even when the answer is \"unreachable\"; exit 1 on compile
    or boot failure; exit 2 on usage errors.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => usage_exit(2),
        Some("play") => play(&args[1..]),
        Some("analyze") => std::process::exit(analyze::run(&args[1..])),
        Some("help" | "--help" | "-h") => println!("{USAGE}"),
        Some(other) => {
            // The bare form is an alias for play: a first argument that
            // names an existing file drives it, anything else is an
            // unknown subcommand.
            if std::path::Path::new(other).is_file() {
                play(&args);
            } else {
                eprintln!("story-cli: unknown subcommand or missing file `{other}`");
                usage_exit(2);
            }
        }
    }
}

fn usage_exit(code: i32) -> ! {
    eprintln!("{USAGE}");
    std::process::exit(code);
}

fn play(args: &[String]) {
    let mut path: Option<String> = None;
    let mut seed: u64 = 1;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" => {
                i += 1;
                seed = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1);
            }
            other => path = Some(other.to_string()),
        }
        i += 1;
    }
    let Some(path) = path else {
        eprintln!("usage: story-cli play <file.sw> [--seed N]");
        std::process::exit(2);
    };
    let out = story_lang::compile_file(std::path::Path::new(&path));
    let mut errors = 0;
    for d in &out.diagnostics {
        println!("{d}");
        if d.is_error() {
            errors += 1;
        }
    }
    let Some(content) = out.content else {
        println!("compilation failed with {errors} error(s)");
        std::process::exit(1);
    };
    if errors > 0 {
        println!("compilation reported {errors} error(s)");
        std::process::exit(1);
    }
    // A content-fixed seed overrides the CLI default; `seed host` content
    // takes the argument.
    let seed = match content.game.as_ref().map(|g| g.seed.clone()) {
        Some(story_content::ir::SeedSpec::Fixed(n)) => n,
        _ => seed,
    };
    let (mut engine, diags) = Engine::boot(Arc::new(content), EffectRegistry::new(), seed, None);
    for d in &diags {
        println!("boot: {d}");
    }
    // An error-severity boot diagnostic means the host must not proceed
    // (kernel-api.md §1); warnings are traffic.
    if diags.iter().any(|d| d.is_error()) {
        std::process::exit(1);
    }
    if let Some(game) = engine.content().game.as_ref() {
        println!("{} — seed {}, turn {}m", game.title, seed, game.turn);
    }

    let mut undo: Vec<story_engine::Snapshot> = Vec::new();
    let stdin = std::io::stdin();
    loop {
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            [] => continue,
            ["quit" | "q"] => break,
            ["help" | "h"] => print_help(),
            ["verbs"] => print_verbs(&engine),
            ["state"] => print_state(&engine),
            ["advance"] => do_advance(&mut engine, &mut undo, 1),
            ["advance", n] => match n.parse::<i64>() {
                Ok(n) if n > 0 => do_advance(&mut engine, &mut undo, n),
                _ => println!("  advance takes a positive turn count"),
            },
            ["commit", verb, rest @ ..] => {
                do_commit(&mut engine, &mut undo, verb, rest);
            }
            ["bind", sit, slot, card] => {
                do_bind(&mut engine, &mut undo, sit, slot, card);
            }
            ["give", card] => do_give(&mut engine, &mut undo, card, 1),
            ["give", card, n] => match n.parse::<i64>() {
                Ok(n) if n > 0 => do_give(&mut engine, &mut undo, card, n),
                _ => println!("  give takes a positive count"),
            },
            ["undo"] => match undo.pop() {
                Some(snap) => match engine.restore(&snap) {
                    Ok(()) => println!("  undone"),
                    Err(d) => println!("  refused: {d}"),
                },
                None => println!("  nothing to undo"),
            },
            _ => {
                print_help();
            }
        }
    }
}

fn print_help() {
    println!(
        "  verbs | commit <verb> [slot=card ...] | bind <sit> <slot> <card> | \
         give <card> [n] | advance [n] | state | undo | quit"
    );
}

/// `give <card> [n]`: mint n copies of a declared card onto the home zone.
/// The host route is snapshot surgery (L2: the snapshot is the save) — the
/// engine's mint pipeline runs through authored effects only, and the probe
/// driver owns the undo snapshot either way.
fn do_give(engine: &mut Engine, undo: &mut Vec<story_engine::Snapshot>, card: &str, n: i64) {
    let Some(decl) = engine
        .content()
        .cards
        .get(&story_core::CardId::from(card))
        .cloned()
    else {
        println!("  no card declared named `{card}`");
        return;
    };
    let snap = engine.snapshot();
    let Ok(mut state) = serde_json::from_slice::<story_core::State>(&snap.state) else {
        println!("  state deserialization failed");
        return;
    };
    let Some(home) = state
        .zones
        .values()
        .find(|z| matches!(z.kind, story_core::ZoneKind::Table))
        .map(|z| z.id)
    else {
        println!("  no table zone");
        return;
    };
    let body = decl
        .mint_alias
        .as_ref()
        .and_then(|a| engine.content().cards.get(a))
        .unwrap_or(&decl);
    let mut minted = 0;
    for _ in 0..n {
        let id = story_core::EntityId(state.next_entity);
        state.next_entity += 1;
        state.entities.insert(
            id,
            story_core::Entity {
                id,
                name: decl.id.to_string(),
                prototype: decl.id.clone(),
                kind: body.kind.clone(),
                aspects: body.aspects.clone(),
                attrs: Default::default(),
                zone: home,
                mint: body.mint.clone(),
                expires_at: None,
            },
        );
        minted += 1;
    }
    let prepared = story_engine::Snapshot {
        format: snap.format,
        content_fingerprint: snap.content_fingerprint,
        state: serde_json::to_vec(&state).expect("serialize"),
    };
    match engine.restore(&prepared) {
        Ok(()) => {
            undo.push(snap);
            println!("  gave {minted} x {card}");
        }
        Err(d) => println!("  refused: {d}"),
    }
}

fn find_entity(engine: &Engine, name: &str) -> Option<EntityId> {
    engine
        .state()
        .entities
        .values()
        .find(|e| e.name == name)
        .map(|e| e.id)
}

fn do_commit(
    engine: &mut Engine,
    undo: &mut Vec<story_engine::Snapshot>,
    verb: &str,
    rest: &[&str],
) {
    let mut bindings: Vec<(SlotName, EntityId)> = Vec::new();
    for pair in rest {
        let Some((slot, card)) = pair.split_once('=') else {
            println!("  bindings are slot=card, e.g. fuel=coal");
            return;
        };
        // `slot=#<id>` addresses an entity directly (the probe driver's
        // escape hatch when several copies share a name).
        let e = card
            .strip_prefix('#')
            .and_then(|n| n.parse::<u64>().ok())
            .map(story_core::EntityId)
            .filter(|id| engine.state().entities.contains_key(id))
            .or_else(|| find_entity(engine, card));
        let Some(e) = e else {
            println!("  no live card named `{card}`");
            return;
        };
        bindings.push((SlotName::from(slot), e));
    }
    let snap = engine.snapshot();
    let action = CommitAction {
        verb: VerbId::from(verb),
        bindings,
    };
    match engine.commit(action) {
        // The undo snapshot is host-held and pushed only for an applied
        // action; a refusal leaves nothing to undo.
        Ok(events) => {
            undo.push(snap);
            print_events(&events);
        }
        Err(d) => println!("  refused: {d}"),
    }
}

fn do_bind(
    engine: &mut Engine,
    undo: &mut Vec<story_engine::Snapshot>,
    sit: &str,
    slot: &str,
    card: &str,
) {
    let Ok(sit) = sit.parse::<u64>() else {
        println!("  bind takes a situation number (see `state`)");
        return;
    };
    let Some(e) = find_entity(engine, card) else {
        println!("  no live card named `{card}`");
        return;
    };
    let snap = engine.snapshot();
    match engine.bind(SituationId(sit), SlotName::from(slot), e) {
        Ok(events) => {
            undo.push(snap);
            print_events(&events);
        }
        Err(d) => println!("  refused: {d}"),
    }
}

fn do_advance(engine: &mut Engine, undo: &mut Vec<story_engine::Snapshot>, turns: i64) {
    let Some(dt) = engine.turn().checked_mul(turns) else {
        println!(
            "  refused: error BadDuration [effects]: advance {turns} turns overflows the clock"
        );
        return;
    };
    let snap = engine.snapshot();
    match engine.advance(dt) {
        Ok(events) => {
            undo.push(snap);
            print_events(&events);
        }
        Err(d) => println!("  refused: {d}"),
    }
    print_ending(engine);
}

fn print_events(events: &[Event]) {
    for e in events {
        match e {
            Event::Narration { text } => println!("  {text}"),
            Event::Bound { sit, slot, entity } => {
                println!("  [sit {sit}] {slot} <- entity {entity}");
            }
            Event::SituationCompleted { sit, recipe } => {
                println!("  [sit {sit}] {recipe} completed");
            }
            Event::Continued { situation, recipe } => {
                println!("  [sit {situation}] continues via {recipe}");
            }
            Event::ChainEnded { verb } => println!("  [chain {verb} ended]"),
            Event::StartSkipped { verb, reason } => {
                println!("  [start {verb} skipped: {reason:?}]");
            }
            Event::SpawnSkipped { name } => println!("  [spawn {name} skipped: not unique]"),
            Event::ResolutionFailed { sit, reason } => {
                println!("  [sit {sit} FAILED: {reason}]");
            }
            Event::Related { from, kind, to } => {
                println!("  [relate] entity {from} -{kind}-> entity {to}");
            }
            Event::Unrelated { from, kind, to } => {
                println!("  [unrelate] entity {from} -{kind}-/-> entity {to}");
            }
        }
    }
}

fn print_ending(engine: &Engine) {
    if let Some(ended) = &engine.state().ended {
        let title = engine
            .content()
            .endings
            .get(ended.as_str())
            .map(|e| format!("\"{}\" ({})", e.title, e.tier))
            .unwrap_or_default();
        println!("  === THE STORY ENDS: {ended} {title} ===");
    }
}

fn print_verbs(engine: &Engine) {
    let mut verbs: Vec<_> = engine.content().verbs.values().collect();
    verbs.sort_by_key(|v| v.decl_index);
    for v in verbs {
        let mode = match v.mode {
            VerbMode::Committable => "committable",
            VerbMode::Autonomous => "autonomous",
            VerbMode::Uncommittable => "uncommittable",
        };
        let instances = match v.instances {
            story_content::ir::InstanceCap::Any => "any".to_string(),
            story_content::ir::InstanceCap::AtMost(n) => n.to_string(),
        };
        let mut recipes: Vec<&story_content::ir::RecipeDecl> = v.recipes.values().collect();
        recipes.sort_by_key(|r| (r.priority, r.decl_index));
        let list: Vec<String> = recipes
            .iter()
            .map(|r| {
                format!(
                    "{} (priority {}, {}m{})",
                    r.name,
                    r.priority,
                    r.duration,
                    if r.craftable {
                        ""
                    } else if r.chain_only {
                        ", chain-only"
                    } else {
                        ", auto"
                    }
                )
            })
            .collect();
        println!(
            "  {} {mode} instances {instances}: {}",
            v.id,
            list.join(", ")
        );
    }
}

fn print_state(engine: &Engine) {
    let st = engine.state();
    println!("  clock: {}m", st.now);
    match &st.ended {
        Some(e) => println!("  ended: {e}"),
        None => println!("  ended: -"),
    }
    let legacy: Vec<String> = st.legacy.iter().map(|(k, v)| format!("{k}={v}")).collect();
    println!("  legacy: {}", legacy.join(", "));
    let mut by_zone: BTreeMap<u64, Vec<String>> = BTreeMap::new();
    for e in st.entities.values() {
        let aspects: Vec<String> = e.aspects.iter().map(|(k, v)| format!("{k}:{v}")).collect();
        by_zone.entry(e.zone.0).or_default().push(format!(
            "{}#{}{}{}",
            e.name,
            e.id,
            e.expires_at
                .map(|t| format!(" (expires {t}m)"))
                .unwrap_or_default(),
            if aspects.is_empty() {
                String::new()
            } else {
                format!(" {{{}}}", aspects.join(", "))
            }
        ));
    }
    for (z, cards) in by_zone {
        let kind = st
            .zones
            .get(&story_core::ZoneId(z))
            .map(|zone| match &zone.kind {
                story_core::ZoneKind::Table => "table",
                story_core::ZoneKind::Hidden => "hidden",
                story_core::ZoneKind::SituationInterior(_) => "interior",
                story_core::ZoneKind::Of(_) => "carrier",
            })
            .unwrap_or("?");
        println!("  zone {z} [{kind}]: {}", cards.join(" | "));
    }
    for s in st.situations.values() {
        let slots: Vec<String> = s
            .bindings
            .iter()
            .map(|(name, binds)| {
                let cards: Vec<String> = binds
                    .iter()
                    .map(|b| format!("{}#{}", entity_name(engine, b.entity), b.entity))
                    .collect();
                format!("{name}: {}", cards.join(", "))
            })
            .collect();
        println!(
            "  sit {} {} [{}] ends {}m",
            s.id,
            s.recipe,
            slots.join("; "),
            s.ends_at
        );
    }
}

fn entity_name(engine: &Engine, id: EntityId) -> &str {
    engine
        .state()
        .entities
        .get(&id)
        .map(|e| e.name.as_str())
        .unwrap_or("?")
}
