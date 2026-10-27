//! The `cs2sw` binary: convert a CS core tree to one `.sw` file, compile-check
//! the output in-process, and optionally write the gap report.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let mut root: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut report: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--report" => {
                i += 1;
                match args.get(i) {
                    Some(p) => report = Some(PathBuf::from(p)),
                    None => {
                        eprintln!("--report takes a path");
                        return ExitCode::from(2);
                    }
                }
            }
            other if root.is_none() => root = Some(PathBuf::from(other)),
            other if out.is_none() => out = Some(PathBuf::from(other)),
            other => {
                eprintln!("unexpected argument `{other}`");
                eprintln!("usage: cs2sw <cs-core-dir> <out.sw> [--report <path>]");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let (Some(root), Some(out)) = (root, out) else {
        eprintln!("usage: cs2sw <cs-core-dir> <out.sw> [--report <path>]");
        return ExitCode::from(2);
    };

    let (conversion, diags) = cs2sw::run(&root);

    let mut report_path = None;
    if let Some(report) = report {
        let md = cs2sw::report::render_markdown(&conversion);
        let js = cs2sw::report::render_json(&conversion);
        if let Err(e) = std::fs::write(&report, format!("{md}\n{js}")) {
            eprintln!("cannot write report: {e}");
            return ExitCode::from(1);
        }
        report_path = Some(report);
    }
    if let Err(e) = std::fs::write(&out, &conversion.sw) {
        eprintln!("cannot write output: {e}");
        return ExitCode::from(1);
    }

    let narrative_path = {
        let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
        match out.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                parent.join(format!("{stem}.narrative.json"))
            }
            _ => PathBuf::from(format!("{stem}.narrative.json")),
        }
    };
    if let Err(e) = conversion.catalog.save_to_file(&narrative_path) {
        eprintln!("cannot write narrative catalog: {e}");
        return ExitCode::from(1);
    }

    let s = &conversion.stats;
    println!("== cs2sw conversion summary ==");
    println!(
        "elements in:        {} (aspects without cards: {})",
        s.elements_in, s.aspect_elements
    );
    println!("cards out:          {}", s.cards_out);
    println!("aspect keys:        {}", s.aspect_keys);
    println!("recipes in/out:     {} / {}", s.recipes_in, s.recipes_out);
    println!("slots out:          {}", s.slots_out);
    println!("verbs out:          {}", s.verbs_out);
    println!("decks out:          {}", s.decks_out);
    println!("endings out:        {}", s.endings_out);
    println!(
        "output:             {} lines, {} bytes",
        s.output_lines,
        conversion.sw.len()
    );
    println!("named gaps:         {}", conversion.gaps.rows.len());
    println!(
        "narrative catalog:  {} cards, {} recipes, {} verbs",
        conversion.catalog.cards.len(),
        conversion.catalog.recipes.len(),
        conversion.catalog.verbs.len()
    );
    let errors: Vec<_> = diags.iter().filter(|d| d.is_error()).collect();
    let warnings: Vec<_> = diags.iter().filter(|d| !d.is_error()).collect();
    println!(
        "compile diagnostics: {} error(s), {} warning(s)",
        errors.len(),
        warnings.len()
    );
    for d in errors.iter().take(40) {
        println!("  {d}");
    }
    if errors.len() > 40 {
        println!("  ... and {} more", errors.len() - 40);
    }
    if errors.is_empty() {
        println!("wrote {}", out.display());
        println!("narrative {}", narrative_path.display());
        if let Some(r) = report_path {
            println!("report {}", r.display());
        }
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
