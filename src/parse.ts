// Recursive-descent sentence parser — spec §5.1.
// The one semantic decision point: where the actor list ends and the verb
// begins. That needs the verb registry (both are uppercase tokens), so the
// parser consults the Dictionary (spec §9.14).
import { lex, type Tok } from './lexer.js'
import type { Arg, Clause, IfArm, Outcome, Role, Sentence } from './sentence.js'
import { ROLES } from './sentence.js'
import { OUTCOME_OPS, type Dictionary } from './dict.js'
import { parsePredicateToks, parseValueExprToks, type Pred, type ValExpr } from './predicate.js'
import type { Ref } from './ref.js'
import { formatDuration, type Clock } from './clock.js'

const CLAUSE_KEYWORDS = new Set(['TO', 'WITH', 'AT', 'ABOUT'])

/** 'ACTORS' → 'actors' (role refs in outcome args are uppercase) */
function asRole(v: string): Role | undefined {
  const lower = v.toLowerCase()
  return ROLES.includes(lower as Role) ? (lower as Role) : undefined
}

export function parseSentence(src: string, dict: Dictionary): Sentence {
  return new Parser(lex(src), dict).sentence()
}

// ---------- functions: parametric outcome macros (§9.40, v0.3.3) ----------

/** one declared function: its parameter names and its body compiled to a
 *  TEMPLATE — an outcome list that may still hold param placeholders
 *  (`{t:'param'}` args, `{form:'param'}` refs) and nested `FN` call nodes */
export interface FnTemplate {
  name: string
  params: string[]
  body: Outcome[]
}

/** the load-time function table, threaded EXPLICITLY into every parse that
 *  may contain calls (never Dictionary state): bodies parse against it, and
 *  call sites expand against it — the engine never sees a call */
export interface FnTable {
  byName: Map<string, FnTemplate>
}

/** Parse an outcome list — used to compile recipe `out` strings at content load.
 *  Unlike the lenient in-play sentence input, authored outcome strings must
 *  parse COMPLETELY (§5.1): trailing tokens after the last outcome — a double
 *  ELSE, a typo'd op, an `IF` keyword forgotten off a `cond -> (…)` — would
 *  otherwise vanish silently at load and surface as wrong behavior in play.
 *  `fns` (§9.40) makes `NAME(arg, …)` dispatch to declared functions: known
 *  names win before op parsing; an unknown `NAME(` is a compile error listing
 *  the vocabulary. Expansion to the function's body happens in `expandFnCalls`,
 *  which content compile runs immediately after this parse — before the load
 *  guards. */
export function parseOutcomes(src: string, dict: Dictionary, fns?: FnTable): Outcome[] {
  const p = new Parser(lex(src), dict, fns)
  const out = p.outcomeListOrEmpty()
  p.expectEnd()
  return out
}

/** Parse one function BODY into its template (§9.40): parameters are active
 *  (a bare param token becomes a placeholder wherever a ref/value literal is
 *  expected), nested calls become FN nodes, and NOTHING expands yet — bodies
 *  may call functions declared later, so expansion waits for the call sites. */
export function parseFunctionBody(src: string, dict: Dictionary, fns: FnTable, params: string[]): Outcome[] {
  const p = new Parser(lex(src), dict, fns, new Set(params))
  const out = p.outcomeListOrEmpty()
  p.expectEnd()
  return out
}

/** every function a body calls, directly (branch bodies included) — the call
 *  graph content compile walks for recursion cycles and the depth cap */
export function collectFnCalls(out: Outcome[], into: Set<string>): void {
  for (const o of out) {
    if (o.op === 'FN' && o.call) into.add(o.call)
    else if (o.op === 'IF') {
      for (const arm of o.arms ?? []) {
        collectFnCalls(arm.then, into)
        // conditions carry no calls — params substitute into cmp operands only
      }
      if (o.fallback) collectFnCalls(o.fallback, into)
    }
  }
}

class Parser {
  private pos = 0
  constructor(
    private toks: Tok[],
    private dict: Dictionary,
    private fns?: FnTable,
    private params?: Set<string>,
  ) {}

  private peek(o = 0): Tok | undefined { return this.toks[this.pos + o] }
  private next(): Tok {
    const t = this.peek()
    if (!t) throw new Error('unexpected end of sentence')
    this.pos++
    return t
  }
  private expectPunct(v: string): void {
    const t = this.next()
    if (t.t !== 'punct' || t.v !== v) throw new Error(`expected '${v}', got '${show(t)}'`)
  }
  private atPunct(v: string, o = 0): boolean {
    const t = this.peek(o)
    return !!t && t.t === 'punct' && t.v === v
  }
  private atKeyword(w: string): boolean {
    const t = this.peek()
    return !!t && t.t === 'name' && t.v === w
  }

  sentence(): Sentence {
    let clock: Clock | null = null
    if (this.atPunct(':', 1) && this.peek()?.t === 'int') {
      const dayTok = this.next()
      if (dayTok.t !== 'int') throw new Error('bad clock')
      this.expectPunct(':')
      const halfhourTok = this.next()
      if (halfhourTok.t !== 'int' || halfhourTok.v < 0 || halfhourTok.v > 47) throw new Error('bad halfhour (0..47)')
      clock = { day: dayTok.v, halfhour: halfhourTok.v }
      this.expectPunct('@')
    }

    if (this.atKeyword('EVENT')) {
      this.next()
      const trig = this.next()
      if (trig.t !== 'name') throw new Error('expected event trigger name')
      const s: Sentence = { kind: 'event', clock, trigger: trig.v, clauses: [], outcomes: [] }
      if (this.atPunct('->')) { this.next(); s.outcomes = this.outcomeList() }
      this.expectEnd()
      return s
    }

    const clauses: Clause[] = [this.clause()]
    while (this.atPunct(';')) { this.next(); clauses.push(this.clause()) }
    let outcomes: Outcome[] = []
    if (this.atPunct('->')) { this.next(); outcomes = this.outcomeList() }
    this.expectEnd()
    return { kind: 'action', clock, trigger: null, clauses, outcomes }
  }

  expectEnd(): void {
    const t = this.peek()
    if (t) throw new Error(`unexpected trailing token '${show(t)}'`)
  }

  clause(): Clause {
    const actors: Ref[] = [this.ref()]
    while (this.atPunct(',')) { this.next(); actors.push(this.ref()) }

    let at: Ref | null = null
    if (this.atKeyword('AT')) { this.next(); at = this.ref() }

    const verbTok = this.next()
    if (verbTok.t !== 'name' || !this.dict.isVerb(verbTok.v)) {
      throw new Error(`expected a registered verb, got '${show(verbTok)}'`)
    }
    const clause: Clause = { actors, at, verb: verbTok.v, to: null, objects: [], with: [], about: null }

    if (this.atKeyword('TO')) { this.next(); clause.to = this.ref() }

    if (this.startsRef()) {
      clause.objects.push(this.ref())
      while (this.atPunct(',')) { this.next(); clause.objects.push(this.ref()) }
    }

    if (this.atKeyword('WITH')) {
      this.next()
      clause.with.push(this.ref())
      while (this.atPunct(',')) { this.next(); clause.with.push(this.ref()) }
    }
    if (this.atKeyword('ABOUT')) { this.next(); clause.about = this.ref() }

    // post-verbal AT is accepted as input sugar, canonicalized pre-verbal (§9.10)
    if (this.atKeyword('AT')) {
      this.next()
      const place = this.ref()
      if (clause.at) throw new Error('AT given twice in one clause')
      clause.at = place
    }
    return clause
  }

  private startsRef(): boolean {
    const t = this.peek()
    if (!t) return false
    if (t.t === 'str' || t.t === 'idref') return true
    if (t.t !== 'name') return false
    return !this.dict.isVerb(t.v) && !CLAUSE_KEYWORDS.has(t.v) && t.v !== 'EVENT'
  }

  /** ref = name | kind ":" name | "#" id | string (§5.1).
   *  Names in the dictionary's reserved set (keywords AND verb lexemes) must be quoted.
   *  §9.40: inside a function body a bare parameter is a ref-position placeholder
   *  (a param shadows any entity/role name); a parameter after `kind:` is refused —
   *  params substitute WHOLE refs. */
  ref(): Ref {
    const t = this.next()
    if (t.t === 'idref') return { form: 'id', id: t.v }
    if (t.t === 'str') return { form: 'name', name: t.v }
    if (t.t !== 'name') throw new Error(`expected a ref, got '${show(t)}'`)
    if (this.params?.has(t.v)) return { form: 'param', name: t.v }
    if (this.dict.reserved.has(t.v)) throw new Error(`'${t.v}' is reserved and cannot be a name (quote it)`)
    if (this.atPunct(':')) {
      this.next()
      const n = this.next()
      if (n.t !== 'name') throw new Error(`expected name after kind ':', got '${show(n)}'`)
      if (this.params?.has(n.v)) throw new Error(`a parameter cannot follow 'kind:' — bind the whole ref, not the name part`)
      if (this.dict.reserved.has(n.v)) throw new Error(`'${n.v}' is reserved and cannot be a name (quote it)`)
      return { form: 'name', kind: t.v, name: n.v }
    }
    return { form: 'name', name: t.v }
  }

  outcomeList(): Outcome[] {
    const out: Outcome[] = [this.outcome()]
    while (this.atPunct(',')) { this.next(); out.push(this.outcome()) }
    return out
  }

  outcomeListOrEmpty(): Outcome[] {
    if (!this.peek()) return []
    return this.outcomeList()
  }

  /** outcome = OPERATOR "(" args ")" | ref  (bare ref = SPAWN, §4.4).
   *  Self-delimiting ops (SPAWN/NARRATE/MOVE/DESTROY/REVEAL/REPLACE/ENDING) also
   *  accept a canonical paren-less form; SET/RELATE/UNRELATE/REFUSE/ADVANCE
   *  require parens. IF (§9.39) is intercepted before op dispatch. A declared
   *  function's NAME( dispatches to a call BEFORE op parsing (§9.40); an
   *  unknown NAME( is a compile error listing the vocabulary — ops + functions.
   *  Calls only ever parse here, in outcome position: clause and ref positions
   *  are untouched, so a function may share a name with an entity or a verb. */
  outcome(): Outcome {
    const t = this.peek()
    if (t?.t === 'name' && t.v === 'IF') return this.ifOutcome()
    if (t?.t === 'name' && this.atPunct('(', 1)) {
      if (this.fns?.byName.has(t.v)) return this.fnCall()
      if (!this.dict.isOp(t.v)) throw new Error(unknownCallError(t.v, this.fns))
    }
    if (t?.t === 'name' && this.dict.isOp(t.v)) {
      this.next()
      const op = t.v
      // REPLACE is paren-less only: its WITH keyword would mis-parse as the
      // `with` role ref inside a generic arg list (§4.4)
      if (op === 'REPLACE' && this.atPunct('(')) {
        throw new Error('REPLACE takes the paren-less form: REPLACE ref WITH prototype')
      }
      // ENDING is paren-less only: its id is a plain name, never an arg list (§9.35)
      if (op === 'ENDING' && this.atPunct('(')) {
        throw new Error('ENDING takes the paren-less form: ENDING ENDING_ID')
      }
      // ROLL is paren-only, and its args are NOT the generic arg grammar:
      // they are §6.3 value expressions (`ROLL(total(actors, aspect:nerve), 6)`
      // carries a comma inside nested parens the generic splitter cannot
      // handle, §9.41) — parsed below by a depth-aware Tok scan
      if (op === 'ROLL') {
        if (!this.atPunct('(')) throw new Error('ROLL takes the paren form: ROLL(<count-expr>, <sides-expr>[, <bonus-expr>])')
        return this.rollOutcome()
      }
      if (this.atPunct('(')) {
        this.next()
        const args: Arg[] = []
        if (!this.atPunct(')')) {
          args.push(this.arg())
          while (this.atPunct(',')) { this.next(); args.push(this.arg()) }
        }
        this.expectPunct(')')
        // ROLLED is the engine-written record of a ROLL (§9.41): the parse
        // entry exists so concrete rows re-parse (journal round-trip, prose
        // rendering), with the arity pinned — authoring one is caught at
        // content compile (paren-form-error precedent)
        if (op === 'ROLLED' && (args.length !== 2 || args[0].t !== 'int' || args[1].t !== 'int')) {
          throw new Error('ROLLED takes exactly two integers: ROLLED(<total>, <ctrAfter>)')
        }
        // NARRATE's canonical form is a single string (§4.4); extra args would
        // be dropped by the formatter, so refuse them at the door (§9.6).
        // §9.40: a parameter standing in the string position is allowed — it
        // binds to the call's string at expansion.
        if (op === 'NARRATE' && (args.length !== 1 || (args[0].t !== 'str' && args[0].t !== 'param'))) {
          throw new Error('NARRATE takes exactly one string')
        }
        return { op, args }
      }
      if (op === 'SPAWN') return { op, args: this.spawnArgs() }
      if (op === 'NARRATE') {
        const s = this.next()
        if (s.t === 'name' && this.params?.has(s.v)) return { op, args: [{ t: 'param', name: s.v }] }
        if (s.t !== 'str') throw new Error(`NARRATE expects a string, got '${show(s)}'`)
        return { op, args: [{ t: 'str', v: s.v }] }
      }
      if (op === 'DESTROY' || op === 'REVEAL') return { op, args: [this.roleOrRefArg()] }
      if (op === 'ENDING') {
        // ENDING id (§9.35): a plain ref-form id, so quoted ids round-trip too
        return { op, args: [{ t: 'ref', ref: this.ref() }] }
      }
      if (op === 'REPLACE') {
        // REPLACE ref WITH prototype (§4.4): target may be a role ref
        const target = this.roleOrRefArg()
        if (!this.atKeyword('WITH')) throw new Error('REPLACE expects WITH')
        this.next()
        return { op, args: [target, { t: 'ref', ref: this.ref() }] }
      }
      if (op === 'MOVE') {
        const args: Arg[] = [this.roleOrRefArg()]
        if (!this.atKeyword('INTO')) throw new Error('MOVE expects INTO')
        this.next()
        args.push({ t: 'into', ref: this.refTarget() })
        return { op, args }
      }
      throw new Error(`${op} requires parentheses`)
    }
    return { op: 'SPAWN', args: [{ t: 'ref', ref: this.ref() }] }
  }

  /** NAME(arg, …) — a call to a declared function (§9.40). Args parse with the
   *  generic grammar (inside a body, bare params become placeholders, so a
   *  body can forward its parameters to nested calls); arity is checked HERE,
   *  a load error. The node is authored form: `expandFnCalls` replaces it with
   *  the function's body before the content finishes compiling. */
  private fnCall(): Outcome {
    const nameTok = this.next()
    if (nameTok.t !== 'name') throw new Error('internal: function dispatch on a non-name token')
    const name = nameTok.v
    const fn = this.fns!.byName.get(name)!
    this.expectPunct('(')
    const args: Arg[] = []
    if (!this.atPunct(')')) {
      args.push(this.arg())
      while (this.atPunct(',')) { this.next(); args.push(this.arg()) }
    }
    this.expectPunct(')')
    if (args.length !== fn.params.length) {
      throw new Error(
        `function '${name}' expects ${fn.params.length} argument(s)` +
        (fn.params.length > 0 ? ` (${fn.params.join(', ')})` : '') +
        `, got ${args.length}`,
      )
    }
    return { op: 'FN', call: name, args }
  }

  /** IF pred -> ( outcomes ) { ELSE IF pred -> ( outcomes ) } [ELSE -> ( outcomes )]
   *  (§9.39). Branch bodies REQUIRE parens. The ELSE chain is consumed
   *  greedily INSIDE this node — the `,`-loop of outcomeList must not strand
   *  an ELSE — so a nested ELSE binds to the nearest IF, and an ELSE after an
   *  IF's closed body belongs to the outer one. */
  private ifOutcome(): Outcome {
    this.next() // IF
    const arms: IfArm[] = [this.ifArm()]
    let fallback: Outcome[] | null = null
    while (this.atKeyword('ELSE')) {
      this.next()
      if (this.atKeyword('IF')) {
        this.next()
        arms.push(this.ifArm())
      } else {
        this.expectPunct('->')
        fallback = this.branchBody()
        break // ELSE is terminal
      }
    }
    return { op: 'IF', args: [], arms, fallback }
  }

  private ifArm(): IfArm {
    // the condition is parsed from the SENTENCE token stream as a balanced
    // unit up to the depth-0 `->` (predicates contain commas via rel()/every()
    // — never comma-split); parsePredicateToks reports where it ended.
    // §9.40: inside a function body the active parameters are handed in, so a
    // bare param parses as a cmp-operand placeholder for expansion to bind.
    const { pred, end } = parsePredicateToks(this.toks.slice(this.pos), this.params)
    this.pos += end
    this.expectPunct('->')
    return { when: pred, then: this.branchBody() }
  }

  private branchBody(): Outcome[] {
    this.expectPunct('(')
    const out = this.outcomeList()
    this.expectPunct(')')
    return out
  }

  /**
   * ROLL(<count-expr>, <sides-expr>[, <bonus-expr>]) — §9.41. The generic
   * paren'd-arg machinery cannot parse these: `total(actors, aspect:nerve)`
   * carries a comma inside nested parens and the generic splitter has no
   * depth tracking. Each argument is instead consumed as a BALANCED Tok unit
   * up to the depth-0 `,` (or the closing `)`), then parsed with the SAME
   * §6.3 value-expression grammar the IF cmp operands use
   * (`parseValueExprToks` — ints, count(role), count(<query>),
   * total(role, aspect:path), role.aspect.x, role.attr.x, the §9.41 `roll`
   * operand, and — inside a function body — bare parameters, so a template's
   * `ROLL(n, 6)` parses and expansion binds the call's number). The exprs
   * ride as `{t:'vexpr'}` args; arity is pinned here, a load error.
   */
  private rollOutcome(): Outcome {
    this.next() // '('
    const args: Arg[] = []
    if (!this.atPunct(')')) {
      for (;;) {
        args.push({ t: 'vexpr', v: this.rollArgExpr() })
        if (this.atPunct(',')) { this.next(); continue }
        break
      }
    }
    this.expectPunct(')')
    if (args.length < 2 || args.length > 3) {
      throw new Error(`ROLL takes (count-expr, sides-expr[, bonus-expr]), got ${args.length} argument(s)`)
    }
    return { op: 'ROLL', args }
  }

  /** one balanced ROLL argument: Toks up to the depth-0 ',' or ')' */
  private rollArgExpr(): ValExpr {
    const start = this.pos
    let depth = 0
    for (;;) {
      const t = this.peek()
      if (!t) throw new Error(`unexpected end of ROLL arguments — ROLL takes (count-expr, sides-expr[, bonus-expr])`)
      if (t.t === 'punct') {
        if (t.v === '(') depth++
        else if (t.v === ')') { if (depth === 0) break; depth-- }
        else if (t.v === ',' && depth === 0) break
      }
      this.next()
    }
    const slice = this.toks.slice(start, this.pos)
    if (slice.length === 0) throw new Error('ROLL arguments cannot be empty')
    return parseValueExprToks(slice, this.params)
  }

  private roleOrRefArg(): Arg {
    const t = this.peek()
    // §9.40: a parameter wins before the role check — a param may shadow a
    // role name inside a body exactly as it shadows an entity name
    if (t && t.t === 'name' && this.params?.has(t.v)) {
      this.next()
      return { t: 'param', name: t.v }
    }
    const role = t && t.t === 'name' ? asRole(t.v) : undefined
    if (role) {
      this.next()
      return { t: 'role', role }
    }
    return { t: 'ref', ref: this.ref() }
  }

  /** SPAWN ref [INTO target] [AS #id] [k=v …] — canonical paren-less form */
  private spawnArgs(): Arg[] {
    const args: Arg[] = [{ t: 'ref', ref: this.ref() }]
    if (this.atKeyword('INTO')) { this.next(); args.push({ t: 'into', ref: this.refTarget() }) }
    if (this.atKeyword('AS')) {
      this.next()
      const id = this.next()
      if (id.t !== 'idref') throw new Error(`expected #id after AS, got '${show(id)}'`)
      args.push({ t: 'asId', id: id.v })
    }
    while (this.atPunct('=', 1)) {
      const k = this.next()
      if (k.t !== 'name') throw new Error(`expected key=value pair, got '${show(k)}'`)
      this.next() // '='
      // §9.40: a parameter as the pair's key OR value replaces the whole pair
      const vTok = this.peek()
      let pName: string | null = null
      if (this.params?.has(k.v)) pName = k.v
      else if (vTok && vTok.t === 'name' && this.params?.has(vTok.v)) pName = vTok.v
      if (pName) { this.next(); args.push({ t: 'param', name: pName }); continue }
      args.push({ t: 'pair', k: k.v, v: this.valueToken() })
    }
    return args
  }

  arg(): Arg {
    const t = this.peek()
    if (!t) throw new Error('unexpected end of outcome args')
    // signed SET value (§9.31): '+'/'-' int is a delta on the entry's current value.
    // §9.40: the magnitude may be a parameter — the sign is carried on the
    // placeholder and folded into the call's number at expansion.
    if (t.t === 'punct' && (t.v === '+' || t.v === '-')) {
      this.next()
      const n = this.next()
      if (n.t === 'name' && this.params?.has(n.v)) {
        return { t: 'param', name: n.v, sign: t.v === '-' ? -1 : 1 }
      }
      if (n.t === 'dur') throw new Error('deltas are integers only (no duration deltas)')
      if (n.t !== 'int') throw new Error(`expected an integer after '${t.v}', got '${show(n)}'`)
      return { t: 'delta', v: t.v === '-' ? -n.v : n.v }
    }
    if (t.t === 'str') { this.next(); return { t: 'str', v: t.v } }
    if (t.t === 'dur') { this.next(); return { t: 'dur', v: t.v } }
    if (t.t === 'int') { this.next(); return { t: 'int', v: t.v } }
    if (t.t === 'idref') { this.next(); return { t: 'ref', ref: { form: 'id', id: t.v } } }
    if (t.t === 'name') {
      if (t.v === 'INTO') { this.next(); return { t: 'into', ref: this.refTarget() } }
      if (t.v === 'AS') {
        this.next()
        const id = this.next()
        if (id.t !== 'idref') throw new Error(`expected #id after AS, got '${show(id)}'`)
        return { t: 'asId', id: id.v }
      }
      // §9.40: a bare parameter stands wherever a ref/value literal is expected
      // (int, string-binding, bool, dur, key, pair, ref positions) — the whole
      // arg is the placeholder; kind mismatches surface as runtime step
      // rejections, like any other kind error
      if (this.params?.has(t.v)) { this.next(); return { t: 'param', name: t.v } }
      const role = asRole(t.v)
      if (role) {
        // ACTORS/OBJECT/… in outcome args are role refs (e.g. MOVE ACTORS INTO OBJECT)
        this.next()
        return { t: 'role', role }
      }
      if (t.v === 'true' || t.v === 'false') { this.next(); return { t: 'bool', v: t.v === 'true' } }
      if (this.atPunct('=', 1)) {
        // §9.40: a parameter as the pair's key OR value replaces the whole pair
        const vTok = this.peek(2)
        let pName: string | null = null
        if (this.params?.has(t.v)) pName = t.v
        else if (vTok && vTok.t === 'name' && this.params?.has(vTok.v)) pName = vTok.v
        this.next(); this.next()
        if (pName) return { t: 'param', name: pName }
        return { t: 'pair', k: t.v, v: this.valueToken() }
      }
      // `aspect:` names the aspects map in SET keys (§9.31): aspect:secret,
      // aspect:language.latin — one key token, like the query form (§4.5)
      if (t.v === 'aspect' && this.atPunct(':', 1) && this.peek(2)?.t === 'name') {
        this.next(); this.next()
        const n = this.next()
        let key = 'aspect:' + n.v
        while (this.atPunct('.') && this.peek(1)?.t === 'name') { this.next(); key += '.' + this.next().v }
        return { t: 'key', v: key }
      }
      // dotted names in arg position are SET keys: SET(x, state.sealed, true)
      if (this.atPunct('.', 1) && this.peek(2)?.t === 'name') {
        let key = t.v
        this.next() // the leading name
        while (this.atPunct('.') && this.peek(1)?.t === 'name') { this.next(); key += '.' + this.next().v }
        return { t: 'key', v: key }
      }
      return { t: 'ref', ref: this.ref() }
    }
    throw new Error(`unexpected token in outcome args: '${show(t)}'`)
  }

  private refTarget(): Ref | { role: Role } {
    const t = this.peek()
    if (t && t.t === 'name' && this.params?.has(t.v)) {
      this.next()
      return { form: 'param', name: t.v }
    }
    const role = t && t.t === 'name' ? asRole(t.v) : undefined
    if (role) {
      this.next()
      return { role }
    }
    return this.ref()
  }

  private valueToken(): string | number | boolean {
    const t = this.next()
    if (t.t === 'int') return t.v
    if (t.t === 'dur') return t.v // minutes
    if (t.t === 'str') return t.v
    if (t.t === 'name') {
      if (t.v === 'true') return true
      if (t.v === 'false') return false
      return t.v
    }
    throw new Error(`expected a value, got '${show(t)}'`)
  }
}

function show(t: Tok): string {
  return t.t === 'str' ? `"${t.v}"` : String(t.v)
}

// ---------- expansion: compile-time call substitution (§9.40) ----------

/**
 * Replace every FN call in an authored outcome list with its function's body,
 * substituting parameters — THE compile-time macro expansion (§9.40). Content
 * compile runs this over every recipe/trigger outcome list right after
 * `parseOutcomes`, BEFORE the trigger guards and checkEndingRefs walk the
 * list, so expanded bodies are guarded exactly like literal ones and the
 * engine (and therefore the log) never sees a call. Canonical equals
 * expansion: a call site IS its expanded body, byte for byte.
 *
 * Two substitution mechanisms meet here:
 *  - AST-level: a `{t:'param'}`/`{form:'param'}` placeholder is replaced by
 *    the call-site arg for that parameter, in position — ref, role, int,
 *    delta sign, string, bool, dur, key, into, pair. A kind mismatch the AST
 *    can represent splices through and surfaces as a runtime step rejection,
 *    like any kind error; one it cannot (a non-ref in an INTO slot) is a load
 *    error naming the parameter.
 *  - STRING-interpolation: `{param}` inside a quoted string in a function
 *    body is rewritten to the call-site arg's canonical spelling. The
 *    reserved prose/dice tokens — {actors} {object} {with} {at} {about} {to}
 *    {roll} — are NEVER param-substituted and pass through; an undeclared
 *    `{token}` is a load-time error (a warning has no channel in this
 *    compiler, and the alternative silently bakes the typo into every call's
 *    journal prose).
 *
 * Bodies nest (a body calling functions expands recursively — forward and
 * mutual references are fine because expansion runs after ALL bodies parse);
 * recursion and depth are load errors, rejected earlier over the call graph.
 */
export function expandFnCalls(out: Outcome[], fns: FnTable): Outcome[] {
  return expandList(out, fns, new Map(), null)
}

/** tokens a body string may carry that are never parameter names:
 *  the §5.3 prose substitutions and F4's {roll} */
export const FN_RESERVED_TOKENS = new Set(['actors', 'object', 'with', 'at', 'about', 'to', 'roll'])

/** nesting cap for legitimate chains (§9.40): depth(f) = 1 + max(depth of its
 *  callees), a body with no calls is depth 1 — depth 9 is a load error */
export const MAX_FN_DEPTH = 8

function expandList(list: Outcome[], fns: FnTable, bind: Map<string, Arg>, fn: string | null): Outcome[] {
  const out: Outcome[] = []
  for (const o of list) {
    if (o.op === 'FN') {
      const name = o.call!
      const tpl = fns.byName.get(name)
      if (!tpl) throw new Error(`internal: call to undeclared function '${name}' survived parsing`)
      // bind this call's args (themselves substituted by the enclosing
      // function's parameters, if any) — always concrete before expansion
      const bind2 = new Map<string, Arg>()
      tpl.params.forEach((p, i) => bind2.set(p, substArg(o.args[i]!, fn, bind)))
      out.push(...expandList(tpl.body, fns, bind2, name))
    } else if (o.op === 'IF') {
      out.push({
        op: 'IF',
        args: [],
        arms: (o.arms ?? []).map(a => ({
          when: fn ? substPred(a.when, fn, bind) : a.when,
          then: expandList(a.then, fns, bind, fn),
        })),
        fallback: o.fallback ? expandList(o.fallback, fns, bind, fn) : o.fallback,
      })
    } else {
      out.push({ op: o.op, args: fn ? o.args.map(a => substArg(a, fn, bind)) : [...o.args] })
    }
  }
  return out
}

/** substitute one arg against the enclosing function's bindings */
function substArg(a: Arg, fn: string | null, bind: Map<string, Arg>): Arg {
  if (a.t === 'param') {
    const bound = bind.get(a.name)
    if (!bound) throw new Error(`internal: parameter '${a.name}' of '${fn ?? '?'}' has no binding`)
    if (a.sign !== undefined) {
      // a delta position (`+n`/`-n`): fold the authored sign into the call's
      // number; anything else splices through for the runtime kind error
      if (bound.t === 'int') return { t: 'delta', v: a.sign * bound.v }
      if (bound.t === 'delta') return { t: 'delta', v: a.sign * bound.v }
      return bound
    }
    return bound
  }
  if (a.t === 'vexpr') {
    // §9.41: a ROLL argument is a value expression; a body template may carry
    // a param operand inside it (`ROLL(n, 6)`), substituted by the same rule
    // as IF-condition cmp operands
    return { t: 'vexpr', v: substExpr(a.v, fn ?? '?', bind) }
  }
  if (a.t === 'ref' && a.ref.form === 'param') {
    // whole-ref position: the bound arg splices in as-is — a ref or a role
    // ref behaves exactly like the same text authored at the call site; a
    // value literal lands where a ref belongs and is rejected at runtime
    const bound = bind.get(a.ref.name)
    if (!bound) throw new Error(`internal: parameter '${a.ref.name}' has no binding`)
    return bound
  }
  if (a.t === 'into' && 'form' in a.ref && a.ref.form === 'param') {
    const bound = bind.get(a.ref.name)
    if (!bound) throw new Error(`internal: parameter '${a.ref.name}' has no binding`)
    if (bound.t === 'ref') return { t: 'into', ref: bound.ref }
    if (bound.t === 'role') return { t: 'into', ref: { role: bound.role } }
    throw new Error(
      `function '${fn ?? '?'}': parameter '${a.ref.name}' stands in an INTO position — ` +
      `the call supplies a ${bound.t}, not a ref or role`,
    )
  }
  if (a.t === 'str' && fn !== null) {
    return { t: 'str', v: substString(a.v, fn, bind) }
  }
  return a
}

/** `{param}` → the call-site arg's canonical spelling, inside a function body string.
 *  One non-rescanning pass over the body string: the exact spelling `{name}`
 *  substitutes (reserved tokens pass through; undeclared names are a load
 *  error); a SPACED spelling (`{ x }`, `{ actors }`) never rewrites — not
 *  here, and not in the §5.3 render pass either, which reads exact `{token}`
 *  spellings — so it could only bake into journal prose literally. That is
 *  the same typo the undeclared-token load error exists for, whatever the
 *  identifier names. Commas/quotes inside braces are ordinary prose. */
function substString(s: string, fn: string, bind: Map<string, Arg>): string {
  return s.replace(/\{(\s*)([A-Za-z_][A-Za-z0-9_]*)(\s*)\}/g, (whole, pre: string, name: string, post: string) => {
    if (pre.length > 0 || post.length > 0) throw undeclaredTokenError(fn, whole)
    if (FN_RESERVED_TOKENS.has(name)) return whole
    const bound = bind.get(name)
    if (!bound) throw undeclaredTokenError(fn, whole)
    return argSpelling(bound, fn, name)
  })
}

/** the undeclared-interpolation load error: names the token, the function,
 *  and the reserved set it would have had to belong to */
function undeclaredTokenError(fn: string, token: string): Error {
  return new Error(
    `function '${fn}': '${token}' in a body string names neither a parameter of '${fn}' ` +
    `nor a reserved token ({${[...FN_RESERVED_TOKENS].join('} {')}}) — ` +
    `undeclared interpolation is a load error, not a silent literal ` +
    `(braced tokens must be spelled exactly, without inner spaces)`,
  )
}

/** the canonical spelling of a call-site arg, for string interpolation */
function argSpelling(a: Arg, fn: string, param: string): string {
  switch (a.t) {
    case 'ref':
      if (a.ref.form === 'id') return `#${a.ref.id}`
      if (a.ref.form === 'param') throw new Error(`internal: parameter '${param}' bound to a parameter`)
      return a.ref.kind ? `${a.ref.kind}:${a.ref.name}` : a.ref.name
    case 'role': return a.role.toUpperCase()
    case 'int': return String(a.v)
    case 'delta': return a.v < 0 ? String(a.v) : `+${a.v}`
    case 'str': return a.v
    case 'bool': return String(a.v)
    case 'dur': return formatDuration(a.v)
    case 'key': return a.v
    case 'pair': return `${a.k}=${String(a.v)}`
    default:
      throw new Error(`function '${fn}': parameter '${param}' cannot interpolate into a string (bound to a ${a.t})`)
  }
}

/** substitute a condition: parameters stand in cmp operands (numeric literal
 *  positions); quantifiers/exists/query terms take none — the grammar already
 *  rejected them at parse */
function substPred(p: Pred, fn: string, bind: Map<string, Arg>): Pred {
  switch (p.t) {
    case 'and': return { t: 'and', a: substPred(p.a, fn, bind), b: substPred(p.b, fn, bind) }
    case 'or': return { t: 'or', a: substPred(p.a, fn, bind), b: substPred(p.b, fn, bind) }
    case 'not': return { t: 'not', a: substPred(p.a, fn, bind) }
    case 'cmp':
      return { t: 'cmp', l: substExpr(p.l, fn, bind), op: p.op, r: substExpr(p.r, fn, bind) }
    default:
      return p // rel/every/any/exists/existsQuery carry no parameters
  }
}

function substExpr(x: ValExpr, fn: string, bind: Map<string, Arg>): ValExpr {
  if (x.t === 'param') {
    const bound = bind.get(x.name)
    if (!bound) throw new Error(`internal: parameter '${x.name}' of '${fn}' has no binding`)
    if (bound.t === 'int') return { t: 'int', v: bound.v }
    if (bound.t === 'delta') return { t: 'int', v: bound.v } // the call's signed magnitude
    if (bound.t === 'dur') return { t: 'int', v: bound.v }   // durations are minutes
    throw new Error(
      `function '${fn}': parameter '${x.name}' stands in an IF condition — ` +
      `the call supplies a ${bound.t}, not a number`,
    )
  }
  return x
}

/** the unknown `NAME(...)` compile error: the full outcome vocabulary — ops,
 *  then declared functions — so the author can see what IS legal; a name that
 *  looks like a predicate helper gets the forgotten-IF hint */
function unknownCallError(name: string, fns?: FnTable): string {
  const predHelpers = ['exists', 'count', 'total', 'rel', 'every', 'any']
  const fnsListed = fns ? [...fns.byName.keys()] : []
  return (
    `unknown '${name}(...)' in outcome position — the outcome vocabulary is: ` +
    `${OUTCOME_OPS.join(', ')}` +
    (fnsListed.length > 0 ? `; declared functions: ${fnsListed.join(', ')}` : '; no functions declared') +
    (predHelpers.includes(name) ? ` (a condition wants the IF keyword: IF …${name}(…) -> ( outcomes ))` : '') +
    ` (bare names spawn — an op's parens are part of its grammar; calls need a declared function)`
  )
}
