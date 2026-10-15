//! Refs (spec §5.1, §9.14). Canonical output is the
//! bare name (unique among the living); `kind:NAME` and `#id` are accepted
//! input sugar; reserved words and unsafe names must be quoted strings.
//! The module is `core::r#ref` in Rust (`ref` is a keyword).

use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::core::dict::Dictionary;
use crate::js::{cmp_utf16, static_regex, ChorusError, Result};

/// A reference to an entity: by bare name (optionally
/// kind-qualified), by `#id`, or a function parameter (authored-only).
///
/// The serde shape is the JSONL log surface: internally tagged with tag
/// `form`, and the NAME variant's field order is `kind` then `name` —
/// `kind` omitted when absent exactly like
/// `JSON.stringify` dropping `undefined`. Never changes wire order.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "form")]
pub enum Ref {
    /// `{ form: 'name', kind?, name }`
    #[serde(rename = "name")]
    Name {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        name: String,
    },
    /// `{ form: 'id', id }`
    #[serde(rename = "id")]
    Id { id: String },
    /// `{ form: 'param', name }` — F3 authored-only: a function parameter
    /// standing in a whole-ref position (§9.40); calls expand at load, so
    /// this never formats, resolves, or round-trips.
    #[serde(rename = "param")]
    Param { name: String },
}

/// SAFE_NAME: `^[A-Za-z_][A-Za-z0-9_]*$`.
fn safe_name_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    static_regex(&RE, || Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$"))
}

/// A segment that re-parses as a bare name token
/// (used by the sentence formatter to decide between bare and quoted spellings).
#[must_use]
pub fn is_safe_name(name: &str) -> bool {
    safe_name_re().is_match(name)
}

/// A name ref, optionally kind-qualified.
#[must_use]
pub fn name_ref(name: impl Into<String>, kind: Option<String>) -> Ref {
    match kind {
        Some(kind) => Ref::Name {
            kind: Some(kind),
            name: name.into(),
        },
        None => Ref::Name {
            kind: None,
            name: name.into(),
        },
    }
}

/// An id ref (`#id`).
#[must_use]
pub fn id_ref(id: impl Into<String>) -> Ref {
    Ref::Id { id: id.into() }
}

/// The only dictionary surface the formatter
/// needs is `dict.reserved`.
pub fn format_ref(r: &Ref, dict: Option<&Dictionary>) -> Result<String> {
    match r {
        Ref::Id { id } => Ok(format!("#{id}")),
        Ref::Name { name, .. } => {
            let safe = safe_name_re().is_match(name)
                && !dict.is_some_and(|d| d.reserved.contains(name));
            if safe {
                Ok(name.clone())
            } else {
                // only `"` is escaped, into `\"`;
                // backslashes pass through raw.
                Ok(format!("\"{}\"", name.replace('"', "\\\"")))
            }
        }
        Ref::Param { .. } => Err(ChorusError::new(
            "a function parameter is authored form only — calls expand at load (§9.40)",
        )),
    }
}

/// Key construction only; the comparisons happen
/// in [`sort_refs`] and the relation-key builder (types.rs), both through
/// `cmp_utf16`. A param ref yields its name.
#[must_use]
pub fn ref_sort_key(r: &Ref) -> String {
    match r {
        Ref::Id { id } => format!("#{id}"),
        Ref::Name { name, .. } | Ref::Param { name } => name.clone(),
    }
}

/// Stable sort by [`ref_sort_key`] in JS UTF-16
/// code-unit order (**never** `str::cmp` — non-BMP names would
/// order differently). `sort_by` is stable, and keys are
/// computed per comparison.
#[must_use]
pub fn sort_refs(refs: &[Ref]) -> Vec<Ref> {
    let mut out = refs.to_vec();
    out.sort_by(|a, b| cmp_utf16(&ref_sort_key(a), &ref_sort_key(b)));
    out
}

// NOTE: a ref equality compare is deliberately absent;
// nothing in the engine calls one.

#[cfg(test)]
mod tests {
    use super::*;

    fn d() -> Dictionary {
        let mut d = Dictionary::new();
        let _ = d.register_verb("READ");
        d
    }

    #[test]
    fn format_ref_quotes_unsafe_and_reserved_names_only() {
        let dict = d();
        assert_eq!(format_ref(&name_ref("RUMOUR", None), None).unwrap(), "RUMOUR");
        assert_eq!(
            format_ref(&name_ref("READ", None), Some(&dict)).unwrap(),
            "\"READ\"" // reserved → quoted
        );
        // QUIRK: only `"` is escaped into \" — backslashes stay raw
        assert_eq!(
            format_ref(&name_ref("a\"b\\c", None), None).unwrap(),
            "\"a\\\"b\\c\""
        );
        assert_eq!(format_ref(&id_ref("c7f3"), None).unwrap(), "#c7f3");
        // param refs never format
        assert!(format_ref(&Ref::Param { name: "n".into() }, None).is_err());
    }

    #[test]
    fn sort_refs_uses_utf16_order_and_is_stable() {
        let refs = vec![
            name_ref("b", None),
            id_ref("2"),
            name_ref("a", None),
            name_ref("b", None),
        ];
        let sorted = sort_refs(&refs);
        let keys: Vec<String> = sorted.iter().map(ref_sort_key).collect();
        // "#2" sorts before names (its key carries '#', U+0023 < 'A')
        assert_eq!(keys, ["#2", "a", "b", "b"]);
        // stability: the two "b" refs keep their input order
        assert_eq!(sorted[2], sorted[3]);
        assert_eq!(sorted[2], name_ref("b", None));
        // non-BMP name sorts before U+E000 in UTF-16 order (and AFTER it in
        // Rust byte order — the exact trap cmp_utf16 exists for)
        let uni = vec![name_ref("\u{e000}", None), name_ref("\u{10000}", None)];
        let keys2: Vec<String> = sort_refs(&uni).iter().map(ref_sort_key).collect();
        assert_eq!(keys2, ["\u{10000}", "\u{e000}"]);
    }
}
