//! The predicate parser: `PredView` (the glued-string + span token view)
//! and the recursive-descent [`PredParser`]. One grammar, three front-ends —
//! `parse_predicate` string-parses `when:` lines (§6.3);
//! `parse_predicate_toks` parses an IF condition out of the SENTENCE token
//! stream (§6.8); `parse_value_expr_toks` is the standalone ROLL-argument
//! form (§4.4). The AST and evaluator live in `predicate`, the load-time
//! lints in `pred_lint`.

use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use crate::core::dict::Dictionary;
use crate::core::lexer::Tok;
use crate::core::num_parse::parse_authored_int;
use crate::core::predicate::{Pred, PredField, ScalarCond, ScalarPath, ValExpr};
use crate::core::query::{parse_cmp_op, parse_query, Query, CMP_OPS};
use crate::core::regex_util::static_regex;
use crate::core::text::{is_dsl_ws_char, DSL_WS_CLASS};
use crate::error::{ChorusError, Result};

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

/// Parse one `when:` predicate string. The dictionary carries the kind
/// vocabulary the embedded §4.5 queries validate against (exists()/count()
/// `kind:` terms) — every load-time caller holds one.
pub fn parse_predicate(src: &str, dict: &Dictionary) -> Result<Pred> {
    PredParser::new(src, None, false, dict)?.parse()
}

/// Parse one IF condition out of the
/// SENTENCE token stream (§6.8) and report where it ended. Returns
/// `(pred, end)` where `end` is the number of Toks consumed, i.e. the index
/// of the `->` arrow in the given slice.
pub fn parse_predicate_toks(
    toks: &[Tok],
    params: Option<&HashSet<String>>,
    dict: &Dictionary,
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
                    return Err(ChorusError::new("an IF needs '-> ( outcomes )' before 'ELSE'"));
                }
            }
        }
    }
    let Some(end) = end else {
        return Err(ChorusError::new("an IF condition needs '-> ( outcomes )'"));
    };
    let slice = &toks[..end];
    if slice.is_empty() {
        return Err(ChorusError::new("an IF needs a condition before '->'"));
    }
    let PredView { src, toks, spans } = pred_view_from_toks(slice)?;
    // `allow_roll`: `roll` (§4.4) is an outcome-list operand — an IF condition
    // may read the list's dice; a `when:` line (the string front-end) may not
    let parser = PredParser { toks, spans, pos: 0, src, params, allow_roll: true, dict };
    let pred = parser.parse()?;
    Ok((pred, end))
}

/// ONE standalone value expression
/// (§4.4): the ROLL argument grammar — one grammar, three front-ends.
pub fn parse_value_expr_toks(
    toks: &[Tok],
    params: Option<&HashSet<String>>,
    dict: &Dictionary,
) -> Result<ValExpr> {
    let PredView { src, toks, spans } = pred_view_from_toks(toks)?;
    let parser = PredParser { toks, spans, pos: 0, src, params, allow_roll: true, dict };
    parser.parse_value_expr()
}

/// Sentence Toks → this parser's own
/// token alphabet (§6.8). Word-adjacent lexemes are space-separated so they
/// cannot merge; everything else glues exactly as the string grammar expects
/// (`aspect:hunter>=1`, `state:!sealed`, dotted paths, `-aspect:winter`).
/// The glued-string + span shape is load-bearing: the inner-query
/// reconstruction reads source offsets, so error texts carry the glued
/// traces — do NOT refactor to direct Tok interpretation.
pub(crate) fn pred_view_from_toks(toks: &[Tok]) -> Result<PredView> {
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
                return Err(ChorusError::new("an IF condition cannot contain a string literal"))
            }
            Tok::Dur { .. } => {
                return Err(ChorusError::new("an IF condition cannot contain a duration literal"))
            }
            Tok::IdRef { .. } => {
                return Err(ChorusError::new("an IF condition cannot contain a #id literal"))
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

// ---------- parser (precedence climbing: NOT > AND > OR) ----------

/// The recursive-descent predicate parser.
struct PredParser<'a> {
    toks: Vec<String>,
    spans: Vec<(usize, usize)>,
    pos: usize,
    src: String,
    params: Option<&'a HashSet<String>>,
    allow_roll: bool,
    /// the kind vocabulary for the embedded §4.5 queries (`kind:` terms in
    /// exists()/count() inner queries)
    dict: &'a Dictionary,
}

impl<'a> PredParser<'a> {
    /// The string front-end constructor.
    ///
    /// §6.3's grammar has no characters beyond the alternatives below:
    /// one outside the vocabulary (plus whitespace) is a LOAD ERROR
    /// naming the character and its offset — formerly QUIRK(Q2), the
    /// tokenizer silently swallowed it (`aspect:x >= @3` ≡
    /// `aspect:x>=3`, the `@` just vanished). Whitespace stays glue
    /// (§6.3's only license: "spaces around a comparison inside a
    /// query are glue") — the ONLY silent skip. `!` is a token because
    /// exists()/count() embed §4.5 queries verbatim (`state:!sealed`)
    /// and the inner text is reconstructed from the source, so its
    /// characters must survive; a stray `!` still errors loudly at the
    /// parser ("expected comparison op, got '!'"), never silently.
    /// `|` survives the same way (§4.5): a LONE `|` rides verbatim
    /// into the inner query as the group separator; outside a query it
    /// errors loudly at the parser ("expected comparison op, got '|'").
    /// (`\d` is ASCII `[0-9]`; offsets are byte offsets, the spans
    /// convention.)
    fn new(
        src: &str,
        params: Option<&'a HashSet<String>>,
        allow_roll: bool,
        dict: &'a Dictionary,
    ) -> Result<Self> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = static_regex(&RE, || {
            Regex::new(&format!(
                r"[A-Za-z_][A-Za-z0-9_.]*|[0-9]+|{}|!|\(|\)|,|:|-|\|",
                CMP_OPS.join("|")
            ))
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
        Ok(PredParser { toks, spans, pos: 0, src: src.to_string(), params, allow_roll, dict })
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

    /// One full value expression (§4.4 ROLL
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
        Ok(if kind == "every" { Pred::Every { role, cond } } else { Pred::Any { role, cond } })
    }

    /// exists(NAME): an ALIVE entity bears the
    /// name; exists(<query>): an alive entity matches the §4.5 slot query.
    fn exists_pred(&mut self) -> Result<Pred> {
        self.next()?; // 'exists'
        self.expect("(")?;
        let (inner, first, close) = self.balanced_group("exists() needs a name or a slot query")?;
        if self.query_starter_at(first) {
            let query = self.inner_query(&inner)?;
            return Ok(Pred::ExistsQuery { query });
        }
        if close - first > 1 {
            return Err(ChorusError::new(format!(
                "exists() takes one name or a kind:/aspect:/state:/name: query, got '{}'",
                inner.trim()
            )));
        }
        Ok(Pred::Exists { name: self.toks[first].clone() })
    }

    /// A query-term starter at token i:
    /// kind:/aspect:/state:/name: or a leading `-` (§4.5).
    fn query_starter_at(&self, i: usize) -> bool {
        let Some(head) = self.toks.get(i) else {
            return false;
        };
        head == "-" || {
            matches!(head.as_str(), "kind" | "aspect" | "state" | "name")
                && self.toks.get(i + 1).map(|t| t.as_str()) == Some(":")
        }
    }

    /// Consume a balanced `(...)` group — the OPENING paren already
    /// consumed — through its matching `)`. Returns the inner text cut from
    /// the source spans plus the first/close token indices of the group's
    /// content. `empty_err` names the `exists()`/`count()` form in the
    /// empty-group error.
    fn balanced_group(&mut self, empty_err: &str) -> Result<(String, usize, usize)> {
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
            return Err(ChorusError::new(empty_err));
        }
        // the inner text is cut from the source spans
        let inner = self.src[self.spans[first].0..self.spans[close].0].to_string();
        Ok((inner, first, close))
    }

    /// The §4.5 inner query of exists()/count(): §4.5 splits terms on
    /// whitespace, so tighten `aspect:hunter >= 1` to `aspect:hunter>=1` —
    /// spaces around a comparison are glue, not structure (the replacement
    /// keeps only the captured op).
    fn inner_query(&self, inner: &str) -> Result<Query> {
        static CMP: OnceLock<Regex> = OnceLock::new();
        let re = static_regex(&CMP, || {
            Regex::new(&format!(r"{}({}){}", DSL_WS_CLASS, CMP_OPS.join("|"), DSL_WS_CLASS))
        });
        parse_query(&re.replace_all(inner, "$1"), self.dict)
    }

    /// scalarCond := 'aspect' ':' key cmp int.
    fn scalar_cond(&mut self) -> Result<ScalarCond> {
        self.expect("aspect")?; // v0.2: scalar conditions read aspects
        self.expect(":")?;
        let key = self.dotted_name()?;
        let op_tok = self.next()?;
        let Some(op) = CMP_OPS.iter().find(|o| **o == op_tok) else {
            return Err(ChorusError::new(format!("expected comparison op, got '{op_tok}'")));
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
            path: ScalarPath { field: PredField::Aspect, key },
            op: parse_cmp_op(op),
            value: parse_authored_int(&v),
        })
    }

    /// cmp := valExpr cmpOp valExpr.
    fn cmp_from_expr(&mut self) -> Result<Pred> {
        let l = self.val_expr()?;
        let op_tok = self.next()?;
        let Some(op) = CMP_OPS.iter().find(|o| **o == op_tok) else {
            return Err(ChorusError::new(format!("expected comparison op, got '{op_tok}'")));
        };
        let r = self.val_expr()?;
        Ok(Pred::Cmp { l, op: parse_cmp_op(op), r })
    }

    /// One operand: param | roll | int | count/total | role.field.key path.
    fn val_expr(&mut self) -> Result<ValExpr> {
        let t = self.next()?;
        // F3 (§10): a bare parameter name is a param operand — expansion
        // binds it to the call's number.
        if self.params.is_some_and(|p| p.contains(&t)) {
            return Ok(ValExpr::Param { name: t });
        }
        // §4.4: the dice operand — Tok front-ends only. A `when:` line
        // parses through the string front-end, where selection provably runs
        // before any outcome could have rolled.
        if t == "roll" {
            if !self.allow_roll {
                return Err(ChorusError::new(
                    "'roll' is not available in 'when:' predicates — it binds inside an outcome list, after a ROLL (§4.4)",
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
            // §4.5: count(role) vs count(<query>) — a query-term starter
            // decides, mirroring exists(); the paren'd arg is consumed
            // balanced regardless
            if self.query_starter_at(self.pos) {
                let (inner, ..) = self.balanced_group("count() needs a role or a slot query")?;
                let query = self.inner_query(&inner)?;
                return Ok(ValExpr::CountQuery { query });
            }
            let role = self.next()?;
            self.expect(")")?;
            return Ok(ValExpr::Count { role });
        }
        if t == "total" {
            self.expect("(")?;
            // total pools a bound ROLE's aspects — the query form is
            // deliberately not offered (§4.5): fail with the rule
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
                return Err(ChorusError::new(format!("expected aspect|attr in path '{t}'")));
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
        self.expect(".")?; // the second dot of role.field.key (split-token shape, §6.8)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::dict::core_dictionary;
    use crate::core::predicate::{eval_pred, PredCtx, RoleBindings};
    use crate::core::query::match_query;
    use crate::core::types::{new_state, Aspects, AttrValue, Attrs, Entity};

    #[test]
    fn tokenizer_errors_on_grammar_illegal_characters() {
        // Q2 fixed (§6.3's grammar has no `@`): the tokenizer is a load
        // error naming the character and its offset — formerly the `@`
        // silently vanished (`aspect:x >= @3` ≡ `aspect:x>=3`).
        let dict = core_dictionary();
        let e = parse_predicate("count(actors) @>= 2", &dict).unwrap_err();
        assert_eq!(e.msg, "illegal character '@' at offset 14 in predicate 'count(actors) @>= 2'");
        // whitespace stays glue (§6.3's only skip): the spaced form parses
        // identical to the tight one
        let p1 = parse_predicate("count(actors) >= 2", &dict).unwrap();
        let p2 = parse_predicate("count(actors)>=2", &dict).unwrap();
        assert_eq!(p1, p2);
        // `!` survives tokenization: exists()/count() embed §4.5 queries
        // verbatim (`state:!sealed`) and the inner text is reconstructed
        // from the source — assert the negation actually rode along (a
        // sealed door does NOT match state:!sealed; a swallowed `!` would
        // compile the positive term and match)
        let pq = parse_predicate("exists(state:!sealed)", &dict).unwrap();
        let Pred::ExistsQuery { query } = pq else {
            panic!("expected ExistsQuery, got {pq:?}");
        };
        let mut door = Entity {
            id: "d1".into(),
            kind: "place".into(),
            name: "DOOR".into(),
            aspects: Aspects::new(),
            attrs: Attrs::new(),
            born_action: Some(0),
            dead_action: None,
            stackable: false,
        };
        door.attrs.insert("state.sealed".into(), AttrValue::Bool(true));
        assert!(!match_query(&door, &query));
        // a stray `!` outside a query is still a loud parse error
        let e2 = parse_predicate("count(actors) ! 2", &dict).unwrap_err();
        assert_eq!(e2.msg, "expected comparison op, got '!'");
        assert!(parse_predicate("", &dict).is_err());
        assert_eq!(parse_predicate("", &dict).unwrap_err().msg, "empty predicate ''");
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
        let dict = core_dictionary();
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
        let (pred, end) = parse_predicate_toks(&toks, None, &dict).unwrap();
        assert_eq!(end, 6);
        assert!(matches!(pred, Pred::Cmp { .. }));
        // comma at depth 0 → the -> required error
        let bad = vec![Tok::Name { v: "true".into() }, Tok::Punct { v: ",".into() }];
        assert_eq!(
            parse_predicate_toks(&bad, None, &dict).unwrap_err().msg,
            "an IF condition needs '-> ( outcomes )'"
        );
        // ELSE at depth 0 → the before-ELSE error
        let bad2 = vec![Tok::Name { v: "ELSE".into() }];
        assert_eq!(
            parse_predicate_toks(&bad2, None, &dict).unwrap_err().msg,
            "an IF needs '-> ( outcomes )' before 'ELSE'"
        );
        // no arrow at all
        let bad3 = vec![Tok::Name { v: "true".into() }];
        assert_eq!(
            parse_predicate_toks(&bad3, None, &dict).unwrap_err().msg,
            "an IF condition needs '-> ( outcomes )'"
        );
    }

    #[test]
    fn roll_operand_is_tok_frontend_only() {
        let dict = core_dictionary();
        let e = parse_predicate("roll >= 3", &dict).unwrap_err();
        assert_eq!(
            e.msg,
            "'roll' is not available in 'when:' predicates — it binds inside an outcome list, after a ROLL (§4.4)"
        );
        // Tok front-end allows it
        let toks = vec![
            Tok::Name { v: "roll".into() },
            Tok::Punct { v: ">=".into() },
            Tok::Int { v: 3 },
            Tok::Punct { v: "->".into() },
        ];
        let (pred, _) = parse_predicate_toks(&toks, None, &dict).unwrap();
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
    fn total_with_query_form_is_rejected_with_the_rule() {
        let dict = core_dictionary();
        let e = parse_predicate("total(aspect:x >= 1, aspect:y) >= 2", &dict).unwrap_err();
        assert_eq!(
            e.msg,
            "total() pools a bound role's aspects — total(<query>, …) is not offered; use count(<query>) to count a pool by name (e.g. count(name:X))"
        );
    }
}
