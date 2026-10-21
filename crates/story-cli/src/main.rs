//! A thin dev driver (spec.md §3): load one root `.sw` file, boot the engine,
//! and follow a dumb terminal loop. Mutates the state only through
//! story-engine's API; the undo stack is host-held (L2).

use std::collections::BTreeMap;
use std::sync::Arc;

use story_content::ir::VerbMode;
use story_core::{EntityId, SituationId, SlotName, VerbId};
use story_engine::{CommitAction, EffectRegistry, Engine, Event};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut path: Option<String> = None;
    let mut seed: u64 = 1;
    let mut i = 1;
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
        eprintln!("usage: story-cli <file.sw> [--seed N]");
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
         advance [n] | state | undo | quit"
    );
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
        let Some(e) = find_entity(engine, card) else {
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
            Event::ChainEnded { verb } => println!("  [chain {verb} ended]"),
            Event::StartSkipped { verb, reason } => {
                println!("  [start {verb} skipped: {reason:?}]");
            }
            Event::SpawnSkipped { name } => println!("  [spawn {name} skipped: not unique]"),
            Event::ResolutionFailed { sit, reason } => {
                println!("  [sit {sit} FAILED: {reason}]");
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
