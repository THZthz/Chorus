//! Card-reachability CLI. "can the player obtain card X?" over a bundled
//! script. Both semantics:
//! default is existence; --usable also requires the card to be bindable into
//! some committable verb slot.
//!
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt THE_WAX_CYLINDER [--usable]
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt --all
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt --demand "aspect:secret>=3"
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt --world
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt THE_WAX_CYLINDER --why
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt --verb WORK
//!
//! Frozen surface:
//!   - stdout text incl. render_path's 4-space indent steps, `path i/n:`,
//!     `verdict: X (existence: y)` — byte-frozen;
//!   - the `--all` name set = prototypes ∪ LIVE initial entities ∪
//!     deck.order (NOT defaultcard), sorted by UTF-16 code-unit order;
//!   - argv errors exit 2 with the usage text; a bad demand query is a
//!     compile-time analyze error exiting 1 (the message line is the frozen
//!     part). Q15 honesty: a trailing `--demand` (no value slot) is an argv
//!     error naming the flag — it used to silently vanish into name mode.
//!   - `--world` renders the whole-world dashboard (reach::world_report) and
//!     wins over --all / a card name / a demand; it needs only the script.
//!   - `--graph` renders the whole-content mint dependency graph as Graphviz
//!     DOT (reach::export::graph_dot — nodes = universe ∪ initial card names
//!     plus proven-site owners, edges = proven mints / requires /
//!     destroyed-by kills, sorted under cmp_utf16 so two runs are
//!     byte-identical). It needs only the script and wins over --verb /
//!     --all / a card name / a demand, losing only to --world.
//!   - `--json` rides the card faces (`<CARD> --json`, `--all --json`,
//!     `--demand "<q>" --json`): the per-card report as pretty JSON —
//!     reach::export's envelope {file, demand, usable, report} with the
//!     report fields in declaration order and the enums as their as_str
//!     strings. `--why --json` MERGES the blocker table into the envelope
//!     as a `why` array (each blocker `CardBlocker::as_str`'s own text);
//!     without --why the key is absent. `--all --json` is one JSON array in
//!     the --all walk order. --json is ignored by --world, --verb and
//!     --graph (their faces stay their text/DOT selves); error faces (usage
//!     exit 2, analyze exit 1) are unchanged.
//!   - the usage first line carries `[--why] [--verb "<NAME>"]` with the
//!     diagnostic faces — the one sanctioned argv-surface byte change of
//!     that wave (the --world precedent), its three usage pins in
//!     tests/session/cli.rs updated in the same change. The export flags
//!     deliberately do NOT touch the usage line (it is byte-pinned); they
//!     are documented here and in §13.1.
//!   - `--by <minute>` (the time-window face, card faces only): "obtainable
//!     BY absolute minute M" (engine clock, §7 — minutes since day 0:00).
//!     The bound rides the Analyzer (one world per run — the bound cannot
//!     drift between cards): a slot trigger whose first crossing lands
//!     beyond M has not fired yet and its sites fall to candidate; the
//!     freeze boundary and M take the tighter horizon (duration>0 recipes
//!     are unprovable at any finite horizon; trigger-held deck caps rewrite
//!     to fired-by-M 1/0). The text head gains `  [by minute M]`
//!     (render_report_by); without --by every face is byte-identical to the
//!     frozen output. Composes with --all / a card / --demand / --why /
//!     --usable and with --from-state; --world / --verb / --graph ignore
//!     it, and the `--json` envelope carries the BOUNDED report with no
//!     `by` marker (the report fields say it: verdict, paths, candidates).
//!     A missing or non-integer value is
//!     the argv error (exit 2); --by does not open the usage gate.
//!   - `--from-state <log.jsonl>` (the midgame face): the analysis initial
//!     world is a log replay — load_log + the engine's own fold (§7,
//!     reach::midgame::replay_state) over the script's initial state. The
//!     replayed entities (dead filtered) and the replayed clock enter the
//!     analysis unchanged; already-fired once triggers demote (they never
//!     fire again); non-reset deck cursors bound the remaining draws
//!     (positions serve from the cursor forward). A fidelity note rides on
//!     every replayed CARD report naming the unmodeled state fields (open
//!     windows / executed-recipe map / rng counter); a log that ends the
//!     game is the analyze error (exit 1). Works on every face (the state
//!     is the Analyzer's input — the note-bearing head is the card face);
//!     composes with --by. A missing value is the
//!     argv error (exit 2); a missing/unreadable/unparsable log file or a
//!     drifting fold is `Error: ...` exit 1. Documented order: after the
//!     script, before the card (flags parse position-free like every other
//!     flag).
//!
//! Diagnostic faces (§13.1):
//!   - `--why` appends one `— why` section per card report (card mode and
//!     --all alike): every live unproven serving site with its owner and the
//!     ladder's first blocker. Without it the output is byte-identical.
//!   - `--cost` appends one `— cost` section per card report (card mode and
//!     --all alike, after `--why`): the cost/count reading of the settled
//!     facts — minimum verb applications over the proven provenance DAG
//!     (recipe hops 1, trigger hops 0, initial 0 — a LOWER-BOUND planning
//!     estimate, the cheapest witness chain on one line), the minimum clock
//!     minutes (recipe hops = the verb's window, slot-trigger hops = the
//!     first crossing), and the copies bound from `avail` (`MANY` →
//!     `unbounded`, a count past the flatten weight cap → `N+ (world cap)`).
//!     A target with no proven DAG renders the one-line exclusion face (the
//!     candidate sources are named by the report above). Composes with
//!     --usable / --why / --all and the bounded-time faces (the --by window
//!     and the --from-state replay bound the DAG through the same settled
//!     facts; a --by note says the window bounds it). Kept OUT of --json
//!     (the envelope carries no cost data), and IGNORED under --demand (the
//!     demand match's provenance is not costed) — documented, not rejected.
//!     --cost does not open the usage gate; the usage line stays byte-frozen
//!     (the export precedent).
//!   - `--verb <NAME>` renders the verb report; it wins over --all, an
//!     explicit card and a demand (the --world precedent — one dedicated,
//!     target-free face), and `--why` does not apply to it. A `--verb` with
//!     no value slot is the argv error (exit 2); an EMPTY value is falsy
//!     (the demand truthiness) and never opens the usage gate; an unknown
//!     verb name is the analyze error (exit 1).

use std::io::Write as _;
use std::process::ExitCode;

use chorus::content::script::{compile_script, CompiledScript};
use chorus::core::dict::core_dictionary;
use chorus::core::text::cmp_utf16;
use chorus::reach::cost::{cost_report, render_cost};
use chorus::reach::export::{card_export, graph_dot, render_json, render_json_array};
use chorus::reach::fixpoint::SiteBlocker;
use chorus::reach::midgame::replay_state;
use chorus::reach::render::{render_report_by, render_verb, render_why};
use chorus::reach::world_report::{self, world_report};
use chorus::reach::{Analyzer, CardOptions};

fn main() -> ExitCode {
    // argv parsing — frozen surface, byte for byte
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usable = args.iter().any(|a| a == "--usable");
    let why = args.iter().any(|a| a == "--why");
    let cost = args.iter().any(|a| a == "--cost");
    // a present --demand with no next argv slot is an ARGV ERROR (Q15):
    // it used to read argv[idx+1] = undefined and silently vanish into
    // name mode. An EMPTY string value is falsy at every consumer —
    // it rides as None exactly as before (the usage gate owns that face).
    let demand_idx = args.iter().position(|a| a == "--demand");
    let demand: Option<String> = match demand_idx {
        None => None,
        Some(i) => match args.get(i + 1) {
            Some(d) if !d.is_empty() => Some(d.clone()),
            Some(_) => None,
            None => {
                eprintln!("--demand needs a value: --demand \"<query>\"");
                eprintln!(
                    "usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"] [--why] [--verb \"<NAME>\"] [--world]\n       reach <script.txt> --all"
                );
                return ExitCode::from(2);
            }
        },
    };
    // `--verb <NAME>` parses like `--demand` (value flag; Q15 honesty): a
    // present --verb with no next argv slot is the argv error; an EMPTY
    // value is falsy (the frozen truthiness) and never opens the usage gate.
    let verb_idx = args.iter().position(|a| a == "--verb");
    let verb: Option<String> = match verb_idx {
        None => None,
        Some(i) => match args.get(i + 1) {
            Some(v) if !v.is_empty() => Some(v.clone()),
            Some(_) => None,
            None => {
                eprintln!("--verb needs a value: --verb \"<NAME>\"");
                eprintln!(
                    "usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"] [--why] [--verb \"<NAME>\"] [--world]\n       reach <script.txt> --all"
                );
                return ExitCode::from(2);
            }
        },
    };
    // `--by <minute>` parses like the other value flags (Q15 honesty): a
    // present --by with no next argv slot, or a value that is not an
    // integer, is the argv error naming the flag. The bound is the authored
    // ABSOLUTE minute (engine clock, §7) — a negative minute parses fine
    // and simply bounds every trigger out of the window.
    let by_idx = args.iter().position(|a| a == "--by");
    let by: Option<i64> = match by_idx {
        None => None,
        Some(i) => match args.get(i + 1) {
            Some(v) => match v.parse::<i64>() {
                Ok(m) => Some(m),
                Err(_) => {
                    eprintln!("--by needs an integer minute: --by <minute>");
                    eprintln!(
                        "usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"] [--why] [--verb \"<NAME>\"] [--world]\n       reach <script.txt> --all"
                    );
                    return ExitCode::from(2);
                }
            },
            None => {
                eprintln!("--by needs a value: --by <minute>");
                eprintln!(
                    "usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"] [--why] [--verb \"<NAME>\"] [--world]\n       reach <script.txt> --all"
                );
                return ExitCode::from(2);
            }
        },
    };
    // `--from-state <file>`: the midgame face's log path (value flag, same
    // honesty); an EMPTY value is falsy like --demand's.
    let from_state_idx = args.iter().position(|a| a == "--from-state");
    let from_state: Option<String> = match from_state_idx {
        None => None,
        Some(i) => match args.get(i + 1) {
            Some(v) if !v.is_empty() => Some(v.clone()),
            Some(_) => None,
            None => {
                eprintln!("--from-state needs a value: --from-state <log.jsonl>");
                eprintln!(
                    "usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"] [--why] [--verb \"<NAME>\"] [--world]\n       reach <script.txt> --all"
                );
                return ExitCode::from(2);
            }
        },
    };
    let mut rest: Vec<String> = Vec::new();
    let mut world = false;
    let mut graph = false;
    let mut json = false;
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == "--usable" || args[i] == "--why" || args[i] == "--cost" {
            i += 1;
            continue;
        }
        if args[i] == "--world" {
            world = true;
            i += 1;
            continue;
        }
        if args[i] == "--graph" {
            graph = true;
            i += 1;
            continue;
        }
        if args[i] == "--json" {
            json = true;
            i += 1;
            continue;
        }
        if args[i] == "--demand" {
            i += 2; // the flag and its value are both consumed
            continue;
        }
        if args[i] == "--verb" {
            i += 2; // the flag and its value are both consumed
            continue;
        }
        if args[i] == "--by" {
            i += 2; // the flag and its value are both consumed
            continue;
        }
        if args[i] == "--from-state" {
            i += 2; // the flag and its value are both consumed
            continue;
        }
        rest.push(args[i].clone());
        i += 1;
    }
    let file: Option<&String> = rest.first();
    let card: Option<&String> = rest.get(1);
    let all = rest.iter().any(|a| a == "--all");
    // the usage gate: a present file AND (a card OR --all OR a demand OR
    // --world OR --graph OR a verb), with the frozen string truthiness; the
    // text (two lines) is exact. An EMPTY argv element is falsy for the file
    // slot and the card slot alike. `reach --world` / `reach --graph` with
    // no script fall through here — both need the file too.
    let file = match file {
        Some(f)
            if !f.is_empty()
                && (all
                    || world
                    || graph
                    || verb.is_some()
                    || card.is_some_and(|c| !c.is_empty())
                    || demand.is_some()) =>
        {
            f
        }
        _ => {
            eprintln!(
                "usage: reach <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"] [--why] [--verb \"<NAME>\"] [--world]\n       reach <script.txt> --all"
            );
            return ExitCode::from(2);
        }
    };

    // compile the script; a missing file / compile error is a hard error →
    // exit 1. Only the message line is reproducible
    // (deliberate: stderr text is not part of the stdout freeze).
    let dict = core_dictionary();
    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let compiled = match compile_script(&text, &dict) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::from(1);
        }
    };
    let CompiledScript { content, state, .. } = compiled;

    // the midgame face: the analysis initial world is the LOG replayed onto
    // the script's initial state (the engine's own fold — §7). A missing or
    // unparsable log and a drifting fold are `Error: ...` exit 1; a log that
    // ENDS the game is the same error — an ended world has nothing left to
    // reach.
    let state = match &from_state {
        None => state,
        Some(log_path) => {
            let mid = match replay_state(std::path::Path::new(log_path), &state, &dict, &content) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: {e}");
                    return ExitCode::from(1);
                }
            };
            match &mid.ended {
                None => mid,
                Some(end) => {
                    eprintln!(
                        "Error: the log ends the game ('{}') — an ended world has nothing left to reach",
                        end.title
                    );
                    return ExitCode::from(1);
                }
            }
        }
    };

    // the branch ORDER is the frozen behavior: --world wins over everything
    // (the dashboard is target-free — no card, no --all walk, no demand);
    // --graph is the second target-free face (the whole-content mint graph,
    // script only — it loses only to --world); --all wins over an explicit
    // card name; a lone --demand reports once (the target is ignored by the
    // analyzer under demand). `--json` is a modifier on the card faces
    // only: --world/--verb/--graph ignore it.
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let fail = |e: chorus::error::ChorusError| -> ExitCode {
        // an analyze error is fatal → exit 1; the message
        // line is the reproducible part
        eprintln!("Error: {e}");
        ExitCode::from(1)
    };
    if world {
        // one Analyzer, one world, zero per-card work — the sections are the
        // post-fixpoint facts re-rendered (reach::world_report)
        let r = world_report(&content, &state);
        let mut buf = String::new();
        world_report::render_report(&mut buf, file, &r);
        if out.write_all(buf.as_bytes()).is_err() {
            return ExitCode::from(1);
        }
        let _ = out.flush();
        return ExitCode::from(0);
    }
    if graph {
        // the whole-content mint dependency graph as Graphviz DOT
        // (reach::export::graph_dot — deterministic, cmp_utf16-sorted)
        let buf = graph_dot(&content, &state);
        if out.write_all(buf.as_bytes()).is_err() {
            return ExitCode::from(1);
        }
        let _ = out.flush();
        return ExitCode::from(0);
    }
    if let Some(name) = &verb {
        // --verb wins over --all, an explicit card and a demand (the --world
        // precedent: one dedicated, target-free face — §13.1); --why does
        // not apply to it. An unknown verb name is the analyze error
        // (exit 1, stderr only).
        let analyzer = Analyzer::new(&content, &state);
        let Some(vr) = analyzer.verb_report(name) else {
            eprintln!("Error: no verb '{name}'");
            return ExitCode::from(1);
        };
        let mut buf = String::new();
        render_verb(&mut buf, file, &vr);
        if out.write_all(buf.as_bytes()).is_err() {
            return ExitCode::from(1);
        }
        let _ = out.flush();
        return ExitCode::from(0);
    }
    if all {
        // prototypes ∪ live initial entities ∪
        // deck.order (NOT defaultcard); insertion set,
        // then UTF-16 code-unit sort
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
        sorted.sort_by(|a, b| cmp_utf16(a, b));
        // one Analyzer, one world: the bound and the replay mark are set
        // once, before the first analysis (the facts cache is per host)
        let mut analyzer = Analyzer::new(&content, &state);
        if let Some(m) = by {
            analyzer = analyzer.by(m);
        }
        if from_state.is_some() {
            analyzer = analyzer.replayed();
        }
        if json {
            // the array face: one envelope per name in the --all walk
            // order, rendered once (the analyze error face is unchanged —
            // the parse error fires on the FIRST card, before any byte is
            // written, exactly like the incremental text walk)
            let mut reports: Vec<chorus::reach::CardReport> = Vec::new();
            let mut blocked: Vec<Vec<SiteBlocker>> = Vec::new();
            for name in &sorted {
                let r = match analyzer
                    .analyze_card(name, CardOptions { usable, demand: demand.clone() })
                {
                    Ok(r) => r,
                    Err(e) => return fail(e),
                };
                if why {
                    blocked.push(analyzer.card_blockers(name));
                }
                reports.push(r);
            }
            let envs: Vec<_> = reports
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    card_export(
                        file,
                        demand.as_deref(),
                        usable,
                        r,
                        if why { Some(&blocked[i]) } else { None },
                    )
                })
                .collect();
            let buf = render_json_array(&envs);
            if out.write_all(buf.as_bytes()).is_err() {
                return ExitCode::from(1);
            }
            let _ = out.flush();
            return ExitCode::from(0);
        }
        for name in &sorted {
            // report(n) passes the CAPTURED demand on — `--all --demand q`
            // renders the demand report once per name
            let r =
                match analyzer.analyze_card(name, CardOptions { usable, demand: demand.clone() }) {
                    Ok(r) => r,
                    Err(e) => return fail(e),
                };
            let mut buf = String::new();
            render_report_by(&mut buf, file, name, demand.as_deref(), usable, by, &r);
            if why {
                // the --why section rides AFTER the report — without the flag
                // this face is byte-identical to the frozen output
                render_why(&mut buf, file, name, &analyzer.card_blockers(name));
            }
            if cost && demand.is_none() {
                // the --cost section rides AFTER --why; under --demand it is
                // documented-ignored (the demand match is not costed)
                render_cost(&mut buf, file, name, &cost_report(&analyzer, name));
            }
            if out.write_all(buf.as_bytes()).is_err() {
                return ExitCode::from(1);
            }
        }
    } else {
        // report(card): card may be absent when
        // only --demand is given; the analyzer ignores the target under
        // demand (the demand branch runs first)
        let card = card.map(String::as_str).unwrap_or("");
        let mut analyzer = Analyzer::new(&content, &state);
        if let Some(m) = by {
            analyzer = analyzer.by(m);
        }
        if from_state.is_some() {
            analyzer = analyzer.replayed();
        }
        let r = match analyzer.analyze_card(card, CardOptions { usable, demand: demand.clone() }) {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        if json {
            // the single-object face; `--why --json` merges the blocker
            // table into the envelope (render_why's rows, as_str text)
            let blockers = if why { Some(analyzer.card_blockers(card)) } else { None };
            let env = card_export(file, demand.as_deref(), usable, &r, blockers.as_deref());
            let buf = render_json(&env);
            if out.write_all(buf.as_bytes()).is_err() {
                return ExitCode::from(1);
            }
            let _ = out.flush();
            return ExitCode::from(0);
        }
        let mut buf = String::new();
        render_report_by(&mut buf, file, card, demand.as_deref(), usable, by, &r);
        if why {
            render_why(&mut buf, file, card, &analyzer.card_blockers(card));
        }
        if cost && demand.is_none() {
            // the --cost section rides AFTER --why; under --demand it is
            // documented-ignored (the demand match is not costed)
            render_cost(&mut buf, file, card, &cost_report(&analyzer, card));
        }
        if out.write_all(buf.as_bytes()).is_err() {
            return ExitCode::from(1);
        }
    }
    let _ = out.flush();
    ExitCode::from(0)
}
