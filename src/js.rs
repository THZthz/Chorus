//! JS semantics infrastructure shared by every module:
//! `cmp_utf16`, the test-side LCG, `ChorusError`, the JS whitespace/word
//! classes, JS number/trim semantics, `static_regex`,
//! `ordinary_own_property_keys` / `clone_map_in_js_order`, and `jsjson` —
//! the `JSON.stringify` clone.

use std::cmp::Ordering;
use std::fmt;
use std::sync::OnceLock;

use regex::Regex;

/// JS compares strings as UTF-16 code-unit sequences; Rust `str::cmp` is
/// byte order, which diverges on non-BMP characters (a surrogate pair starts
/// at 0xD800, below every single unit >= 0xE000). **Never** substitute
/// `str::cmp` — ref sorting, relation keys and reach lists all require JS
/// order.
#[must_use]
pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// QUIRK(Q8): JS `\s` includes `\uFEFF` and excludes `\u{85}`;
/// the regex crate's `\s` (Unicode White_Space) is exactly the reverse. All
/// regexes must spell whitespace with this explicit class;
/// `\S` is the negation of the same class, never the crate's `\S`.
pub const JS_WS_CLASS: &str =
    "[\\t\\n\\v\\f\\r \\u{a0}\\u{1680}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{202f}\\u{205f}\\u{3000}\\u{feff}]";

/// The negation of [`JS_WS_CLASS`] — JS `\S` (Q8). The regex crate's `\S` is
/// the complement of White_Space: it EXCLUDES U+0085 and INCLUDES U+FEFF —
/// exactly the reverse of JS. Every `\S` must spell this class.
pub const JS_NON_WS_CLASS: &str =
    "[^\\t\\n\\v\\f\\r \\u{a0}\\u{1680}\\u{2000}-\\u{200a}\\u{2028}\\u{2029}\\u{202f}\\u{205f}\\u{3000}\\u{feff}]";

/// The same JS whitespace set as a code-unit predicate — the hand-written
/// lexer scanners run on `Vec<u16>` code units where regex classes
/// cannot reach.
#[must_use]
pub fn js_is_ws(c: u16) -> bool {
    matches!(
        c,
        0x09 | 0x0a | 0x0b | 0x0c | 0x0d | 0x20 | 0x00a0 | 0x1680
        | 0x2000..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff
    )
}

/// One error type for the whole engine: every error message lives at exactly
/// one construction site `ChorusError::new(format!(...))` with its exact
/// text. Deliberately a plain message, NOT an enum
/// of variants — the variants + Display double maintenance surface is where
/// messages drift.
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

/// JS `Number(<digit literal>)` semantics: the value goes through an f64
/// middle step and only then lands in the integer domain; direct
/// `str::parse::<i64>()` diverges past 2^53 ("9007199254740993" → JS …992,
/// direct parse …993). Rust's `str::parse::<f64>` is correctly
/// rounded (nearest, ties to even) exactly like JS `Number`, so the f64
/// middle step is reproduced bit-for-bit; the final f64→i64 `as` saturates
/// at the i64 bounds where JS would keep a lossy f64 (no arbitrary
/// precision; no log pins that region).
///
/// NaN can only occur for inputs the call sites' regexes already rejected;
/// it maps to 0 rather than panicking (JS would propagate NaN).
#[must_use]
pub fn js_num_parse(s: &str) -> i64 {
    let f: f64 = s.parse().unwrap_or(f64::NAN);
    if f.is_nan() {
        0
    } else {
        f as i64
    }
}

/// The set of characters JS `String.prototype.trim` strips: WhiteSpace plus
/// line terminators — includes `\u{FEFF}`, excludes `\u{85}`. Rust's
/// `str::trim` (char::is_whitespace) is exactly the reverse on those two,
/// so every `trim()` before a `\s`-split must use this predicate
/// (Q8). Same set as [`js_is_ws`], on a `char`.
#[must_use]
pub fn js_char_is_ws(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}'
        | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}'
        | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// `String.prototype.trim` in the JS whitespace set.
#[must_use]
pub fn js_trim(s: &str) -> &str {
    s.trim_matches(js_char_is_ws)
}

/// Cache for the module-level regex literals. A literal failing to compile
/// is a programmer error with no runtime
/// behavior to define, hence `unreachable!` (engine
/// paths use no `unwrap`/`expect`).
#[must_use]
pub fn static_regex(
    cell: &OnceLock<Regex>,
    build: impl FnOnce() -> std::result::Result<Regex, regex::Error>,
) -> &Regex {
    cell.get_or_init(|| match build() {
        Ok(re) => re,
        Err(e) => unreachable!("static regex literal failed to compile: {e}"),
    })
}

/// The text of a capture group that ALWAYS participates in its pattern
/// (a non-participation would be a programmer error, never runtime
/// behavior). Only optional groups go through [`grp_opt`].
#[must_use]
pub fn grp_text<'t>(c: &regex::Captures<'t>, i: usize) -> &'t str {
    c.get(i)
        .map(|m| m.as_str())
        .unwrap_or_else(|| unreachable!("capture group {i} always participates in this pattern"))
}

/// Group 0 (the whole match) — always present after a successful match.
#[must_use]
pub fn grp0_text<'a>(c: &'a regex::Captures<'a>) -> &'a str {
    c.get(0)
        .map(|m| m.as_str())
        .unwrap_or_else(|| unreachable!("regex group 0 is always present"))
}

/// Group 0's start offset (bytes; byte offsets are monotonic with
/// the JS UTF-16 unit offsets the `.index` compare/slice decisions use).
#[must_use]
pub fn grp0_start(c: &regex::Captures) -> usize {
    c.get(0)
        .map(|m| m.start())
        .unwrap_or_else(|| unreachable!("regex group 0 is always present"))
}

/// An OPTIONAL capture group's text (`(?:…)?` — may legitimately not
/// participate; JS reads it as `undefined`).
#[must_use]
pub fn grp_opt<'t>(c: &regex::Captures<'t>, i: usize) -> Option<&'t str> {
    c.get(i).map(|m| m.as_str())
}

/// JS `OrdinaryOwnPropertyKeys`: integer-like keys
/// first in ascending numeric value, everything else in insertion order.
/// Frozen judgment: a key is integer-like iff it is a canonical unsigned
/// decimal string with no leading zero (`0`, `123` — never `+5`, `0123`,
/// `1e2`) whose value round-trips `ToUint32` and stays `<= 2^32 - 2`
/// (V8 treats 2^32-1 as a regular property).
#[must_use]
pub fn is_integer_like_key(k: &str) -> bool {
    let bytes = k.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return false;
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return false; // leading zero: not canonical
    }
    // ≤ 10 digits (2^32-2 has 10) and in range → ToUint32 round-trip holds.
    k.parse::<u64>().is_ok_and(|v| v <= 4_294_967_294)
}

/// The key order every JS object spread/clone/
/// `Object.entries` site produces. `IndexMap` alone only preserves insertion
/// order; JS additionally fronts integer-like keys, so every observable
/// clone/spread/serialize point must rebuild through this ordering
/// (the clone points live in `core/types.rs`).
#[must_use]
pub fn ordinary_own_property_keys<V>(m: &indexmap::IndexMap<String, V>) -> Vec<String> {
    let mut int_keys: Vec<&String> = m
        .keys()
        .filter(|k| is_integer_like_key(k))
        .collect();
    int_keys.sort_by(|a, b| {
        // numeric ascending; both sides are canonical digit strings that fit
        // u64, so numeric compare on the parsed value is exact
        let av = a.parse::<u64>().unwrap_or(0);
        let bv = b.parse::<u64>().unwrap_or(0);
        av.cmp(&bv)
    });
    let mut out: Vec<String> = int_keys.into_iter().cloned().collect();
    for k in m.keys() {
        if !is_integer_like_key(k) {
            out.push(k.clone());
        }
    }
    out
}

/// Clone a string-keyed map rebuilding keys in JS `OrdinaryOwnPropertyKeys`
/// order (the `{ ...obj }` spread semantics).
#[must_use]
pub fn clone_map_in_js_order<V: Clone>(m: &indexmap::IndexMap<String, V>) -> indexmap::IndexMap<String, V> {
    let mut out = indexmap::IndexMap::with_capacity(m.len());
    for k in ordinary_own_property_keys(m) {
        // every key came from m; the lookup cannot miss
        if let Some(v) = m.get(&k) {
            out.insert(k, v.clone());
        }
    }
    out
}

/// A JSON value in the exact shape `JSON.stringify` consumes.
/// The integer domain is FROZEN: every serialized number in the
/// record surface (id/advancedMin/startMin/rng/int/delta/dur/pair.v) is an
/// integer, so there is no float variant and no shortest-round-trip
/// algorithm — `i64` Display prints identically to JS `Number → String` for
/// every value that can reach serialization.
#[derive(Debug, Clone, PartialEq)]
pub enum JsJson {
    /// JS `undefined`: dropped as an object value, `null` inside an array,
    /// the bare text `undefined` at top level (all three behaviors of
    /// `JSON.stringify`).
    Undefined,
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<JsJson>),
    /// An object's entries in EXACTLY the output order. JS serializes in
    /// `OrdinaryOwnPropertyKeys` order — build it through
    /// [`jsjson_obj_in_js_order`] (or [`ordinary_own_property_keys`]), never
    /// from raw insertion order.
    Obj(Vec<(String, JsJson)>),
}

/// The FROZEN APPROXIMATION of
/// `String.prototype.localeCompare` for the suggestion pool sort:
/// ASCII-lowercase both sides, then Rust byte order (`str::cmp`). The repo's
/// name domain is uppercase-ASCII entity names over lowercase English words,
/// where the expected order matches ICU; the approximation surface is pinned
/// by the golden `suggestions()` dumps.
/// ICU collation stays OUT of the dependency set; revisit only if
/// non-ASCII names ever appear (a separately-tracked topic).
#[must_use]
pub fn cmp_ascii_ci(a: &str, b: &str) -> Ordering {
    a.to_ascii_lowercase().cmp(&b.to_ascii_lowercase())
}

/// `JSON.stringify(value)` — compact separators, JS key
/// order (the caller's), undefined dropping, and the exact `QuoteJSONString`
/// escape set: `"` → `\"`, `\` → `\\`, `\b\t\n\f\r` short forms, other code
/// units < 0x20 as lowercase `\u00xx` (lowercase hex); DEL (0x7f), U+2028/U+2029 and
/// all other non-ASCII pass through RAW (Node-verified 2026-10-14, char-code
/// dump). Lone surrogates would escape as `\udXXX` (well-formed
/// stringify, ES2019) but are unreachable: a Rust `String` is valid Unicode.
pub fn jsjson(v: &JsJson) -> String {
    let mut out = String::new();
    jsjson_into(v, &mut out);
    out
}

fn jsjson_into(v: &JsJson, out: &mut String) {
    match v {
        JsJson::Undefined => out.push_str("undefined"),
        JsJson::Null => out.push_str("null"),
        JsJson::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsJson::Int(n) => out.push_str(&n.to_string()),
        JsJson::Str(s) => jsjson_escape(s, out),
        JsJson::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // `undefined` in an array serializes as `null`
                if matches!(item, JsJson::Undefined) {
                    out.push_str("null");
                } else {
                    jsjson_into(item, out);
                }
            }
            out.push(']');
        }
        JsJson::Obj(entries) => {
            out.push('{');
            let mut first = true;
            for (k, val) in entries {
                // undefined-valued properties are DROPPED, not emitted
                if matches!(val, JsJson::Undefined) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                jsjson_escape(k, out);
                out.push(':');
                jsjson_into(val, out);
            }
            out.push('}');
        }
    }
}

/// JSON `QuoteJSONString` (ECMA-262) — Node-verified escape set, see
/// [`jsjson`].
fn jsjson_escape(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => {
                // lowercase hex digits, exactly like JSON.stringify
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// `JSON.stringify({ ...map })` — the object SPREAD
/// itself reorders integer-like keys to the front (OrdinaryOwnPropertyKeys),
/// so serialization must rebuild through the same order. The engine's
/// aspects/attrs pairs and the JSONL log
/// go through this; `IndexMap` insertion order alone would drift after
/// `SET(x,"5",true)` + clone.
#[must_use]
pub fn jsjson_obj_in_js_order(m: &indexmap::IndexMap<String, JsJson>) -> JsJson {
    let mut entries: Vec<(String, JsJson)> = Vec::with_capacity(m.len());
    for k in ordinary_own_property_keys(m) {
        // every key came from m; the lookup cannot miss
        if let Some(v) = m.get(&k) {
            entries.push((k, v.clone()));
        }
    }
    JsJson::Obj(entries)
}

/// The deterministic test-side LCG,
/// `s = (Math.imul(s, 1664525) + 1013904223) >>> 0` — the frozen sequences
/// shared by the reach-fuzz, reach-scale and explorer drivers. u32 wrapping
/// arithmetic is bit-exact for `Math.imul` + `>>>0`.
///
/// The three seed entry points, kept one-to-one:
/// - fuzz:  `Lcg::new(seed)` — XOR 0x9e3779b9
/// - scale: `Lcg::new(0xc5)`
/// - explorer: `Lcg::from_raw_state(0x4e494748)` — 'NIGH', seeded as the raw
///   constant with **no XOR**.
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
    /// the JS expression (kept in f64, never integerized).
    #[must_use]
    pub fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4294967296.0
    }

    /// `Math.floor(r() * n)` — computed in f64, exactly like the JS expression.
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
        // says the opposite — the exact trap `cmp_utf16` exists for.
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
        // explorer entry: 'NIGH' as the RAW state, no XOR
        let mut nigh = Lcg::from_raw_state(0x4e49_4748);
        assert_eq!(nigh.next_f64(), 0.3694802538957447);
        assert_eq!(nigh.next_f64(), 0.35568378726020455);
        // scale entry: lcg(0xc5)
        let mut scale = Lcg::new(0xc5);
        assert_eq!(scale.next_f64(), 0.23735849070362747);
        // Math.floor(r() * n) stays in f64
        assert_eq!(scale.next_f64(), 0.3778064283542335);
        assert_eq!(scale.next_int(10), 4);
    }

    #[test]
    fn js_num_parse_rounds_like_f64_not_direct_i64() {
        // direct i64 parse gives …993, JS f64 …992
        assert_eq!(js_num_parse("9007199254740993"), 9007199254740992);
        assert_eq!(js_num_parse("0"), 0);
        assert_eq!(js_num_parse("47"), 47);
        assert_eq!(js_num_parse("-5"), -5);
        // > i64 range: saturates where JS keeps a lossy f64 (no log pins
        // that region)
        assert_eq!(js_num_parse("99999999999999999999"), i64::MAX);
    }

    #[test]
    fn js_trim_uses_the_js_whitespace_set() {
        // \u{feff} IS JS trim-able, \u{85} is not — Rust str::trim is reversed
        assert_eq!(js_trim("\u{feff}abc "), "abc");
        assert_eq!(js_trim("\u{85}abc\u{85}"), "\u{85}abc\u{85}");
        assert_eq!(js_trim("  a\tb\u{2028}"), "a\tb");
    }

    #[test]
    fn integer_like_keys_follow_the_frozen_v8_judgment() {
        assert!(is_integer_like_key("0"));
        assert!(is_integer_like_key("5"));
        assert!(is_integer_like_key("4294967294")); // 2^32-2, the frozen cap
        assert!(!is_integer_like_key("4294967295")); // 2^32-1: regular property
        assert!(!is_integer_like_key("0123")); // leading zero
        assert!(!is_integer_like_key("+5")); // sign
        assert!(!is_integer_like_key("1e2"));
        assert!(!is_integer_like_key("container"));
        assert!(!is_integer_like_key(""));
    }

    #[test]
    fn ordinary_own_property_keys_front_integer_keys() {
        use indexmap::IndexMap;
        let mut m = IndexMap::new();
        m.insert("container".to_string(), 1);
        m.insert("5".to_string(), 2);
        m.insert("0".to_string(), 3);
        m.insert("0123".to_string(), 4);
        m.insert("12".to_string(), 5);
        // integer-like keys ascend to the front ("0","5","12"), the rest keep
        // insertion order ("container","0123") — exactly the JS spread order
        // pinned for `SET(x,"5",true)` then clone
        assert_eq!(
            ordinary_own_property_keys(&m),
            ["0", "5", "12", "container", "0123"]
        );
        let cloned = clone_map_in_js_order(&m);
        let keys: Vec<String> = cloned.keys().cloned().collect();
        assert_eq!(keys, ["0", "5", "12", "container", "0123"]);
    }

    // ---- Node cross-checks: every expectation below was generated
    // ---- by a single `node -e` dump (Object.keys over V8 objects /
    // ---- JSON.stringify) on 2026-10-14 and hard-coded here, so the suite
    // ---- never depends on a runtime node.

    fn map_from(pairs: &[(&str, i64)]) -> indexmap::IndexMap<String, i64> {
        let mut m = indexmap::IndexMap::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), *v);
        }
        m
    }

    #[test]
    fn own_property_keys_matches_the_node_v8_dump() {
        // insertion (container,5,0,0123,12) → node k_mixed
        let got: Vec<String> = ordinary_own_property_keys(&map_from(&[
            ("container", 1),
            ("5", 2),
            ("0", 3),
            ("0123", 4),
            ("12", 5),
        ]));
        assert_eq!(got, ["0", "5", "12", "container", "0123"]);
        // node k_big: 2^32-2 IS an integer index; 2^32-1 is a REGULAR
        // property and stays in insertion order among the string keys
        let got: Vec<String> = ordinary_own_property_keys(&map_from(&[
            ("4294967294", 1),
            ("4294967295", 2),
            ("4294967293", 3),
            ("x", 4),
        ]));
        assert_eq!(got, ["4294967293", "4294967294", "4294967295", "x"]);
        // node k_leading: "007" is not canonical — only "7" fronts
        let got: Vec<String> =
            ordinary_own_property_keys(&map_from(&[("007", 1), ("7", 2)]));
        assert_eq!(got, ["7", "007"]);
        // node k_huge: > 2^32-2 never fronts, whatever its digits
        let got: Vec<String> = ordinary_own_property_keys(&map_from(&[
            ("99999999999999999999", 1),
            ("3", 2),
        ]));
        assert_eq!(got, ["3", "99999999999999999999"]);
        // node k_neg / k_exp: signs and exponents are not canonical numeric
        // strings
        let got: Vec<String> =
            ordinary_own_property_keys(&map_from(&[("-1", 1), ("1", 2)]));
        assert_eq!(got, ["1", "-1"]);
        let got: Vec<String> =
            ordinary_own_property_keys(&map_from(&[("1e2", 1), ("100", 2)]));
        assert_eq!(got, ["100", "1e2"]);
        // node k_spread: the SPREAD clone reorders (2,10 to the front) —
        // clone_map_in_js_order is the { ...obj } twin
        let got: Vec<String> = clone_map_in_js_order(&map_from(&[
            ("a", 1),
            ("10", 2),
            ("2", 3),
            ("b", 4),
        ]))
        .into_keys()
        .collect();
        assert_eq!(got, ["2", "10", "a", "b"]);
        // node k_ten_nine: numeric ascending, not lexicographic
        let got: Vec<String> = ordinary_own_property_keys(&map_from(&[
            ("10", 1),
            ("9", 2),
            ("100", 3),
            ("2", 4),
        ]));
        assert_eq!(got, ["2", "9", "10", "100"]);
    }

    #[test]
    fn jsjson_matches_the_node_json_stringify_dump() {
        use JsJson::*;
        // aspects-shaped: node j_plain
        assert_eq!(
            jsjson(&Obj(vec![
                ("secret".into(), Int(3)),
                ("language".into(), Obj(vec![("latin".into(), Int(2))])),
            ])),
            r#"{"secret":3,"language":{"latin":2}}"#
        );
        // attrs-shaped with integer-like keys: node j_intkeys — the JS key
        // order is the CALLER's (built via jsjson_obj_in_js_order below)
        let mut attrs = indexmap::IndexMap::new();
        attrs.insert("container".to_string(), Str("villa".into()));
        attrs.insert("5".to_string(), Bool(true));
        attrs.insert("0".to_string(), Int(1));
        assert_eq!(
            jsjson(&jsjson_obj_in_js_order(&attrs)),
            r#"{"0":1,"5":true,"container":"villa"}"#
        );
        // node j_ctrl: NUL/b/t/n/f/r short escapes, quote+backslash, other
        // <0x20 as lowercase \u00xx — and DEL (0x7f) RAW at the end
        assert_eq!(
            jsjson(&Str("a\u{0}\u{8}\t\n\u{c}\r\"\\\u{1}\u{f}\u{1f}\u{7f}".into())),
            "\"a\\u0000\\b\\t\\n\\f\\r\\\"\\\\\\u0001\\u000f\\u001f\u{7f}\""
        );
        // node j_unicode: non-ASCII (incl. U+2028/U+2029) stays raw
        assert_eq!(
            jsjson(&Str("héllo → 世界 \u{2028}\u{2029} \u{1F600}".into())),
            "\"héllo → 世界 \u{2028}\u{2029} \u{1F600}\""
        );
        // node j_neg / j_bigint_like: integer printing, no float machinery
        assert_eq!(jsjson(&Obj(vec![("v".into(), Int(-42))])), r#"{"v":-42}"#);
        assert_eq!(
            jsjson(&Obj(vec![("n".into(), Int(9_007_199_254_740_992))])),
            r#"{"n":9007199254740992}"#
        );
        // node j_bool_arr / j_empty / j_nested_subkey
        assert_eq!(
            jsjson(&Arr(vec![Bool(true), Bool(false), Null, Int(0)])),
            "[true,false,null,0]"
        );
        assert_eq!(jsjson(&Obj(vec![])), "{}");
        assert_eq!(
            jsjson(&Obj(vec![(
                "language".into(),
                Obj(vec![("b.c".into(), Int(7)), ("a".into(), Int(1))]),
            )])),
            r#"{"language":{"b.c":7,"a":1}}"#
        );
        // undefined dropping: node j_undef_top / j_undef_obj / j_undef_arr
        assert_eq!(jsjson(&Undefined), "undefined");
        assert_eq!(
            jsjson(&Obj(vec![
                ("a".into(), Undefined),
                ("b".into(), Int(2)),
                ("c".into(), Undefined),
                ("d".into(), Null),
            ])),
            r#"{"b":2,"d":null}"#
        );
        assert_eq!(
            jsjson(&Arr(vec![Int(1), Undefined, Int(2)])),
            "[1,null,2]"
        );
    }
}
