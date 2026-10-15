//! grep gate: `rand::` must not
//! appear anywhere under src/ — the counter RNG in core/dice.rs is the
//! ONLY randomness (law 5). The gate covers
//! `rand::` over the WHOLE tree, and the
//! literal `Math.random` absent likewise. Both, like the
//! engine-path unwrap/expect gate below, are TEXTUAL scans — the honest
//! limitations are registered on each test (defanged comments/strings, no
//! syntax tree; a determined adversary can defeat a grep, and the gate only
//! ever promised to catch accidents).

use std::fs;
use std::path::{Path, PathBuf};

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return, // a module dir that has not landed yet (engine/ is W3)
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_rs_files(&p, out);
        } else if p.extension().is_some_and(|ext| ext == "rs") {
            out.push(p);
        }
    }
}

fn rust_sources() -> Vec<PathBuf> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    files.sort();
    files
}

#[test]
fn no_rand_crate_anywhere_in_rust_sources() {
    // the early skeleton scanned core/ + engine/ only; the final gate
    // is the whole tree (bins included — a CLI reaching for `rand` would be
    // exactly the law-5 accident this exists to catch).
    let offenders: Vec<String> = rust_sources()
        .iter()
        .filter(|f| {
            fs::read_to_string(f)
                .map(|text| text.contains("rand::"))
                .unwrap_or(false)
        })
        .map(|f| f.display().to_string())
        .collect();
    assert_eq!(
        offenders,
        Vec::<String>::new(),
        "rand:: must not appear in rust sources: {offenders:?}"
    );
}

/// the gate's literal form: grep the sources for
/// `Math.random`; the Rust
/// tree must not carry the literal either (outside comments — doc prose
/// ABOUT the gate is defanged, see [`blank_comments_and_strings`]).
#[test]
fn no_math_random_literal_anywhere_in_rust_sources() {
    let offenders: Vec<String> = rust_sources()
        .iter()
        .filter_map(|f| {
            let text = fs::read_to_string(f).ok()?;
            let defanged = blank_comments_and_strings(&text);
            defanged
                .contains("Math.random")
                .then(|| f.display().to_string())
        })
        .collect();
    assert_eq!(
        offenders,
        Vec::<String>::new(),
        "Math.random must not appear in rust sources (RNG is core/dice.rs's counter only): {offenders:?}"
    );
}

/// The discardable-TUI zero-residue check: the
/// ink/react component layer was ruled out of scope and no
/// equivalent was ever written — this is the standing check that it stays
/// that way. Scanned after comment/string blanking, so the doc comments
/// SAYING "the ink/react TUI is absent" do not trip it, and the reach
/// CLI's FROZEN usage text (a string literal)
/// is defanged too.
#[test]
fn no_tui_ecosystem_residue_in_rust_sources() {
    const RESIDUE: [&str; 5] = ["react", "jsx", "useState", "useEffect", "ink!"];
    let offenders: Vec<String> = rust_sources()
        .iter()
        .filter_map(|f| {
            let text = fs::read_to_string(f).ok()?;
            let defanged = blank_comments_and_strings(&text);
            let hits: Vec<&str> = RESIDUE
                .iter()
                .copied()
                .filter(|tok| defanged.contains(tok))
                .collect();
            (!hits.is_empty()).then(|| format!("{}: {hits:?}", f.display()))
        })
        .collect();
    assert_eq!(
        offenders,
        Vec::<String>::new(),
        "terminal-UI framework residue found in rust sources: {offenders:?}"
    );
}

// ---------------------------------------------------------------------------
// Engine-path panic gate (plan rule 11): `unwrap`/`expect` are forbidden on
// every non-test path under src/ (test code is exempt) — a TEXTUAL
// scan with the documented limitations below.
// ---------------------------------------------------------------------------

/// Blank out comment and string/char-literal CONTENTS while preserving every
/// newline (so offender line numbers map back to the original file) and all
/// structural characters outside those regions. This defanges braces inside
/// test strings before the `#[cfg(test)]` cut below, and prevents message
/// text like `"…unwrap…"` from triggering the scan.
fn blank_comments_and_strings(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = vec![b' '; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        // line comment
        if c == b'/' && bytes.get(i + 1) == Some(&b'/') {
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue; // keep the newline itself
        }
        // block comment (non-nested approximation)
        if c == b'/' && bytes.get(i + 1) == Some(&b'*') {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                if bytes[i] == b'\n' {
                    out[i] = b'\n';
                }
                i += 1;
            }
            i = (i + 2).min(bytes.len());
            continue;
        }
        // raw string: `r` + `#`* + `"` … closing `"` + the same `#`* — NO
        // escape processing inside (this is the shape every regex literal
        // in src/ uses). A `b`/`c` prefix needs no special case: `b"…"`
        // escapes like a normal string and `c"…"` is not Rust.
        if c == b'r' {
            let mut hashes = 0usize;
            let mut j = i + 1;
            while bytes.get(j) == Some(&b'#') {
                hashes += 1;
                j += 1;
            }
            if bytes.get(j) == Some(&b'"') {
                let closer: Vec<u8> = {
                    let mut v = vec![b'"'];
                    v.extend(std::iter::repeat_n(b'#', hashes));
                    v
                };
                let mut k = j + 1;
                while k + closer.len() <= bytes.len()
                    && &bytes[k..k + closer.len()] != closer.as_slice()
                {
                    if bytes[k] == b'\n' {
                        out[k] = b'\n';
                    }
                    k += 1;
                }
                i = (k + closer.len()).min(bytes.len());
                continue;
            }
            // plain identifier (`r` as a name, `ret`, …) — normal text
            out[i] = c;
            i += 1;
            continue;
        }
        // normal string with escapes
        if c == b'"' {
            i += 1;
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' {
                    i += 1; // the escaped byte (kept blank, incl. \" )
                }
                if bytes.get(i) == Some(&b'\n') {
                    out[i] = b'\n';
                }
                i += 1;
            }
            i += 1; // closing quote
            continue;
        }
        // char literal vs lifetime: `'\n'`, `'}'`, `'a'` are literals;
        // `'a>` (a lifetime) is not — require the closing quote within 2
        // chars or after an escape
        if c == b'\'' {
            if bytes.get(i + 1) == Some(&b'\\') {
                // escape form: skip to the closing quote
                let mut k = i + 2;
                while k < bytes.len() && bytes[k] != b'\'' {
                    k += 1;
                }
                i = k + 1;
                continue;
            }
            if bytes.get(i + 2) == Some(&b'\'') {
                i += 3;
                continue;
            }
            // lifetime like `'a` — normal text
            out[i] = c;
            i += 1;
            continue;
        }
        out[i] = c;
        i += 1;
    }
    String::from_utf8(out).unwrap_or_default()
}

/// Remove every `#[cfg(test)]`-gated item: the attribute plus the whole
/// brace-matched `mod … { … }` body (or up to the `;` of a file-module
/// declaration). Runs AFTER blanking, so braces inside test strings cannot
/// unbalance the match. Input must be the blanked text.
fn cut_cfg_test_items(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut kept: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if text[i..].starts_with("#[cfg(test)]") {
            // find the item this attribute annotates, skipping whitespace
            // and any further attributes
            let mut j = i + "#[cfg(test)]".len();
            loop {
                while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                    j += 1;
                }
                if bytes.get(j) == Some(&b'#') {
                    match text[j..].find(']') {
                        Some(end) => {
                            j += end + 1;
                            continue;
                        }
                        None => break,
                    }
                }
                break;
            }
            if text[j..].starts_with("mod") {
                match bytes[j..].iter().position(|b| *b == b'{' || *b == b';') {
                    Some(rel_brace) if bytes[j + rel_brace] == b'{' => {
                        let b = j + rel_brace;
                        let mut depth: i64 = 0;
                        let mut k = b;
                        loop {
                            match bytes[k] {
                                b'{' => depth += 1,
                                b'}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            k += 1;
                        }
                        i = k + 1; // drop the whole test module
                        continue;
                    }
                    _ => {
                        i = j; // `mod x;` — drop attribute+item conservatively from `mod`
                        while i < bytes.len() && bytes[i] != b';' {
                            i += 1;
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }
        kept.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(kept).unwrap_or_default()
}

#[test]
fn no_unwrap_or_expect_outside_cfg_test_paths_under_src() {
    // plan rule 11: engine paths forbid `unwrap`/`expect`; test code
    // (#[cfg(test)] modules in src, everything under tests/) is exempt.
    // Textual approximation — documented limitations: (1) a call split
    // across lines (`.unwrap`\n`()`) would slip; (2) block comments are
    // treated as non-nested; (3) pathological lifetimes cannot be confused
    // with char literals in valid Rust. The gate keeps the mechanism TEXTUAL
    // (upgrading to a syntax tree would need a new dependency — rule 6) and
    // registers the limitations instead; the rule itself is unchanged.
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    files.sort();
    assert!(!files.is_empty(), "gate found no sources — path broken?");
    let offenders: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let text = fs::read_to_string(f).ok()?;
            let defanged = cut_cfg_test_items(&blank_comments_and_strings(&text));
            let mut hits: Vec<String> = Vec::new();
            for (ln, line) in defanged.lines().enumerate() {
                // `.expect(` needs one exclusion: predicate.rs's
                // `PredParser::expect` is the parser's OWN grammar method
                // (predicate.rs:330, returns Result — it never panics), and
                // its receiver is always exactly `self`. The exclusion is
                // the exact text `self.expect(`; any other receiver
                // (`x.expect(`, `self.f.expect(`) still trips the gate.
                // Limitation: a std expect written literally as
                // `self.expect(` would slip — none exists.
                let mut rest = line;
                let mut bad = rest.contains(".unwrap()");
                while let Some(pos) = rest.find(".expect(") {
                    if rest[..pos].ends_with("self") {
                        rest = &rest[pos + 1..];
                        continue;
                    }
                    bad = true;
                    break;
                }
                if bad {
                    hits.push(format!("  {}:{}: {}", f.display(), ln + 1, line.trim()));
                }
            }
            if hits.is_empty() {
                None
            } else {
                Some(hits.join("\n"))
            }
        })
        .collect();
    assert_eq!(
        offenders,
        Vec::<String>::new(),
        "unwrap/expect found on non-test src paths:\n{}",
        offenders.join("\n")
    );
}
