//! A thin dev driver (spec.md §3) and design analyzer front end: `play`
//! loads one root `.sw` file, boots the engine, and follows a dumb terminal
//! loop (mutating the state only through story-engine's API; the undo stack
//! is host-held, L2); `analyze` boots the same content and runs
//! story-analyze's design-time analyses, printing reports to stdout.

mod analyze;
mod profile;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use story_content::ir::{CmpOp, QueryTerm, RecipeDecl, SlotDecl};
use story_core::{EntityId, SituationId, SlotName, Snapshot, VerbId};
use story_engine::{CommitAction, EffectRegistry, Engine, Event};
use story_present::{BeatKind, PresentationCatalog, Presenter};

const USAGE: &str = "\
story-cli — the Chorus² dev driver and design analyzer

USAGE:
    story-cli play <file.sw> [options]
    story-cli analyze <file.sw> [options]
    story-cli <file.sw> [options]             alias for play

PLAY — the interactive terminal driver; commands at the `>` prompt:
    verbs [available|all] | inspect <card> | commit <verb> [slot=card ...] |
    bind <sit> <slot> <card> | give <card> [n] | advance [n] | trace <file> |
    state | undo | mode | filter | quit
    --verbose, -v   print detailed state, slot criteria, and action feedback
    --available, -a filter out recipes that cannot currently be run
    --seed N        host seed when the content declares `seed host` (default 1)
    --record <f>    record forward play trace to JSON file on exit
    run `story-cli play --help` for the interactive command reference.

ANALYZE — boot the content and run design analyses; reports go to stdout.
    story-cli analyze <file.sw> [options]
    --reach <goal> | --classify | --mdp [<goal>] | --audit | --pareto <objs>
    run `story-cli analyze --help` for the full option reference.
Notes:
    Analyses exit 0 even when the answer is \"unreachable\"; exit 1 on compile
    or boot failure; exit 2 on usage errors.";

const PLAY_USAGE: &str = "\
story-cli play — interactive story driver

USAGE:
    story-cli play <file.sw> [options]
    story-cli <file.sw> [options]

OPTIONS:
    --verbose, -v   print detailed board state, recipe slot requirements, and action progress
    --available, -a filter out recipes that cannot currently be run
    --seed N        host seed when the content declares `seed host` (default 1)
    --record <f>    record forward play trace to JSON file on exit
    -h, --help      print this help message

INTERACTIVE COMMANDS:
    state                     show table cards, active situations, and clock
    verbs [available|all]     list actions (optionally filter to runnable only)
    inspect <card>            inspect card details, lore, aspects, and relations
    commit <verb> [slot=card] start an action (e.g. commit sup morsel=bread#10)
    bind <sit> <slot> <card>  bind a card to an open situation slot
    advance [n]               pass n turns (default 1) to progress situations
    undo                      revert the last action or advance
    mode [verbose|normal]     toggle detailed output mode
    filter [on|off]           toggle filtering of unrunnable recipes
    give <card> [n]           mint copies of a card onto the table
    trace <file>              export recorded play trace to JSON
    quit, q                   exit the session

GAME LOOP:
    1. Inspect table cards and running situations with `state`.
    2. Check available actions and slot criteria with `verbs`.
    3. Start actions with `commit <verb> [slot=card ...]`.
    4. Pass time with `advance` to complete recipes and trigger effects.";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => usage_exit(2),
        Some("play") => play(&args[1..]),
        Some("analyze") => std::process::exit(analyze::run(&args[1..])),
        Some("help" | "--help" | "-h") => println!("{USAGE}"),
        Some(other) => {
            if other == "-v" || other == "--verbose" || other == "-a" || other == "--available" {
                if args
                    .iter()
                    .skip(1)
                    .any(|a| std::path::Path::new(a).is_file())
                {
                    play(&args);
                } else {
                    eprintln!("story-cli: missing file for play");
                    usage_exit(2);
                }
            } else if std::path::Path::new(other).is_file() {
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
    let mut record_path: Option<String> = None;
    let mut verbose = false;
    let mut filter_runnable = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("{PLAY_USAGE}");
                std::process::exit(0);
            }
            "--verbose" | "-v" => {
                verbose = true;
            }
            "--available" | "-a" => {
                filter_runnable = true;
            }
            "--seed" => {
                i += 1;
                seed = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1);
            }
            "--record" => {
                i += 1;
                record_path = args.get(i).cloned();
            }
            arg if arg.starts_with('-') => {
                eprintln!("story-cli play: unknown option `{arg}`\n\n{PLAY_USAGE}");
                std::process::exit(2);
            }
            other => {
                if path.is_none() {
                    path = Some(other.to_string());
                } else {
                    eprintln!("story-cli play: unexpected argument `{other}`\n\n{PLAY_USAGE}");
                    std::process::exit(2);
                }
            }
        }
        i += 1;
    }
    let Some(path) = path else {
        eprintln!("usage: story-cli play <file.sw> [options]\n\n{PLAY_USAGE}");
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
    let (engine, diags) = Engine::boot(Arc::new(content), EffectRegistry::new(), seed, None);
    for d in &diags {
        println!("boot: {d}");
    }
    // An error-severity boot diagnostic means the host must not proceed
    // (kernel-api.md §1); warnings are traffic.
    if diags.iter().any(|d| d.is_error()) {
        std::process::exit(1);
    }

    let catalog = load_companion_catalog(std::path::Path::new(&path));
    let mut presenter = Presenter::new(engine, catalog);

    if let Some(game) = presenter.content().game.as_ref() {
        if verbose {
            println!(
                "{} — seed {}, turn {}m [verbose]",
                game.title, seed, game.turn
            );
            println!("Type 'help' for commands, 'mode normal' to reduce verbosity.\n");
            println!("Initial Board State:");
            print_state_verbose(&presenter);
            println!("\nAvailable Actions:");
            print_verbs_verbose(&presenter, filter_runnable);
        } else {
            println!("{} — seed {}, turn {}m", game.title, seed, game.turn);
            println!("Type 'help' for commands, 'verbs' for actions, 'state' for the board.");
        }
    } else if verbose {
        println!("Type 'help' for commands, 'mode normal' to reduce verbosity.\n");
        println!("Initial Board State:");
        print_state_verbose(&presenter);
        println!("\nAvailable Actions:");
        print_verbs_verbose(&presenter, filter_runnable);
    } else {
        println!("Type 'help' for commands, 'verbs' for actions, 'state' for the board.");
    }

    let mut undo: Vec<(Snapshot, bool)> = Vec::new();
    let mut trace_steps: Vec<story_analyze::TraceStep> = Vec::new();
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
            ["help"] => print_help(),
            ["help", cmd] => print_help_cmd(cmd),
            ["mode"] => {
                verbose = !verbose;
                println!(
                    "  mode set to {}",
                    if verbose { "verbose" } else { "normal" }
                );
            }
            ["mode", "verbose"] => {
                verbose = true;
                println!("  mode set to verbose");
            }
            ["mode", "normal"] => {
                verbose = false;
                println!("  mode set to normal");
            }
            ["filter"] => {
                filter_runnable = !filter_runnable;
                println!(
                    "  recipe filtering is {}",
                    if filter_runnable {
                        "on (only runnable recipes shown)"
                    } else {
                        "off (all recipes shown)"
                    }
                );
            }
            ["filter", "on"] => {
                filter_runnable = true;
                println!("  recipe filtering is on (only runnable recipes shown)");
            }
            ["filter", "off"] => {
                filter_runnable = false;
                println!("  recipe filtering is off (all recipes shown)");
            }
            ["verbs"] => {
                if verbose {
                    print_verbs_verbose(&presenter, filter_runnable);
                } else {
                    print_verbs(&presenter, filter_runnable);
                }
            }
            ["verbs", "available"] => {
                if verbose {
                    print_verbs_verbose(&presenter, true);
                } else {
                    print_verbs(&presenter, true);
                }
            }
            ["verbs", "all"] => {
                if verbose {
                    print_verbs_verbose(&presenter, false);
                } else {
                    print_verbs(&presenter, false);
                }
            }
            ["state"] => {
                if verbose {
                    print_state_verbose(&presenter);
                } else {
                    print_state(&presenter);
                }
            }
            ["inspect"] => println!("  usage: inspect <card>"),
            ["inspect", card] => match presenter.inspect_card(card) {
                Some(lore) => print_inspect(&lore),
                None => println!("  card not found: `{card}`"),
            },
            ["advance"] => {
                if let Some(step) = do_advance(&mut presenter, &mut undo, 1, verbose) {
                    trace_steps.push(step);
                }
            }
            ["advance", n] => match n.parse::<i64>() {
                Ok(n) if n > 0 => {
                    if let Some(step) = do_advance(&mut presenter, &mut undo, n, verbose) {
                        trace_steps.push(step);
                    }
                }
                _ => println!("  advance takes a positive turn count"),
            },
            ["commit", verb, rest @ ..] => {
                if let Some(step) = do_commit(&mut presenter, &mut undo, verb, rest, verbose) {
                    trace_steps.push(step);
                }
            }
            ["bind", sit, slot, card] => {
                if let Some(step) = do_bind(&mut presenter, &mut undo, sit, slot, card, verbose) {
                    trace_steps.push(step);
                }
            }
            ["give", card] => do_give(&mut presenter, &mut undo, card, 1),
            ["give", card, n] => match n.parse::<i64>() {
                Ok(n) if n > 0 => do_give(&mut presenter, &mut undo, card, n),
                _ => println!("  give takes a positive count"),
            },
            ["undo"] => match undo.pop() {
                Some((snap, was_trace_step)) => match presenter.restore(&snap) {
                    Ok(()) => {
                        if was_trace_step {
                            trace_steps.pop();
                        }
                        println!("  undone");
                        if verbose {
                            println!(
                                "  restored previous state (clock is now {}m)",
                                presenter.state().now
                            );
                        }
                    }
                    Err(d) => println!("  refused: {d}"),
                },
                None => println!("  nothing to undo"),
            },
            ["trace", out_path] => {
                let mut trace = story_analyze::PlayTrace::new(seed, None);
                trace.game = presenter.content().game.as_ref().map(|g| g.title.clone());
                trace.steps = trace_steps.clone();
                match trace.to_json() {
                    Ok(json) => match std::fs::write(out_path, json) {
                        Ok(()) => println!(
                            "  exported trace ({} steps) to {out_path}",
                            trace.steps.len()
                        ),
                        Err(e) => println!("  failed to write trace file: {e}"),
                    },
                    Err(e) => println!("  failed to serialize trace: {e}"),
                }
            }
            _ => {
                print_help();
            }
        }
    }
    if let Some(path) = record_path {
        let mut trace = story_analyze::PlayTrace::new(seed, None);
        trace.game = presenter.content().game.as_ref().map(|g| g.title.clone());
        trace.steps = trace_steps;
        if let Ok(json) = trace.to_json() {
            let _ = std::fs::write(&path, json);
        }
    }
}

fn load_companion_catalog(sw_path: &std::path::Path) -> PresentationCatalog {
    let candidate1 = sw_path.with_extension("narrative.json");
    let candidate2 = {
        let mut s = sw_path.as_os_str().to_os_string();
        s.push(".narrative.json");
        std::path::PathBuf::from(s)
    };
    let target = if candidate1.is_file() {
        Some(candidate1)
    } else if candidate2.is_file() {
        Some(candidate2)
    } else {
        None
    };

    if let Some(cat_path) = target {
        match PresentationCatalog::from_file(&cat_path) {
            Ok(catalog) => {
                println!("loaded companion narrative catalog: {}", cat_path.display());
                catalog
            }
            Err(e) => {
                eprintln!("failed to load companion narrative catalog: {e}");
                PresentationCatalog::default()
            }
        }
    } else {
        PresentationCatalog::default()
    }
}

fn print_help() {
    println!(
        "Commands:\n  \
         state                     show table cards, active situations, and clock\n  \
         verbs [available|all]     list actions (optionally filter to runnable only)\n  \
         inspect <card>            inspect card details, lore, aspects, and relations\n  \
         commit <verb> [slot=card] start an action (e.g. commit sup morsel=bread#10)\n  \
         bind <sit> <slot> <card>  bind a card to an open situation slot\n  \
         advance [n]               pass n turns (default 1) to progress situations\n  \
         undo                      revert the last action or advance\n  \
         mode [verbose|normal]     toggle detailed output mode\n  \
         filter [on|off]           toggle filtering of unrunnable recipes\n  \
         give <card> [n]           mint copies of a card onto the table\n  \
         trace <file>              export recorded play trace to JSON\n  \
         quit, q                   exit the session\n\n\
         Game Loop:\n  \
         1. Inspect table cards and running situations with `state`.\n  \
         2. Check available actions and slot criteria with `verbs`.\n  \
         3. Start actions with `commit <verb> [slot=card ...] `.\n  \
         4. Pass time with `advance` to complete recipes and trigger effects.\n\
         Type `help <command>` for detailed syntax on any command."
    );
}

fn print_help_cmd(cmd: &str) {
    match cmd {
        "state" => println!(
            "state\n  Display the current game clock, legacy variables, cards on the table, and active situations.\n  In verbose mode, also shows remaining situation countdowns and open slots."
        ),
        "verbs" => println!(
            "verbs [available|all]\n  List declared verbs and recipes.\n  Use `verbs available` to filter out recipes that cannot currently be run.\n  Use `verbs all` to show all declared recipes.\n  In verbose mode, also shows slot criteria and guards."
        ),
        "inspect" => println!(
            "inspect <card>\n  Display card details including presentation title, lore description, aspects, expiry, and relation edges.\n  Supports #<id>, name#<id>, or name.\n  Example: inspect bread#10"
        ),
        "filter" => println!(
            "filter [on|off]\n  Toggle filtering of unrunnable recipes for the `verbs` command.\n  When enabled, recipes whose when/guard/slot requirements cannot be met do not appear."
        ),
        "commit" => println!(
            "commit <verb> [slot=card ...]\n  Start an action with optional slot bindings.\n  Cards can be bound by name (slot=coal) or by entity ID (slot=#4).\n  Example: commit tend fuel=coal#4"
        ),
        "bind" => println!(
            "bind <sit> <slot> <card>\n  Bind a card from the table into an open slot of an active situation.\n  Example: bind 1 fuel coal#5"
        ),
        "advance" => println!(
            "advance [n]\n  Advance the clock by n turns (default: 1 turn).\n  Progresses situation timers, triggers recipe completions, and handles expiry."
        ),
        "undo" => println!(
            "undo\n  Revert the most recent state change (commit, bind, advance, or give)."
        ),
        "mode" => println!(
            "mode [verbose|normal]\n  Toggle or set output detail level.\n  Verbose mode displays slot criteria, situation countdowns, and detailed action hints."
        ),
        "give" => println!(
            "give <card> [n]\n  Mint n copies of a declared card onto the table (development utility).\n  Example: give coal 2"
        ),
        "trace" => println!(
            "trace <file.json>\n  Export the sequence of player actions taken during this session to a JSON file."
        ),
        "quit" | "q" => println!(
            "quit\n  Exit the play session."
        ),
        other => println!("Unknown command `{other}`. Type `help` for command list."),
    }
}

fn format_cmp(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Ge => ">=",
        CmpOp::Le => "<=",
        CmpOp::Gt => ">",
        CmpOp::Lt => "<",
        CmpOp::Eq => "==",
        CmpOp::Ne => "!=",
    }
}

fn format_query_term(term: &QueryTerm) -> String {
    match term {
        QueryTerm::Aspect(a) => match a.op {
            Some(op) => format!("aspect:{}{}{}", a.key, format_cmp(op), a.value),
            None => format!("aspect:{}", a.key),
        },
        QueryTerm::Name(id) => format!("name:{id}"),
        QueryTerm::Kind(k) => format!("kind:{k}"),
        QueryTerm::Not(inner) => format!("not({})", format_query_term(inner)),
        QueryTerm::Pred(_) => "predicate".to_string(),
    }
}

fn format_slot(slot: &SlotDecl) -> String {
    let bounds = match slot.max {
        Some(max) => format!("{}..{}", slot.min, max),
        None => format!("{}..", slot.min),
    };
    let mut flags = Vec::new();
    if slot.consuming {
        flags.push("consuming");
    }
    if slot.open {
        flags.push("open");
    }
    if slot.greedy {
        flags.push("greedy");
    }
    let flags_str = if flags.is_empty() {
        String::new()
    } else {
        format!(" {}", flags.join(" "))
    };
    let query_str = if slot.query.is_empty() {
        "any".to_string()
    } else {
        slot.query
            .iter()
            .map(format_query_term)
            .collect::<Vec<_>>()
            .join(" ")
    };
    format!("{}: {bounds}{flags_str} ({query_str})", slot.name)
}

fn find_recipe_decl<'a>(
    engine: &'a Engine,
    verb: &VerbId,
    recipe: &story_core::RecipeId,
) -> Option<&'a RecipeDecl> {
    let v = engine.content().verbs.get(verb.as_str())?;
    v.recipes.get(recipe.as_str()).or_else(|| {
        v.recipes
            .values()
            .find(|r| r.id == *recipe || r.name == recipe.as_str())
    })
}

fn print_commit_hint(engine: &Engine, verb: &str) {
    if let Some(v) = engine.content().verbs.get(verb) {
        println!("  available craftable recipes for `{verb}`:");
        let craftable: Vec<_> = v.recipes.values().filter(|r| r.craftable).collect();
        if craftable.is_empty() {
            println!("    (no craftable recipes)");
        } else {
            for r in craftable {
                let dur = format!("{}m", r.duration);
                if r.slots.is_empty() {
                    println!("    {} ({dur}): no slots required", r.name);
                } else {
                    println!("    {} ({dur}):", r.name);
                    for s in &r.slots {
                        println!("      {}", format_slot(s));
                    }
                }
            }
        }
    }
}

/// `give <card> [n]`: mint n copies of a declared card onto the home zone.
/// The host route is snapshot surgery (L2: the snapshot is the save) — the
/// engine's mint pipeline runs through authored effects only, and the probe
/// driver owns the undo snapshot either way.
fn do_give(presenter: &mut Presenter, undo: &mut Vec<(Snapshot, bool)>, card: &str, n: i64) {
    let Some(decl) = presenter
        .content()
        .cards
        .get(&story_core::CardId::from(card))
        .cloned()
    else {
        println!("  no card declared named `{card}`");
        return;
    };
    let snap = presenter.snapshot();
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
        .and_then(|a| presenter.content().cards.get(a))
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
    let prepared = Snapshot {
        format: snap.format,
        content_fingerprint: snap.content_fingerprint,
        state: serde_json::to_vec(&state).expect("serialize"),
    };
    match presenter.restore(&prepared) {
        Ok(()) => {
            undo.push((snap, false));
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
        .find(|e| e.name == name || e.prototype.as_str() == name)
        .map(|e| e.id)
}

fn do_commit(
    presenter: &mut Presenter,
    undo: &mut Vec<(Snapshot, bool)>,
    verb: &str,
    rest: &[&str],
    verbose: bool,
) -> Option<story_analyze::TraceStep> {
    let mut bindings: Vec<(SlotName, EntityId)> = Vec::new();
    let mut trace_bindings: Vec<(SlotName, String)> = Vec::new();
    for pair in rest {
        let Some((slot, card)) = pair.split_once('=') else {
            println!("  bindings are slot=card, e.g. fuel=coal");
            return None;
        };
        // Support `#<id>`, `name#<id>`, or `name`
        let e = card
            .split_once('#')
            .map(|(_, id_str)| id_str)
            .or_else(|| card.strip_prefix('#'))
            .and_then(|n| n.parse::<u64>().ok())
            .map(story_core::EntityId)
            .filter(|id| presenter.state().entities.contains_key(id))
            .or_else(|| find_entity(presenter.engine(), card));
        let Some(e) = e else {
            println!("  no live card named `{card}`");
            return None;
        };
        bindings.push((SlotName::from(slot), e));
        trace_bindings.push((SlotName::from(slot), card.to_string()));
    }
    let snap = presenter.snapshot();
    let action = CommitAction {
        verb: VerbId::from(verb),
        bindings,
    };
    match presenter.commit(action) {
        Ok(output) => {
            let old_st = serde_json::from_slice::<story_core::State>(&snap.state).ok();
            undo.push((snap, true));
            if verbose {
                let new_sit = old_st.as_ref().and_then(|st| {
                    presenter
                        .state()
                        .situations
                        .values()
                        .find(|s| !st.situations.contains_key(&s.id))
                });
                if let Some(s) = new_sit {
                    let dur = s.ends_at.saturating_sub(presenter.state().now);
                    println!(
                        "  [sit {}] {} started ({}m, ends at {}m)",
                        s.id, s.recipe, dur, s.ends_at
                    );
                } else if output.beats.is_empty() && output.raw_events.is_empty() {
                    println!("  committed {verb}");
                }
            } else if output.beats.iter().all(|b| b.kind != BeatKind::ActionStart) {
                let new_sit = old_st.as_ref().and_then(|st| {
                    presenter
                        .state()
                        .situations
                        .values()
                        .find(|s| !st.situations.contains_key(&s.id))
                });
                if let Some(s) = new_sit {
                    let dur = s.ends_at.saturating_sub(presenter.state().now);
                    println!("  [sit {}] {} started ({}m)", s.id, s.recipe, dur);
                }
            }
            print_story_output(&output, verbose);
            Some(story_analyze::TraceStep::Commit {
                verb: VerbId::from(verb),
                bindings: trace_bindings,
            })
        }
        Err(d) => {
            println!("  refused: {d}");
            if verbose {
                print_commit_hint(presenter.engine(), verb);
            }
            None
        }
    }
}

fn do_bind(
    presenter: &mut Presenter,
    undo: &mut Vec<(Snapshot, bool)>,
    sit: &str,
    slot: &str,
    card: &str,
    verbose: bool,
) -> Option<story_analyze::TraceStep> {
    let Ok(sit_id) = sit.parse::<u64>() else {
        println!("  bind takes a situation number (see `state`)");
        return None;
    };
    // Support `#<id>`, `name#<id>`, or `name`
    let e = card
        .split_once('#')
        .map(|(_, id_str)| id_str)
        .or_else(|| card.strip_prefix('#'))
        .and_then(|n| n.parse::<u64>().ok())
        .map(story_core::EntityId)
        .filter(|id| presenter.state().entities.contains_key(id))
        .or_else(|| find_entity(presenter.engine(), card));
    let Some(e) = e else {
        println!("  no live card named `{card}`");
        return None;
    };
    let snap = presenter.snapshot();
    match presenter.bind(SituationId(sit_id), SlotName::from(slot), e) {
        Ok(output) => {
            undo.push((snap, true));
            print_story_output(&output, verbose);
            if verbose {
                if let Some(s) = presenter.state().situations.get(&SituationId(sit_id)) {
                    let cur = s
                        .bindings
                        .get(&SlotName::from(slot))
                        .map(|b| b.len())
                        .unwrap_or(0);
                    println!("  [sit {sit}] slot `{slot}` now has {cur} card(s) bound");
                }
            }
            Some(story_analyze::TraceStep::Bind {
                situation: SituationId(sit_id),
                slot: SlotName::from(slot),
                card: card.to_string(),
            })
        }
        Err(d) => {
            println!("  refused: {d}");
            if verbose {
                if let Some(s) = presenter.state().situations.get(&SituationId(sit_id)) {
                    if let Some(r) = find_recipe_decl(presenter.engine(), &s.verb, &s.recipe) {
                        if let Some(slot_decl) = r.slots.iter().find(|sd| sd.name.as_str() == slot)
                        {
                            println!("  slot `{slot}` criteria: {}", format_slot(slot_decl));
                        }
                    }
                }
            }
            None
        }
    }
}

fn do_advance(
    presenter: &mut Presenter,
    undo: &mut Vec<(Snapshot, bool)>,
    turns: i64,
    verbose: bool,
) -> Option<story_analyze::TraceStep> {
    let Some(dt) = presenter.engine().turn().checked_mul(turns) else {
        println!(
            "  refused: error BadDuration [effects]: advance {turns} turns overflows the clock"
        );
        return None;
    };
    let snap = presenter.snapshot();
    match presenter.advance(dt) {
        Ok(output) => {
            undo.push((snap, true));
            if verbose {
                println!(
                    "  advanced {turns} turn(s) ({dt}m) -> clock is now {}m",
                    presenter.state().now
                );
                if !presenter.state().situations.is_empty() {
                    for s in presenter.state().situations.values() {
                        let rem = s.ends_at.saturating_sub(presenter.state().now);
                        println!(
                            "  [sit {}] {}: {rem}m remaining (ends at {}m)",
                            s.id, s.recipe, s.ends_at
                        );
                    }
                }
            } else if output.beats.is_empty() && output.raw_events.is_empty() {
                println!(
                    "  advanced {turns} turn(s) (clock: {}m)",
                    presenter.state().now
                );
            }
            print_story_output(&output, verbose);
            Some(story_analyze::TraceStep::Advance { turns })
        }
        Err(d) => {
            println!("  refused: {d}");
            None
        }
    }
}

fn print_inspect(lore: &story_present::CardLoreView) {
    let id_str = match lore.entity_id {
        Some(id) => format!(" (#{id})"),
        None => String::new(),
    };
    let kind_str = match &lore.kind {
        Some(k) => format!(" [{k}]"),
        None => String::new(),
    };
    if lore.title != lore.prototype {
        println!(
            "  {} ({}){}{}",
            lore.title, lore.prototype, kind_str, id_str
        );
    } else {
        println!("  {}{}{}", lore.title, kind_str, id_str);
    }
    if let Some(desc) = &lore.description {
        println!("    lore: \"{desc}\"");
    }
    if !lore.aspects.is_empty() {
        let aspects: Vec<String> = lore
            .aspects
            .iter()
            .map(|(k, v)| format!("{k}:{v}"))
            .collect();
        println!("    aspects: {}", aspects.join(", "));
    }
    if let Some(exp) = lore.expires_at {
        println!("    expires at: {exp}m");
    } else if let Some(life) = lore.lifetime {
        println!("    lifetime: {life}m");
    }
    if !lore.relations.is_empty() {
        println!("    relations:");
        for (rel_kind, target_title, target_id) in &lore.relations {
            let tgt_str = match target_id {
                Some(id) => format!("{target_title} (#{id})"),
                None => target_title.clone(),
            };
            println!("      -{rel_kind}-> {tgt_str}");
        }
    }
}

fn print_story_output(output: &story_present::StoryOutput, verbose: bool) {
    let mut completed_printed = BTreeSet::new();

    // 1. In verbose mode, print event diagnostics (such as slot bindings and countdowns)
    if verbose {
        for ev in &output.raw_events {
            match ev {
                Event::Bound {
                    sit,
                    slot,
                    entity,
                    card,
                } => {
                    println!("  [sit {sit}] {slot} <- {card} (#{entity})");
                }
                Event::SituationStarted { sit, verb, recipe } => {
                    println!("  [sit {sit}] {verb} started via {recipe}");
                }
                Event::SituationCompleted { sit, recipe, .. } => {
                    println!("  [sit {sit}] {recipe} completed");
                    completed_printed.insert(*sit);
                }
                Event::Continued { sit, recipe } => {
                    println!("  [sit {sit}] continues via {recipe}");
                }
                Event::ResolutionFailed { sit, reason } => {
                    println!("  [sit {sit} FAILED: {reason}]");
                }
                Event::Related { from, kind, to } => {
                    println!("  [relate] entity {from} -{kind}-> entity {to}");
                }
                Event::Unrelated { from, kind, to } => {
                    println!("  [unrelate] entity {from} -{kind}-/-> entity {to}");
                }
                _ => {}
            }
        }
    }

    // 2. Format StoryOutput beats
    let mut has_action_complete_beat = false;
    for beat in &output.beats {
        match beat.kind {
            BeatKind::ActionStart => {
                if !beat.text.trim().is_empty() {
                    println!("  {}", beat.text);
                }
            }
            BeatKind::ActionComplete => {
                if !beat.text.trim().is_empty() {
                    has_action_complete_beat = true;
                    println!("  {}", beat.text);
                }
            }
            BeatKind::NarrativeCue => {
                if !beat.text.trim().is_empty() {
                    println!("  [{}]", beat.text);
                }
            }
            BeatKind::Ending => {
                if !beat.text.trim().is_empty() {
                    println!("  {}", beat.text);
                }
            }
            BeatKind::ActionContinue | BeatKind::RuleTriggered | BeatKind::WorldUpdate => {
                if !beat.text.trim().is_empty() {
                    println!("  {}", beat.text);
                }
            }
        }
    }

    // 3. Fallbacks in non-verbose mode for events not covered by beats
    if !verbose {
        for ev in &output.raw_events {
            match ev {
                Event::SituationCompleted { sit, recipe, .. } => {
                    if !has_action_complete_beat && !completed_printed.contains(sit) {
                        println!("  [sit {sit}] {recipe} completed");
                        completed_printed.insert(*sit);
                    }
                }
                Event::ResolutionFailed { sit, reason } => {
                    println!("  [sit {sit} FAILED: {reason}]");
                }
                Event::Ended { ending } => {
                    if output.beats.iter().all(|b| b.kind != BeatKind::Ending) {
                        println!("  [game ended: {ending}]");
                    }
                }
                _ => {}
            }
        }
    }
}

fn print_verbs(presenter: &Presenter, filter_runnable: bool) {
    let views = presenter.verbs_view(filter_runnable);
    let mut any_printed = false;
    for v in &views {
        let title_display = if v.title != v.id {
            format!("{} ({})", v.title, v.id)
        } else {
            v.id.clone()
        };
        let mut recipes = v.recipes.clone();
        recipes.sort_by_key(|r| r.priority);
        if filter_runnable && recipes.is_empty() {
            continue;
        }
        any_printed = true;
        let list: Vec<String> = recipes
            .iter()
            .map(|r| {
                let r_display = if r.label != r.name {
                    format!("{} ({})", r.label, r.name)
                } else {
                    r.name.clone()
                };
                let kind_str = if r.craftable {
                    ""
                } else if r.chain_only {
                    ", chain-only"
                } else {
                    ", auto"
                };
                format!(
                    "{} (priority {}, {}m{})",
                    r_display, r.priority, r.duration, kind_str
                )
            })
            .collect();
        println!(
            "  {} {} instances {}: {}",
            title_display,
            v.mode,
            v.instances,
            list.join(", ")
        );
    }
    if filter_runnable && !any_printed {
        println!("  no recipes can be run in the current state");
    }
}

fn print_verbs_verbose(presenter: &Presenter, filter_runnable: bool) {
    let views = presenter.verbs_view(filter_runnable);
    let mut any_printed = false;
    for v in &views {
        if filter_runnable && v.recipes.is_empty() {
            continue;
        }
        any_printed = true;
        let title_display = if v.title != v.id {
            format!("{} ({})", v.title, v.id)
        } else {
            v.id.clone()
        };
        println!(
            "  {} [{}, instances: {}]:",
            title_display, v.mode, v.instances
        );
        for r in &v.recipes {
            let kind = if r.craftable {
                "craftable"
            } else if r.chain_only {
                "chain-only"
            } else {
                "auto"
            };
            let r_display = if r.label != r.name {
                format!("{} ({})", r.label, r.name)
            } else {
                r.name.clone()
            };
            println!(
                "    {} (priority {}, {}m, {})",
                r_display, r.priority, r.duration, kind
            );
            if !r.guards.is_empty() {
                for g in &r.guards {
                    println!("      guard: \"{}\"", g);
                }
            }
            if !r.slots.is_empty() {
                println!("      slots:");
                for s in &r.slots {
                    let flags_str = if s.flags.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", s.flags)
                    };
                    let label_str = if s.label != s.name {
                        format!(" ({})", s.label)
                    } else {
                        String::new()
                    };
                    println!(
                        "        {}{}: {}{flags_str} ({})",
                        s.name, label_str, s.bounds, s.criteria
                    );
                }
            }
        }
    }
    if filter_runnable && !any_printed {
        println!("  no recipes can be run in the current state");
    }
}

fn print_state(presenter: &Presenter) {
    let board = presenter.board_view();
    println!("  clock: {}m", board.clock);
    match &board.ended {
        Some(e) => {
            if e.title != e.id && !e.title.is_empty() {
                println!("  ended: {} ({})", e.id, e.title);
            } else {
                println!("  ended: {}", e.id);
            }
        }
        None => println!("  ended: -"),
    }
    let legacy: Vec<String> = board
        .legacy
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    println!("  legacy: {}", legacy.join(", "));
    for z in &board.zones {
        if z.cards.is_empty() {
            continue;
        }
        let cards: Vec<String> = z
            .cards
            .iter()
            .map(|c| {
                let aspects: Vec<String> =
                    c.aspects.iter().map(|(k, v)| format!("{k}:{v}")).collect();
                let title = if c.title != c.prototype {
                    format!("{} ({})", c.title, c.prototype)
                } else {
                    c.prototype.clone()
                };
                let count_str = if c.count > 1 {
                    format!(" x{}", c.count)
                } else {
                    String::new()
                };
                let expires_str = c
                    .expires_at
                    .map(|t| format!(" (expires {t}m)"))
                    .unwrap_or_default();
                let aspects_str = if aspects.is_empty() {
                    String::new()
                } else {
                    format!(" {{{}}}", aspects.join(", "))
                };
                format!(
                    "{}#{}{count_str}{expires_str}{aspects_str}",
                    title, c.entity_id
                )
            })
            .collect();
        println!("  zone {} [{}]: {}", z.zone_id.0, z.kind, cards.join(" | "));
    }
    for s in &board.situations {
        let mut by_slot: BTreeMap<&str, Vec<String>> = BTreeMap::new();
        for sc in &s.slotted {
            let title = if sc.title != sc.prototype {
                format!("{} ({})", sc.title, sc.prototype)
            } else {
                sc.prototype.clone()
            };
            by_slot
                .entry(&sc.slot)
                .or_default()
                .push(format!("{}#{}", title, sc.entity_id));
        }
        let slots: Vec<String> = by_slot
            .iter()
            .map(|(slot, cards)| format!("{}: {}", slot, cards.join(", ")))
            .collect();
        let r_display = if s.recipe_label != s.recipe {
            format!("{} ({})", s.recipe_label, s.recipe)
        } else {
            s.recipe.clone()
        };
        println!(
            "  sit {} {} [{}] ends {}m",
            s.id,
            r_display,
            slots.join("; "),
            s.ends_at
        );
    }
}

fn print_state_verbose(presenter: &Presenter) {
    let board = presenter.board_view();
    println!("  clock: {}m", board.clock);
    match &board.ended {
        Some(e) => {
            if e.title != e.id && !e.title.is_empty() {
                println!("  ended: {} ({})", e.id, e.title);
            } else {
                println!("  ended: {}", e.id);
            }
        }
        None => println!("  ended: -"),
    }
    let legacy: Vec<String> = board
        .legacy
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    println!("  legacy: {}", legacy.join(", "));
    for z in &board.zones {
        if z.cards.is_empty() {
            continue;
        }
        println!("  zone {} [{}]:", z.zone_id.0, z.kind);
        for c in &z.cards {
            let aspects: Vec<String> = c.aspects.iter().map(|(k, v)| format!("{k}:{v}")).collect();
            let title = if c.title != c.prototype {
                format!("{} ({})", c.title, c.prototype)
            } else {
                c.prototype.clone()
            };
            let count_str = if c.count > 1 {
                format!(" x{}", c.count)
            } else {
                String::new()
            };
            let expires_str = c
                .expires_at
                .map(|t| {
                    let rem = t.saturating_sub(board.clock);
                    format!(" (expires in {rem}m at {t}m)")
                })
                .unwrap_or_default();
            let aspects_str = if aspects.is_empty() {
                String::new()
            } else {
                format!(" {{{}}}", aspects.join(", "))
            };
            println!(
                "    {}#{}{count_str}{expires_str}{aspects_str}",
                title, c.entity_id
            );
        }
    }
    if board.situations.is_empty() {
        println!("  active situations: none");
    } else {
        println!("  active situations:");
        for s in &board.situations {
            let r_display = if s.recipe_label != s.recipe {
                format!("{} ({})", s.recipe_label, s.recipe)
            } else {
                s.recipe.clone()
            };
            println!(
                "    [sit {}] {}: {}m remaining (ends at {}m)",
                s.id, r_display, s.remaining_time, s.ends_at
            );
            let mut by_slot: BTreeMap<&str, Vec<String>> = BTreeMap::new();
            for sc in &s.slotted {
                let title = if sc.title != sc.prototype {
                    format!("{} ({})", sc.title, sc.prototype)
                } else {
                    sc.prototype.clone()
                };
                by_slot
                    .entry(&sc.slot)
                    .or_default()
                    .push(format!("{}#{}", title, sc.entity_id));
            }
            for (slot_name, cards) in by_slot {
                println!("      bound {slot_name}: {}", cards.join(", "));
            }
            for slot in &s.open_slots {
                let cur_count = s.slotted.iter().filter(|sc| sc.slot == slot.name).count();
                let label_str = if slot.label != slot.name {
                    format!(" ({})", slot.label)
                } else {
                    String::new()
                };
                println!(
                    "      open slot: {}{} ({} bound, bounds {}, accepts {}) -> bind {} {} <card>",
                    slot.name, label_str, cur_count, slot.bounds, slot.criteria, s.id, slot.name
                );
            }
        }
    }
}
