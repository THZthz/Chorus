//! JS semantics infrastructure shared by every ported module (plan §2.2).
//! W0 skeleton: `cmp_utf16` / the test-side LCG / `ChorusError` /
//! `JS_WS_CLASS`. `jsjson`, `ordinary_own_property_keys` and `js_num_parse`
//! arrive in W1 (plan §4-W1).

use std::cmp::Ordering;
use std::fmt;

/// ts: src/core/ref.ts:47-49 (sortRefs), src/core/types.ts:225-230
/// (relationKey), tools/reach-cli.ts:73 (`--all` list) — plan §3.6.
/// JS compares strings as UTF-16 code-unit sequences; Rust `str::cmp` is
/// byte order, which diverges on non-BMP characters (a surrogate pair starts
/// at 0xD800, below every single unit >= 0xE000). **Never** substitute
/// `str::cmp` (verification CORE-05).
#[must_use]
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// QUIRK(CORE-08(1)/Q8): JS `\s` includes `\uFEFF` and excludes `\u{85}`;
/// the regex crate's `\s` (Unicode White_Space) is exactly the reverse. All
/// ported regexes must spell whitespace with this explicit class (plan §3.8);
/// `\S` is the negation of the same class, never the crate's `\S`.
pub const JS_WS_CLASS: &str =
    "[\\t\\n\\v\\f\\r \\u{a0}\\u{1680}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{202f}\\u{205f}\\u{3000}\\u{feff}]";

/// The same JS whitespace set as a code-unit predicate — the hand-written
/// lexer scanners (W1) run on `Vec<u16>` code units where regex classes
/// cannot reach (plan §2.3 / PR2-08). ts: src/core/lexer.ts:26 (`/\s/`).
#[must_use]
pub fn js_is_ws(c: u16) -> bool {
    matches!(
        c,
        0x09 | 0x0a | 0x0b | 0x0c | 0x0d | 0x20 | 0x00a0 | 0x1680
        | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff
    )
}

/// ts: (plan §2.5) every TS `throw new Error(...)` becomes exactly one Rust
/// construction site `ChorusError::new(format!(...))` with the template
/// copied character-for-character. Deliberately a plain message, NOT an enum
/// of variants — the variants + Display double maintenance surface is where
/// messages drift (§2.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChorusError {
    pub msg: String,
}

impl ChorusError {
    pub fn new(msg: impl Into<String>) -> Self {
        ChorusError { msg: msg.into() }
    }
}

impl fmt::Display for ChorusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl std::error::Error for ChorusError {}

pub type Result<T> = std::result::Result<T, ChorusError>;

/// ts: test/reach-fuzz-lib.ts:36-38 — the deterministic test-side LCG,
/// `s = (Math.imul(s, 1664525) + 1013904223) >>> 0`, identical in
/// test/reach-scale-lib.ts:28-31 (`lcg`) and inlined in
/// test/explorer-lib.ts:276-277 (plan §3.12, REACH-14). u32 wrapping
/// arithmetic is bit-exact for `Math.imul` + `>>>0`.
///
/// The three TS seed entries (TEST-04), kept one-to-one:
/// - fuzz:  `Lcg::new(seed)` — XOR 0x9e3779b9 (reach-fuzz-lib.ts:37)
/// - scale: `Lcg::new(0xc5)` (reach-scale-lib.ts:33)
/// - explorer: `Lcg::from_raw_state(0x4e494748)` — 'NIGH', seeded as the raw
///   constant with **no XOR** (explorer-lib.ts:276).
#[derive(Debug, Clone)]
pub struct Lcg {
    s: u32,
}

impl Lcg {
    #[must_use]
    pub fn new(seed: u32) -> Self {
        Lcg { s: seed ^ 0x9e37_79b9 }
    }

    #[must_use]
    pub fn from_raw_state(s: u32) -> Self {
        Lcg { s }
    }

    #[must_use]
    pub fn next_u32(&mut self) -> u32 {
        self.s = self.s.wrapping_mul(1664525).wrapping_add(1013904223);
        self.s
    }

    /// `((s = …) >>> 0) / 0x100000000` — u32→f64 is exact and the division by
    /// 2^32 is a power-of-two scaling, so the f64 result is bit-identical to
    /// TS's (plan §3.12: kept in f64, never integerized).
    #[must_use]
    pub fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4294967296.0
    }

    /// `Math.floor(r() * n)` — computed in f64 exactly as TS (plan §3.12).
    #[must_use]
    pub fn next_int(&mut self, n: usize) -> usize {
        (self.next_f64() * n as f64).floor() as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmp_utf16_orders_a_surrogate_pair_before_high_bmp() {
        // U+10000 encodes as the pair D800 DC00; U+E000 is one unit 0xE000.
        // UTF-16 order puts "\u{10000}" BEFORE "\u{E000}"; Rust byte order
        // says the opposite — the exact trap plan §3.6 forbids.
        assert_eq!(cmp_utf16("\u{10000}", "\u{E000}"), Ordering::Less);
        assert_eq!(cmp_utf16("\u{E000}", "\u{10000}"), Ordering::Greater);
        assert_eq!(cmp_utf16("abc", "abc"), Ordering::Equal);
        assert_eq!(cmp_utf16("ab", "abc"), Ordering::Less); // prefix
        assert_eq!(cmp_utf16("AB", "B"), Ordering::Less);
    }

    #[test]
    fn js_ws_class_pins_the_q8_set_exactly() {
        let re = regex::Regex::new(JS_WS_CLASS).unwrap();
        assert!(re.is_match("\u{feff}")); // JS \s includes the BOM…
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
            assert!(js_is_ws(u), "u+{u:04x} must be JS whitespace");
        }
        assert!(!js_is_ws(0x85)); // NEL: White_Space yes, JS \s no
        assert!(!js_is_ws(0x180e)); // Mongolian vowel separator: neither
    }

    #[test]
    fn lcg_reproduces_the_ts_sequence_bit_for_bit() {
        // fuzz entry, seed 0 — states cross-checked against node
        // (Math.imul + >>>0 discipline), 2026-10-14
        let mut fuzz = Lcg::new(0);
        assert_eq!(fuzz.next_u32(), 1120982980);
        assert_eq!(fuzz.next_u32(), 3911581779);
        assert_eq!(fuzz.next_u32(), 1066994070);
        assert_eq!(fuzz.next_u32(), 1621898237);
        // explorer entry: 'NIGH' as the RAW state, no XOR (explorer-lib.ts:276)
        let mut nigh = Lcg::from_raw_state(0x4e49_4748);
        assert_eq!(nigh.next_f64(), 0.3694802538957447);
        assert_eq!(nigh.next_f64(), 0.35568378726020455);
        // scale entry: lcg(0xc5) (reach-scale-lib.ts:33)
        let mut scale = Lcg::new(0xc5);
        assert_eq!(scale.next_f64(), 0.23735849070362747);
        // Math.floor(r() * n) stays in f64
        assert_eq!(scale.next_f64(), 0.3778064283542335);
        assert_eq!(scale.next_int(10), 4);
    }
}
