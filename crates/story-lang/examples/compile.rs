//! Manual compile driver: `cargo run -p story-lang --example compile -- <file.sw>`
//! prints diagnostics for one root file.

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: compile <file.sw>");
        std::process::exit(2);
    };
    let out = story_lang::compile_file(Path::new(path));
    let mut errors = 0usize;
    for d in &out.diagnostics {
        println!("{d}");
        if d.is_error() {
            errors += 1;
        }
    }
    println!("{} diagnostic(s), {errors} error(s)", out.diagnostics.len());
    if errors > 0 || out.content.is_none() {
        std::process::exit(1);
    }
}
