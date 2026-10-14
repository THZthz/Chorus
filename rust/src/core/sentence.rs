//! Sentence AST — ts: src/core/sentence.ts (mirrors spec §5.1 exactly).
//! `Role/ROLES` are sunk into `core::roles` (CORE-02) and re-exported here
//! to mirror the TS export surface.

use crate::core::clock::Clock;
use crate::core::predicate::{Pred, ValExpr};
use crate::core::r#ref::Ref;

pub use crate::core::roles::{Role, ROLES};

/// ts: src/core/sentence.ts:10-18
#[derive(Debug, Clone, PartialEq)]
pub struct Clause {
    pub actors: Vec<Ref>, // unordered set; canonical form is sorted
    pub at: Option<Ref>,  // pre-verbal in canonical form (§9.10)
    pub verb: String,
    pub to: Option<Ref>,
    pub objects: Vec<Ref>,
    pub with: Vec<Ref>,
    pub about: Option<Ref>,
}

/// The `into` target's `Ref | { role }` union (ts: sentence.ts:29), kept as
/// an untagged enum — the serde shape plan §2.3 vetted for W3's log records.
/// Untagged = the wire form is the bare inner object (`{ role: 'object' }`
/// or the ref itself), exactly the TS values; the variants are disjoint on
/// the wire (only one has `role`, only the other has `form`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum IntoTarget {
    /// a whole `Ref` (including the authored-only param form)
    Ref {
        #[serde(rename = "ref")]
        r#ref: Ref,
    },
    /// `{ role }`
    Role { role: Role },
}

/// `1 | -1` (ts: sentence.ts:36) — the sign a delta position carries on a
/// param placeholder, folded into the call's number at expansion. Wire form
/// (plan §2.3): the JSON numbers `1` / `-1`, omitted when absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sign {
    Pos,
    Neg,
}

impl serde::Serialize for Sign {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_i64(self.as_i64())
    }
}

impl<'de> serde::Deserialize<'de> for Sign {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = i64::deserialize(deserializer)?;
        match v {
            1 => Ok(Sign::Pos),
            -1 => Ok(Sign::Neg),
            other => Err(serde::de::Error::custom(format!(
                "sign must be 1 or -1, got {other}"
            ))),
        }
    }
}

impl Sign {
    /// ts: parse.ts:725-726 — `a.sign * bound.v`.
    #[must_use]
    pub fn apply(self, v: i64) -> i64 {
        match self {
            Sign::Pos => v,
            Sign::Neg => -v,
        }
    }

    /// ts: parse.ts:553 — `sign: t.v === '-' ? -1 : 1`.
    #[must_use]
    pub fn as_i64(self) -> i64 {
        match self {
            Sign::Pos => 1,
            Sign::Neg => -1,
        }
    }
}

/// ts: src/core/sentence.ts:20-39 — the Arg union. The serde tag is `t`
/// (plan §2.3); `pair`/`valueToken` reuse [`AttrValue`] because TS spells
/// both as `string | number | boolean`.
///
/// Wire order (W3 JSONL, ENG-20): every TS construction literal (parse.ts
/// :310-609, engine.ts :389-589) writes `t` first and then the payload
/// fields in the declaration order below — `param`'s `sign` omitted when
/// absent, exactly like `JSON.stringify` dropping `undefined`. The log only
/// ever carries the ref/role/int/delta/str/bool/dur/key/into/asId/pair
/// shapes (vexpr/param are authored-only and expand/roll away), but the
/// serde impl covers every variant so the model has one representation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "t")]
pub enum Arg {
    /// `{ t: 'ref', ref }`
    #[serde(rename = "ref")]
    Ref {
        #[serde(rename = "ref")]
        r#ref: Ref,
    },
    /// `{ t: 'role', role }` — outcome-arg role ref: ACTORS, OBJECT…
    #[serde(rename = "role")]
    Role { role: Role },
    /// `{ t: 'int', v }`
    #[serde(rename = "int")]
    Int { v: i64 },
    /// `{ t: 'delta', v }` — signed SET value (+1/-2), applied to the
    /// current entry (§9.31)
    #[serde(rename = "delta")]
    Delta { v: i64 },
    /// `{ t: 'str', v }`
    #[serde(rename = "str")]
    Str { v: String },
    /// `{ t: 'bool', v }`
    #[serde(rename = "bool")]
    Bool { v: bool },
    /// `{ t: 'dur', v }` — minutes
    #[serde(rename = "dur")]
    Dur { v: i64 },
    /// `{ t: 'key', v }` — SET key: 'level', 'state.sealed', 'aspect:dread' (§9.31)
    #[serde(rename = "key")]
    Key { v: String },
    /// `{ t: 'into', ref: Ref | { role } }`
    #[serde(rename = "into")]
    Into {
        #[serde(rename = "ref")]
        r#ref: IntoTarget,
    },
    /// `{ t: 'asId', id }`
    #[serde(rename = "asId")]
    AsId { id: String },
    /// `{ t: 'pair', k, v: string | number | boolean }`
    #[serde(rename = "pair")]
    Pair { k: String, v: crate::core::types::AttrValue },
    /// `{ t: 'vexpr', v }` — ROLL argument (§9.41): a §6.3 value expression.
    /// Authored form only: the log records the computed ROLLED ints.
    #[serde(rename = "vexpr")]
    Vexpr { v: ValExpr },
    /// `{ t: 'param', name, sign? }` — F3 authored-only: a function parameter
    /// standing in an arg position (§9.40); `sign` carries the +/- of a delta
    /// position (`+n`). Calls expand at load — the engine and the log never
    /// see one.
    #[serde(rename = "param")]
    Param {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sign: Option<Sign>,
    },
}

impl Arg {
    /// The TS `t` tag string — error messages name the tag
    /// (`the call supplies a ${bound.t}`, fn_expand substExpr/substArg).
    #[must_use]
    pub fn tag(&self) -> &'static str {
        match self {
            Arg::Ref { .. } => "ref",
            Arg::Role { .. } => "role",
            Arg::Int { .. } => "int",
            Arg::Delta { .. } => "delta",
            Arg::Str { .. } => "str",
            Arg::Bool { .. } => "bool",
            Arg::Dur { .. } => "dur",
            Arg::Key { .. } => "key",
            Arg::Into { .. } => "into",
            Arg::AsId { .. } => "asId",
            Arg::Pair { .. } => "pair",
            Arg::Vexpr { .. } => "vexpr",
            Arg::Param { .. } => "param",
        }
    }
}

/// ts: src/core/sentence.ts:42-45 — one IF/ELSE IF arm (§9.39): a condition
/// and its paren'd outcome body. Serde derives exist only so [`Outcome`]
/// (whose `arms` field holds these) has one representation — IF outcomes are
/// authored form only and never reach the log (§9.39), so the wire shape of
/// this type is unobservable.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct IfArm {
    pub when: Pred,
    pub then: Vec<Outcome>,
}

/// ts: src/core/sentence.ts:47-62. The serde field order matches the only
/// construction shape that can reach the log — `{ op, args }` (parse.ts:356+,
/// engine.ts:520-591); `arms`/`fallback`/`call` are the authored-only
/// IF/FN extras and serialize as absent exactly like `JSON.stringify`
/// dropping `undefined`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Outcome {
    pub op: String,
    pub args: Vec<Arg>,
    /// op 'IF' only (§9.39): the arms in source order, ELSE IF chain consumed
    /// greedily INSIDE the node. Authored form only — evaluation flattens the
    /// TAKEN branch's outcomes into the record position; the log never
    /// contains an IF op.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arms: Option<Vec<IfArm>>,
    /// the ELSE body, or None when authored without one (no true arm → nothing)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<Vec<Outcome>>,
    /// op 'FN' only (§9.40, F3): the declared function this authored call
    /// invokes. Authored form only — calls expand at load time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<String>,
}

/// ts: src/core/sentence.ts:65 — `kind: 'action' | 'event' | 'complete' | 'wait'`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SentenceKind {
    Action,
    Event,
    Complete,
    Wait,
}

impl SentenceKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SentenceKind::Action => "action",
            SentenceKind::Event => "event",
            SentenceKind::Complete => "complete",
            SentenceKind::Wait => "wait",
        }
    }
}

/// ts: src/core/sentence.ts:64-73
#[derive(Debug, Clone, PartialEq)]
pub struct Sentence {
    pub kind: SentenceKind,
    pub clock: Option<Clock>,
    /// event trigger id; the VERB of a COMPLETE row
    pub trigger: Option<String>,
    pub clauses: Vec<Clause>,
    pub outcomes: Vec<Outcome>,
    /// wait rows only: the advanced span in minutes (records carry the same
    /// value as advancedMin — the clock row is the only clock-mover, v0.5)
    pub span_min: Option<i64>,
}

/// ts: src/core/sentence.ts:75-77 — MUST be ported: explorer-lib/reach-fuzz-lib
/// consume it (plan §1.2 "死代码不移植" list explicitly spares emptyClause).
#[must_use]
pub fn empty_clause(actors: Vec<Ref>) -> Clause {
    Clause {
        actors,
        at: None,
        verb: String::new(),
        to: None,
        objects: Vec::new(),
        with: Vec::new(),
        about: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::r#ref::id_ref;

    #[test]
    fn empty_clause_mirrors_the_ts_shape() {
        let c = empty_clause(vec![id_ref("e1")]);
        assert_eq!(c.actors.len(), 1);
        assert!(c.verb.is_empty());
        assert!(c.at.is_none() && c.to.is_none() && c.about.is_none());
        assert!(c.objects.is_empty() && c.with.is_empty());
    }

    #[test]
    fn arg_tags_match_the_ts_discriminator_strings() {
        assert_eq!(Arg::Int { v: 3 }.tag(), "int");
        assert_eq!(Arg::Delta { v: -1 }.tag(), "delta");
        assert_eq!(
            Arg::Param {
                name: "n".into(),
                sign: Some(Sign::Neg)
            }
            .tag(),
            "param"
        );
        assert_eq!(Sign::Neg.apply(5), -5);
        assert_eq!(Sign::Pos.as_i64(), 1);
    }
}
