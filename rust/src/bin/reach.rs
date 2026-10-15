//! Card-reachability CLI — ts: tools/reach-cli.ts, ported byte for byte.
//! "can the player obtain card X?" over a bundled script. Both semantics:
//! default is existence; --usable also requires the card to be bindable into
//! some committable verb slot.
//!
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt THE_WAX_CYLINDER [--usable]
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt --all
//!   cargo run --bin reach -- scripts/the-nightjar-flyer.txt --demand "aspect:secret >= 3"
//!
//! Frozen surface (verification 03 CLI-01/CLI-02, plan §4-W5):
//!   - stdout text incl. renderPath's 4-space indent steps, `path i/n:`,
//!     `verdict: X (existence: y)` — golden-frozen via tools/golden-dump.ts;
//!   - the `--all` name set = prototypes ∪ LIVE initial entities ∪
//!     deck.order (NOT defaultcard), sorted by UTF-16 code-unit order;
//!   - Q15/CLI-01 current-shape quirks: a trailing `--demand` silently loses
//!     its value (argv[demandIdx+1] undefined), a missing card+all+demand
//!     combination exits 2 with the usage text, and a bad demand query is a
//!     compile-time analyze error exiting 1 (the TS prints a JS stack trace;
//!     the message line is the frozen part).

use std::io::Write as _;
use std::process::ExitCode;

use chorus::content::script::{compile_script, CompiledScript};
use chorus::core::dict::core_dictionary;
use chorus::js::cmp_utf16;
use chorus::reach::{render_report, Analyzer, CardOptions};

fn main() -> ExitCode {
    // ts: tools/reach-cli.ts:17-33 — argv parsing replicated verbatim
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usable = args.iter().any(|a| a == "--usable");
    // ts: reach-cli.ts:19-20 — `args[demandIdx + 1]` blind read; a trailing
    // --demand yields undefined → demand silently vanishes (Q15/CLI-01)
    let demand_idx = args.iter().position(|a| a == "--demand");
    let demand: Option<String> = demand_idx.and_then(|i| args.get(i + 1).cloned());
    let mut rest: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == "--usable" {
            i += 1;
            continue;
        }
        if args[i] == "--demand" {
            i += 2; // ts: `i++; continue` of the outer for-loop
            continue;
        }
        rest.push(args[i].clone());
        i += 1;
    }
    let file: Option<&String> = rest.first();
    let card: Option<&String> = rest.get(1);
    let all = rest.iter().any(|a| a == "--all");
    // ts: reach-cli.ts:30-33 — the usage gate `!file || (!card && !all &&
    // !demand)`; the text (two lines) is exact. JS string truthiness: an EMPTY
    // argv element is falsy for the file slot and the card slot alike.
    let file = match file {
        Some(f)
            if !f.is_empty()
                && (all || card.is_some_and(|c| !c.is_empty()) || demand.is_some()) =>
        {
            f
        }
        _ => {
            eprintln!(
                "usage: tsx tools/reach-cli.ts <script.txt> <CARD_NAME> [--usable] [--demand \"<query>\"]\n       tsx tools/reach-cli.ts <script.txt> --all"
            );
            return ExitCode::from(2);
        }
    };

    // ts: reach-cli.ts:35-36 — compileScript(readFileSync(file, 'utf8'), dict);
    // a missing file / compile error is an uncaught throw → exit 1. The TS
    // prints a JS stack after the message; only the message line is
    // reproducible (recorded divergence — stderr text, not the stdout freeze).
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

    // ts: reach-cli.ts:69-76 — the branch ORDER is the frozen behavior: --all
    // wins over an explicit card name; a lone --demand reports once (card
    // undefined in the TS, ignored by the analyzer under demand).
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    let fail = |e: chorus::js::ChorusError| -> ExitCode {
        // ts: the analyzeCard throw escapes uncaught → exit 1; the message
        // line is the reproducible part
        eprintln!("Error: {e}");
        ExitCode::from(1)
    };
    if all {
        // ts: reach-cli.ts:70-73 — prototypes ∪ live initial entities ∪
        // deck.order (CLI-02 correction: NOT defaultcard); insertion set,
        // then UTF-16 code-unit sort (REACH-05 must)
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
        let analyzer = Analyzer::new(&content, &state);
        for name in &sorted {
            // ts: report(n) passes the CAPTURED demand on — `--all --demand q`
            // renders the demand report once per name (the closure shape is
            // the behavior)
            let r = match analyzer.analyze_card(name, CardOptions { usable, demand: demand.clone() })
            {
                Ok(r) => r,
                Err(e) => return fail(e),
            };
            let mut buf = String::new();
            render_report(&mut buf, file, name, demand.as_deref(), usable, &r);
            if out.write_all(buf.as_bytes()).is_err() {
                return ExitCode::from(1);
            }
        }
    } else {
        // ts: reach-cli.ts:74-76 — `report(card!)`: card may be absent when
        // only --demand is given; the analyzer ignores the target under
        // demand (analyzeCard's demandQ branch runs first)
        let card = card.map(String::as_str).unwrap_or("");
        let analyzer = Analyzer::new(&content, &state);
        let r = match analyzer.analyze_card(card, CardOptions { usable, demand: demand.clone() }) {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        let mut buf = String::new();
        render_report(&mut buf, file, card, demand.as_deref(), usable, &r);
        if out.write_all(buf.as_bytes()).is_err() {
            return ExitCode::from(1);
        }
    }
    let _ = out.flush();
    ExitCode::from(0)
}
