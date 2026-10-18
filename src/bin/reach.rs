//! Card-reachability CLI. "can the player obtain card X?" over a bundled
//! script. Both semantics:
//! default is existence; --usable also requires the card to be bindable into
//! some committable verb slot.
//!
//! The argv surface is the cargo/uv-style Commands + Options grammar:
//!
//!   reach <script.txt> <COMMAND> [OPTIONS]
//!
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER [--usable]
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt all
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt demand "aspect:secret>=3"
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt world
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --why
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt verb WORK
//!
//! Frozen surface:
//!   - stdout text incl. render_path's 4-space indent steps, `path i/n:`,
//!     `verdict: X (existence: y)` — byte-frozen. With `--color` the SAME
//!     bytes gain ANSI SGR sequences through `chorus::cli::Styles`
//!     (line-level prefix matching over the rendered text — the library
//!     renderers are untouched); without the flag the styler is the
//!     identity transform, so the default output is byte-identical to the
//!     unstyled face. The machine-readable faces (`--json` envelopes and
//!     the `--graph` DOT) are NEVER colorized — they pass through unstyled
//!     even under `--color`.
//!   - the `all` name set = prototypes ∪ LIVE initial entities ∪
//!     deck.order (NOT defaultcard), sorted by UTF-16 code-unit order;
//!   - argv errors exit 2 (stderr, naming the offending word — the Q15
//!     honesty tradition); a bad demand query is a compile-time analyze
//!     error exiting 1 (the message line is the frozen part). The exact
//!     error faces: no arguments → the short usage + the `reach --help`
//!     hint; `--help`/`-h` anywhere → the full help on stdout, exit 0; a
//!     script with no command, or an unknown command word → exit 2 naming
//!     it and listing the legal commands; an unknown `-`-flag word in flag
//!     position (a typo'd `--usablee`, a stray `--` — there is no
//!     end-of-options separator on this surface) → exit 2 naming it and
//!     listing the legal flags; extra positionals beyond
//!     `<script> <COMMAND> [value]` → exit 2 naming the first extra;
//!     `card`/`demand`/`verb` with no (or an empty) value → exit 2 naming
//!     the command; `--by` with no value or a non-integer value → exit 2
//!     naming the flag (a NEGATIVE integer is a value — `--by -5` is
//!     legal); `--from-state` with no value, or a `-`-leading token where
//!     the value belongs (that token is a flag, not a path) → exit 2
//!     naming the flag.
//!   - the `world` face renders the whole-world dashboard
//!     (reach::world_report); the `graph` face renders the whole-content
//!     mint dependency graph as Graphviz DOT (reach::export::graph_dot —
//!     nodes = universe ∪ initial card names plus proven-site owners,
//!     edges = proven mints / requires / destroyed-by kills, sorted under
//!     cmp_utf16 so two runs are byte-identical). Both need only the
//!     script. As subcommands they are mutually exclusive by construction.
//!   - `--json` rides the card faces (`card <NAME> --json`,
//!     `all --json`, `demand "<q>" --json`): the per-card report as pretty
//!     JSON — reach::export's envelope {file, demand, usable, report} with
//!     the report fields in declaration order and the enums as their as_str
//!     strings. `--why --json` MERGES the blocker table into the envelope
//!     as a `why` array (each blocker `CardBlocker::as_str`'s own text);
//!     without --why the key is absent. `all --json` is one JSON array in
//!     the walk order. --json is ignored by world, verb and graph (their
//!     faces stay their text/DOT selves); error faces (usage exit 2,
//!     analyze exit 1) are unchanged.
//!   - `--by <minute>` (the time-window face, card faces only): "obtainable
//!     BY absolute minute M" (engine clock, §7 — minutes since day 0:00).
//!     The bound rides the Analyzer (one world per run — the bound cannot
//!     drift between cards): a slot trigger whose first crossing lands
//!     beyond M has not fired yet and its sites fall to candidate; the
//!     freeze boundary and M take the tighter horizon (duration>0 recipes
//!     are unprovable at any finite horizon; trigger-held deck caps rewrite
//!     to fired-by-M 1/0). The text head gains `  [by minute M]`
//!     (render_report_by); without --by every face is byte-identical to the
//!     frozen output. Composes with card / all / --why / --usable and with
//!     --from-state; world / verb / graph ignore it, and the `--json`
//!     envelope carries the BOUNDED report with no `by` marker (the report
//!     fields say it: verdict, paths, candidates). A missing or non-integer
//!     value is the argv error (exit 2).
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
//!     composes with --by. A missing value is the argv error (exit 2); a
//!     missing/unreadable/unparsable log file or a drifting fold is
//!     `Error: ...` exit 1.
//!
//! Diagnostic faces (§13.1):
//!   - `--why` appends one `— why` section per card report (card and all
//!     alike): every live unproven serving site with its owner and the
//!     ladder's first blocker. Without it the output is byte-identical.
//!   - `--cost` appends one `— cost` section per card report (card and all
//!     alike, after `--why`): the cost/count reading of the settled facts —
//!     minimum verb applications over the proven provenance DAG (recipe
//!     hops 1, trigger hops 0, initial 0 — a LOWER-BOUND planning estimate,
//!     the cheapest witness chain on one line), the minimum clock minutes,
//!     and the copies bound from `avail` (`MANY` → `unbounded`, a count
//!     past the flatten weight cap → `N+ (world cap)`). A target with no
//!     proven DAG renders the one-line exclusion face. Composes with
//!     --usable / --why / all and the bounded-time faces. Kept OUT of
//!     --json, and IGNORED under demand (the demand match's provenance is
//!     not costed) — documented, not rejected.
//!   - the `verb` face renders the verb report; `--why` does not apply to
//!     it. An unknown verb name is the analyze error (exit 1).

use std::io::Write;
use std::process::ExitCode;

use chorus::cli::Styles;
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

/// One parsed invocation. The command IS the face; the options compose as
/// documented above.
struct Cli {
    script: String,
    command: Command,
    usable: bool,
    why: bool,
    cost: bool,
    json: bool,
    color: bool,
    by: Option<i64>,
    from_state: Option<String>,
}

enum Command {
    Card(String),
    All,
    Demand(String),
    Verb(String),
    World,
    Graph,
}

/// The legal command words, in help order — the list an argv error prints.
const COMMANDS: [&str; 6] = ["card", "all", "demand", "verb", "world", "graph"];

/// The legal option words, in help order — the list an unknown-flag argv
/// error prints (`-h`/`--help` included for completeness, though help is
/// checked before this list ever fires).
const FLAG_LIST: &str =
    "--usable, --why, --cost, --json, --by <MINUTE>, --from-state <LOG>, --color, -h/--help";

/// The short usage: the stderr face of every argv error, and the no
/// -arguments face (with the `--help` hint — the cargo/uv convention).
const SHORT_USAGE: &str =
    "usage: reach <script.txt> <COMMAND> [OPTIONS]\ntry 'reach --help' for the command list";

/// The full help (stdout, exit 0): the command table, the option table and
/// three real invocations over the bundled scripts.
fn help_text() -> String {
    format!(
        "\
reach — static card reachability over a compiled script

{SHORT_USAGE}

Commands:
  card <NAME>      one card's verdict: proven paths and bindable slots
  all              the verdict for every name
  demand <QUERY>   the cards whose aspects/properties match a slot query like aspect:secret>=2
  verb <NAME>      one verb's report
  world            the whole-world dashboard
  graph            the mint dependency graph as Graphviz DOT

Options:
  --usable             also require bindability into a committable verb slot
  --why                append the per-site blocker table (card/all/demand)
  --cost               append the cost/count section (card/all; ignored under demand)
  --json               machine-readable output (card/all/demand; world/verb/graph ignore it)
  --by <MINUTE>        bind every time-keyed judgment to absolute minute MINUTE
  --from-state <LOG>   replay a JSONL action log as the analysis initial world
  --color              ANSI color (default off; when off the output is byte-identical to the plain face)
  -h, --help           print help

  a value option (--by, --from-state) repeated on one line: the last occurrence wins

examples:
  reach scripts/the-nightjar-flyer.txt card THE_WAX_CYLINDER --why
  reach scripts/the-nightjar-flyer.txt all --cost --by 600
  reach scripts/the-nightjar-flyer.txt world
"
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(ParseOutcome::Help) => {
            print!("{}", help_text());
            return ExitCode::from(0);
        }
        Ok(ParseOutcome::Cli(cli)) => cli,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    run(cli)
}

enum ParseOutcome {
    Help,
    Cli(Cli),
}

/// An argv error: the message, then the short usage, exit 2 (the caller
/// prints the message itself so multi-line errors stay one write).
fn argv_err(msg: &str) -> String {
    format!("{msg}\n{SHORT_USAGE}")
}

/// The cargo/uv-style grammar: positionals are `<script> <COMMAND> [value]`
/// and NOTHING further; every `--flag` parses position-free; unknown
/// `-`-words error naming themselves; `-h/--help` anywhere wins. Errors
/// name the offending word and carry the short usage (the Q15 tradition).
fn parse_args(args: &[String]) -> Result<ParseOutcome, String> {
    if args.is_empty() {
        // the short usage itself carries the `reach --help` hint
        return Err(SHORT_USAGE.to_string());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(ParseOutcome::Help);
    }
    let mut cli = Cli {
        script: String::new(),
        command: Command::All, // overwritten below; no default face
        usable: false,
        why: false,
        cost: false,
        json: false,
        color: false,
        by: None,
        from_state: None,
    };
    let mut positionals: Vec<&str> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--usable" => cli.usable = true,
            "--why" => cli.why = true,
            "--cost" => cli.cost = true,
            "--json" => cli.json = true,
            "--color" => cli.color = true,
            "--by" => match args.get(i + 1) {
                Some(v) => match v.parse::<i64>() {
                    Ok(m) => {
                        cli.by = Some(m);
                        i += 1; // the value is consumed with the flag
                    }
                    Err(_) => {
                        return Err(argv_err("--by needs an integer minute: --by <minute>"));
                    }
                },
                None => return Err(argv_err("--by needs a value: --by <minute>")),
            },
            // the Q15 honesty: a value flag with no next slot is the argv
            // error naming the flag, never a silent fallthrough; an EMPTY
            // value is falsy (the frozen truthiness) and reads the same; and
            // a `-`-leading token is a FLAG, not a path (the `--by`
            // exemption below is deliberate: a negative minute is a value).
            "--from-state" => match args.get(i + 1) {
                Some(v) if !v.is_empty() && !v.starts_with('-') => {
                    cli.from_state = Some(v.clone());
                    i += 1;
                }
                Some(v) if v.starts_with('-') => {
                    return Err(argv_err(&format!(
                        "--from-state needs a value, not '{v}': --from-state <log.jsonl>"
                    )))
                }
                _ => return Err(argv_err("--from-state needs a value: --from-state <log.jsonl>")),
            },
            other => {
                // an unknown `-`-word in flag position is the argv error
                // naming it — a typo'd `--usablee` must never silently drop
                // its flag and change the face's semantics (`--` included:
                // there is no end-of-options separator on this surface, the
                // lint-scripts alignment). A lone `-` (len 1) stays a word.
                if other.len() > 1 && other.starts_with('-') {
                    return Err(argv_err(&format!(
                        "unknown flag '{other}' — expected one of: {FLAG_LIST}"
                    )));
                }
                positionals.push(other);
            }
        }
        i += 1;
    }

    // positional 0: the script (an EMPTY argv element is falsy, the frozen
    // truthiness)
    match positionals.first() {
        Some(s) if !s.is_empty() => cli.script = (*s).to_string(),
        _ => {
            return Err(format!(
                "reach needs a script and a command — expected one of: {}\n{SHORT_USAGE}",
                COMMANDS.join(", ")
            ))
        }
    }
    // positional 1: the command — required, one of the six
    let word = match positionals.get(1) {
        Some(w) if !w.is_empty() => *w,
        _ => {
            return Err(format!(
                "reach needs a command — expected one of: {}\n{SHORT_USAGE}",
                COMMANDS.join(", ")
            ))
        }
    };
    // the value commands read positional 2; a missing or EMPTY value is the
    // argv error naming the command (the frozen truthiness, per command)
    let value = |name: &str, shape: &str| -> Result<String, String> {
        match positionals.get(2) {
            Some(v) if !v.is_empty() => Ok((*v).to_string()),
            _ => Err(argv_err(&format!("{name} needs a value: reach <script.txt> {name} {shape}"))),
        }
    };
    cli.command = match word {
        "card" => Command::Card(value("card", "<NAME>")?),
        "all" => Command::All,
        "demand" => Command::Demand(value("demand", "\"<query>\"")?),
        "verb" => Command::Verb(value("verb", "\"<NAME>\"")?),
        "world" => Command::World,
        "graph" => Command::Graph,
        other => {
            return Err(format!(
                "unknown command '{other}' — expected one of: {}\n{SHORT_USAGE}",
                COMMANDS.join(", ")
            ))
        }
    };
    // the grammar takes at most `<script> <COMMAND> [value]` — any further
    // positional is the argv error naming the first extra (`all extra`,
    // `card X Y`), never a silently-ignored word
    let takes_value =
        matches!(cli.command, Command::Card(_) | Command::Demand(_) | Command::Verb(_));
    if let Some(extra) = positionals.get(if takes_value { 3 } else { 2 }) {
        return Err(argv_err(&format!("unexpected argument '{extra}'")));
    }
    Ok(ParseOutcome::Cli(cli))
}

fn run(cli: Cli) -> ExitCode {
    // compile the script; a missing file / compile error is a hard error →
    // exit 1. Only the message line is reproducible
    // (deliberate: stderr text is not part of the stdout freeze). The load
    // hands back its EFFECTIVE dictionary (§4.1: the core dictionary plus
    // the script's `kinds:` declarations) — the demand query parse
    // validates `kind:` terms against it, so a demand on a custom kind
    // parses.
    let dict = core_dictionary();
    let text = match std::fs::read_to_string(&cli.script) {
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
    let CompiledScript { content, state, dict: eff, .. } = compiled;

    // the midgame face: the analysis initial world is the LOG replayed onto
    // the script's initial state (the engine's own fold — §7). A missing or
    // unparsable log and a drifting fold are `Error: ...` exit 1; a log that
    // ENDS the game is the same error — an ended world has nothing left to
    // reach.
    let state = match &cli.from_state {
        None => state,
        Some(log_path) => {
            // the fold re-parses the log's sentences, so it must read the
            // script's LOAD-EFFECTIVE dictionary (declared verbs/kinds
            // included — the same dictionary the playing bin adopted and
            // wrote the log with), not the bare core seed
            let mid = match replay_state(std::path::Path::new(log_path), &state, &eff, &content) {
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

    let styles = Styles::new(cli.color);
    let file: &str = &cli.script;
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let fail = |e: chorus::error::ChorusError| -> ExitCode {
        // an analyze error is fatal → exit 1; the message
        // line is the reproducible part
        eprintln!("Error: {e}");
        ExitCode::from(1)
    };
    // write_one: the text faces' single exit — colorize (identity when
    // --color is absent) then flush. The machine faces (json/graph) write
    // their bytes directly and never pass through here.
    let write_one = |out: &mut dyn Write, buf: &str| -> bool {
        let painted = styles.colorize(buf);
        out.write_all(painted.as_bytes()).is_ok()
    };
    match &cli.command {
        Command::World => {
            // one Analyzer, one world, zero per-card work — the sections are
            // the post-fixpoint facts re-rendered (reach::world_report)
            let r = world_report(&content, &state, &eff);
            let mut buf = String::new();
            world_report::render_report(&mut buf, file, &r);
            let ok = write_one(&mut out, &buf);
            let _ = out.flush();
            return if ok { ExitCode::from(0) } else { ExitCode::from(1) };
        }
        Command::Graph => {
            // the whole-content mint dependency graph as Graphviz DOT
            // (reach::export::graph_dot — deterministic, cmp_utf16-sorted).
            // A machine-readable face: never colorized, even under --color.
            let buf = graph_dot(&content, &state, &eff);
            if out.write_all(buf.as_bytes()).is_err() {
                return ExitCode::from(1);
            }
            let _ = out.flush();
            return ExitCode::from(0);
        }
        Command::Verb(name) => {
            // one dedicated, target-free face (§13.1); --why does not apply
            // to it. An unknown verb name is the analyze error (exit 1,
            // stderr only).
            let analyzer = Analyzer::new(&content, &state, &eff);
            let Some(vr) = analyzer.verb_report(name) else {
                eprintln!("Error: no verb '{name}'");
                return ExitCode::from(1);
            };
            let mut buf = String::new();
            render_verb(&mut buf, file, &vr);
            let ok = write_one(&mut out, &buf);
            let _ = out.flush();
            return if ok { ExitCode::from(0) } else { ExitCode::from(1) };
        }
        _ => {} // the card faces: card / demand / all
    }

    // the card faces share one analyzer discipline: the bound and the replay
    // mark set once, before the first analysis (the facts cache is per
    // host); the analyzer parses the script's load-effective dictionary (a
    // demand query on a declared kind parses against it)
    let make_analyzer = || {
        let mut analyzer = Analyzer::new(&content, &state, &eff);
        if let Some(m) = cli.by {
            analyzer = analyzer.by(m);
        }
        if cli.from_state.is_some() {
            analyzer = analyzer.replayed();
        }
        analyzer
    };

    match &cli.command {
        Command::All => {
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
            let analyzer = make_analyzer();
            if cli.json {
                // the array face: one envelope per name in the walk order,
                // rendered once (the analyze error face is unchanged — the
                // parse error fires on the FIRST card, before any byte is
                // written, exactly like the incremental text walk)
                let mut reports: Vec<chorus::reach::CardReport> = Vec::new();
                let mut blocked: Vec<Vec<SiteBlocker>> = Vec::new();
                for name in &sorted {
                    let r = match analyzer
                        .analyze_card(name, CardOptions { usable: cli.usable, demand: None })
                    {
                        Ok(r) => r,
                        Err(e) => return fail(e),
                    };
                    if cli.why {
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
                            None,
                            cli.usable,
                            r,
                            if cli.why { Some(&blocked[i]) } else { None },
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
                let mut buf = String::new();
                let r = match analyzer
                    .analyze_card(name, CardOptions { usable: cli.usable, demand: None })
                {
                    Ok(r) => r,
                    Err(e) => return fail(e),
                };
                render_report_by(&mut buf, file, name, None, cli.usable, cli.by, &r);
                if cli.why {
                    // the --why section rides AFTER the report — without the
                    // flag this face is byte-identical to the frozen output
                    render_why(&mut buf, file, name, &analyzer.card_blockers(name));
                }
                if cli.cost {
                    render_cost(&mut buf, file, name, &cost_report(&analyzer, name));
                }
                if !write_one(&mut out, &buf) {
                    return ExitCode::from(1);
                }
            }
        }
        command @ (Command::Card(_) | Command::Demand(_)) => {
            // card: the single-card face; demand: the demand match (the
            // analyzer's demand branch runs first, the target is empty)
            let (card, demand) = match command {
                Command::Card(name) => (name.as_str(), None),
                Command::Demand(q) => ("", Some(q.as_str())),
                _ => unreachable!("the match above guards the card faces"),
            };
            let analyzer = make_analyzer();
            let r = match analyzer.analyze_card(
                card,
                CardOptions { usable: cli.usable, demand: demand.map(str::to_string) },
            ) {
                Ok(r) => r,
                Err(e) => return fail(e),
            };
            if cli.json {
                // the single-object face; `--why --json` merges the blocker
                // table into the envelope (render_why's rows, as_str text)
                let blockers = if cli.why { Some(analyzer.card_blockers(card)) } else { None };
                let env = card_export(file, demand, cli.usable, &r, blockers.as_deref());
                let buf = render_json(&env);
                if out.write_all(buf.as_bytes()).is_err() {
                    return ExitCode::from(1);
                }
                let _ = out.flush();
                return ExitCode::from(0);
            }
            let mut buf = String::new();
            render_report_by(&mut buf, file, card, demand, cli.usable, cli.by, &r);
            if cli.why {
                render_why(&mut buf, file, card, &analyzer.card_blockers(card));
            }
            if cli.cost && demand.is_none() {
                // the --cost section rides AFTER --why; under demand it is
                // documented-ignored (the demand match is not costed)
                render_cost(&mut buf, file, card, &cost_report(&analyzer, card));
            }
            let ok = write_one(&mut out, &buf);
            let _ = out.flush();
            return if ok { ExitCode::from(0) } else { ExitCode::from(1) };
        }
        _ => unreachable!("world/graph/verb returned above"),
    }
    let _ = out.flush();
    ExitCode::from(0)
}
