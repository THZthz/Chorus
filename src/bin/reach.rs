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
//!   - the usage first line carries `[--why] [--verb "<NAME>"]` with the
//!     diagnostic faces — the one sanctioned argv-surface byte change of
//!     that wave (the --world precedent), its three usage pins in
//!     tests/session/cli.rs updated in the same change.
//!
//! Diagnostic faces (§13.1):
//!   - `--why` appends one `— why` section per card report (card mode and
//!     --all alike): every live unproven serving site with its owner and the
//!     ladder's first blocker. Without it the output is byte-identical.
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
use chorus::reach::render::{render_verb, render_why};
use chorus::reach::world_report::{self, world_report};
use chorus::reach::{render_report, Analyzer, CardOptions};

fn main() -> ExitCode {
    // argv parsing — frozen surface, byte for byte
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usable = args.iter().any(|a| a == "--usable");
    let why = args.iter().any(|a| a == "--why");
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
    let mut rest: Vec<String> = Vec::new();
    let mut world = false;
    let mut i = 0usize;
    while i < args.len() {
        if args[i] == "--usable" || args[i] == "--why" {
            i += 1;
            continue;
        }
        if args[i] == "--world" {
            world = true;
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
        rest.push(args[i].clone());
        i += 1;
    }
    let file: Option<&String> = rest.first();
    let card: Option<&String> = rest.get(1);
    let all = rest.iter().any(|a| a == "--all");
    // the usage gate: a present file AND (a card OR --all OR a demand OR
    // --world OR a verb), with the frozen string truthiness; the text (two
    // lines) is exact. An EMPTY argv element is falsy for the file slot and
    // the card slot alike. `reach --world` with no script falls through
    // here — the dashboard needs the file too.
    let file = match file {
        Some(f)
            if !f.is_empty()
                && (all
                    || world
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

    // the branch ORDER is the frozen behavior: --world wins over everything
    // (the dashboard is target-free — no card, no --all walk, no demand);
    // --all wins over an explicit card name; a lone --demand reports once
    // (the target is ignored by the analyzer under demand).
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
        let analyzer = Analyzer::new(&content, &state);
        for name in &sorted {
            // report(n) passes the CAPTURED demand on — `--all --demand q`
            // renders the demand report once per name
            let r =
                match analyzer.analyze_card(name, CardOptions { usable, demand: demand.clone() }) {
                    Ok(r) => r,
                    Err(e) => return fail(e),
                };
            let mut buf = String::new();
            render_report(&mut buf, file, name, demand.as_deref(), usable, &r);
            if why {
                // the --why section rides AFTER the report — without the flag
                // this face is byte-identical to the frozen output
                render_why(&mut buf, file, name, &analyzer.card_blockers(name));
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
        let analyzer = Analyzer::new(&content, &state);
        let r = match analyzer.analyze_card(card, CardOptions { usable, demand: demand.clone() }) {
            Ok(r) => r,
            Err(e) => return fail(e),
        };
        let mut buf = String::new();
        render_report(&mut buf, file, card, demand.as_deref(), usable, &r);
        if why {
            render_why(&mut buf, file, card, &analyzer.card_blockers(card));
        }
        if out.write_all(buf.as_bytes()).is_err() {
            return ExitCode::from(1);
        }
    }
    let _ = out.flush();
    ExitCode::from(0)
}
