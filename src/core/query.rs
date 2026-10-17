//! Slot-query mini-language (spec §4.5). Tests ONE
//! card at a time:
//!   kind:text  aspect:secret>=3  aspect:language.latin>=2  state:!sealed  -aspect:winter
//!   name:SUSPICION  -name:SUSPICION   (§4.5: exact-case, like name refs/exists)
//!   aspect:ability | aspect:job aspect:ritual   (§4.5: `|` separates
//!   conjunction groups — ability, or job AND ritual)

use std::sync::OnceLock;

use regex::Regex;

use crate::core::dict::Dictionary;
use crate::core::num_parse::parse_authored_int;
use crate::core::regex_util::static_regex;
use crate::core::text::{dsl_trim, split_dsl_ws, DSL_WS_CLASS};
use crate::core::types::{aspect_get, AttrValue, Entity};
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

/// The comparison-op alternation `>=|<=|!=|=|>|<`, leftmost-first (each
/// two-char op before its one-char prefix) — the single source for
/// `parse_cmp_op`, the predicate parser's lookup table, and every regex
/// alternation spelled from it.
pub(crate) const CMP_OPS: [&str; 6] = [">=", "<=", "!=", "=", ">", "<"];

/// The [`CMP_OPS`] alternation, in that order (leftmost-first);
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
    Kind { neg: bool, kind: String },
    Aspect { neg: bool, path: String, op: Option<CmpOp>, level: i64 },
    State { neg: bool, key: String, op: StateOp, value: Option<AttrValue> },
    Name { neg: bool, name: String },
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

/// A query: the OR of its conjunction groups (§4.5 alternation) —
/// `[[a, b], [c]]` is `(a AND b) OR c`; `|` binds loosest. One group (the
/// `|`-less form) behaves exactly as the pre-alternation flat conjunction;
/// the zero-group query (from `''` — the frozen-falsy no-query, guarded at
/// every home before parsing) stays vacuous-true like the old empty
/// conjunction. No round-trip printer in the engine core: queries are
/// authored strings, never serialized and never logged (§4.5) — `reach`
/// renders display strings for its analyzer reports only.
pub type Query = Vec<Vec<QueryTerm>>;

/// The §4.5 grammar: `query := and ( '|' and )*`, `and := term+`,
/// `term := '-'? atom`. Terms split on runs of the frozen DSL whitespace
/// (Q8), and a `|` is a GROUP separator only when it stands ALONE as one
/// of those tokens — glued into a term's value (`aspect:a|b`) it rides
/// along and the atom's own grammar rejects it loudly (no atom form admits
/// `|`), never a silent second group. A leading `|`, a trailing `|`, or an
/// empty group (`| |`) is a parse error. The dictionary carries the kind
/// vocabulary (§4.1: the built-ins plus the script's `kinds:` declarations)
/// — the one externally-owned word class this grammar validates.
pub fn parse_query(src: &str, dict: &Dictionary) -> Result<Query> {
    let mut groups: Query = Vec::new();
    let mut group: Vec<QueryTerm> = Vec::new();
    for term in split_dsl_ws(dsl_trim(src)) {
        if term == "|" {
            if group.is_empty() {
                return Err(ChorusError::new(format!(
                    "bad query '{src}' (expected a group before '|' — query := and ('|' and)*)"
                )));
            }
            groups.push(std::mem::take(&mut group));
        } else {
            group.push(parse_term(term, dict)?);
        }
    }
    if group.is_empty() {
        if groups.is_empty() {
            return Ok(groups); // '' — the vacuous zero-group query, unchanged
        }
        return Err(ChorusError::new(format!(
            "bad query '{src}' (expected a group after '|' — query := and ('|' and)*)"
        )));
    }
    groups.push(group);
    Ok(groups)
}

/// One term: optional leading `-` negation, then
/// `kind:`/`name:`/`aspect:`/`state:`.
fn parse_term(raw: &str, dict: &Dictionary) -> Result<QueryTerm> {
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
        "kind" => {
            // §4.5/§6.1: a kind is one of the dictionary's §4.1 kinds — an
            // empty or unknown kind could only ever match nothing, and "a
            // slot that compiles and silently never matches" is exactly what
            // load errors exist for (a `kind:txet` typo used to be inert)
            if rest.is_empty() {
                return Err(ChorusError::new(format!(
                    "bad kind term '{raw}' (expected kind:<KIND>)"
                )));
            }
            if !dict.kinds.contains(rest) {
                return Err(ChorusError::new(format!(
                    "bad kind term '{raw}' (unknown kind '{rest}' — expected one of {})",
                    dict.kinds_list()
                )));
            }
            Ok(QueryTerm::Kind { neg, kind: rest.to_string() })
        }
        "name" => {
            if rest.is_empty() {
                return Err(ChorusError::new(format!(
                    "bad name term '{raw}' (expected name:<NAME>)"
                )));
            }
            // exact-case, like name refs and exists(NAME) (§4.5). A
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
            Ok(QueryTerm::Name { neg, name: rest.to_string() })
        }
        "aspect" => {
            // `\s` is the frozen class (Q8), `-?\d+`
            // is ASCII digits; the alternation order is leftmost-first.
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = static_regex(&RE, || {
                Regex::new(&format!(
                    r"^(?:([A-Za-z_][A-Za-z0-9_.]*))?(?:{ws}*({ops}){ws}*(-?[0-9]+))?$",
                    ws = DSL_WS_CLASS,
                    ops = CMP_OPS.join("|")
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
                Some(g) => parse_authored_int(g.as_str()),
                None => 1,
            };
            Ok(QueryTerm::Aspect { neg, path: path.as_str().to_string(), op, level })
        }
        "state" => {
            // the same `\s` treatment; `(.+)` is
            // line-free by construction (terms are split on DSL whitespace).
            static RE: OnceLock<Regex> = OnceLock::new();
            let re = static_regex(&RE, || {
                Regex::new(&format!(
                    r"^(!)?([A-Za-z_][A-Za-z0-9_.]*)(?:{ws}*({ops}){ws}*(.+))?$",
                    ws = DSL_WS_CLASS,
                    ops = CMP_OPS.join("|")
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
        return AttrValue::Num(parse_authored_int(v));
    }
    AttrValue::Str(v.to_string())
}

/// A card matches iff ANY group's terms ALL match (§4.5 DNF). The
/// zero-group query (from `''`) stays vacuous-true — the empty
/// conjunction's pre-alternation semantics.
#[must_use]
pub fn match_query(e: &Entity, query: &Query) -> bool {
    query.is_empty() || query.iter().any(|g| match_conjunction(e, g))
}

/// Every term of one conjunction group must match (each term's `neg` flips
/// its result) — the pre-alternation flat-conjunction semantics, one group.
#[must_use]
pub(crate) fn match_conjunction(e: &Entity, group: &[QueryTerm]) -> bool {
    group.iter().all(|t| match_term(e, t) != t.neg())
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
            // (an empty attrs map misses every key — skip the key build,
            // this term runs once per entity in the match_query scans)
            let v = if e.attrs.is_empty() { None } else { e.attrs.get(&format!("state.{key}")) };
            match op {
                // strict inequality against `true`: the string "false", a 0, a missing value all
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dict::core_dictionary;
    use crate::core::types::{AspectVal, Aspects, Attrs};

    #[test]
    fn parse_and_match_the_canonical_term_shapes() {
        let dict = core_dictionary();
        let q =
            parse_query("kind:text aspect:secret>=3 state:!sealed -aspect:winter", &dict).unwrap();
        assert_eq!(q.len(), 1); // no '|' — one conjunction group (§4.5)
        assert_eq!(q[0].len(), 4);
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
        let q2 = parse_query("state:sealed", &dict).unwrap();
        assert!(!match_query(&e, &q2));
        let mut sealed = e.clone();
        sealed.attrs.insert("state.sealed".into(), AttrValue::Bool(true));
        assert!(match_query(&sealed, &q2));
    }

    #[test]
    fn name_term_rejects_comparisons_and_defaults_match() {
        let dict = core_dictionary();
        assert!(parse_query("name:X>=1", &dict).is_err());
        let q = parse_query("aspect:language.latin", &dict).unwrap();
        match &q[0][0] {
            QueryTerm::Aspect { path, op, level, .. } => {
                assert_eq!(path, "language.latin");
                assert_eq!(*op, None);
                assert_eq!(*level, 1); // the default when no comparison follows
            }
            _ => panic!("expected aspect term"),
        }
    }
}
