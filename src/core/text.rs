//! The engine's frozen text classes: the whitespace character set (Q8),
//! the UTF-16 code-unit collation (B-19) and the ASCII-case suggestion
//! collation (B-21). Each class is a frozen choice of this engine — the
//! `str`-native equivalents (`char::is_whitespace`, `str::cmp`) are NOT
//! drop-in substitutes; see each item.

use std::cmp::Ordering;

/// The engine's frozen collation (B-19): strings order as UTF-16 code-unit
/// sequences. Rust `str::cmp` is byte order, which diverges exactly on
/// non-BMP characters (a surrogate pair starts at 0xD800, below every
/// single unit >= 0xE000). **Never** substitute `str::cmp` — ref sorting,
/// relation keys and reach lists all require this order.
#[must_use]
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// The engine's frozen whitespace class (Q8): U+FEFF IS whitespace and
/// U+0085 is NOT; the regex crate's `\s` (Unicode White_Space) is exactly
/// the reverse on those two. All regexes must spell whitespace with this
/// explicit class; `\S` is the negation of the same class, never the
/// crate's `\S`.
pub const DSL_WS_CLASS: &str = "[\\t\\n\\v\\f\\r \\u{a0}\\u{1680}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{202f}\\u{205f}\\u{3000}\\u{feff}]";

/// The negation of [`DSL_WS_CLASS`] — the class every regex `\S` must spell
/// (Q8). The regex crate's `\S` is the complement of White_Space: it
/// EXCLUDES U+FEFF and INCLUDES U+0085 — exactly the reverse of this class.
pub const DSL_NON_WS_CLASS: &str = "[^\\t\\n\\v\\f\\r \\u{a0}\\u{1680}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{202f}\\u{205f}\\u{3000}\\u{feff}]";

/// The same frozen whitespace set as a code-unit predicate — the
/// hand-written lexer scanners run on `Vec<u16>` code units where regex
/// classes cannot reach.
#[must_use]
pub fn is_dsl_ws(c: u16) -> bool {
    matches!(
        c,
        0x09 | 0x0a | 0x0b | 0x0c | 0x0d | 0x20 | 0x00a0 | 0x1680 | 0x2000
            ..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff
    )
}

/// The engine's frozen trim set: whitespace plus line terminators —
/// includes `\u{FEFF}`, excludes `\u{85}`. Rust's `str::trim`
/// (`char::is_whitespace`) is exactly the reverse on those two, so every
/// `trim()` before a whitespace split must use this predicate (Q8). Same
/// set as [`is_dsl_ws`], on a `char`.
#[must_use]
pub fn is_dsl_ws_char(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// Trim over the engine's frozen whitespace set.
#[must_use]
pub fn dsl_trim(s: &str) -> &str {
    s.trim_matches(is_dsl_ws_char)
}

/// `split(/\s+/)` over the DSL whitespace class: runs of whitespace split;
/// the inputs are always trimmed or `\S`-anchored, so filtering the empty
/// pieces yields exactly the canonical token sequence.
#[must_use]
pub fn split_dsl_ws(s: &str) -> Vec<&str> {
    s.split(is_dsl_ws_char).filter(|p| !p.is_empty()).collect()
}

/// The engine's frozen suggestion-pool collation (B-21): ASCII-lowercase
/// both sides, then Rust byte order (`str::cmp`). The repo's
/// name domain is uppercase-ASCII entity names over lowercase English words,
/// where the expected order matches ICU; the approximation surface is pinned
/// by the `suggestions()` tests.
/// ICU collation stays OUT of the dependency set; revisit only if
/// non-ASCII names ever appear (a separately-tracked topic).
#[must_use]
pub fn cmp_ascii_ci(a: &str, b: &str) -> Ordering {
    a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmp_utf16_orders_a_surrogate_pair_before_high_bmp() {
        // U+10000 encodes as the pair D800 DC00; U+E000 is one unit 0xE000.
        // UTF-16 order puts "\u{10000}" BEFORE "\u{E000}"; Rust byte order
        // says the opposite — the exact trap `cmp_utf16` exists for.
        assert_eq!(cmp_utf16("\u{10000}", "\u{E000}"), Ordering::Less);
        assert_eq!(cmp_utf16("\u{E000}", "\u{10000}"), Ordering::Greater);
        assert_eq!(cmp_utf16("abc", "abc"), Ordering::Equal);
        assert_eq!(cmp_utf16("ab", "abc"), Ordering::Less); // prefix
        assert_eq!(cmp_utf16("AB", "B"), Ordering::Less);
    }

    #[test]
    fn dsl_ws_class_pins_the_q8_set_exactly() {
        let re = regex::Regex::new(DSL_WS_CLASS).unwrap();
        assert!(re.is_match("\u{feff}")); // the frozen class includes the BOM…
        assert!(!re.is_match("\u{85}")); // …but excludes NEL — Rust \s is the reverse
        assert!(re.is_match("\u{00a0}"));
        assert!(re.is_match("\u{1680}"));
        assert!(re.is_match("\u{2009}")); // inside the 2000..200a run
        assert!(re.is_match("\u{2028}"));
        assert!(re.is_match("\u{202f}"));
        assert!(re.is_match("\u{205f}"));
        assert!(re.is_match("\u{3000}"));
        assert!(re.is_match("\t"));
        assert!(re.is_match("\u{000b}")); // \v
        assert!(!re.is_match("x"));
        // the code-unit predicate agrees with the class on the edges
        for u in [
            0x09u16, 0x0a, 0x0b, 0x0c, 0x0d, 0x20, 0x00a0, 0x1680, 0x2000, 0x200a, 0x2028, 0x2029,
            0x202f, 0x205f, 0x3000, 0xfeff,
        ] {
            assert!(is_dsl_ws(u), "u+{u:04x} must be engine whitespace");
        }
        assert!(!is_dsl_ws(0x85)); // NEL: White_Space yes, the frozen class no
        assert!(!is_dsl_ws(0x180e)); // Mongolian vowel separator: neither
    }

    #[test]
    fn dsl_trim_uses_the_dsl_whitespace_set() {
        // \u{feff} IS trim-able, \u{85} is not — Rust str::trim is reversed
        assert_eq!(dsl_trim("\u{feff}abc "), "abc");
        assert_eq!(dsl_trim("\u{85}abc\u{85}"), "\u{85}abc\u{85}");
        assert_eq!(dsl_trim("  a\tb\u{2028}"), "a\tb");
    }

    #[test]
    fn dsl_non_ws_class_is_the_exact_complement_of_dsl_ws_class() {
        // the frozen pair must stay complements on every edge unit — the
        // deciding ones are U+FEFF (whitespace here, not for Rust \s) and
        // U+0085 (the reverse); U+180E is in neither class
        let ws = regex::Regex::new(DSL_WS_CLASS).unwrap();
        let non_ws = regex::Regex::new(DSL_NON_WS_CLASS).unwrap();
        for u in [
            0x09u16, 0x0b, 0x20, 0x85, 0xa0, 0x1680, 0x180e, 0x2000, 0x200a, 0x200b, 0x2028,
            0x202f, 0x205f, 0x3000, 0xfeff,
        ] {
            let s = char::from_u32(u32::from(u)).unwrap().to_string();
            assert_eq!(ws.is_match(&s), !non_ws.is_match(&s), "u+{u:04x} must classify oppositely");
        }
    }
}
