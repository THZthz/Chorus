//! The outcome-side grammar of the sentence parser (spec §4.4, §6.8, §10):
//! the op dispatch, the generic paren'd arg list, the dedicated paren-less
//! op forms (SPAWN / NARRATE / REPLACE / MOVE / DRAW / ENDING), IF/FN
//! calls, ROLL's value-expression args, and RUN. A second `impl Parser`
//! block — the token primitives (`peek`/`next`/`at_punct`/`at_keyword`),
//! `ref_`, and the §10 parameter scope (`is_param`/`param_at`) live in the
//! parent module.

use crate::core::fn_expand::unknown_call_error;
use crate::core::lexer::Tok;
use crate::core::predicate::{parse_predicate_toks, parse_value_expr_toks, ValExpr};
use crate::core::r#ref::Ref;
use crate::core::sentence::{Arg, IfArm, IntoTarget, Outcome, Role, Sign};
use crate::core::types::AttrValue;
use crate::error::{ChorusError, Result};

use super::{param_pair_error, show, Parser};

/// 'ACTORS' → 'actors' (role refs in outcome
/// args are uppercase). QUIRK(Q12): `as_role` intercepts BEFORE any
/// bare-ref fallback in outcome-arg positions, but the role names are NOT in
/// RESERVED_WORDS — a soft reservation whose interception order is
/// load-bearing (an entity named `object` cannot be bare-referenced in an
/// arg position, but can in a clause position).
#[must_use]
fn as_role(v: &str) -> Option<Role> {
    Role::from_lower(&v.to_lowercase())
}

impl Parser<'_> {
    /// outcome ( "," outcome )*.
    pub(super) fn outcome_list(&mut self) -> Result<Vec<Outcome>> {
        let mut out = vec![self.outcome()?];
        while self.at_punct(",", 0) {
            self.next()?;
            out.push(self.outcome()?);
        }
        Ok(out)
    }

    /// An outcome list, or empty when nothing follows.
    pub(super) fn outcome_list_or_empty(&mut self) -> Result<Vec<Outcome>> {
        if self.peek(0).is_none() {
            return Ok(Vec::new());
        }
        self.outcome_list()
    }

    /// outcome = OPERATOR "(" args ")" | ref (bare
    /// ref = SPAWN, §4.4). IF (§6.8) is intercepted before op dispatch. A
    /// declared function's NAME( dispatches to a call BEFORE op parsing
    /// (§10); an unknown NAME( is a compile error listing the vocabulary.
    pub(super) fn outcome(&mut self) -> Result<Outcome> {
        let t = self.peek(0).cloned();
        if let Some(Tok::Name { v }) = &t {
            if v == "IF" {
                return self.if_outcome();
            }
        }
        if let Some(Tok::Name { v }) = &t {
            if self.at_punct("(", 1) {
                if self.fns.is_some_and(|f| f.by_name.contains_key(v)) {
                    return self.fn_call();
                }
                if !self.dict.is_op(v) {
                    return Err(ChorusError::new(unknown_call_error(v, self.fns)));
                }
            }
            if self.dict.is_op(v) {
                return self.op_outcome(v.clone());
            }
        }
        // bare ref = SPAWN (§4.4)
        Ok(Outcome::concrete("SPAWN", vec![Arg::Ref { r#ref: self.ref_()? }]))
    }

    /// The op dispatch arm (a NAME token that `is_op` accepts). The arm
    /// order below is LOAD-BEARING and tracks the authored grammar: the
    /// paren-less-only ops' `(`-errors and the ROLL/DRAW/RUN dedicated
    /// grammars dispatch BEFORE the generic paren'd path (so `DRAW(RUMOURS)`
    /// errors as a form error instead of parsing a generic arg list), and
    /// every other op's paren'd form — DESTROY(OBJECT), NARRATE("x"),
    /// ROLLED(14, 3) — rides the generic path with its concrete-row shape
    /// guarded, so what the engine writes re-parses (law 2).
    fn op_outcome(&mut self, op: String) -> Result<Outcome> {
        self.next()?; // the op name
        match op.as_str() {
            // REPLACE is paren-less only: its WITH keyword would mis-parse as
            // the `with` role ref inside a generic arg list (§4.4)
            "REPLACE" if self.at_punct("(", 0) => Err(ChorusError::new(
                "REPLACE takes the paren-less form: REPLACE ref WITH prototype",
            )),
            // ENDING is paren-less only: its id is a plain name, never an arg
            // list (§4.4)
            "ENDING" if self.at_punct("(", 0) => {
                Err(ChorusError::new("ENDING takes the paren-less form: ENDING ENDING_ID"))
            }
            // ROLL is paren-only, and its args are NOT the generic arg grammar:
            // they are §6.3 value expressions, parsed by a depth-aware Tok scan
            "ROLL" if !self.at_punct("(", 0) => Err(ChorusError::new(
                "ROLL takes the paren form: ROLL(<count-expr>, <sides-expr>[, <bonus-expr>])",
            )),
            "ROLL" => self.roll_outcome(),
            // DRAW is paren-less only (§4.4): the deck id is a plain ref; the
            // INTO target is the SPAWN grammar
            "DRAW" => self.draw_outcome(),
            // RUN is the recipe-chain op (§4.4): `RUN "id" [, "id" …]` — the
            // args are QUOTED-STRING refs only (recipe ids are lowercase and
            // dotted; the lexer's NAME class has no dot, so a bare id is
            // unlexable — the DRAWN precedent). The paren'd canonical shell
            // that format renders is accepted too, so the canonical spelling
            // re-parses (§5.2).
            "RUN" => self.run_outcome(),
            // the generic paren'd arg grammar — every op not intercepted
            // above takes its paren'd form here
            _ if self.at_punct("(", 0) => {
                let mut args = self.paren_args()?;
                // DRAWN is the engine-written record of a DRAW (§4.4): an
                // unsafe spelling formats re-quoted — a quoted string IS a
                // name ref (ref()'s rule), so coerce before the shape check
                // or the recorded row would not re-parse (law 2)
                if op == "DRAWN" {
                    for a in args.iter_mut() {
                        if let Arg::Str { v } = a {
                            *a = Arg::Ref { r#ref: Ref::Name { kind: None, name: v.clone() } };
                        }
                    }
                }
                Self::concrete_shape_guard(&op, &args)?;
                Ok(Outcome::concrete(op.clone(), args))
            }
            // SPAWN's canonical paren-less form (§4.4)
            "SPAWN" => Ok(Outcome::concrete("SPAWN", self.spawn_args()?)),
            // NARRATE's paren-less form: one string, or a §10 parameter
            "NARRATE" => self.narrate_outcome(),
            "DESTROY" | "REVEAL" => {
                Ok(Outcome::concrete(op.clone(), vec![self.role_or_ref_arg()?]))
            }
            // ENDING id (§4.4): a plain ref-form id, so quoted ids
            // round-trip too
            "ENDING" => Ok(Outcome::concrete("ENDING", vec![Arg::Ref { r#ref: self.ref_()? }])),
            // REPLACE ref WITH prototype (§4.4): target may be a role ref
            "REPLACE" => self.replace_outcome(),
            "MOVE" => self.move_outcome(),
            _ => Err(ChorusError::new(format!("{op} requires parentheses"))),
        }
    }

    /// `(` arg (`,` arg)* `)` — the generic paren'd outcome-arg list, shared
    /// by the op dispatch's generic arm and `fn_call`.
    fn paren_args(&mut self) -> Result<Vec<Arg>> {
        self.expect_punct("(")?;
        let mut args: Vec<Arg> = Vec::new();
        if !self.at_punct(")", 0) {
            args.push(self.arg()?);
            while self.at_punct(",", 0) {
                self.next()?;
                args.push(self.arg()?);
            }
        }
        self.expect_punct(")")?;
        Ok(args)
    }

    /// The engine-written records' pinned shapes on the generic paren'd
    /// path: ROLLED is the fold's dice counter row and DRAWN the deck cursor
    /// row (§4.4) — their arities are pinned so every concrete row
    /// re-parses (law 2); NARRATE's canonical form is one string (§4.4),
    /// with a §10 parameter allowed in the string position — it binds to
    /// the call's string at expansion.
    fn concrete_shape_guard(op: &str, args: &[Arg]) -> Result<()> {
        match op {
            "ROLLED" => {
                let two_ints = matches!(
                    (args.first(), args.get(1)),
                    (Some(Arg::Int { .. }), Some(Arg::Int { .. }))
                );
                if args.len() != 2 || !two_ints {
                    return Err(ChorusError::new(
                        "ROLLED takes exactly two integers: ROLLED(<total>, <ctrAfter>)",
                    ));
                }
            }
            "DRAWN" => {
                let two_names = matches!(
                    (args.first(), args.get(1)),
                    (
                        Some(Arg::Ref { r#ref: Ref::Name { .. } }),
                        Some(Arg::Ref { r#ref: Ref::Name { .. } })
                    )
                );
                if args.len() != 2 || !two_names {
                    return Err(ChorusError::new(
                        "DRAWN takes exactly two name refs: DRAWN(<DECK>, <name>)",
                    ));
                }
            }
            "NARRATE" => {
                let one_str =
                    matches!(args.first(), Some(Arg::Str { .. }) | Some(Arg::Param { .. }));
                if args.len() != 1 || !one_str {
                    return Err(ChorusError::new("NARRATE takes exactly one string"));
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// DRAW <DECK> [INTO target] (§4.4) — the paren-less form only.
    fn draw_outcome(&mut self) -> Result<Outcome> {
        if self.at_punct("(", 0) {
            return Err(ChorusError::new(
                "DRAW takes the paren-less form: DRAW <DECK> [INTO target]",
            ));
        }
        if self.peek(0).is_none() {
            return Err(ChorusError::new("DRAW needs a deck id: DRAW <DECK> [INTO target]"));
        }
        let mut draw_args = vec![Arg::Ref { r#ref: self.ref_()? }];
        if self.at_keyword("INTO") {
            self.next()?;
            draw_args.push(Arg::Into { r#ref: self.ref_target()? });
        }
        Ok(Outcome::concrete("DRAW", draw_args))
    }

    /// NARRATE's paren-less form (§4.4): a single string; §10: a parameter
    /// in the string position is allowed — it binds to the call's string at
    /// expansion.
    fn narrate_outcome(&mut self) -> Result<Outcome> {
        if let Some(name) = self.param_at(0) {
            self.next()?;
            return Ok(Outcome::concrete("NARRATE", vec![Arg::Param { name, sign: None }]));
        }
        let s = self.next()?;
        if s.t() != "str" {
            return Err(ChorusError::new(format!("NARRATE expects a string, got '{}'", show(&s))));
        }
        Ok(Outcome::concrete("NARRATE", vec![Arg::Str { v: s.v_str() }]))
    }

    /// REPLACE ref WITH prototype (§4.4): target may be a role ref.
    fn replace_outcome(&mut self) -> Result<Outcome> {
        let target = self.role_or_ref_arg()?;
        if !self.at_keyword("WITH") {
            return Err(ChorusError::new("REPLACE expects WITH"));
        }
        self.next()?;
        Ok(Outcome::concrete("REPLACE", vec![target, Arg::Ref { r#ref: self.ref_()? }]))
    }

    /// MOVE target INTO target (§4.4): the target may be a role ref, the
    /// destination is the INTO grammar.
    fn move_outcome(&mut self) -> Result<Outcome> {
        let mut args = vec![self.role_or_ref_arg()?];
        if !self.at_keyword("INTO") {
            return Err(ChorusError::new("MOVE expects INTO"));
        }
        self.next()?;
        args.push(Arg::Into { r#ref: self.ref_target()? });
        Ok(Outcome::concrete("MOVE", args))
    }

    /// NAME(arg, …): a call to a declared function
    /// (§10). Arity is checked HERE, a load error.
    fn fn_call(&mut self) -> Result<Outcome> {
        let name_tok = self.next()?;
        let Tok::Name { v: name } = name_tok else {
            return Err(ChorusError::new("internal: function dispatch on a non-name token"));
        };
        let params = match self.fns.and_then(|f| f.by_name.get(&name)) {
            Some(tpl) => tpl.params.clone(),
            // the dispatch guard checked `by_name.contains_key(name)` above;
            // unreachable
            None => unreachable!("fn_call dispatched on a name absent from FnTable"),
        };
        let args = self.paren_args()?;
        if args.len() != params.len() {
            return Err(ChorusError::new(format!(
                "function '{name}' expects {n} argument(s){list}, got {got}",
                n = params.len(),
                list = if !params.is_empty() {
                    format!(" ({})", params.join(", "))
                } else {
                    String::new()
                },
                got = args.len()
            )));
        }
        Ok(Outcome::fn_call(name, args))
    }

    /// IF pred -> ( outcomes ) { ELSE IF … } [ELSE …]
    /// (§6.8). The ELSE chain is consumed greedily INSIDE this node — the
    /// `,`-loop of outcome_list must not strand an ELSE.
    fn if_outcome(&mut self) -> Result<Outcome> {
        self.next()?; // IF
        let mut arms = vec![self.if_arm()?];
        let mut fallback: Option<Vec<Outcome>> = None;
        while self.at_keyword("ELSE") {
            self.next()?;
            if self.at_keyword("IF") {
                self.next()?;
                arms.push(self.if_arm()?);
            } else {
                self.expect_punct("->")?;
                fallback = Some(self.branch_body()?);
                break; // ELSE is terminal
            }
        }
        Ok(Outcome {
            op: "IF".to_string(),
            args: Vec::new(),
            arms: Some(arms),
            fallback,
            call: None,
        })
    }

    /// One IF arm: the condition is a balanced unit up to the depth-0 `->`,
    /// then the branch body.
    fn if_arm(&mut self) -> Result<IfArm> {
        // the condition is parsed from the SENTENCE token stream as a
        // balanced unit up to the depth-0 `->` (predicates contain commas via
        // rel()/every() — never comma-split)
        let (pred, end) = parse_predicate_toks(&self.toks[self.pos..], self.params.as_ref())?;
        self.pos += end;
        self.expect_punct("->")?;
        let then = self.branch_body()?;
        Ok(IfArm { when: pred, then })
    }

    /// Branch body: `( outcomes )`.
    fn branch_body(&mut self) -> Result<Vec<Outcome>> {
        self.expect_punct("(")?;
        let out = self.outcome_list()?;
        self.expect_punct(")")?;
        Ok(out)
    }

    /// ROLL(<count-expr>, <sides-expr>[, <bonus-expr>])
    /// (§4.4). Each argument is consumed as a BALANCED Tok unit up to the
    /// depth-0 `,` (or the closing `)`), then parsed with the SAME §6.3
    /// value-expression grammar the IF cmp operands use.
    fn roll_outcome(&mut self) -> Result<Outcome> {
        self.next()?; // '('
        let mut args: Vec<Arg> = Vec::new();
        if !self.at_punct(")", 0) {
            loop {
                args.push(Arg::Vexpr { v: self.roll_arg_expr()? });
                if self.at_punct(",", 0) {
                    self.next()?;
                    continue;
                }
                break;
            }
        }
        self.expect_punct(")")?;
        if args.len() < 2 || args.len() > 3 {
            return Err(ChorusError::new(format!(
                "ROLL takes (count-expr, sides-expr[, bonus-expr]), got {n} argument(s)",
                n = args.len()
            )));
        }
        Ok(Outcome::concrete("ROLL", args))
    }

    /// RUN "id" [, "id" …] (§4.4): every
    /// candidate is a quoted string — coerced to a name ref (the DRAWN
    /// coercion, so the arg shape matches the log-ref rule); a bare or dotted
    /// id is a parse error naming the syntax, and an empty list is one too.
    /// The paren'd canonical shell `RUN("id", …)` format renders parses to
    /// the identical outcome, so the §5.2 round-trip holds.
    fn run_outcome(&mut self) -> Result<Outcome> {
        let paren = self.at_punct("(", 0);
        if paren {
            self.next()?;
        }
        let syntax = "RUN takes quoted-string recipe ids: RUN \"id\" [, \"id\" …]";
        let mut args: Vec<Arg> = Vec::new();
        loop {
            if self.peek(0).is_none() {
                return Err(ChorusError::new(format!("{syntax} — got none")));
            }
            let t = self.next()?;
            let Tok::Str { v } = t else {
                return Err(ChorusError::new(format!("{syntax} — got {}", show(&t))));
            };
            args.push(Arg::Ref { r#ref: Ref::Name { kind: None, name: v } });
            // a comma continues the candidate list only when a QUOTED STRING
            // follows (in the paren'd shell: always) — otherwise the comma is
            // the outcome list's separator and the next outcome takes it
            // (`RUN "b", NARRATE "chained."`). The one ambiguity this leaves —
            // `RUN "a", "SOME NAME"` reading the bare spawn as a candidate —
            // authors parenthesize out of: `RUN("a"), SPAWN "SOME NAME"`.
            let more = if paren {
                self.at_punct(",", 0)
            } else {
                self.at_punct(",", 0) && matches!(self.peek(1), Some(Tok::Str { .. }))
            };
            if more {
                self.next()?;
                continue;
            }
            break;
        }
        if paren {
            self.expect_punct(")")?;
        }
        Ok(Outcome::concrete("RUN", args))
    }

    /// One balanced ROLL argument: Toks up to the
    /// depth-0 ',' or ')'.
    fn roll_arg_expr(&mut self) -> Result<ValExpr> {
        let start = self.pos;
        let mut depth: i64 = 0;
        loop {
            let Some(t) = self.peek(0) else {
                return Err(ChorusError::new(
                    "unexpected end of ROLL arguments — ROLL takes (count-expr, sides-expr[, bonus-expr])",
                ));
            };
            if t.t() == "punct" {
                match t.v_str().as_str() {
                    "(" => depth += 1,
                    ")" => {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    }
                    "," if depth == 0 => break,
                    _ => {}
                }
            }
            self.next()?;
        }
        let slice = &self.toks[start..self.pos];
        if slice.is_empty() {
            return Err(ChorusError::new("ROLL arguments cannot be empty"));
        }
        parse_value_expr_toks(slice, self.params.as_ref())
    }

    /// The DESTROY/REVEAL/REPLACE/MOVE target arg. §10: a parameter wins
    /// before the role check (a param may shadow a role name inside a body
    /// exactly as it shadows an entity name); QUIRK(Q12): the role check
    /// intercepts BEFORE any bare-ref fallback — the soft-reservation order
    /// is load-bearing.
    ///
    /// The param shape differs from `ref_target`'s on purpose: here a
    /// parameter IS the whole arg (`Arg::Param`), while an INTO target
    /// carries it as a `Ref::Param` inside the target — `subst_arg` keeps a
    /// substitution arm for each.
    fn role_or_ref_arg(&mut self) -> Result<Arg> {
        Ok(match self.ref_target()? {
            IntoTarget::Ref(Ref::Param { name }) => Arg::Param { name, sign: None },
            IntoTarget::Ref(r#ref) => Arg::Ref { r#ref },
            IntoTarget::Role { role } => Arg::Role { role },
        })
    }

    /// SPAWN ref [INTO target] [AS #id] [expires duration] [k=v …]
    /// (canonical paren-less form). The authored `expires <duration>`
    /// modifier (§4.4, §4.2) arms the mint at birth — the engine
    /// materializes the SET-expires `expires`/`expiresAt` pair inside the
    /// concrete row's `attrs=` pair (§6.6), so the modifier itself is
    /// authored-sugar only and never reaches the log. §10: a parameter as
    /// the pair's key OR value replaces the whole pair (these param checks
    /// are LIVE — unlike arg()'s dead key check, Q3).
    fn spawn_args(&mut self) -> Result<Vec<Arg>> {
        let mut args = vec![Arg::Ref { r#ref: self.ref_()? }];
        if self.at_keyword("INTO") {
            self.next()?;
            args.push(Arg::Into { r#ref: self.ref_target()? });
        }
        if self.at_keyword("AS") {
            self.next()?;
            let id = self.next()?;
            let Tok::IdRef { v } = id else {
                return Err(ChorusError::new(format!(
                    "expected #id after AS, got '{}'",
                    show(&id)
                )));
            };
            args.push(Arg::AsId { id: v });
        }
        if self.at_keyword("expires") {
            self.next()?;
            // every §5.1 duration form: the bare literal lexes to minutes
            // (§5.1 compounds included); the quoted form rides §4.2's
            // quoted-string path — the engine runs both through the one
            // duration parser. The DEDICATED arg shape (E12 review): a
            // §10 parameter here is a parse error naming the grammar, so
            // the modifier can never be reached by substitution, and no
            // bare dur/str arg is ever mistaken for it at application.
            let t = self.next()?;
            match &t {
                Tok::Dur { v } => args.push(Arg::Expires { v: AttrValue::Num(*v) }),
                Tok::Str { v } => args.push(Arg::Expires { v: AttrValue::Str(v.clone()) }),
                other => {
                    return Err(ChorusError::new(format!(
                        "SPAWN expires takes a §5.1 duration (e.g. 2h, 90m, 1halfhour), got '{}'",
                        show(other)
                    )));
                }
            }
        }
        while self.at_punct("=", 1) {
            let k = self.next()?;
            let Tok::Name { v: key } = k else {
                return Err(ChorusError::new(format!(
                    "expected key=value pair, got '{}'",
                    show(&k)
                )));
            };
            self.next()?; // '='
                          // §10: a parameter as the pair's key OR value replaces the
                          // whole pair (the LIVE twin of arg()'s dead branch, Q3)
            let param_name = if self.is_param(&key) { Some(key.clone()) } else { self.param_at(0) };
            if let Some(p_name) = param_name {
                self.next()?; // consumes the value token (even when the KEY was the param)
                args.push(Arg::Param { name: p_name, sign: None });
                continue;
            }
            // §4.4: the only SPAWN pairs are the engine-written aspects=/attrs=
            // materialization — apply reads no other key, so any other
            // authored pair would vanish silently at apply (no error, no log
            // trace); fail loud at parse instead. The decay modifier is the
            // `expires <duration>` shape above, never a pair.
            if key != "aspects" && key != "attrs" {
                return Err(ChorusError::new(format!(
                    "SPAWN: unknown pair key '{key}' — the only SPAWN pairs are the engine-written aspects=/attrs= materialization (§4.4); authored decay is the `expires <duration>` modifier"
                )));
            }
            args.push(Arg::Pair { k: key, v: self.value_token()? });
        }
        Ok(args)
    }

    /// The generic outcome arg.
    fn arg(&mut self) -> Result<Arg> {
        let Some(t) = self.peek(0).cloned() else {
            return Err(ChorusError::new("unexpected end of outcome args"));
        };
        // signed SET value (§4.4): '+'/'-' int is a delta on the entry's
        // current value. §10: the magnitude may be a parameter — the sign
        // is carried on the placeholder and folded into the call's number at
        // expansion.
        if t.t() == "punct" && (t.v_str() == "+" || t.v_str() == "-") {
            self.next()?;
            let n = self.next()?;
            if let Tok::Name { v } = &n {
                if self.is_param(v) {
                    return Ok(Arg::Param {
                        name: v.clone(),
                        sign: Some(if t.v_str() == "-" { Sign::Neg } else { Sign::Pos }),
                    });
                }
            }
            if n.t() == "dur" {
                return Err(ChorusError::new("deltas are integers only (no duration deltas)"));
            }
            let Tok::Int { v } = n else {
                return Err(ChorusError::new(format!(
                    "expected an integer after '{}', got '{}'",
                    t.v_str(),
                    show(&n)
                )));
            };
            return Ok(Arg::Delta { v: if t.v_str() == "-" { -v } else { v } });
        }
        match t {
            Tok::Str { v } => {
                self.next()?;
                Ok(Arg::Str { v })
            }
            Tok::Dur { v } => {
                self.next()?;
                Ok(Arg::Dur { v })
            }
            Tok::Int { v } => {
                self.next()?;
                Ok(Arg::Int { v })
            }
            Tok::IdRef { v } => {
                self.next()?;
                Ok(Arg::Ref { r#ref: Ref::Id { id: v } })
            }
            Tok::Name { v } => self.name_arg(v),
            other => Err(ChorusError::new(format!(
                "unexpected token in outcome args: '{}'",
                show(&other)
            ))),
        }
    }

    /// The name-token arm of arg().
    fn name_arg(&mut self, v: String) -> Result<Arg> {
        if v == "INTO" {
            self.next()?;
            return Ok(Arg::Into { r#ref: self.ref_target()? });
        }
        if v == "AS" {
            self.next()?;
            let id = self.next()?;
            let Tok::IdRef { v: idv } = id else {
                return Err(ChorusError::new(format!(
                    "expected #id after AS, got '{}'",
                    show(&id)
                )));
            };
            return Ok(Arg::AsId { id: idv });
        }
        // The param check below returns the WHOLE arg as a param. A param
        // followed by `=` reads as a pair KEY — a form only spawn_args
        // accepts (§10's pair face); here it would strand the `=` and
        // doom the outcome loop with a bare `expected ')', got '='`, so
        // error here instead with the teach message (Q3). The acceptance
        // face is UNCHANGED: the spelling was and remains a parse error.
        if self.is_param(&v) {
            if self.at_punct("=", 1) {
                return Err(ChorusError::new(param_pair_error("key", &v, self.params.as_ref())));
            }
            self.next()?;
            return Ok(Arg::Param { name: v, sign: None });
        }
        let role = as_role(&v);
        if let Some(role) = role {
            // ACTORS/OBJECT/… in outcome args are role refs (e.g. MOVE ACTORS
            // INTO OBJECT)
            self.next()?;
            return Ok(Arg::Role { role });
        }
        if v == "true" || v == "false" {
            self.next()?;
            return Ok(Arg::Bool { v: v == "true" });
        }
        if self.at_punct("=", 1) {
            // §10 gives a parameter the pair KEY or VALUE face in
            // spawn_args only (the whole pair is replaced by the
            // parameter, value token consumed). In generic outcome args a
            // param as the pair VALUE used to be "returned unconsumed" —
            // stranding the token for the outcome loop's bare
            // `expected ')', got 'p'`. Q3's teach: say so directly,
            // naming the param and the legal forms. The acceptance face
            // is UNCHANGED: the spelling was and remains a parse error.
            if let Some(name) = self.param_at(2) {
                return Err(ChorusError::new(param_pair_error(
                    "value",
                    &name,
                    self.params.as_ref(),
                )));
            }
            self.next()?;
            self.next()?;
            let k = v;
            return Ok(Arg::Pair { k, v: self.value_token()? });
        }
        // `aspect:` names the aspects map in SET keys (§4.4) — one key
        // token, like the query form (§4.5)
        if v == "aspect" && self.at_punct(":", 1) && matches!(self.peek(2), Some(Tok::Name { .. }))
        {
            self.next()?;
            self.next()?;
            let n = self.next()?;
            let mut key = format!("aspect:{}", n.v_str());
            while self.at_punct(".", 0) && matches!(self.peek(1), Some(Tok::Name { .. })) {
                self.next()?;
                let seg = self.next()?;
                key.push('.');
                key.push_str(&seg.v_str());
            }
            return Ok(Arg::Key { v: key });
        }
        // dotted names in arg position are SET keys: SET(x, state.sealed, true)
        if self.at_punct(".", 1) && matches!(self.peek(2), Some(Tok::Name { .. })) {
            let mut key = v;
            self.next()?; // the leading name
            while self.at_punct(".", 0) && matches!(self.peek(1), Some(Tok::Name { .. })) {
                self.next()?;
                let seg = self.next()?;
                key.push('.');
                key.push_str(&seg.v_str());
            }
            return Ok(Arg::Key { v: key });
        }
        Ok(Arg::Ref { r#ref: self.ref_()? })
    }

    /// The INTO target: param | role | ref.
    fn ref_target(&mut self) -> Result<IntoTarget> {
        if let Some(name) = self.param_at(0) {
            self.next()?;
            return Ok(IntoTarget::Ref(Ref::Param { name }));
        }
        if let Some(Tok::Name { v }) = self.peek(0).cloned() {
            if let Some(role) = as_role(&v) {
                self.next()?;
                return Ok(IntoTarget::Role { role });
            }
        }
        Ok(IntoTarget::Ref(self.ref_()?))
    }

    /// A scalar value token for k=v pairs.
    fn value_token(&mut self) -> Result<AttrValue> {
        let t = self.next()?;
        match t {
            Tok::Int { v } => Ok(AttrValue::Num(v)),
            Tok::Dur { v } => Ok(AttrValue::Num(v)), // minutes
            Tok::Str { v } => Ok(AttrValue::Str(v)),
            Tok::Name { v } => {
                if v == "true" {
                    Ok(AttrValue::Bool(true))
                } else if v == "false" {
                    Ok(AttrValue::Bool(false))
                } else {
                    Ok(AttrValue::Str(v))
                }
            }
            other => Err(ChorusError::new(format!("expected a value, got '{}'", show(&other)))),
        }
    }
}
