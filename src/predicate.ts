// Condition predicates for recipe `when` — spec §6.3.
//   count(actors) = 2 AND rel(parent_child, actors) AND NOT any(actors, aspect:occult >= 2)
//   total(actors, aspect:lantern) < object.aspect.demand
//   exists(aspect:hunter>=1) AND NOT exists(THE_SEALED_DOOR)   # world-existential
// Path operands use dots (object.aspect.demand); scalar conditions inside
// every/any use the query colon form (aspect:occult >= 2).
// The SAME grammar serves the IF conditions of outcome lists (§9.39):
// `parsePredicate` string-parses `when:` lines; `parsePredicateToks` parses
// an IF condition out of the SENTENCE token stream. One grammar, two
// front-ends — no duplicated semantics.
import type { Entity, State } from './types.js'
import { aspectGet, relationAlive } from './types.js'
import { matchQuery, parseQuery, type CmpOp, type Query } from './query.js'
import { ROLES } from './sentence.js'
import type { Tok } from './lexer.js'

export type RoleBindings = Record<string, Entity[]>

export interface PredCtx {
  b: RoleBindings
  state: State
  /** §9.39 asymmetric failure discipline: IF conditions evaluate STRICTLY —
   *  a path operand whose role has no bound card cannot resolve and REJECTS
   *  the step (§9.19), where a `when:` predicate would silently read false.
   *  Missing aspects, counts over anything and world forms still read
   *  false/0 — they are legitimate branch selection, not failures. */
  strict?: boolean
}

export type Pred =
  | { t: 'and'; a: Pred; b: Pred }
  | { t: 'or'; a: Pred; b: Pred }
  | { t: 'not'; a: Pred }
  | { t: 'cmp'; l: ValExpr; op: CmpOp; r: ValExpr }
  | { t: 'rel'; rtype: string; role: string; role2?: string }
  | { t: 'every'; role: string; cond: ScalarCond }
  | { t: 'any'; role: string; cond: ScalarCond }
  | { t: 'exists'; name: string }        // ∃ an ALIVE entity bearing this exact name
  | { t: 'existsQuery'; query: Query }   // ∃ an alive entity matching a §4.5 slot query

export type ValExpr =
  | { t: 'int'; v: number }
  | { t: 'count'; role: string }
  | { t: 'countQuery'; query: Query }    // §9.38: count over the whole living world (any §4.5 query)
  | { t: 'total'; role: string; path: string }
  | { t: 'path'; role: string; field: 'aspect' | 'attr'; key: string }
  | { t: 'param'; name: string }         // F3 authored-only: a function parameter standing in a cmp
                                         // operand (§9.40); expansion replaces it with the call's
                                         // number — it never reaches evaluation
// F4 will add `{ t: 'roll' }` here — the seeded die result, evaluated from the
// APPLY context's per-clause `roll` binding (never from State: the binding
// dies with the draft on REFUSE). This evaluator is the extension point.

export interface ScalarCond {
  path: { field: 'aspect' | 'attr'; key: string }
  op: CmpOp
  value: number
}

// ---------- parser (precedence climbing: NOT > AND > OR) ----------

const CMP_OPS = ['>=', '<=', '!=', '=', '>', '<']

/** a pre-lexed predicate: lexemes + source spans + the source the spans index.
 *  The string front-end builds it from the regex tokenizer below; the Tok
 *  front-end (parsePredicateToks) re-serializes sentence Toks into the same
 *  shape, so every grammar rule — including the exists()/count() inner-query
 *  reconstruction, which reads source offsets — behaves identically. */
interface PredView {
  src: string
  toks: string[]
  spans: { start: number; end: number }[]
}

export function parsePredicate(src: string): Pred {
  return new PredParser(src).parse()
}

/**
 * Parse one IF condition out of the SENTENCE token stream (§9.39) and report
 * where it ended. The condition is consumed as a balanced unit up to the
 * depth-0 `->` — predicates carry commas inside rel()/every() parens, so a
 * comma is never a split point; a depth-0 `,`/`;`/ELSE means the required
 * `-> ( outcomes )` is missing. Returns `end` = the number of Toks consumed,
 * i.e. the index of the `->` arrow in the given slice. `params` (F3, §9.40)
 * names the enclosing function's parameters: a bare operand token matching a
 * parameter parses as a param ValExpr, expanded to the call's number at load.
 */
export function parsePredicateToks(toks: Tok[], params?: ReadonlySet<string>): { pred: Pred; end: number } {
  let depth = 0
  let end = -1
  for (let i = 0; i < toks.length; i++) {
    const t = toks[i]!
    if (t.t === 'punct') {
      if (t.v === '(') depth++
      else if (t.v === ')') {
        depth--
        if (depth < 0) throw new Error(`unbalanced ')' in IF condition`)
      } else if (depth === 0 && t.v === '->') { end = i; break }
      else if (depth === 0 && (t.v === ',' || t.v === ';')) {
        throw new Error(`an IF condition needs '-> ( outcomes )'`)
      }
    } else if (depth === 0 && t.t === 'name' && t.v === 'ELSE') {
      throw new Error(`an IF needs '-> ( outcomes )' before 'ELSE'`)
    }
  }
  if (end === -1) throw new Error(`an IF condition needs '-> ( outcomes )'`)
  const slice = toks.slice(0, end)
  if (slice.length === 0) throw new Error(`an IF needs a condition before '->'`)
  const view = predViewFromToks(slice)
  return { pred: new PredParser(view.src, view, params).parse(), end }
}

/** sentence Toks → this parser's own token alphabet (§9.39). Word-adjacent
 *  lexemes are space-separated so they cannot merge; everything else glues
 *  exactly as the string grammar expects (`aspect:hunter>=1`,
 *  `state:!sealed`, dotted paths, `-aspect:winter`). */
function predViewFromToks(toks: Tok[]): PredView {
  let src = ''
  const ptoks: string[] = []
  const spans: { start: number; end: number }[] = []
  for (const t of toks) {
    let text: string
    switch (t.t) {
      case 'name': text = t.v; break
      case 'int': text = String(t.v); break
      case 'punct': text = t.v; break
      case 'str': throw new Error(`an IF condition cannot contain a string literal`)
      case 'dur': throw new Error(`an IF condition cannot contain a duration literal`)
      case 'idref': throw new Error(`an IF condition cannot contain a #id literal`)
    }
    if (spans.length > 0 && /[A-Za-z0-9_]$/.test(src) && /^[A-Za-z0-9_]/.test(text)) src += ' '
    spans.push({ start: src.length, end: src.length + text.length })
    src += text
    ptoks.push(text)
  }
  return { src, toks: ptoks, spans }
}

class PredParser {
  private toks: string[] = []
  private spans: { start: number; end: number }[] = [] // source offsets per token
  private pos = 0
  private readonly src: string
  constructor(src: string, pre?: PredView, private params?: ReadonlySet<string>) {
    this.src = src
    if (pre) { this.toks = pre.toks; this.spans = pre.spans; return }
    // `-` is a token only so exists(...) query terms can negate (`-aspect:winter`)
    const re = /[A-Za-z_][A-Za-z0-9_.]*|\d+|>=|<=|!=|=|>|<|\(|\)|,|:|-/g
    let m: RegExpExecArray | null
    while ((m = re.exec(src)) !== null) {
      this.toks.push(m[0])
      this.spans.push({ start: m.index, end: m.index + m[0].length })
    }
    if (this.toks.length === 0) throw new Error(`empty predicate '${src}'`)
  }
  private peek(): string | undefined { return this.toks[this.pos] }
  private next(): string {
    const t = this.peek()
    if (t === undefined) throw new Error('unexpected end of predicate')
    this.pos++
    return t
  }
  private expect(w: string): void {
    const t = this.next()
    if (t !== w) throw new Error(`expected '${w}', got '${t}'`)
  }

  parse(): Pred {
    const p = this.or()
    if (this.pos !== this.toks.length) throw new Error(`trailing tokens in predicate at '${this.peek()}'`)
    return p
  }

  private or(): Pred {
    let a = this.and()
    while (this.peek() === 'OR') { this.next(); a = { t: 'or', a, b: this.and() } }
    return a
  }
  private and(): Pred {
    let a = this.unary()
    while (this.peek() === 'AND') { this.next(); a = { t: 'and', a, b: this.unary() } }
    return a
  }
  private unary(): Pred {
    if (this.peek() === 'NOT') { this.next(); return { t: 'not', a: this.unary() } }
    return this.atom()
  }
  private atom(): Pred {
    if (this.peek() === '(') { this.next(); const p = this.or(); this.expect(')'); return p }
    const t = this.peek()
    if (t === 'every' || t === 'any') return this.quantified()
    if (t === 'exists') return this.existsPred()
    if (t === 'rel') {
      this.next()
      this.expect('(')
      const rtype = this.dottedName()
      this.expect(',')
      const role = this.dottedName()
      let role2: string | undefined
      if (this.peek() === ',') {
        this.next()
        role2 = this.dottedName()
      }
      this.expect(')')
      return { t: 'rel', rtype, role, role2 }
    }
    return this.cmpFromExpr()
  }

  private quantified(): Pred {
    const kind = this.next() as 'every' | 'any'
    this.expect('(')
    const role = this.next()
    this.expect(',')
    const cond = this.scalarCond()
    this.expect(')')
    return { t: kind, role, cond }
  }

  // exists(NAME) — an ALIVE entity bears the name; exists(<query>) — an alive
  // entity matches the §4.5 slot query. The first token inside the parens
  // decides: a query-term starter (kind:/aspect:/state:/name: or a leading -)
  // means query form, anything else is a single NAME.
  private existsPred(): Pred {
    this.next() // 'exists'
    this.expect('(')
    const first = this.pos
    let depth = 1
    while (depth > 0) {
      const tk = this.next()
      if (tk === '(') depth++
      else if (tk === ')') depth--
    }
    const close = this.pos - 1 // token index of the matching ')'
    if (close === first) throw new Error('exists() needs a name or a slot query')
    const head = this.toks[first]
    const inner = (): string => this.src.substring(this.spans[first].start, this.spans[close].start)
    if (this.queryStarterAt(first)) {
      // §4.5 splits terms on whitespace, so tighten `aspect:hunter >= 1` to
      // `aspect:hunter>=1` — spaces around a comparison are glue, not structure
      return { t: 'existsQuery', query: parseQuery(inner().replace(/\s*(>=|<=|!=|=|>|<)\s*/g, '$1')) }
    }
    if (close - first > 1) throw new Error(`exists() takes one name or a kind:/aspect:/state:/name: query, got '${inner().trim()}'`)
    return { t: 'exists', name: head }
  }

  /** a query-term starter at token i: kind:/aspect:/state:/name: or a leading `-` (§4.5, §9.38) */
  private queryStarterAt(i: number): boolean {
    const head = this.toks[i]
    return head === '-' ||
      ((head === 'kind' || head === 'aspect' || head === 'state' || head === 'name') && this.toks[i + 1] === ':')
  }

  private scalarCond(): ScalarCond {
    this.expect('aspect')  // v0.2: scalar conditions read aspects
    this.expect(':')
    const key = this.dottedName()
    const opTok = this.next()
    const op = CMP_OPS.find(o => o === opTok)
    if (!op) throw new Error(`expected comparison op, got '${opTok}'`)
    const v = this.next()
    if (!/^\d+$/.test(v)) throw new Error(`expected integer in scalar condition, got '${v}'`)
    return { path: { field: 'aspect', key }, op: op as CmpOp, value: Number(v) }
  }

  private cmpFromExpr(): Pred {
    const l = this.valExpr()
    const opTok = this.next()
    const op = CMP_OPS.find(o => o === opTok)
    if (!op) throw new Error(`expected comparison op, got '${opTok}'`)
    const r = this.valExpr()
    return { t: 'cmp', l, op: op as CmpOp, r }
  }

  private valExpr(): ValExpr {
    const t = this.next()
    // F3 (§9.40): a bare parameter name is a param operand — expansion binds
    // it to the call's number. Parameters only ever stand where a numeric
    // literal is expected; query terms and scalar conditions take none.
    if (this.params?.has(t)) return { t: 'param', name: t }
    if (/^\d+$/.test(t)) return { t: 'int', v: Number(t) }
    if (t === 'count') {
      this.expect('(')
      // §9.38: count(role) vs count(<query>) — a query-term starter decides,
      // mirroring exists(); the paren'd arg can carry commas inside nested
      // parens (none in the §4.5 grammar, but consume balanced regardless)
      if (this.queryStarterAt(this.pos)) {
        const first = this.pos
        let depth = 1
        while (depth > 0) {
          const tk = this.next()
          if (tk === '(') depth++
          else if (tk === ')') depth--
        }
        const close = this.pos - 1
        if (close === first) throw new Error('count() needs a role or a slot query')
        const inner = this.src.substring(this.spans[first].start, this.spans[close].start)
        return { t: 'countQuery', query: parseQuery(inner.replace(/\s*(>=|<=|!=|=|>|<)\s*/g, '$1')) }
      }
      const role = this.next()
      this.expect(')')
      return { t: 'count', role }
    }
    if (t === 'total') {
      this.expect('(')
      // total pools a bound ROLE's aspects — the query form is deliberately not
      // offered (§9.38): fail with the rule, not a cryptic `expected ','`
      if (this.queryStarterAt(this.pos)) {
        throw new Error(
          `total() pools a bound role's aspects — total(<query>, …) is not offered; ` +
          `use count(<query>) to count a pool by name (e.g. count(name:X))`,
        )
      }
      const role = this.next()
      this.expect(',')
      this.expect('aspect')
      this.expect(':')
      const path = this.dottedName()
      this.expect(')')
      return { t: 'total', role, path }
    }
    // path operand: role '.' (aspect|attr) '.' dottedKey — the tokenizer may
    // deliver the whole dotted path as one token, so handle both shapes
    if (t.includes('.')) {
      const parts = t.split('.')
      if (parts.length < 3) throw new Error(`bad path '${t}'`)
      const field = parts[1]
      if (field !== 'aspect' && field !== 'attr') {
        throw new Error(`expected aspect|attr in path '${t}'`)
      }
      return { t: 'path', role: parts[0], field, key: parts.slice(2).join('.') }
    }
    this.expect('.')
    const field = this.next()
    if (field !== 'aspect' && field !== 'attr') {
      throw new Error(`expected aspect|attr after '${t}.', got '${field}'`)
    }
    this.expect('.') // the second dot of role.field.key (split-token shape, §9.39)
    const key = this.dottedName()
    return { t: 'path', role: t, field, key }
  }

  private dottedName(): string {
    let name = this.next()
    while (this.peek() === '.') { this.next(); name += '.' + this.next() }
    return name
  }
}

// ---------- evaluation ----------

/** a role's bound cards, ALIVE only (§9.39 draft reads). `when:` selection
 *  never sees a dead card (slots resolve against the living), so this filter
 *  only ever matters mid-list, after a DESTROY/REPLACE earlier in the same
 *  outcome list: a dead-stamped entity reads as absent — `DESTROY OBJECT`
 *  then `IF object.aspect.x` is a silent false, never a resolution failure. */
function roleCards(ctx: PredCtx, role: string): Entity[] {
  return (ctx.b[role] ?? []).filter(e => e.deadAction === null)
}

export function evalPred(p: Pred, ctx: PredCtx): boolean {
  switch (p.t) {
    case 'and': return evalPred(p.a, ctx) && evalPred(p.b, ctx)
    case 'or': return evalPred(p.a, ctx) || evalPred(p.b, ctx)
    case 'not': return !evalPred(p.a, ctx)
    case 'rel': {
      // rel(type, role) pairs cards within one role; rel(type, a, b) crosses
      // two roles — e.g. rel(neighbor, actors.container, object) checks the
      // first actor's location against each object card (adjacency gates)
      const cardsOf = (role: string): Entity[] => {
        const base = role.endsWith('.container') ? role.slice(0, -'.container'.length) : role
        const cards = roleCards(ctx, base)
        if (!role.endsWith('.container')) return cards
        return cards.map(e => ctx.state.entities.get(String(e.attrs['container'] ?? '')) ?? e)
      }
      const a = cardsOf(p.role)
      const b = cardsOf(p.role2 ?? p.role)
      for (let i = 0; i < a.length; i++) {
        for (let j = p.role2 ? 0 : i + 1; j < b.length; j++) {
          if (!p.role2 && a[i]!.id === b[j]!.id) continue
          if (relationAlive(ctx.state, a[i]!.id, b[j]!.id, p.rtype)) return true
        }
      }
      return false
    }
    case 'every': {
      const role = roleCards(ctx, p.role)
      return role.every(e => scalarOk(e, p.cond))
    }
    case 'any': {
      const role = roleCards(ctx, p.role)
      return role.some(e => scalarOk(e, p.cond))
    }
    case 'exists': {
      // byName holds ALIVE entities only (§9.14); the deadAction re-check is
      // the same belt-and-braces resolveRef wears. Under pools (§9.38) the
      // existential is ANY living copy of the name.
      const pool = ctx.state.byName.get(p.name)
      return pool !== undefined && pool.some(id => ctx.state.entities.get(id)?.deadAction === null)
    }
    case 'existsQuery': {
      // the WHOLE world, not the step's bound roles — CS extantreqs ("anywhere
      // in play"); same input state every other condition reads (§6.4)
      for (const e of ctx.state.entities.values()) {
        if (e.deadAction === null && matchQuery(e, p.query)) return true
      }
      return false
    }
    case 'cmp': {
      const l = evalExpr(p.l, ctx), r = evalExpr(p.r, ctx)
      if (l === undefined || r === undefined) return false
      return numCmp(l, p.op, r)
    }
  }
}

function scalarOk(e: Entity, cond: ScalarCond): boolean {
  const v = cond.path.field === 'aspect' ? aspectGet(e, cond.path.key) : numberAttr(e, cond.path.key)
  return v !== undefined && numCmp(v, cond.op, cond.value)
}

function numberAttr(e: Entity, key: string): number | undefined {
  const v = e.attrs[key]
  return typeof v === 'number' ? v : undefined
}

function evalExpr(x: ValExpr, ctx: PredCtx): number | undefined {
  switch (x.t) {
    case 'int': return x.v
    case 'count': return roleCards(ctx, x.role).length
    case 'countQuery': {
      // world scope, like existsQuery (§6.3): every ALIVE card matching the query
      let n = 0
      for (const e of ctx.state.entities.values()) {
        if (e.deadAction === null && matchQuery(e, x.query)) n++
      }
      return n
    }
    case 'total':
      return roleCards(ctx, x.role).reduce((sum, e) => sum + (aspectGet(e, x.path) ?? 0), 0)
    case 'path': {
      const bound = ctx.b[x.role] ?? []
      const e = bound.find(c => c.deadAction === null) // the oldest ALIVE bound card
      if (!e) {
        // §9.39: an IF condition names a CARD that is not there — that is an
        // unresolvable ref, a step rejection (§9.19). All-dead bindings fell
        // through roleCards above only in spirit: the card EXISTS, it died
        // mid-list — reads false silently (the documented dead-stamp rule).
        if (ctx.strict && bound.length === 0) {
          throw new Error(
            `IF condition: operand '${x.role}.${x.field}.${x.key}' cannot resolve — ` +
            `role '${x.role}' has no bound card`,
          )
        }
        return undefined
      }
      return x.field === 'aspect' ? aspectGet(e, x.key) : numberAttr(e, x.key)
    }
    case 'param':
      // expansion (§9.40) replaces every param before compile finishes; this
      // is a defensive tripwire, never a gameplay path
      throw new Error(`internal: unresolved function parameter '${x.name}' was evaluated`)
  }
}

// ---------- load-time argument lint (§9.38) ----------

const ROLE_LIST = ROLES.join(', ')

/** count()/total() arguments must be a DECLARED ROLE or a query-term starter —
 *  `count(DREAD) >= 3` (the most tempting typo: a name where a role belongs)
 *  would otherwise silently evaluate to 0 at runtime. Called at content
 *  compile for every recipe `when`; the runtime path is unaffected. */
export function lintPred(where: string, p: Pred): void {
  switch (p.t) {
    case 'and':
    case 'or':
      lintPred(where, p.a)
      lintPred(where, p.b)
      return
    case 'not':
      lintPred(where, p.a)
      return
    case 'cmp':
      lintValExpr(where, p.l)
      lintValExpr(where, p.r)
      return
    default:
      return // rel/every/any/exists/existsQuery carry no count/total args
  }
}

function lintValExpr(where: string, x: ValExpr): void {
  if (x.t === 'count' && !(ROLES as readonly string[]).includes(x.role)) {
    throw new Error(
      `${where}: count() argument '${x.role}' is neither a role (${ROLE_LIST}) nor a query ` +
      `(name:x, kind:k, aspect:a>=1, state:s, -term) — did you mean count(name:${x.role})?`,
    )
  }
  if (x.t === 'total' && !(ROLES as readonly string[]).includes(x.role)) {
    throw new Error(
      `${where}: total() first argument '${x.role}' is not a declared role (${ROLE_LIST}) ` +
      `— total pools a role's aspects; use count(name:x) to count a pool by name`,
    )
  }
}

/** §9.39: trigger (slot + expiry) outcome lists have EMPTY event bindings, so
 *  any role-dependent operand in an IF condition there would silently read
 *  false/0 — the same philosophy as the §9.34 trigger guards, at load time.
 *  Called from content compile for every IF condition authored in a trigger;
 *  the world forms (exists, exists(<query>), count(<query>)) are the trigger
 *  vocabulary. Recipes are unaffected: their steps carry real bindings. */
export function lintTriggerPred(where: string, p: Pred): void {
  switch (p.t) {
    case 'and':
    case 'or':
      lintTriggerPred(where, p.a)
      lintTriggerPred(where, p.b)
      return
    case 'not':
      lintTriggerPred(where, p.a)
      return
    case 'cmp':
      lintTriggerValExpr(where, p.l)
      lintTriggerValExpr(where, p.r)
      return
    case 'rel':
      throw new Error(
        `${where}: IF operand rel(${p.rtype}, ${p.role}${p.role2 ? `, ${p.role2}` : ''}) reads a step role — ` +
        `trigger bindings are empty, it would always be false; use world forms (exists(…), count(name:…))`,
      )
    case 'every':
    case 'any':
      throw new Error(
        `${where}: IF operand ${p.t}(${p.role}, …) reads a step role — ` +
        `trigger bindings are empty, it would always be ${p.t === 'every' ? 'true' : 'false'}; ` +
        `use world forms (exists(…), count(name:…))`,
      )
  }
}

function lintTriggerValExpr(where: string, x: ValExpr): void {
  switch (x.t) {
    case 'count':
      throw new Error(
        `${where}: IF operand count(${x.role}) reads a step role — trigger bindings are empty, ` +
        `it would always be 0; use count(<query>) (e.g. count(name:${x.role}))`,
      )
    case 'total':
      throw new Error(
        `${where}: IF operand total(${x.role}, aspect:${x.path}) reads a step role — ` +
        `trigger bindings are empty, it would always be 0`,
      )
    case 'path':
      throw new Error(
        `${where}: IF operand ${x.role}.${x.field}.${x.key} reads a step role — ` +
        `trigger bindings are empty, it would always be false; use world forms (exists(…), count(name:…))`,
      )
    default:
      return // int / countQuery are world forms — the trigger vocabulary
  }
}

function numCmp(a: number, op: CmpOp, b: number): boolean {
  switch (op) {
    case '>=': return a >= b
    case '<=': return a <= b
    case '>': return a > b
    case '<': return a < b
    case '=': return a === b
    case '!=': return a !== b
  }
}
