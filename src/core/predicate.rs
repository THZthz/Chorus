//! Condition predicates for recipe `when` (spec
//! §6.3). The SAME grammar serves the IF conditions of outcome lists (§9.39):
//! `parse_predicate` string-parses `when:` lines; `parse_predicate_toks`
//! parses an IF condition out of the SENTENCE token stream. One grammar, two
//! front-ends — no duplicated semantics.

use std::collections::HashSet;
use std::sync::OnceLock;

use indexmap::IndexMap;
use regex::Regex;

use crate::core::query::{match_query, parse_cmp_op, parse_query, CmpOp, Query};
use crate::core::roles::{Role, ROLES};
use crate::core::sentence::Outcome;
use crate::core::lexer::Tok;
use crate::core::types::{aspect_get, relation_alive, Entity, State};
use crate::core::num_parse::parse_authored_int;
use crate::core::regex_util::static_regex;
use crate::core::text::{is_dsl_ws_char, DSL_WS_CLASS};
use crate::error::{ChorusError, Result};

/// Role bindings: role → bound entity IDS (`IndexMap<Role, Vec<String>>`),
/// never
/// cloned entities: role_cards and eval_pred must read the CURRENT dead_action
/// through `state.entities.get` mid-list (dead-stamp reads false silently).
pub type RoleBindings = IndexMap<Role, Vec<String>>;

/// The evaluation context: bindings, state, strictness, dice.
pub struct PredCtx<'a> {
    pub b: &'a RoleBindings,
    pub state: &'a State,
    /// §9.39 asymmetric failure discipline: IF conditions evaluate STRICTLY —
    /// a path operand whose role has no bound card REJECTS the step (§9.19),
    /// where a `when:` predicate would silently read false.
    pub strict: bool,
    /// §9.41: the apply context's per-clause dice binding — the last ROLL of
    /// this outcome list, last-wins. Absent during recipe `when:` selection
    /// and at fold.
    pub roll: Option<i64>,
}

/// The path field: 'aspect' | 'attr'.
/// Serde derives: type-graph completeness only (predicates never serialize
/// into the log — `when:` lives in compiled content, IF arms are authored
/// form only), so the default wire shape is fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PredField {
    Aspect,
    Attr,
}

impl PredField {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            PredField::Aspect => "aspect",
            PredField::Attr => "attr",
        }
    }

    /// The two-word field validation, shared by every path grammar site.
    #[must_use]
    pub fn from_name(s: &str) -> Option<PredField> {
        match s {
            "aspect" => Some(PredField::Aspect),
            "attr" => Some(PredField::Attr),
            _ => None,
        }
    }
}

/// Serde derives: type-graph completeness
/// only (see PredField — never on the wire).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Pred {
    And(Box<Pred>, Box<Pred>),
    Or(Box<Pred>, Box<Pred>),
    Not(Box<Pred>),
    Cmp { l: ValExpr, op: CmpOp, r: ValExpr },
    Rel { rtype: String, role: String, role2: Option<String> },
    Every { role: String, cond: ScalarCond },
    Any { role: String, cond: ScalarCond },
    /// ∃ an ALIVE entity bearing this exact name
    Exists { name: String },
    /// ∃ an alive entity matching a §4.5 slot query
    ExistsQuery { query: Query },
}

/// Serde derives: type-graph completeness
/// only (Arg::Vexpr's payload; ROLL args are authored-only and the log
/// records computed ROLLED ints, so this is never on the wire).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ValExpr {
    Int { v: i64 },
    Count { role: String },
    /// §9.38: count over the whole living world (any §4.5 query)
    CountQuery { query: Query },
    Total { role: String, path: String },
    Path { role: String, field: PredField, key: String },
    /// F3 authored-only: a function parameter standing in a cmp operand
    /// (§9.40); expansion replaces it with the call's number — it never
    /// reaches evaluation.
    Param { name: String },
    /// §9.41: the per-clause dice binding from the APPLY context. Parses only
    /// in the Tok front-end (IF conditions, ROLL args).
    Roll,
}

/// Serde derives: type-graph completeness
/// only (see PredField).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScalarCond {
    pub path: ScalarPath,
    pub op: CmpOp,
    pub value: i64,
}

/// `{ field: 'aspect' | 'attr', key }`. Serde derives:
/// type-graph completeness only (see PredField).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ScalarPath {
    pub field: PredField,
    pub key: String,
}

// ---------- parser (precedence climbing: NOT > AND > OR) ----------

/// The comparison-op alternation; lookup order preserved
/// (leftmost-first matching).
const CMP_OPS: [&str; 6] = [">=", "<=", "!=", "=", ">", "<"];

/// A pre-lexed predicate: lexemes + source
/// spans + the source the spans index. The string front-end builds it from
/// the regex tokenizer below; the Tok front-end (parse_predicate_toks)
/// re-serializes sentence Toks into the same shape, so every grammar rule —
/// including the exists()/count() inner-query reconstruction, which reads
/// source offsets — behaves identically. (Spans are BYTE offsets:
/// all token edges are ASCII, so byte offsets land on the same boundaries
/// UTF-16 offsets would; the substring CONTENT is identical either way.)
#[derive(Debug, Clone)]
pub struct PredView {
    pub src: String,
    pub toks: Vec<String>,
    pub spans: Vec<(usize, usize)>,
}

/// Parse one `when:` predicate string.
pub fn parse_predicate(src: &str) -> Result<Pred> {
    PredParser::new(src, None, None, false)?.parse()
}

/// Parse one IF condition out of the
/// SENTENCE token stream (§9.39) and report where it ended. Returns
/// `(pred, end)` where `end` is the number of Toks consumed, i.e. the index
/// of the `->` arrow in the given slice.
pub fn parse_predicate_toks(
    toks: &[Tok],
    params: Option<&HashSet<String>>,
) -> Result<(Pred, usize)> {
    let mut depth: i64 = 0;
    let mut end: Option<usize> = None;
    for (i, t) in toks.iter().enumerate() {
        if let Tok::Punct { v } = t {
            if v == "(" {
                depth += 1;
            } else if v == ")" {
                depth -= 1;
                if depth < 0 {
                    return Err(ChorusError::new("unbalanced ')' in IF condition"));
                }
            } else if depth == 0 && v == "->" {
                end = Some(i);
                break;
            } else if depth == 0 && (v == "," || v == ";") {
                return Err(ChorusError::new("an IF condition needs '-> ( outcomes )'"));
            }
        } else if depth == 0 {
            if let Tok::Name { v } = t {
                if v == "ELSE" {
                    return Err(ChorusError::new(
                        "an IF needs '-> ( outcomes )' before 'ELSE'",
                    ));
                }
            }
        }
    }
    let Some(end) = end else {
        return Err(ChorusError::new(
            "an IF condition needs '-> ( outcomes )'",
        ));
    };
    let slice = &toks[..end];
    if slice.is_empty() {
        return Err(ChorusError::new("an IF needs a condition before '->'"));
    }
    let PredView { src, toks, spans } = pred_view_from_toks(slice)?;
    // `allow_roll`: `roll` (§9.41) is an outcome-list operand — an IF condition
    // may read the list's dice; a `when:` line (the string front-end) may not
    let parser = PredParser {
        toks,
        spans,
        pos: 0,
        src,
        params,
        allow_roll: true,
    };
    let pred = parser.parse()?;
    Ok((pred, end))
}

/// ONE standalone value expression
/// (§9.41): the ROLL argument grammar — one grammar, three front-ends.
pub fn parse_value_expr_toks(
    toks: &[Tok],
    params: Option<&HashSet<String>>,
) -> Result<ValExpr> {
    let PredView { src, toks, spans } = pred_view_from_toks(toks)?;
    let parser = PredParser {
        toks,
        spans,
        pos: 0,
        src,
        params,
        allow_roll: true,
    };
    parser.parse_value_expr()
}

/// Sentence Toks → this parser's own
/// token alphabet (§9.39). Word-adjacent lexemes are space-separated so they
/// cannot merge; everything else glues exactly as the string grammar expects
/// (`aspect:hunter>=1`, `state:!sealed`, dotted paths, `-aspect:winter`).
/// The glued-string + span shape is load-bearing: the inner-query
/// reconstruction reads source offsets, so error texts carry the glued
/// traces — do NOT refactor to direct Tok interpretation.
pub fn pred_view_from_toks(toks: &[Tok]) -> Result<PredView> {
    let mut src = String::new();
    let mut ptoks: Vec<String> = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    // word-edge detection: a lexeme ending in a word char glues a space
    // before a lexeme starting with one
    static END_WORD: OnceLock<Regex> = OnceLock::new();
    static START_WORD: OnceLock<Regex> = OnceLock::new();
    let end_word = static_regex(&END_WORD, || Regex::new(r"[A-Za-z0-9_]$"));
    let start_word = static_regex(&START_WORD, || Regex::new(r"^[A-Za-z0-9_]"));
    for t in toks {
        let text: String = match t {
            Tok::Name { v } => v.clone(),
            Tok::Int { v } => v.to_string(),
            Tok::Punct { v } => v.clone(),
            Tok::Str { .. } => {
                return Err(ChorusError::new(
                    "an IF condition cannot contain a string literal",
                ))
            }
            Tok::Dur { .. } => {
                return Err(ChorusError::new(
                    "an IF condition cannot contain a duration literal",
                ))
            }
            Tok::IdRef { .. } => {
                return Err(ChorusError::new(
                    "an IF condition cannot contain a #id literal",
                ))
            }
        };
        if !spans.is_empty() && end_word.is_match(&src) && start_word.is_match(&text) {
            src.push(' ');
        }
        spans.push((src.len(), src.len() + text.len()));
        src.push_str(&text);
        ptoks.push(text);
    }
    Ok(PredView { src, toks: ptoks, spans })
}

/// The recursive-descent predicate parser.
struct PredParser<'a> {
    toks: Vec<String>,
    spans: Vec<(usize, usize)>,
    pos: usize,
    src: String,
    params: Option<&'a HashSet<String>>,
    allow_roll: bool,
}

impl<'a> PredParser<'a> {
    /// The string front-end constructor.
    fn new(
        src: &str,
        pre: Option<PredView>,
        params: Option<&'a HashSet<String>>,
        allow_roll: bool,
    ) -> Result<PredParser<'a>> {
        if let Some(PredView { toks, spans, .. }) = pre {
            return Ok(PredParser {
                toks,
                spans,
                pos: 0,
                src: src.to_string(),
                params,
                allow_roll,
            });
        }
        // §6.3's grammar has no characters beyond the alternatives below:
        // one outside the vocabulary (plus whitespace) is a LOAD ERROR
        // naming the character and its offset — formerly QUIRK(Q2), the
        // tokenizer silently swallowed it (`aspect:x >= @3` ≡
        // `aspect:x>=3`, the `@` just vanished). Whitespace stays glue
        // (§6.3's only license: "spaces around a comparison inside a
        // query are glue") — the ONLY silent skip. `!` is a token because
        // exists()/count() embed §4.5 queries verbatim (`state:!sealed`)
        // and the inner text is reconstructed from the source, so its
        // characters must survive; a stray `!` still errors loudly at the
        // parser ("expected comparison op, got '!'"), never silently.
        // `|` survives the same way (§9.49): a LONE `|` rides verbatim
        // into the inner query as the group separator; outside a query it
        // errors loudly at the parser ("expected comparison op, got '|'").
        // (`\d` is ASCII `[0-9]`; offsets are byte offsets, the spans
        // convention.)
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = static_regex(&RE, || {
            Regex::new(r"[A-Za-z_][A-Za-z0-9_.]*|[0-9]+|>=|<=|!=|!|=|>|<|\(|\)|,|:|-|\|")
        });
        let mut toks = Vec::new();
        let mut spans = Vec::new();
        let mut at = 0;
        while at < src.len() {
            let Some(c) = src[at..].chars().next() else {
                unreachable!(
                    "at is always a char boundary: the scanner advances by whole tokens and whitespace chars only"
                );
            };
            if is_dsl_ws_char(c) {
                at += c.len_utf8();
                continue;
            }
            match re.find_at(src, at) {
                Some(m) if m.start() == at => {
                    spans.push((m.start(), m.end()));
                    toks.push(m.as_str().to_string());
                    at = m.end();
                }
                _ => {
                    return Err(ChorusError::new(format!(
                        "illegal character '{c}' at offset {at} in predicate '{src}'"
                    )));
                }
            }
        }
        Ok(PredParser {
            toks,
            spans,
            pos: 0,
            src: src.to_string(),
            params,
            allow_roll,
        })
    }

    fn peek(&self) -> Option<&String> {
        self.toks.get(self.pos)
    }

    /// Next token, or the end-of-predicate error.
    fn next(&mut self) -> Result<String> {
        match self.peek() {
            Some(t) => {
                let t = t.clone();
                self.pos += 1;
                Ok(t)
            }
            None => Err(ChorusError::new("unexpected end of predicate")),
        }
    }

    /// Consume `w` or fail.
    fn expect(&mut self, w: &str) -> Result<()> {
        let t = self.next()?;
        if t != w {
            return Err(ChorusError::new(format!("expected '{w}', got '{t}'")));
        }
        Ok(())
    }

    /// Parse one full predicate.
    fn parse(mut self) -> Result<Pred> {
        if self.toks.is_empty() {
            // the empty-predicate error surfaces at parse call time, not
            // construction (same message, same caller-visible failure)
            return Err(ChorusError::new(format!("empty predicate '{}'", self.src)));
        }
        let p = self.or()?;
        if self.pos != self.toks.len() {
            return Err(ChorusError::new(format!(
                "trailing tokens in predicate at '{}'",
                self.peek().map(String::as_str).unwrap_or("?")
            )));
        }
        Ok(p)
    }

    /// One full value expression (§9.41 ROLL
    /// args): the §6.3 operand grammar, all tokens consumed.
    fn parse_value_expr(mut self) -> Result<ValExpr> {
        let v = self.val_expr()?;
        if self.pos != self.toks.len() {
            return Err(ChorusError::new(format!(
                "trailing tokens in ROLL argument at '{}'",
                self.peek().map(String::as_str).unwrap_or("?")
            )));
        }
        Ok(v)
    }

    /// or := and ('OR' and)*.
    fn or(&mut self) -> Result<Pred> {
        let mut a = self.and()?;
        while self.peek().map(|t| t.as_str()) == Some("OR") {
            self.next()?;
            let b = self.and()?;
            a = Pred::Or(Box::new(a), Box::new(b));
        }
        Ok(a)
    }

    /// and := unary ('AND' unary)*.
    fn and(&mut self) -> Result<Pred> {
        let mut a = self.unary()?;
        while self.peek().map(|t| t.as_str()) == Some("AND") {
            self.next()?;
            let b = self.unary()?;
            a = Pred::And(Box::new(a), Box::new(b));
        }
        Ok(a)
    }

    /// unary := 'NOT' unary | atom.
    fn unary(&mut self) -> Result<Pred> {
        if self.peek().map(|t| t.as_str()) == Some("NOT") {
            self.next()?;
            let a = self.unary()?;
            return Ok(Pred::Not(Box::new(a)));
        }
        self.atom()
    }

    /// atom := '(' or ')' | every/any | exists | rel | cmp.
    fn atom(&mut self) -> Result<Pred> {
        if self.peek().map(|t| t.as_str()) == Some("(") {
            self.next()?;
            let p = self.or()?;
            self.expect(")")?;
            return Ok(p);
        }
        let t = self.peek().map(String::as_str);
        if t == Some("every") || t == Some("any") {
            return self.quantified();
        }
        if t == Some("exists") {
            return self.exists_pred();
        }
        if t == Some("rel") {
            self.next()?;
            self.expect("(")?;
            let rtype = self.dotted_name()?;
            self.expect(",")?;
            let role = self.dotted_name()?;
            let mut role2: Option<String> = None;
            if self.peek().map(|t| t.as_str()) == Some(",") {
                self.next()?;
                role2 = Some(self.dotted_name()?);
            }
            self.expect(")")?;
            return Ok(Pred::Rel { rtype, role, role2 });
        }
        self.cmp_from_expr()
    }

    /// quantified := ('every'|'any') '(' role ',' scalarCond ')'.
    fn quantified(&mut self) -> Result<Pred> {
        let kind = self.next()?;
        self.expect("(")?;
        let role = self.next()?;
        self.expect(",")?;
        let cond = self.scalar_cond()?;
        self.expect(")")?;
        Ok(if kind == "every" {
            Pred::Every { role, cond }
        } else {
            Pred::Any { role, cond }
        })
    }

    /// exists(NAME): an ALIVE entity bears the
    /// name; exists(<query>): an alive entity matches the §4.5 slot query.
    fn exists_pred(&mut self) -> Result<Pred> {
        self.next()?; // 'exists'
        self.expect("(")?;
        let first = self.pos;
        let mut depth: i64 = 1;
        while depth > 0 {
            let tk = self.next()?;
            if tk == "(" {
                depth += 1;
            } else if tk == ")" {
                depth -= 1;
            }
        }
        let close = self.pos - 1; // token index of the matching ')'
        if close == first {
            return Err(ChorusError::new("exists() needs a name or a slot query"));
        }
        // the inner text is cut from the source spans
        let inner = self.src[self.spans[first].0..self.spans[close].0].to_string();
        if self.query_starter_at(first) {
            // §4.5 splits terms on whitespace, so tighten `aspect:hunter >= 1`
            // to `aspect:hunter>=1` — spaces around a comparison are glue,
            // not structure (the replacement keeps only the captured op)
            static CMP: OnceLock<Regex> = OnceLock::new();
            let re = static_regex(&CMP, || {
                Regex::new(&format!(r"{}(>=|<=|!=|=|>|<){}", DSL_WS_CLASS, DSL_WS_CLASS))
            });
            let query = parse_query(&re.replace_all(&inner, "$1"))?;
            return Ok(Pred::ExistsQuery { query });
        }
        if close - first > 1 {
            return Err(ChorusError::new(format!(
                "exists() takes one name or a kind:/aspect:/state:/name: query, got '{}'",
                inner.trim()
            )));
        }
        Ok(Pred::Exists {
            name: self.toks[first].clone(),
        })
    }

    /// A query-term starter at token i:
    /// kind:/aspect:/state:/name: or a leading `-` (§4.5, §9.38).
    fn query_starter_at(&self, i: usize) -> bool {
        let Some(head) = self.toks.get(i) else {
            return false;
        };
        head == "-" || {
            matches!(head.as_str(), "kind" | "aspect" | "state" | "name")
                && self.toks.get(i + 1).map(|t| t.as_str()) == Some(":")
        }
    }

    /// scalarCond := 'aspect' ':' key cmp int.
    fn scalar_cond(&mut self) -> Result<ScalarCond> {
        self.expect("aspect")?; // v0.2: scalar conditions read aspects
        self.expect(":")?;
        let key = self.dotted_name()?;
        let op_tok = self.next()?;
        let Some(op) = CMP_OPS.iter().find(|o| **o == op_tok) else {
            return Err(ChorusError::new(format!(
                "expected comparison op, got '{op_tok}'"
            )));
        };
        let v = self.next()?;
        // ASCII digits only
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = static_regex(&RE, || Regex::new(r"^[0-9]+$"));
        if !re.is_match(&v) {
            return Err(ChorusError::new(format!(
                "expected integer in scalar condition, got '{v}'"
            )));
        }
        Ok(ScalarCond {
            path: ScalarPath {
                field: PredField::Aspect,
                key,
            },
            op: parse_cmp_op(op),
            value: parse_authored_int(&v),
        })
    }

    /// cmp := valExpr cmpOp valExpr.
    fn cmp_from_expr(&mut self) -> Result<Pred> {
        let l = self.val_expr()?;
        let op_tok = self.next()?;
        let Some(op) = CMP_OPS.iter().find(|o| **o == op_tok) else {
            return Err(ChorusError::new(format!(
                "expected comparison op, got '{op_tok}'"
            )));
        };
        let r = self.val_expr()?;
        Ok(Pred::Cmp {
            l,
            op: parse_cmp_op(op),
            r,
        })
    }

    /// One operand: param | roll | int | count/total | role.field.key path.
    fn val_expr(&mut self) -> Result<ValExpr> {
        let t = self.next()?;
        // F3 (§9.40): a bare parameter name is a param operand — expansion
        // binds it to the call's number.
        if self.params.is_some_and(|p| p.contains(&t)) {
            return Ok(ValExpr::Param { name: t });
        }
        // §9.41: the dice operand — Tok front-ends only. A `when:` line
        // parses through the string front-end, where selection provably runs
        // before any outcome could have rolled.
        if t == "roll" {
            if !self.allow_roll {
                return Err(ChorusError::new(
                    "'roll' is not available in 'when:' predicates — it binds inside an outcome list, after a ROLL (§9.41)",
                ));
            }
            return Ok(ValExpr::Roll);
        }
        static RE: OnceLock<Regex> = OnceLock::new();
        let int_re = static_regex(&RE, || Regex::new(r"^[0-9]+$"));
        if int_re.is_match(&t) {
            return Ok(ValExpr::Int { v: parse_authored_int(&t) });
        }
        if t == "count" {
            self.expect("(")?;
            // §9.38: count(role) vs count(<query>) — a query-term starter
            // decides, mirroring exists(); the paren'd arg is consumed
            // balanced regardless
            if self.query_starter_at(self.pos) {
                let first = self.pos;
                let mut depth: i64 = 1;
                while depth > 0 {
                    let tk = self.next()?;
                    if tk == "(" {
                        depth += 1;
                    } else if tk == ")" {
                        depth -= 1;
                    }
                }
                let close = self.pos - 1;
                if close == first {
                    return Err(ChorusError::new("count() needs a role or a slot query"));
                }
                let inner = self.src[self.spans[first].0..self.spans[close].0].to_string();
                static CMP: OnceLock<Regex> = OnceLock::new();
                let re = static_regex(&CMP, || {
                    Regex::new(&format!(r"{}(>=|<=|!=|=|>|<){}", DSL_WS_CLASS, DSL_WS_CLASS))
                });
                let query = parse_query(&re.replace_all(&inner, "$1"))?;
                return Ok(ValExpr::CountQuery { query });
            }
            let role = self.next()?;
            self.expect(")")?;
            return Ok(ValExpr::Count { role });
        }
        if t == "total" {
            self.expect("(")?;
            // total pools a bound ROLE's aspects — the query form is
            // deliberately not offered (§9.38): fail with the rule
            if self.query_starter_at(self.pos) {
                return Err(ChorusError::new(
                    "total() pools a bound role's aspects — total(<query>, …) is not offered; use count(<query>) to count a pool by name (e.g. count(name:X))",
                ));
            }
            let role = self.next()?;
            self.expect(",")?;
            self.expect("aspect")?;
            self.expect(":")?;
            let path = self.dotted_name()?;
            self.expect(")")?;
            return Ok(ValExpr::Total { role, path });
        }
        // path operand: role '.' (aspect|attr) '.' dottedKey — the tokenizer
        // may deliver the whole dotted path as one token, so handle both shapes
        if t.contains('.') {
            let parts: Vec<&str> = t.split('.').collect();
            if parts.len() < 3 {
                return Err(ChorusError::new(format!("bad path '{t}'")));
            }
            let Some(field) = PredField::from_name(parts[1]) else {
                return Err(ChorusError::new(format!(
                    "expected aspect|attr in path '{t}'"
                )));
            };
            return Ok(ValExpr::Path {
                role: parts[0].to_string(),
                field,
                key: parts[2..].join("."),
            });
        }
        self.expect(".")?;
        let field_tok = self.next()?;
        let Some(field) = PredField::from_name(&field_tok) else {
            return Err(ChorusError::new(format!(
                "expected aspect|attr after '{t}.', got '{field_tok}'"
            )));
        };
        self.expect(".")?; // the second dot of role.field.key (split-token shape, §9.39)
        let key = self.dotted_name()?;
        Ok(ValExpr::Path { role: t, field, key })
    }

    /// A dotted name consumed as one logical lexeme.
    fn dotted_name(&mut self) -> Result<String> {
        let mut name = self.next()?;
        while self.peek().map(|t| t.as_str()) == Some(".") {
            self.next()?;
            name.push('.');
            name.push_str(&self.next()?);
        }
        Ok(name)
    }
}

// ---------- evaluation ----------

/// A role's bound cards, ALIVE only
/// (§9.39 draft reads). Reads the CURRENT entities through the id handles:
/// a dead-stamped entity reads as absent mid-list —
/// `DESTROY OBJECT` then `IF object.aspect.x` is a silent false.
fn role_cards<'a>(ctx: &PredCtx<'a>, role: &str) -> Vec<&'a Entity> {
    let Some(r) = Role::from_lower(role) else {
        return Vec::new(); // a missing role binds nothing — reads as empty
    };
    ctx.b
        .get(&r)
        .map(|ids| {
            ids.iter()
                .filter_map(|id| ctx.state.entities.get(id))
                .filter(|e| e.dead_action.is_none())
                .collect()
        })
        .unwrap_or_default()
}

/// Evaluate one predicate node.
pub fn eval_pred(p: &Pred, ctx: &PredCtx) -> Result<bool> {
    match p {
        Pred::And(a, b) => {
            // `&&` short-circuits: b is not evaluated (and may error)
            if eval_pred(a, ctx)? {
                eval_pred(b, ctx)
            } else {
                Ok(false)
            }
        }
        Pred::Or(a, b) => {
            if eval_pred(a, ctx)? {
                Ok(true)
            } else {
                eval_pred(b, ctx)
            }
        }
        Pred::Not(a) => Ok(!eval_pred(a, ctx)?),
        Pred::Rel { rtype, role, role2 } => {
            // rel(type, role) pairs cards within one role; rel(type, a, b)
            // crosses two roles — e.g. rel(neighbor, actors.container, object)
            let cards_of = |role: &str| -> Vec<&Entity> {
                let Some(base) = role.strip_suffix(".container") else {
                    return role_cards(ctx, role);
                };
                let cards = role_cards(ctx, base);
                cards
                    .into_iter()
                    .map(|e| {
                        // container redirect: the container attr's stringified
                        // value names the container entity, else the card itself
                        // (the canonical string spelling)
                        let cid = e
                            .attrs
                            .get("container")
                            .map(|v| v.to_canonical_string())
                            .unwrap_or_default();
                        ctx.state.entities.get(&cid).unwrap_or(e)
                    })
                    .collect()
            };
            let a = cards_of(role);
            let b = cards_of(role2.as_deref().unwrap_or(role));
            for (i, ca) in a.iter().enumerate() {
                let start = if role2.is_none() { i + 1 } else { 0 };
                for cb in b.iter().skip(start) {
                    if role2.is_none() && ca.id == cb.id {
                        continue;
                    }
                    if relation_alive(ctx.state, &ca.id, &cb.id, rtype) {
                        return Ok(true);
                    }
                }
            }
            Ok(false)
        }
        Pred::Every { role, cond } => {
            let cards = role_cards(ctx, role);
            Ok(cards.iter().all(|e| scalar_ok(e, cond)))
        }
        Pred::Any { role, cond } => {
            let cards = role_cards(ctx, role);
            Ok(cards.iter().any(|e| scalar_ok(e, cond)))
        }
        Pred::Exists { name } => {
            // `by_name` holds ALIVE entities only (§9.14); the `dead_action`
            // re-check is the same belt-and-braces `resolve_ref` wears. Under
            // pools (§9.38) the existential is ANY living copy of the name.
            match ctx.state.by_name.get(name) {
                Some(pool) => Ok(pool
                    .iter()
                    .any(|id| ctx.state.entities.get(id).is_some_and(|e| e.dead_action.is_none()))),
                None => Ok(false),
            }
        }
        Pred::ExistsQuery { query } => {
            // the WHOLE world, not the step's bound roles — CS extantreqs
            // ("anywhere in play"); entities insertion order is the scan
            // order (match_query is order-insensitive per-card)
            for e in ctx.state.entities.values() {
                if e.dead_action.is_none() && match_query(e, query) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Pred::Cmp { l, op, r } => {
            // QUIRK: either operand missing → false (a missing value
            // never compares)
            let lv = eval_expr(l, ctx)?;
            let rv = eval_expr(r, ctx)?;
            Ok(match (lv, rv) {
                (Some(lv), Some(rv)) => op.num_cmp(lv, rv),
                _ => false,
            })
        }
    }
}

/// Evaluate a scalar condition (missing aspect → false).
fn scalar_ok(e: &Entity, cond: &ScalarCond) -> bool {
    let v = match cond.path.field {
        PredField::Aspect => aspect_get(e, &cond.path.key),
        PredField::Attr => number_attr(e, &cond.path.key),
    };
    match v {
        Some(v) => cond.op.num_cmp(v, cond.value),
        None => false,
    }
}

/// Numeric attr read (non-number → None).
fn number_attr(e: &Entity, key: &str) -> Option<i64> {
    match e.attrs.get(key) {
        Some(crate::core::types::AttrValue::Num(n)) => Some(*n),
        _ => None,
    }
}

/// Evaluate one value expression (None = unresolved).
pub fn eval_expr(x: &ValExpr, ctx: &PredCtx) -> Result<Option<i64>> {
    match x {
        ValExpr::Int { v } => Ok(Some(*v)),
        ValExpr::Roll => {
            // §9.41: unbound in a taken branch is a step rejection (§9.19) —
            // never a silent false or an implicit 0
            match ctx.roll {
                Some(roll) => Ok(Some(roll)),
                None => Err(ChorusError::new(
                    "'roll' is unbound — no ROLL has run in this outcome list before this point",
                )),
            }
        }
        ValExpr::Count { role } => Ok(Some(role_cards(ctx, role).len() as i64)),
        ValExpr::CountQuery { query } => {
            // world scope, like `ExistsQuery` (§6.3): every ALIVE card matching
            let mut n: i64 = 0;
            for e in ctx.state.entities.values() {
                if e.dead_action.is_none() && match_query(e, query) {
                    n += 1;
                }
            }
            Ok(Some(n))
        }
        ValExpr::Total { role, path } => {
            // reduce along role_cards order; saturating add — only absurd
            // magnitudes overflow, and no log pins their sums
            let sum = role_cards(ctx, role)
                .iter()
                .fold(0i64, |sum, e| sum.saturating_add(aspect_get(e, path).unwrap_or(0)));
            Ok(Some(sum))
        }
        ValExpr::Path { role, field, key } => {
            // the oldest ALIVE bound card: first id, in binding order, that
            // is still alive
            let bound: &[String] = match Role::from_lower(role) {
                Some(r) => ctx.b.get(&r).map(|v| v.as_slice()).unwrap_or(&[]),
                None => &[], // an undeclared role binds nothing
            };
            let found = bound
                .iter()
                .filter_map(|id| ctx.state.entities.get(id))
                .find(|e| e.dead_action.is_none());
            match found {
                Some(e) => Ok(match field {
                    PredField::Aspect => aspect_get(e, key),
                    PredField::Attr => number_attr(e, key),
                }),
                None => {
                    // §9.39: an IF condition names a CARD that is not there —
                    // an unresolvable ref, a step rejection (§9.19) — but ONLY
                    // when the role has NO bindings at all; all-dead bindings
                    // read false silently (the dead-stamp rule)
                    if ctx.strict && bound.is_empty() {
                        return Err(ChorusError::new(format!(
                            "IF condition: operand '{role}.{field}.{key}' cannot resolve — role '{role}' has no bound card",
                            field = field.as_str()
                        )));
                    }
                    Ok(None)
                }
            }
        }
        ValExpr::Param { name } => {
            // expansion (§9.40) replaces every param before compile finishes;
            // this is a defensive tripwire, never a gameplay path
            Err(ChorusError::new(format!(
                "internal: unresolved function parameter '{name}' was evaluated"
            )))
        }
    }
}

// ---------- load-time argument lint (§9.38) ----------

/// `ROLES` joined with ', ' (the ROLE_LIST message text).
#[must_use]
pub fn role_list() -> String {
    ROLES
        .iter()
        .map(|r| r.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// count()/total() arguments must be a
/// DECLARED ROLE or a query-term starter. Called at content compile for
/// every recipe `when`; the runtime path is unaffected.
pub fn lint_pred(where_: &str, p: &Pred) -> Result<()> {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => {
            lint_pred(where_, a)?;
            lint_pred(where_, b)
        }
        Pred::Not(a) => lint_pred(where_, a),
        Pred::Cmp { l, r, .. } => {
            lint_val_expr(where_, l)?;
            lint_val_expr(where_, r)
        }
        // Rel/Every/Any/Exists/ExistsQuery carry no count/total args
        _ => Ok(()),
    }
}

/// Lint the count()/total() operand expressions of a predicate.
pub fn lint_val_expr(where_: &str, x: &ValExpr) -> Result<()> {
    let is_role = |role: &str| Role::from_lower(role).is_some();
    if let ValExpr::Count { role } = x {
        if !is_role(role) {
            return Err(ChorusError::new(format!(
                "{where_}: count() argument '{role}' is neither a role ({}) nor a query (name:x, kind:k, aspect:a>=1, state:s, -term) — did you mean count(name:{role})?",
                role_list()
            )));
        }
    }
    if let ValExpr::Total { role, .. } = x {
        if !is_role(role) {
            return Err(ChorusError::new(format!(
                "{where_}: total() first argument '{role}' is not a declared role ({}) — total pools a role's aspects; use count(name:x) to count a pool by name",
                role_list()
            )));
        }
    }
    Ok(())
}

/// Trigger (slot + expiry) outcome lists
/// have EMPTY event bindings, so any role-dependent operand in an IF
/// condition there would silently read false/0. Called from content compile.
pub fn lint_trigger_pred(where_: &str, p: &Pred) -> Result<()> {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => {
            lint_trigger_pred(where_, a)?;
            lint_trigger_pred(where_, b)
        }
        Pred::Not(a) => lint_trigger_pred(where_, a),
        Pred::Cmp { l, r, .. } => {
            lint_trigger_val_expr(where_, l)?;
            lint_trigger_val_expr(where_, r)
        }
        Pred::Rel { rtype, role, role2 } => Err(ChorusError::new(format!(
            "{where_}: IF operand rel({rtype}, {role}{role2}) reads a step role — trigger bindings are empty, it would always be false; use world forms (exists(…), count(name:…))",
            role2 = role2.as_ref().map(|r| format!(", {r}")).unwrap_or_default()
        ))),
        Pred::Every { role, .. } | Pred::Any { role, .. } => {
            let is_every = matches!(p, Pred::Every { .. });
            Err(ChorusError::new(format!(
                "{where_}: IF operand {t}({role}, …) reads a step role — trigger bindings are empty, it would always be {b}; use world forms (exists(…), count(name:…))",
                t = if is_every { "every" } else { "any" },
                b = if is_every { "true" } else { "false" }
            )))
        }
        _ => Ok(()), // `Exists`/`ExistsQuery` are world forms
    }
}

/// `what` names the operand kind in the messages
/// ('IF operand' by default; content compile passes 'ROLL argument' for
/// vexpr operands).
pub fn lint_trigger_val_expr_what(where_: &str, x: &ValExpr, what: &str) -> Result<()> {
    match x {
        ValExpr::Count { role } => Err(ChorusError::new(format!(
            "{where_}: {what} count({role}) reads a step role — trigger bindings are empty, it would always be 0; use count(<query>) (e.g. count(name:{role}))"
        ))),
        ValExpr::Total { role, path } => Err(ChorusError::new(format!(
            "{where_}: {what} total({role}, aspect:{path}) reads a step role — trigger bindings are empty, it would always be 0"
        ))),
        ValExpr::Path { role, field, key } => Err(ChorusError::new(format!(
            "{where_}: {what} {role}.{field}.{key} reads a step role — trigger bindings are empty, it would always be false; use world forms (exists(…), count(name:…))",
            field = field.as_str()
        ))),
        // `Int` / `CountQuery` are world forms; `roll` (§9.41) reads the list's
        // own dice, not a role
        _ => Ok(()),
    }
}

/// The default-`what` entry point.
pub fn lint_trigger_val_expr(where_: &str, x: &ValExpr) -> Result<()> {
    lint_trigger_val_expr_what(where_, x, "IF operand")
}

// ---------- §9.41 compile-time may-bind lint for the `roll` operand ----------

/// Does this predicate read the `roll`
/// operand anywhere?
#[must_use]
fn pred_uses_roll(p: &Pred) -> bool {
    match p {
        Pred::And(a, b) | Pred::Or(a, b) => pred_uses_roll(a) || pred_uses_roll(b),
        Pred::Not(a) => pred_uses_roll(a),
        Pred::Cmp { l, r, .. } => matches!(l, ValExpr::Roll) || matches!(r, ValExpr::Roll),
        _ => false,
    }
}

/// Best-effort may-bind analysis (§9.41,
/// false NEGATIVES allowed). The scan walks each list linearly, descending
/// into IF branch bodies; a ROLL seen anywhere earlier makes later references
/// lint-clean.
pub fn lint_roll_may_bind(where_: &str, out: &[Outcome]) -> Result<()> {
    walk_roll_may_bind(where_, out, false)?;
    Ok(())
}

/// Walk one list; returns whether it
/// could possibly bind `roll` (contain a ROLL).
fn walk_roll_may_bind(where_: &str, list: &[Outcome], bound: bool) -> Result<bool> {
    let mut may_bind = bound;
    for o in list {
        if o.op == "ROLL" {
            may_bind = true;
        } else if o.op == "IF" {
            if let Some(arms) = &o.arms {
                for arm in arms {
                    if pred_uses_roll(&arm.when) && !may_bind {
                        return Err(ChorusError::new(format!(
                            "{where_}: 'roll' is read but no ROLL can have run before it in this outcome list — author a ROLL earlier in the list (the dice binding is per list/clause, §9.41)"
                        )));
                    }
                }
                // branch bodies may bind: ANY branch containing a possible
                // ROLL makes everything after this IF lint-clean (false
                // negatives allowed); note the recursion sees may_bind as of
                // the IF's entry, before branch updates
                let mut branch_binds = false;
                for arm in arms {
                    if walk_roll_may_bind(where_, &arm.then, may_bind)? {
                        branch_binds = true;
                    }
                }
                if let Some(fallback) = &o.fallback {
                    if walk_roll_may_bind(where_, fallback, may_bind)? {
                        branch_binds = true;
                    }
                }
                if branch_binds {
                    may_bind = true;
                }
            }
        }
    }
    Ok(may_bind)
}

// (the modules' cyclic type use is intra-crate by design)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::{new_state, AspectVal, Aspects, Attrs, Entity};

    fn ent(id: &str, name: &str, kind: &str, aspects: Aspects, attrs: Attrs) -> Entity {
        Entity {
            id: id.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            aspects,
            attrs,
            born_action: Some(0),
            dead_action: None,
            stackable: false,
        }
    }

    #[test]
    fn parses_and_evaluates_the_doc_grammar() {
        // count(actors) = 2 AND NOT any(actors, aspect:occult >= 2)
        let p = parse_predicate("count(actors) = 2 AND NOT any(actors, aspect:occult >= 2)").unwrap();
        let mut state = new_state(0);
        let mut e1 = ent("a1", "A", "character", Aspects::new(), Attrs::new());
        e1.aspects.insert("occult".into(), AspectVal::Num(3));
        let _ = state.entities.insert("a1".into(), e1.clone());
        let mut e2 = ent("a2", "B", "character", Aspects::new(), Attrs::new());
        e2.aspects.insert("occult".into(), AspectVal::Num(1));
        let _ = state.entities.insert("a2".into(), e2.clone());
        let mut bindings = RoleBindings::new();
        bindings.insert(Role::Actors, vec!["a1".into(), "a2".into()]);
        let ctx = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        // count(actors)=2 holds, any(occult>=2) holds (a1:3) → NOT any fails:
        // the conjunction is FALSE
        assert!(!eval_pred(&p, &ctx).unwrap());
        // the same tree with a predicate that passes end to end
        let p_pos = parse_predicate("count(actors) = 2 AND any(actors, aspect:occult >= 2)").unwrap();
        assert!(eval_pred(&p_pos, &ctx).unwrap());
        let p2 = parse_predicate("any(actors, aspect:occult >= 2)").unwrap();
        assert!(eval_pred(&p2, &ctx).unwrap());
        let p3 = parse_predicate("every(actors, aspect:occult >= 2)").unwrap();
        assert!(!eval_pred(&p3, &ctx).unwrap()); // e2 has 1 — every fails
    }

    #[test]
    fn three_valued_boundaries_are_frozen() {
        // every([]) is vacuously true, any([]) vacuously false
        let p = parse_predicate("every(actors, aspect:x >= 1)").unwrap();
        let p2 = parse_predicate("any(actors, aspect:x >= 1)").unwrap();
        let mut state = new_state(0);
        let bindings = RoleBindings::new();
        let ctx = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        assert!(eval_pred(&p, &ctx).unwrap());
        assert!(!eval_pred(&p2, &ctx).unwrap());
        // all-dead bindings read false silently, never a strict throw
        let mut dead = ent("d1", "D", "character", Aspects::new(), Attrs::new());
        dead.dead_action = Some(9);
        let _ = state.entities.insert("d1".into(), dead);
        let mut bindings2 = RoleBindings::new();
        bindings2.insert(Role::Object, vec!["d1".into()]);
        let ctx2 = PredCtx { b: &bindings2, state: &state, strict: true, roll: None };
        let p3 = parse_predicate("object.aspect.x >= 1").unwrap();
        assert!(!eval_pred(&p3, &ctx2).unwrap());
        // strict + ZERO bindings → the exact rejection message
        let bindings3 = RoleBindings::new();
        let ctx3 = PredCtx { b: &bindings3, state: &state, strict: true, roll: None };
        let e = eval_pred(&p3, &ctx3).unwrap_err();
        assert_eq!(
            e.msg,
            "IF condition: operand 'object.aspect.x' cannot resolve — role 'object' has no bound card"
        );
    }

    #[test]
    fn exists_and_count_query_forms() {
        let p = parse_predicate("exists(THE_SEALED_DOOR)").unwrap();
        let pq = parse_predicate("exists(aspect:occult >= 2)").unwrap();
        let pc = parse_predicate("count(aspect:occult >= 2) = 1").unwrap();
        let mut state = new_state(0);
        let mut door = ent("x1", "THE_SEALED_DOOR", "place", Aspects::new(), Attrs::new());
        door.aspects.insert("occult".into(), AspectVal::Num(5));
        // `by_name` pool drives exists(NAME)
        let _ = state.entities.insert("x1".into(), door.clone());
        let _ = state.by_name.insert("THE_SEALED_DOOR".into(), vec!["x1".into()]);
        let bindings = RoleBindings::new();
        let ctx = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        assert!(eval_pred(&p, &ctx).unwrap());
        assert!(eval_pred(&pq, &ctx).unwrap());
        assert!(eval_pred(&pc, &ctx).unwrap());
        // dead entity: exists(NAME) false, count drops it (rebuild the ctx
        // after the mutation to end the shared borrow)
        let _ = state.entities.get_mut("x1").map(|e| e.dead_action = Some(3));
        let ctx = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        assert!(!eval_pred(&p, &ctx).unwrap());
        assert!(!eval_pred(&pc, &ctx).unwrap());
    }

    #[test]
    fn tokenizer_errors_on_grammar_illegal_characters() {
        // Q2 fixed (§6.3's grammar has no `@`): the tokenizer is a load
        // error naming the character and its offset — formerly the `@`
        // silently vanished (`aspect:x >= @3` ≡ `aspect:x>=3`).
        let e = parse_predicate("count(actors) @>= 2").unwrap_err();
        assert_eq!(
            e.msg,
            "illegal character '@' at offset 14 in predicate 'count(actors) @>= 2'"
        );
        // whitespace stays glue (§6.3's only skip): the spaced form parses
        // identical to the tight one
        let p1 = parse_predicate("count(actors) >= 2").unwrap();
        let p2 = parse_predicate("count(actors)>=2").unwrap();
        assert_eq!(p1, p2);
        // `!` survives tokenization: exists()/count() embed §4.5 queries
        // verbatim (`state:!sealed`) and the inner text is reconstructed
        // from the source — assert the negation actually rode along (a
        // sealed door does NOT match state:!sealed; a swallowed `!` would
        // compile the positive term and match)
        let pq = parse_predicate("exists(state:!sealed)").unwrap();
        let Pred::ExistsQuery { query } = pq else {
            panic!("expected ExistsQuery, got {pq:?}");
        };
        let mut door = ent("d1", "DOOR", "place", Aspects::new(), Attrs::new());
        door.attrs
            .insert("state.sealed".into(), crate::core::types::AttrValue::Bool(true));
        assert!(!match_query(&door, &query));
        // a stray `!` outside a query is still a loud parse error
        let e2 = parse_predicate("count(actors) ! 2").unwrap_err();
        assert_eq!(e2.msg, "expected comparison op, got '!'");
        assert!(parse_predicate("").is_err());
        assert_eq!(
            parse_predicate("").unwrap_err().msg,
            "empty predicate ''"
        );
    }

    #[test]
    fn pred_view_word_adjacency_and_spans() {
        // `aspect` `:` `secret` must stay glue-tight (no space around ':')
        let view = pred_view_from_toks(&[
            Tok::Name { v: "aspect".into() },
            Tok::Punct { v: ":".into() },
            Tok::Name { v: "secret".into() },
            Tok::Int { v: 3 },
        ])
        .unwrap();
        // the glue space goes before ANY word-initial text — digits
        // are word chars — so an int after a name is "aspect:secret 3"
        assert_eq!(view.src, "aspect:secret 3");
        // word-adjacent tokens get a space so they cannot merge
        let view2 = pred_view_from_toks(&[
            Tok::Name { v: "count".into() },
            Tok::Punct { v: "(".into() },
            Tok::Name { v: "actors".into() },
            Tok::Punct { v: ")".into() },
        ])
        .unwrap();
        // punct tokens glue tight — only WORD-adjacent pairs get the space
        assert_eq!(view2.src, "count(actors)");
    }

    #[test]
    fn if_condition_scan_reports_end_and_errors() {
        let toks = vec![
            Tok::Name { v: "count".into() },
            Tok::Punct { v: "(".into() },
            Tok::Name { v: "actors".into() },
            Tok::Punct { v: ")".into() },
            Tok::Punct { v: ">=".into() },
            Tok::Int { v: 2 },
            Tok::Punct { v: "->".into() },
            Tok::Punct { v: "(".into() },
        ];
        let (pred, end) = parse_predicate_toks(&toks, None).unwrap();
        assert_eq!(end, 6);
        assert!(matches!(pred, Pred::Cmp { .. }));
        // comma at depth 0 → the -> required error
        let bad = vec![
            Tok::Name { v: "true".into() },
            Tok::Punct { v: ",".into() },
        ];
        assert_eq!(
            parse_predicate_toks(&bad, None).unwrap_err().msg,
            "an IF condition needs '-> ( outcomes )'"
        );
        // ELSE at depth 0 → the before-ELSE error
        let bad2 = vec![Tok::Name { v: "ELSE".into() }];
        assert_eq!(
            parse_predicate_toks(&bad2, None).unwrap_err().msg,
            "an IF needs '-> ( outcomes )' before 'ELSE'"
        );
        // no arrow at all
        let bad3 = vec![Tok::Name { v: "true".into() }];
        assert_eq!(
            parse_predicate_toks(&bad3, None).unwrap_err().msg,
            "an IF condition needs '-> ( outcomes )'"
        );
    }

    #[test]
    fn roll_operand_is_tok_frontend_only() {
        let e = parse_predicate("roll >= 3").unwrap_err();
        assert_eq!(
            e.msg,
            "'roll' is not available in 'when:' predicates — it binds inside an outcome list, after a ROLL (§9.41)"
        );
        // Tok front-end allows it
        let toks = vec![
            Tok::Name { v: "roll".into() },
            Tok::Punct { v: ">=".into() },
            Tok::Int { v: 3 },
            Tok::Punct { v: "->".into() },
        ];
        let (pred, _) = parse_predicate_toks(&toks, None).unwrap();
        let bindings = RoleBindings::new();
        let mut state = new_state(0);
        let ctx_none = PredCtx { b: &bindings, state: &state, strict: false, roll: None };
        assert_eq!(
            eval_pred(&pred, &ctx_none).unwrap_err().msg,
            "'roll' is unbound — no ROLL has run in this outcome list before this point"
        );
        let ctx_some = PredCtx { b: &bindings, state: &state, strict: false, roll: Some(4) };
        assert!(eval_pred(&pred, &ctx_some).unwrap());
        let _ = &mut state; // keep mut binding honest
    }

    #[test]
    fn lint_messages_name_roles_and_world_forms() {
        let p = parse_predicate("count(DREAD) >= 3").unwrap();
        let e = lint_pred("recipe r1", &p).unwrap_err();
        assert_eq!(
            e.msg,
            "recipe r1: count() argument 'DREAD' is neither a role (actors, object, with, at, about, to) nor a query (name:x, kind:k, aspect:a>=1, state:s, -term) — did you mean count(name:DREAD)?"
        );
        let p2 = parse_predicate("total(BOB, aspect:x) >= 1").unwrap();
        let e2 = lint_pred("recipe r2", &p2).unwrap_err();
        assert_eq!(
            e2.msg,
            "recipe r2: total() first argument 'BOB' is not a declared role (actors, object, with, at, about, to) — total pools a role's aspects; use count(name:x) to count a pool by name"
        );
        // trigger lint: rel/every/count over step roles are load errors
        let p3 = parse_predicate("rel(neighbor, actors)").unwrap();
        let e3 = lint_trigger_pred("trigger t", &p3).unwrap_err();
        assert_eq!(
            e3.msg,
            "trigger t: IF operand rel(neighbor, actors) reads a step role — trigger bindings are empty, it would always be false; use world forms (exists(…), count(name:…))"
        );
        let p4 = parse_predicate("every(actors, aspect:x >= 1)").unwrap();
        let e4 = lint_trigger_pred("trigger t", &p4).unwrap_err();
        assert_eq!(
            e4.msg,
            "trigger t: IF operand every(actors, …) reads a step role — trigger bindings are empty, it would always be true; use world forms (exists(…), count(name:…))"
        );
    }

    #[test]
    fn total_with_query_form_is_rejected_with_the_rule() {
        let e = parse_predicate("total(aspect:x >= 1, aspect:y) >= 2").unwrap_err();
        assert_eq!(
            e.msg,
            "total() pools a bound role's aspects — total(<query>, …) is not offered; use count(<query>) to count a pool by name (e.g. count(name:X))"
        );
    }
}
