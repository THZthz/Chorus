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

/// The key order every object spread/clone/entries site produces
/// (the frozen `OrdinaryOwnPropertyKeys` judgment, B-20): integer-like keys
/// first in ascending numeric value, everything else in insertion order.
/// `IndexMap` alone only preserves insertion order; every observable
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

/// Clone a string-keyed map rebuilding keys in
/// [`ordinary_own_property_keys`] order (the object-spread semantics).
#[must_use]
pub fn clone_map_in_key_order<V: Clone>(m: &indexmap::IndexMap<String, V>) -> indexmap::IndexMap<String, V> {
    let mut out = indexmap::IndexMap::with_capacity(m.len());
    for k in ordinary_own_property_keys(m) {
        // every key came from m; the lookup cannot miss
        if let Some(v) = m.get(&k) {
            out.insert(k, v.clone());
        }
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
    /// [`jsonw_obj_in_key_order`] (or [`ordinary_own_property_keys`]), never
    /// from raw insertion order.
    Obj(Vec<(String, Jsonw)>),
}

/// The engine's canonical JSON serialization: compact separators, the
/// caller's key order, `undefined` dropping, and the frozen
/// `QuoteJSONString` escape set (ECMA-262): `"` → `\"`, `\` → `\\`,
/// `\b\t\n\f\r` short forms, other code units < 0x20 as lowercase `\u00xx`
/// (lowercase hex); DEL (0x7f), U+2028/U+2029 and
/// all other non-ASCII pass through RAW (Node-verified 2026-10-14, char-code
/// dump). Lone surrogates would escape as `\udXXX` (well-formed
/// stringify, ES2019) but are unreachable: a Rust `String` is valid Unicode.
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

/// The frozen string escape set (ECMA-262 `QuoteJSONString`), see
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
/// [`ordinary_own_property_keys`]), so maps must rebuild through that
/// order — raw insertion order would drift after `SET(x,"5",true)` + clone.
/// The engine's aspects/attrs pairs and the JSONL log go through this.
#[must_use]
pub fn jsonw_obj_in_key_order(m: &indexmap::IndexMap<String, Jsonw>) -> Jsonw {
    let mut entries: Vec<(String, Jsonw)> = Vec::with_capacity(m.len());
    for k in ordinary_own_property_keys(m) {
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
    fn ordinary_own_property_keys_front_integer_keys() {
        use indexmap::IndexMap;
        let mut m = IndexMap::new();
        m.insert("container".to_string(), 1);
        m.insert("5".to_string(), 2);
        m.insert("0".to_string(), 3);
        m.insert("0123".to_string(), 4);
        m.insert("12".to_string(), 5);
        // integer-like keys ascend to the front ("0","5","12"), the rest keep
        // insertion order ("container","0123") — exactly the spread order
        // pinned for `SET(x,"5",true)` then clone
        assert_eq!(
            ordinary_own_property_keys(&m),
            ["0", "5", "12", "container", "0123"]
        );
        let cloned = clone_map_in_key_order(&m);
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
        // clone_map_in_key_order is the spread twin
        let got: Vec<String> = clone_map_in_key_order(&map_from(&[
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
    fn jsonw_matches_the_node_json_stringify_dump() {
        use Jsonw::*;
        // aspects-shaped: node j_plain
        assert_eq!(
            jsonw(&Obj(vec![
                ("secret".into(), Int(3)),
                ("language".into(), Obj(vec![("latin".into(), Int(2))])),
            ])),
            r#"{"secret":3,"language":{"latin":2}}"#
        );
        // attrs-shaped with integer-like keys: node j_intkeys — the key
        // order is the CALLER's (built via jsonw_obj_in_key_order below)
        let mut attrs = indexmap::IndexMap::new();
        attrs.insert("container".to_string(), Str("villa".into()));
        attrs.insert("5".to_string(), Bool(true));
        attrs.insert("0".to_string(), Int(1));
        assert_eq!(
            jsonw(&jsonw_obj_in_key_order(&attrs)),
            r#"{"0":1,"5":true,"container":"villa"}"#
        );
        // node j_ctrl: NUL/b/t/n/f/r short escapes, quote+backslash, other
        // <0x20 as lowercase \u00xx — and DEL (0x7f) RAW at the end
        assert_eq!(
            jsonw(&Str("a\u{0}\u{8}\t\n\u{c}\r\"\\\u{1}\u{f}\u{1f}\u{7f}".into())),
            "\"a\\u0000\\b\\t\\n\\f\\r\\\"\\\\\\u0001\\u000f\\u001f\u{7f}\""
        );
        // node j_unicode: non-ASCII (incl. U+2028/U+2029) stays raw
        assert_eq!(
            jsonw(&Str("héllo → 世界 \u{2028}\u{2029} \u{1F600}".into())),
            "\"héllo → 世界 \u{2028}\u{2029} \u{1F600}\""
        );
        // node j_neg / j_bigint_like: integer printing, no float machinery
        assert_eq!(jsonw(&Obj(vec![("v".into(), Int(-42))])), r#"{"v":-42}"#);
        assert_eq!(
            jsonw(&Obj(vec![("n".into(), Int(9_007_199_254_740_992))])),
            r#"{"n":9007199254740992}"#
        );
        // node j_bool_arr / j_empty / j_nested_subkey
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
        // undefined dropping: node j_undef_top / j_undef_obj / j_undef_arr
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
        assert_eq!(
            jsonw(&Arr(vec![Int(1), Undefined, Int(2)])),
            "[1,null,2]"
        );
    }
}
