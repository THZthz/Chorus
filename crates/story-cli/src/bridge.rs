//! Line-delimited JSON (NDJSON) bridge over stdio for external visualizers (Godot, Web, etc.).
//! Keeps stdout strictly reserved for single-line JSON responses.

use std::collections::BTreeMap;
use std::io::{BufRead, Write};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use story_core::{CardId, EntityId, RecipeId, SituationId, SlotName, Snapshot, VerbId};
use story_engine::{CommitAction, EffectRegistry, Engine};
use story_present::{
    interpolate, BoardView, CardLoreView, PresentationCatalog, PresentationContext, Presenter,
    StoryOutput,
};

#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Request {
    State {
        #[serde(default)]
        available: bool,
    },
    Preview {
        verb: String,
        #[serde(default)]
        bindings: Vec<(String, u64)>,
    },
    Staging {
        verb: String,
        #[serde(default)]
        bindings: Vec<(String, u64)>,
    },
    Commit {
        verb: String,
        #[serde(default)]
        bindings: Vec<(String, u64)>,
        #[serde(default)]
        available: bool,
    },
    Bind {
        sit: u64,
        slot: String,
        entity: u64,
        #[serde(default)]
        available: bool,
    },
    Advance {
        #[serde(default = "default_turns")]
        turns: i64,
        #[serde(default)]
        available: bool,
    },
    Undo {
        #[serde(default)]
        available: bool,
    },
    Inspect {
        query: String,
    },
    Candidates {
        recipe: String,
        slot: String,
    },
    Quit,
}

fn default_turns() -> i64 {
    1
}

#[derive(Serialize)]
struct StateEnvelope {
    ok: bool,
    title: String,
    turn: i64,
    can_undo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<StoryOutput>,
    board: BoardView,
    verbs: Vec<VerbEntry>,
    open_slot_candidates: BTreeMap<String, Vec<EntityId>>,
}

/// Slim verb index for the visualizer: identity and capacity only. Slot and
/// recipe detail lives behind the `staging` request.
#[derive(Serialize)]
struct VerbEntry {
    id: String,
    title: String,
    mode: String,
    instances: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

const BRIDGE_USAGE: &str = "\
story-cli bridge — line-delimited JSON stdio bridge for external visualizers

USAGE:
    story-cli bridge <file.sw> [options]

OPTIONS:
    --seed N        host seed when the content declares `seed host` (default 1)
    -h, --help      print this help message

PROTOCOL:
    Requests and responses are single-line JSON objects exchanged over stdin/stdout.
    Commands: state, preview, staging, commit, bind, advance, undo, inspect, candidates, quit.";

pub fn run(args: &[String]) -> i32 {
    let mut path: Option<String> = None;
    let mut seed: u64 = 1;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("{BRIDGE_USAGE}");
                return 0;
            }
            "--seed" => {
                i += 1;
                seed = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(1);
            }
            other if !other.starts_with('-') && path.is_none() => {
                path = Some(other.to_string());
            }
            other => {
                emit_err(&format!("unknown argument `{other}`"));
                return 2;
            }
        }
        i += 1;
    }

    let Some(path_str) = path else {
        emit_err("usage: story-cli bridge <file.sw> [--seed N]");
        return 2;
    };

    let sw_path = Path::new(&path_str);
    let out = story_lang::compile_file(sw_path);
    let errors: Vec<String> = out
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| d.to_string())
        .collect();
    let Some(content) = out.content else {
        emit_err(&format!("compile failed: {}", errors.join("; ")));
        return 1;
    };
    if !errors.is_empty() {
        emit_err(&format!("compile failed: {}", errors.join("; ")));
        return 1;
    }

    let seed = match content.game.as_ref().map(|g| g.seed.clone()) {
        Some(story_content::ir::SeedSpec::Fixed(n)) => n,
        _ => seed,
    };
    let (engine, diags) = Engine::boot(Arc::new(content), EffectRegistry::new(), seed, None);
    if let Some(err) = diags.iter().find(|d| d.is_error()) {
        emit_err(&format!("boot error: {err}"));
        return 1;
    }

    let catalog = load_catalog_quiet(sw_path);
    let mut presenter = Presenter::new(engine, catalog);
    let mut undo_stack: Vec<Snapshot> = Vec::new();

    let title = presenter
        .content()
        .game
        .as_ref()
        .map(|g| g.title.clone())
        .unwrap_or_else(|| "Chorus²".to_string());
    let turn = presenter.engine().turn();

    emit_state(&presenter, &title, turn, !undo_stack.is_empty(), None, false);

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let req: Request = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                emit_err(&format!("invalid json request: {e}"));
                continue;
            }
        };

        match req {
            Request::State { available } => {
                emit_state(
                    &presenter,
                    &title,
                    turn,
                    !undo_stack.is_empty(),
                    None,
                    available,
                );
            }
            Request::Preview { verb, bindings } => {
                let action_bindings = resolve_staged(&presenter, &verb, to_staged(bindings));
                let action = CommitAction {
                    verb: VerbId::from(verb),
                    bindings: action_bindings,
                };
                match presenter.engine().preview_commit(&action) {
                    Ok(rec_id) => {
                        let rec_decl = presenter.content().recipe(rec_id.as_str());
                        let label = rec_decl
                            .map(|r| presenter.catalog().recipe_label(rec_id.as_str(), r).to_string())
                            .unwrap_or_else(|| rec_id.to_string());
                        let duration = rec_decl.map(|r| r.duration).unwrap_or(0);
                        let mut bound_cards: BTreeMap<SlotName, Vec<CardId>> = BTreeMap::new();
                        for (slot, eid) in &action.bindings {
                            if let Some(ent) = presenter.state().entities.get(eid) {
                                bound_cards
                                    .entry(slot.clone())
                                    .or_default()
                                    .push(ent.prototype.clone());
                            }
                        }
                        let ctx = PresentationContext {
                            recipe: Some(rec_id.clone()),
                            verb: Some(action.verb.clone()),
                            bound_cards,
                            ..Default::default()
                        };
                        let prose = rec_decl
                            .and_then(|r| presenter.catalog().recipe_start(rec_id.as_str(), r))
                            .map(|p| {
                                interpolate(
                                    p,
                                    presenter.state(),
                                    presenter.content(),
                                    presenter.catalog(),
                                    &ctx,
                                )
                            });
                        emit_json(&serde_json::json!({
                            "ok": true,
                            "matched": true,
                            "recipe_id": rec_id.as_str(),
                            "label": label,
                            "duration": duration,
                            "prose": prose,
                        }));
                    }
                    Err(diag) => {
                        emit_json(&serde_json::json!({
                            "ok": true,
                            "matched": false,
                            "kind": diag.kind.word(),
                            "reason": diag.message,
                        }));
                    }
                }
            }
            Request::Staging { verb, bindings } => {
                let staged = resolve_staged(&presenter, &verb, to_staged(bindings));
                let view = presenter.staging_view(&verb, &staged);
                emit_json(&serde_json::json!({ "ok": true, "staging": view }));
            }
            Request::Commit {
                verb,
                bindings,
                available,
            } => {
                let snap = presenter.snapshot();
                let staged = resolve_staged(&presenter, &verb, to_staged(bindings));
                match presenter.commit_staged(&verb, &staged) {
                    Ok(output) => {
                        undo_stack.push(snap);
                        emit_state(
                            &presenter,
                            &title,
                            turn,
                            !undo_stack.is_empty(),
                            Some(output),
                            available,
                        );
                    }
                    Err(diag) => emit_err(&diag.to_string()),
                }
            }
            Request::Bind {
                sit,
                slot,
                entity,
                available,
            } => {
                let snap = presenter.snapshot();
                match presenter.bind(SituationId(sit), SlotName::from(slot), EntityId(entity)) {
                    Ok(output) => {
                        undo_stack.push(snap);
                        emit_state(
                            &presenter,
                            &title,
                            turn,
                            !undo_stack.is_empty(),
                            Some(output),
                            available,
                        );
                    }
                    Err(diag) => emit_err(&diag.to_string()),
                }
            }
            Request::Advance { turns, available } => {
                let snap = presenter.snapshot();
                let dt = turn.saturating_mul(turns.max(1));
                match presenter.advance(dt) {
                    Ok(output) => {
                        undo_stack.push(snap);
                        emit_state(
                            &presenter,
                            &title,
                            turn,
                            !undo_stack.is_empty(),
                            Some(output),
                            available,
                        );
                    }
                    Err(diag) => emit_err(&diag.to_string()),
                }
            }
            Request::Undo { available } => {
                if let Some(snap) = undo_stack.pop() {
                    if let Err(diag) = presenter.restore(&snap) {
                        emit_err(&diag.to_string());
                    } else {
                        emit_state(
                            &presenter,
                            &title,
                            turn,
                            !undo_stack.is_empty(),
                            None,
                            available,
                        );
                    }
                } else {
                    emit_err("nothing to undo");
                }
            }
            Request::Inspect { query } => {
                let card: Option<CardLoreView> = presenter.inspect_card(&query);
                emit_json(&serde_json::json!({
                    "ok": card.is_some(),
                    "card": card,
                }));
            }
            Request::Candidates { recipe, slot } => {
                let entities = presenter
                    .engine()
                    .slot_candidates(RecipeId::from(recipe), SlotName::from(slot));
                emit_json(&serde_json::json!({
                    "ok": true,
                    "entities": entities,
                }));
            }
            Request::Quit => break,
        }
    }
    0
}

fn emit_state(
    presenter: &Presenter,
    title: &str,
    turn: i64,
    can_undo: bool,
    output: Option<StoryOutput>,
    available: bool,
) {
    let board = presenter.board_view();
    let verbs = presenter
        .verbs_view(available)
        .into_iter()
        .map(|v| VerbEntry {
            id: v.id,
            title: v.title,
            mode: v.mode,
            instances: v.instances,
            description: v.description,
        })
        .collect();
    let engine = presenter.engine();

    let mut open_slot_candidates: BTreeMap<String, Vec<EntityId>> = BTreeMap::new();
    let all_entities: Vec<EntityId> = engine.state().entities.keys().copied().collect();
    for sit in &board.situations {
        for os in &sit.open_slots {
            let key = format!("{}:{}", sit.id.0, os.name);
            let cands: Vec<EntityId> = all_entities
                .iter()
                .copied()
                .filter(|&eid| {
                    engine
                        .can_bind(sit.id, SlotName::from(os.name.as_str()), eid)
                        .is_ok()
                })
                .collect();
            open_slot_candidates.insert(key, cands);
        }
    }

    let env = StateEnvelope {
        ok: true,
        title: title.to_string(),
        turn,
        can_undo,
        output,
        board,
        verbs,
        open_slot_candidates,
    };
    emit_json(&env);
}

fn to_staged(bindings: Vec<(String, u64)>) -> Vec<(SlotName, EntityId)> {
    bindings
        .into_iter()
        .map(|(s, id)| (SlotName::from(s), EntityId(id)))
        .collect()
}

/// Resolve staged pairs whose slot name is empty ("place this card for me"):
/// the entity takes the first fitting slot of the verb's craftable, chain-free
/// recipes in commit-selection order (priority, then declaration order);
/// `internal` slots never take player cards. Named pairs pass through
/// unchanged, so an unresolved name-less pair stays name-less and the engine
/// reports its real refusal.
fn resolve_staged(
    presenter: &Presenter,
    verb: &str,
    staged: Vec<(SlotName, EntityId)>,
) -> Vec<(SlotName, EntityId)> {
    if !staged.iter().any(|(s, _)| s.0.is_empty()) {
        return staged;
    }
    let Some(v) = presenter.content().verbs.get(verb) else {
        return staged;
    };
    let mut recipes: Vec<&story_content::ir::RecipeDecl> = v
        .recipes
        .values()
        .filter(|r| r.craftable && !r.chain_only)
        .collect();
    recipes.sort_by_key(|r| (r.priority, r.decl_index));
    let engine = presenter.engine();
    staged
        .into_iter()
        .map(|(s, e)| {
            if !s.0.is_empty() {
                return (s, e);
            }
            for r in &recipes {
                for sl in &r.slots {
                    if sl.internal {
                        continue;
                    }
                    if engine
                        .slot_candidates(r.id.clone(), sl.name.clone())
                        .contains(&e)
                    {
                        return (sl.name.clone(), e);
                    }
                }
            }
            (s, e)
        })
        .collect()
}

fn emit_err(msg: &str) {
    emit_json(&serde_json::json!({
        "ok": false,
        "error": msg,
    }));
}

fn emit_json<T: Serialize>(val: &T) {
    if let Ok(line) = serde_json::to_string(val) {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

fn load_catalog_quiet(sw_path: &Path) -> PresentationCatalog {
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
    target
        .and_then(|p| PresentationCatalog::from_file(&p).ok())
        .unwrap_or_default()
}
