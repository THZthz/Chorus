// Condition predicates for recipe `when` — spec §6.3.
//   count(actors) = 2 AND rel(parent_child, actors) AND NOT any(actors, aspect:occult >= 2)
//   total(actors, aspect:lantern) < object.aspect.demand
//   exists(aspect:hunter>=1) AND NOT exists(THE_SEALED_DOOR)   # world-existential
// Path operands use dots (object.aspect.demand); scalar conditions inside
// every/any use the query colon form (aspect:occult >= 2).
import type { Entity, State } from './types.js'
import { aspectGet, relationAlive } from './types.js'
import { matchQuery, parseQuery, type CmpOp, type Query } from './query.js'
import { ROLES } from './sentence.js'

export type RoleBindings = Record<string, Entity[]>

export interface PredCtx {
  b: RoleBindings
  state: State
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

export interface ScalarCond {
  path: { field: 'aspect' | 'attr'; key: string }
  op: CmpOp
  value: number
}

// ---------- parser (precedence climbing: NOT > AND > OR) ----------

const CMP_OPS = ['>=', '<=', '!=', '=', '>', '<']

export function parsePredicate(src: string): Pred {
  return new PredParser(src).parse()
}

class PredParser {
  private toks: string[] = []
  private spans: { start: number; end: number }[] = [] // source offsets per token
  private pos = 0
  private readonly src: string
  constructor(src: string) {
    this.src = src
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
        const cards = ctx.b[base] ?? []
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
      const role = ctx.b[p.role] ?? []
      return role.every(e => scalarOk(e, p.cond))
    }
    case 'any': {
      const role = ctx.b[p.role] ?? []
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
    case 'count': return (ctx.b[x.role] ?? []).length
    case 'countQuery': {
      // world scope, like existsQuery (§6.3): every ALIVE card matching the query
      let n = 0
      for (const e of ctx.state.entities.values()) {
        if (e.deadAction === null && matchQuery(e, x.query)) n++
      }
      return n
    }
    case 'total':
      return (ctx.b[x.role] ?? []).reduce((sum, e) => sum + (aspectGet(e, x.path) ?? 0), 0)
    case 'path': {
      const e = (ctx.b[x.role] ?? [])[0]
      if (!e) return undefined
      return x.field === 'aspect' ? aspectGet(e, x.key) : numberAttr(e, x.key)
    }
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
