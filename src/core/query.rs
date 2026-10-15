//! Slot-query mini-language (spec §4.5). Tests ONE
//! card at a time:
//!   kind:text  aspect:secret>=3  aspect:language.latin>=2  state:!sealed  -aspect:winter
//!   name:SUSPICION  -name:SUSPICION   (§9.38: exact-case, like name refs/exists)

use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;

use crate::core::types::{aspect_get, Aspects, AspectVal, AttrValue, Entity};
use crate::core::num_parse::parse_i64_like_f64;
use crate::core::regex_util::static_regex;
use crate::core::text::{js_trim, JS_WS_CLASS};
use crate::error::{ChorusError, Result};

/// A comparison operator: `'>=' | '<=' | '>' | '<' | '=' | '!='`.
/// Serde derives exist for type-graph completeness only — queries never
/// serialize into the log (slots/queries live in compiled content only),
/// so the wire shape is unobservable and the default representation is fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CmpOp {
    Ge,
    Le,
    Gt,
    Lt,
    Eq,
    Ne,
}

impl CmpOp {
    /// The exact operator spelling (error messages and serialization are pinned).
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            CmpOp::Ge => ">=",
            CmpOp::Le => "<=",
            CmpOp::Gt => ">",
            CmpOp::Lt => "<",
            CmpOp::Eq => "=",
            CmpOp::Ne => "!=",
        }
    }

    /// Integer comparison shared by the query and predicate evaluators (the
    /// two modules carry identical semantics; both route through this one).
    #[must_use]
    pub fn num_cmp(self, a: i64, b: i64) -> bool {
        match self {
            CmpOp::Ge => a >= b,
            CmpOp::Le => a <= b,
            CmpOp::Gt => a > b,
            CmpOp::Lt => a < b,
            CmpOp::Eq => a == b,
            CmpOp::Ne => a != b,
        }
    }
}

/// The `(>=|<=|>|<|!=|=)` alternation, in that order (leftmost-first);
/// unreachable default — the alternation only yields these six.
#[must_use]
pub fn parse_cmp_op(s: &str) -> CmpOp {
    match s {
        ">=" => CmpOp::Ge,
        "<=" => CmpOp::Le,
        ">" => CmpOp::Gt,
        "<" => CmpOp::Lt,
        "!=" => CmpOp::Ne,
        "=" => CmpOp::Eq,
        _ => unreachable!("comparison op alternation only yields the six above"),
    }
}

/// The state term's op: a CmpOp or the `!`
/// negation (with an optional attr value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StateOp {
    Bang,
    Cmp(CmpOp),
}

/// Serde derives: type-graph completeness only
/// (see CmpOp — queries never reach the log).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum QueryTerm {
    Kind {
        neg: bool,
        kind: String,
    },
    Aspect {
        neg: bool,
        path: String,
        op: Option<CmpOp>,
        level: i64,
    },
    State {
        neg: bool,
        key: String,
        op: StateOp,
        value: Option<AttrValue>,
    },
    Name {
        neg: bool,
        name: String,
    },
}

impl QueryTerm {
    /// The term's `neg` flag on every variant.
    #[must_use]
    pub fn neg(&self) -> bool {
        match self {
            QueryTerm::Kind { neg, .. }
            | QueryTerm::Aspect { neg, .. }
            | QueryTerm::State { neg, .. }
            | QueryTerm::Name { neg, .. } => *neg,
        }
    }
}

/// A query: the AND of its terms.
pub type Query = Vec<QueryTerm>;

/// Terms are `src.trim().split(/\s+/).filter(Boolean)` — collapsed into
/// "split on runs of JS whitespace,
/// keep non-empty" (the same term list results); the whitespace set is the
/// JS one (Q8), not Rust's.
pub fn parse_query(src: &str) -> Result<Query> {
    let mut out = Vec::new();
    for term in split_js_ws(js_trim(src)) {
        out.push(parse_term(term)?);
    }
    Ok(out)
}

/// `split(/\s+/).filter(Boolean)` on the JS whitespace class;
/// hand-splitting the
/// same class is term-for-term identical because `\s+` never matches empty.
#[must_use]
fn split_js_ws(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in s.char_indices() {
        if crate::core::text::js_char_is_ws(c) {
            if let Some(st) = start.take() {
                out.push(&s[st..i]);
            }
        } else {
            start.get_or_insert(i);
        }
    }
    if let Some(st) = start {
        out.push(&s[st..]);
    }
    out
}

/// One term: optional leading `-` negation, then
/// `kind:`/`name:`/`aspect:`/`state:`.
fn parse_term(raw: &str) -> Result<QueryTerm> {
    let mut term = raw;
    let neg = term.starts_with('-');
    if neg {
        term = &term[1..];
    }
    let Some(colon) = term.find(':') else {
        return Err(ChorusError::new(format!(
            "bad query term '{raw}' (expected kind:/aspect:/state:/name:)"
        )));
    };
    let field = &term[..colon];
    let rest = &term[colon + 1..];
    match field {
        "kind" => Ok(QueryTerm::Kind {
            neg,
            kind: rest.to_string(),
        }),
        "name" => {
            if rest.is_empty() {
                return Err(ChorusError::new(format!(
                    "bad name term '{raw}' (expected name:<NAME>)"
                )));
            }
            // exact-case, like name refs and exists(NAME) (§9.38). A
            // comparison is meaningless on a name — and the count()/exists()
            // glue-tightening would silently bake `X>=1` in as the NAME (a
            // silent 0), so reject it loudly (any of `<>=!` in the value).
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = static_regex(&RE, || Regex::new(r"[<>=!]"));
            if re.is_match(rest) {
                return Err(ChorusError::new(format!(
                    "bad name term '{raw}' (name:X is an exact-case match — put the comparison outside: count(name:X) >= 1)"
                )));
            }
            Ok(QueryTerm::Name {
                neg,
                name: rest.to_string(),
            })
        }
        "aspect" => {
            // `\s` is the JS class (Q8), `-?\d+`
            // is ASCII digits; the alternation order is leftmost-first.
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = static_regex(&RE, || {
                Regex::new(&format!(
                    r"^(?:([A-Za-z_][A-Za-z0-9_.]*))?(?:{ws}*(>=|<=|>|<|!=|=){ws}*(-?[0-9]+))?$",
                    ws = JS_WS_CLASS
                ))
            });
            let Some(m) = re.captures(rest) else {
                return Err(ChorusError::new(format!("bad aspect term '{raw}'")));
            };
            // the optional path group absent (or empty,
            // which its leading [A-Za-z_] makes impossible) is the error.
            let Some(path) = m.get(1).filter(|g| !g.is_empty()) else {
                return Err(ChorusError::new(format!("bad aspect term '{raw}'")));
            };
            let op = m.get(2).map(|g| parse_cmp_op(g.as_str()));
            let level = match m.get(3) {
                Some(g) => parse_i64_like_f64(g.as_str()),
                None => 1,
            };
            Ok(QueryTerm::Aspect {
                neg,
                path: path.as_str().to_string(),
                op,
                level,
            })
        }
        "state" => {
            // same JS `\s` treatment; `(.+)` is
            // line-free by construction (terms are split on JS whitespace).
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = static_regex(&RE, || {
                Regex::new(&format!(
                    r"^(!)?([A-Za-z_][A-Za-z0-9_.]*)(?:{ws}*(>=|<=|>|<|!=|=){ws}*(.+))?$",
                    ws = JS_WS_CLASS
                ))
            });
            let Some(m) = re.captures(rest) else {
                return Err(ChorusError::new(format!("bad state term '{raw}'")));
            };
            let Some(key) = m.get(2).filter(|g| !g.is_empty()) else {
                return Err(ChorusError::new(format!("bad state term '{raw}'")));
            };
            if m.get(1).is_some() {
                return Ok(QueryTerm::State {
                    neg,
                    key: key.as_str().to_string(),
                    op: StateOp::Bang,
                    value: None,
                });
            }
            let op = parse_cmp_op(m.get(3).map(|g| g.as_str()).unwrap_or("="));
            let value = coerce(m.get(4).map(|g| g.as_str()).unwrap_or("true"));
            Ok(QueryTerm::State {
                neg,
                key: key.as_str().to_string(),
                op: StateOp::Cmp(op),
                value: Some(value),
            })
        }
        _ => Err(ChorusError::new(format!(
            "bad query term '{raw}' (expected kind:/aspect:/state:/name:)"
        ))),
    }
}

/// Value coercion: "true"/"false" → bool, then
/// integer → number, else the raw string.
fn coerce(v: &str) -> AttrValue {
    if v == "true" {
        return AttrValue::Bool(true);
    }
    if v == "false" {
        return AttrValue::Bool(false);
    }
    // `-?\d+`, ASCII digits
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = static_regex(&RE, || Regex::new(r"^-?[0-9]+$"));
    if re.is_match(v) {
        return AttrValue::Num(parse_i64_like_f64(v));
    }
    AttrValue::Str(v.to_string())
}

/// Every term must match the entity (each term's `neg` flips its result).
#[must_use]
pub fn match_query(e: &Entity, query: &[QueryTerm]) -> bool {
    query.iter().all(|t| match_term(e, t) != t.neg())
}

/// One term against one entity (pre-negation).
fn match_term(e: &Entity, t: &QueryTerm) -> bool {
    match t {
        QueryTerm::Kind { kind, .. } => e.kind == *kind,
        QueryTerm::Aspect { path, op, level, .. } => {
            let Some(v) = aspect_get(e, path) else {
                return false;
            };
            match op {
                None => v >= *level,
                Some(op) => op.num_cmp(v, *level),
            }
        }
        QueryTerm::State { key, op, value, .. } => {
            // `state:key` addresses the state.* attr namespace (§4.2):
            // state:!sealed → attrs['state.sealed']
            let v = e.attrs.get(&format!("state.{key}"));
            match op {
                // JS `!== true`: a string "false", a 0, and undefined all
                // count as unsealed
                StateOp::Bang => !matches!(v, Some(AttrValue::Bool(true))),
                StateOp::Cmp(op) => {
                    let Some(v) = v else {
                        return false;
                    };
                    // numeric-vs-numeric takes the numeric compare; then the
                    // `===`/`!==` generic branch; an ordering op over a
                    // non-number reads false (the number-type guard)
                    if let (AttrValue::Num(vn), Some(AttrValue::Num(n))) = (v, value.as_ref()) {
                        return op.num_cmp(*vn, *n);
                    }
                    match op {
                        CmpOp::Eq => Some(v) == value.as_ref(),
                        CmpOp::Ne => Some(v) != value.as_ref(),
                        _ => false,
                    }
                }
            }
        }
        QueryTerm::Name { name, .. } => e.name == *name,
    }
}

/// Build Aspects from `{key: n}` pairs in
/// the caller's iteration order (JS Object.entries = insertion order; the
/// compile sites own that). QUIRK(Q13): the head collision clobbers
/// a scalar with a sub-map (and a scalar write clobbers a sub-map) — both
/// directions silent, frozen.
pub fn aspects_from_pairs(pairs: &IndexMap<String, i64>) -> Aspects {
    let mut out = Aspects::new();
    for (k, v) in pairs {
        let Some(dot) = k.find('.') else {
            out.insert(k.clone(), AspectVal::Num(*v));
            continue;
        };
        let head = &k[..dot];
        // an existing non-sub-map head is replaced by a fresh sub-map; a
        // missing head creates one (the sub-scalar clobber, Q13)
        if !matches!(out.get(head), Some(AspectVal::Sub(_))) {
            out.insert(head.to_string(), AspectVal::Sub(IndexMap::new()));
        }
        if let Some(AspectVal::Sub(sub)) = out.get_mut(head) {
            sub.insert(k[dot + 1..].to_string(), *v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::Attrs;

    #[test]
    fn parse_and_match_the_canonical_term_shapes() {
        let q = parse_query("kind:text aspect:secret>=3 state:!sealed -aspect:winter").unwrap();
        assert_eq!(q.len(), 4);
        let e = Entity {
            id: "e1".into(),
            kind: "text".into(),
            name: "T".into(),
            aspects: {
                let mut a = Aspects::new();
                a.insert("secret".into(), AspectVal::Num(4));
                a
            },
            attrs: {
                let mut a = Attrs::new();
                a.insert("state.sealed".into(), AttrValue::Str("false".into()));
                a
            },
            born_action: Some(0),
            dead_action: None,
            stackable: false,
        };
        assert!(match_query(&e, &q));
        // state:!x reads false only for EXACT boolean true ("false" string is unsealed)
        let q2 = parse_query("state:sealed").unwrap();
        assert!(!match_query(&e, &q2));
        let mut sealed = e.clone();
        sealed.attrs.insert("state.sealed".into(), AttrValue::Bool(true));
        assert!(match_query(&sealed, &q2));
    }

    #[test]
    fn name_term_rejects_comparisons_and_defaults_match() {
        assert!(parse_query("name:X>=1").is_err());
        let q = parse_query("aspect:language.latin").unwrap();
        match &q[0] {
            QueryTerm::Aspect { path, op, level, .. } => {
                assert_eq!(path, "language.latin");
                assert_eq!(*op, None);
                assert_eq!(*level, 1); // the default when no comparison follows
            }
            _ => panic!("expected aspect term"),
        }
    }

    #[test]
    fn aspects_from_pairs_clobbers() {
        // QUIRK Q13: a later 'a.b' clobbers the scalar 'a' whole
        let mut pairs = IndexMap::new();
        pairs.insert("a".to_string(), 1);
        pairs.insert("a.b".to_string(), 2);
        let out = aspects_from_pairs(&pairs);
        assert_eq!(out.get("a"), Some(&AspectVal::Sub(
            IndexMap::from([("b".to_string(), 2)])
        )));
        // 'a.b.c' stores the literal sub-key 'b.c' — no nested tree
        let mut pairs2 = IndexMap::new();
        pairs2.insert("a.b.c".to_string(), 7);
        let out2 = aspects_from_pairs(&pairs2);
        match out2.get("a") {
            Some(AspectVal::Sub(m)) => assert_eq!(m.get("b.c"), Some(&7)),
            other => panic!("expected sub-map, got {other:?}"),
        }
    }
}
