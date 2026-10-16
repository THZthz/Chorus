//! The canonical JSON surface: the engine's frozen object-key order
//! (integer-like keys ascending first, everything else in insertion order —
//! the `OrdinaryOwnPropertyKeys` judgment, B-20) and the canonical JSON
//! writer [`jsonw`], whose escape set and integer-only number domain are
//! frozen (B-20). serde_json's `preserve_order` feature is load-bearing for
//! the pinned key order — do not touch.

/// The engine's frozen integer-like-key judgment: a key is integer-like iff
/// it is a canonical unsigned decimal string with no leading zero (`0`,
/// `123` — never `+5`, `0123`, `1e2`) whose value round-trips `ToUint32`
/// and stays `<= 2^32 - 2` (2^32-1 ranks as a regular property).
#[must_use]
pub fn is_integer_like_key(k: &str) -> bool {
    integer_like_key_value(k).is_some()
}

/// [`is_integer_like_key`] plus the parsed value in one scan — the callers
/// below need the value for the numeric sort, and the judgment is defined BY
/// the parse, so one pass serves both.
fn integer_like_key_value(k: &str) -> Option<u64> {
    let bytes = k.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return None; // leading zero: not canonical
    }
    // ≤ 10 digits (2^32-2 has 10) and in range → ToUint32 round-trip holds.
    k.parse::<u64>().ok().filter(|v| *v <= 4_294_967_294)
}

/// The key order every object spread/clone/entries site produces
/// (the frozen `OrdinaryOwnPropertyKeys` judgment, B-20): integer-like keys
/// first in ascending numeric value, everything else in insertion order.
/// `IndexMap` alone only preserves insertion order; every observable
/// clone/spread/serialize point must rebuild through this ordering
/// (the clone points live in `core/types.rs`).
#[must_use]
pub fn canonical_key_order<V>(m: &indexmap::IndexMap<String, V>) -> Vec<String> {
    if !m.keys().any(|k| is_integer_like_key(k)) {
        // no integer-like keys ⇒ the canonical order IS the insertion order
        // (the reorder only fronts integer-like keys), so the two-pass walk
        // would re-emit the keys verbatim
        return m.keys().cloned().collect();
    }
    let mut out: Vec<String> = Vec::with_capacity(m.len());
    for i in integer_keys_in_order(m) {
        let Some((k, _)) = m.get_index(i) else {
            unreachable!("every index comes from this same map's key walk");
        };
        out.push(k.clone());
    }
    for k in m.keys() {
        if !is_integer_like_key(k) {
            out.push(k.clone());
        }
    }
    out
}

/// The integer-like keys' indices of `m`, ascending by numeric value. Keys
/// are unique canonical digit strings, so the values are distinct and the
/// sort is total.
fn integer_keys_in_order<V>(m: &indexmap::IndexMap<String, V>) -> impl Iterator<Item = usize> {
    let mut ints: Vec<(u64, usize)> = m
        .keys()
        .enumerate()
        .filter_map(|(i, k)| integer_like_key_value(k).map(|v| (v, i)))
        .collect();
    ints.sort_unstable_by_key(|(v, _)| *v);
    ints.into_iter().map(|(_, i)| i)
}

/// Clone a string-keyed map rebuilding keys in
/// [`canonical_key_order`] order (the canonical key-order clone).
///
/// Index-partitioned: the order is computed on INDICES (no key-String
/// churn, no re-lookup per key), and a map with no integer-like keys takes
/// the plain-clone fast path — the rebuild would be entry-for-entry
/// identical, and every `Entity::clone`/state clone rides this function.
#[must_use]
pub fn clone_map_in_key_order<V: Clone>(
    m: &indexmap::IndexMap<String, V>,
) -> indexmap::IndexMap<String, V> {
    let has_int_keys = m.keys().any(|k| is_integer_like_key(k));
    if !has_int_keys {
        return m.clone();
    }
    let plain: Vec<usize> = m
        .keys()
        .enumerate()
        .filter(|(_, k)| !is_integer_like_key(k))
        .map(|(i, _)| i)
        .collect();
    let mut out = indexmap::IndexMap::with_capacity(m.len());
    for i in integer_keys_in_order(m).chain(plain) {
        let Some((k, v)) = m.get_index(i) else {
            unreachable!("every index comes from this same map's key walk");
        };
        out.insert(k.clone(), v.clone());
    }
    out
}

/// A JSON value in the engine's canonical serialized shape.
/// The integer domain is FROZEN: every serialized number in the
/// record surface (id/advancedMin/startMin/rng/int/delta/dur/pair.v) is an
/// integer, so there is no float variant and no shortest-round-trip
/// algorithm — `i64` Display prints identically to the canonical number
/// rendering for every value that can reach serialization.
#[derive(Debug, Clone, PartialEq)]
pub enum Jsonw {
    /// `undefined`: dropped as an object value, `null` inside an array,
    /// the bare text `undefined` at top level (all three serialization
    /// behaviors).
    Undefined,
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Arr(Vec<Jsonw>),
    /// An object's entries in EXACTLY the output order. Serialization uses
    /// the frozen `OrdinaryOwnPropertyKeys` order — build it through
    /// [`jsonw_obj_in_key_order`] (or [`canonical_key_order`]), never
    /// from raw insertion order.
    Obj(Vec<(String, Jsonw)>),
}

/// The engine's canonical JSON serialization: compact separators, the
/// caller's key order, `undefined` dropping, and the frozen
/// `QuoteJSONString` escape set (plan B-20): `"` → `\"`, `\` → `\\`,
/// `\b\t\n\f\r` short forms, other code units < 0x20 as lowercase `\u00xx`
/// (lowercase hex); DEL (0x7f), U+2028/U+2029 and
/// all other non-ASCII pass through RAW. Lone surrogates would escape as
/// `\udXXX` but are unreachable: a Rust `String` is valid Unicode.
pub fn jsonw(v: &Jsonw) -> String {
    let mut out = String::new();
    jsonw_into(v, &mut out);
    out
}

fn jsonw_into(v: &Jsonw, out: &mut String) {
    match v {
        Jsonw::Undefined => out.push_str("undefined"),
        Jsonw::Null => out.push_str("null"),
        Jsonw::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Jsonw::Int(n) => out.push_str(&n.to_string()),
        Jsonw::Str(s) => jsonw_escape(s, out),
        Jsonw::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // `undefined` in an array serializes as `null`
                if matches!(item, Jsonw::Undefined) {
                    out.push_str("null");
                } else {
                    jsonw_into(item, out);
                }
            }
            out.push(']');
        }
        Jsonw::Obj(entries) => {
            out.push('{');
            let mut first = true;
            for (k, val) in entries {
                // undefined-valued properties are DROPPED, not emitted
                if matches!(val, Jsonw::Undefined) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                jsonw_escape(k, out);
                out.push(':');
                jsonw_into(val, out);
            }
            out.push('}');
        }
    }
}

/// The frozen string escape set (`QuoteJSONString`, plan B-20), see
/// [`jsonw`].
fn jsonw_escape(s: &str, out: &mut String) {
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
                // lowercase hex digits, exactly like the canonical form
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Serialize a string-keyed map as a canonical object: the frozen key order
/// applies BEFORE serialization (integer-like keys front, see
/// [`canonical_key_order`]), so maps must rebuild through that
/// order — raw insertion order would drift after `SET(x,"5",true)` + clone.
/// The engine's aspects/attrs pairs and the JSONL log go through this.
#[must_use]
pub fn jsonw_obj_in_key_order(m: &indexmap::IndexMap<String, Jsonw>) -> Jsonw {
    let mut entries: Vec<(String, Jsonw)> = Vec::with_capacity(m.len());
    for k in canonical_key_order(m) {
        // every key came from m; the lookup cannot miss
        if let Some(v) = m.get(&k) {
            entries.push((k, v.clone()));
        }
    }
    Jsonw::Obj(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn canonical_key_order_front_integer_keys() {
        use indexmap::IndexMap;
        let mut m = IndexMap::new();
        m.insert("container".to_string(), 1);
        m.insert("5".to_string(), 2);
        m.insert("0".to_string(), 3);
        m.insert("0123".to_string(), 4);
        m.insert("12".to_string(), 5);
        // integer-like keys ascend to the front ("0","5","12"), the rest keep
        // insertion order ("container","0123") — exactly the clone order
        // pinned for `SET(x,"5",true)` then clone
        assert_eq!(
            canonical_key_order(&m),
            ["0", "5", "12", "container", "0123"]
        );
        let cloned = clone_map_in_key_order(&m);
        let keys: Vec<String> = cloned.keys().cloned().collect();
        assert_eq!(keys, ["0", "5", "12", "container", "0123"]);
    }

    fn map_from(pairs: &[(&str, i64)]) -> indexmap::IndexMap<String, i64> {
        let mut m = indexmap::IndexMap::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), *v);
        }
        m
    }

    #[test]
    fn own_property_keys_matches_the_node_v8_dump() {
        // insertion (container,5,0,0123,12) — mixed shapes in one map
        let got: Vec<String> = canonical_key_order(&map_from(&[
            ("container", 1),
            ("5", 2),
            ("0", 3),
            ("0123", 4),
            ("12", 5),
        ]));
        assert_eq!(got, ["0", "5", "12", "container", "0123"]);
        // 2^32-2 IS integer-like; 2^32-1 is a REGULAR
        // key and stays in insertion order among the string keys
        let got: Vec<String> = canonical_key_order(&map_from(&[
            ("4294967294", 1),
            ("4294967295", 2),
            ("4294967293", 3),
            ("x", 4),
        ]));
        assert_eq!(got, ["4294967293", "4294967294", "4294967295", "x"]);
        // "007" is not canonical — only "7" fronts
        let got: Vec<String> = canonical_key_order(&map_from(&[("007", 1), ("7", 2)]));
        assert_eq!(got, ["7", "007"]);
        // > 2^32-2 never fronts, whatever its digits
        let got: Vec<String> =
            canonical_key_order(&map_from(&[("99999999999999999999", 1), ("3", 2)]));
        assert_eq!(got, ["3", "99999999999999999999"]);
        // signs and exponents are not canonical numeric
        // strings
        let got: Vec<String> = canonical_key_order(&map_from(&[("-1", 1), ("1", 2)]));
        assert_eq!(got, ["1", "-1"]);
        let got: Vec<String> = canonical_key_order(&map_from(&[("1e2", 1), ("100", 2)]));
        assert_eq!(got, ["100", "1e2"]);
        // the key-order clone reorders (2,10 to the front) —
        // clone_map_in_key_order is that clone path
        let got: Vec<String> =
            clone_map_in_key_order(&map_from(&[("a", 1), ("10", 2), ("2", 3), ("b", 4)]))
                .into_keys()
                .collect();
        assert_eq!(got, ["2", "10", "a", "b"]);
        // numeric ascending, not lexicographic
        let got: Vec<String> =
            canonical_key_order(&map_from(&[("10", 1), ("9", 2), ("100", 3), ("2", 4)]));
        assert_eq!(got, ["2", "9", "10", "100"]);
    }

    #[test]
    fn jsonw_matches_the_node_json_stringify_dump() {
        use Jsonw::*;
        // aspects-shaped object
        assert_eq!(
            jsonw(&Obj(vec![
                ("secret".into(), Int(3)),
                ("language".into(), Obj(vec![("latin".into(), Int(2))])),
            ])),
            r#"{"secret":3,"language":{"latin":2}}"#
        );
        // attrs-shaped with integer-like keys — the key
        // order is the CALLER's (built via jsonw_obj_in_key_order below)
        let mut attrs = indexmap::IndexMap::new();
        attrs.insert("container".to_string(), Str("villa".into()));
        attrs.insert("5".to_string(), Bool(true));
        attrs.insert("0".to_string(), Int(1));
        assert_eq!(
            jsonw(&jsonw_obj_in_key_order(&attrs)),
            r#"{"0":1,"5":true,"container":"villa"}"#
        );
        // control characters: NUL/b/t/n/f/r short escapes, quote+backslash,
        // other <0x20 as lowercase \u00xx — and DEL (0x7f) RAW at the end
        assert_eq!(
            jsonw(&Str(
                "a\u{0}\u{8}\t\n\u{c}\r\"\\\u{1}\u{f}\u{1f}\u{7f}".into()
            )),
            "\"a\\u0000\\b\\t\\n\\f\\r\\\"\\\\\\u0001\\u000f\\u001f\u{7f}\""
        );
        // non-ASCII (incl. U+2028/U+2029) stays raw
        assert_eq!(
            jsonw(&Str("héllo → 世界 \u{2028}\u{2029} \u{1F600}".into())),
            "\"héllo → 世界 \u{2028}\u{2029} \u{1F600}\""
        );
        // negative / 2^53-scale values: integer printing, no float machinery
        assert_eq!(jsonw(&Obj(vec![("v".into(), Int(-42))])), r#"{"v":-42}"#);
        assert_eq!(
            jsonw(&Obj(vec![("n".into(), Int(9_007_199_254_740_992))])),
            r#"{"n":9007199254740992}"#
        );
        // arrays, the empty object, nested sub-keys
        assert_eq!(
            jsonw(&Arr(vec![Bool(true), Bool(false), Null, Int(0)])),
            "[true,false,null,0]"
        );
        assert_eq!(jsonw(&Obj(vec![])), "{}");
        assert_eq!(
            jsonw(&Obj(vec![(
                "language".into(),
                Obj(vec![("b.c".into(), Int(7)), ("a".into(), Int(1))]),
            )])),
            r#"{"language":{"b.c":7,"a":1}}"#
        );
        // undefined dropping: top level, object values, array items
        assert_eq!(jsonw(&Undefined), "undefined");
        assert_eq!(
            jsonw(&Obj(vec![
                ("a".into(), Undefined),
                ("b".into(), Int(2)),
                ("c".into(), Undefined),
                ("d".into(), Null),
            ])),
            r#"{"b":2,"d":null}"#
        );
        assert_eq!(jsonw(&Arr(vec![Int(1), Undefined, Int(2)])), "[1,null,2]");
    }
}
