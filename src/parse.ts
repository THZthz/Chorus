// Recursive-descent sentence parser — spec §5.1.
// The one semantic decision point: where the actor list ends and the verb
// begins. That needs the verb registry (both are uppercase tokens), so the
// parser consults the Dictionary (spec §9.14).
import { lex, type Tok } from './lexer.js'
import type { Arg, Clause, Outcome, Role, Sentence } from './sentence.js'
import { ROLES } from './sentence.js'
import type { Ref } from './ref.js'
import type { Dictionary } from './dict.js'
import type { Clock } from './clock.js'

const CLAUSE_KEYWORDS = new Set(['TO', 'WITH', 'AT', 'ABOUT'])

/** 'ACTORS' → 'actors' (role refs in outcome args are uppercase) */
function asRole(v: string): Role | undefined {
  const lower = v.toLowerCase()
  return ROLES.includes(lower as Role) ? (lower as Role) : undefined
}

export function parseSentence(src: string, dict: Dictionary): Sentence {
  return new Parser(lex(src), dict).sentence()
}

/** Parse an outcome list — used to compile recipe `out` strings at content load. */
export function parseOutcomes(src: string, dict: Dictionary): Outcome[] {
  return new Parser(lex(src), dict).outcomeListOrEmpty()
}

class Parser {
  private pos = 0
  constructor(private toks: Tok[], private dict: Dictionary) {}

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

  private expectEnd(): void {
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
   *  Names in the dictionary's reserved set (keywords AND verb lexemes) must be quoted. */
  ref(): Ref {
    const t = this.next()
    if (t.t === 'idref') return { form: 'id', id: t.v }
    if (t.t === 'str') return { form: 'name', name: t.v }
    if (t.t !== 'name') throw new Error(`expected a ref, got '${show(t)}'`)
    if (this.dict.reserved.has(t.v)) throw new Error(`'${t.v}' is reserved and cannot be a name (quote it)`)
    if (this.atPunct(':')) {
      this.next()
      const n = this.next()
      if (n.t !== 'name') throw new Error(`expected name after kind ':', got '${show(n)}'`)
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
   *  Self-delimiting ops (SPAWN/NARRATE/MOVE/DESTROY/REVEAL) also accept a
   *  canonical paren-less form; SET/RELATE/UNRELATE/REFUSE/ADVANCE require parens. */
  outcome(): Outcome {
    const t = this.peek()
    if (t?.t === 'name' && this.dict.isOp(t.v)) {
      this.next()
      const op = t.v
      if (this.atPunct('(')) {
        this.next()
        const args: Arg[] = []
        if (!this.atPunct(')')) {
          args.push(this.arg())
          while (this.atPunct(',')) { this.next(); args.push(this.arg()) }
        }
        this.expectPunct(')')
        // NARRATE's canonical form is a single string (§4.4); extra args would
        // be dropped by the formatter, so refuse them at the door (§9.6)
        if (op === 'NARRATE' && (args.length !== 1 || args[0].t !== 'str')) {
          throw new Error('NARRATE takes exactly one string')
        }
        return { op, args }
      }
      if (op === 'SPAWN') return { op, args: this.spawnArgs() }
      if (op === 'NARRATE') {
        const s = this.next()
        if (s.t !== 'str') throw new Error(`NARRATE expects a string, got '${show(s)}'`)
        return { op, args: [{ t: 'str', v: s.v }] }
      }
      if (op === 'DESTROY' || op === 'REVEAL') return { op, args: [this.roleOrRefArg()] }
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

  private roleOrRefArg(): Arg {
    const t = this.peek()
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
      args.push({ t: 'pair', k: k.v, v: this.valueToken() })
    }
    return args
  }

  arg(): Arg {
    const t = this.peek()
    if (!t) throw new Error('unexpected end of outcome args')
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
      const role = asRole(t.v)
      if (role) {
        // ACTORS/OBJECT/… in outcome args are role refs (e.g. MOVE ACTORS INTO OBJECT)
        this.next()
        return { t: 'role', role }
      }
      if (t.v === 'true' || t.v === 'false') { this.next(); return { t: 'bool', v: t.v === 'true' } }
      if (this.atPunct('=', 1)) {
        this.next(); this.next()
        return { t: 'pair', k: t.v, v: this.valueToken() }
      }
      // dotted names in arg position are string keys: SET(x, state.sealed, true)
      if (this.atPunct('.') && this.peek(1)?.t === 'name') {
        let key = t.v
        while (this.atPunct('.')) { this.next(); const part = this.next(); if (part.t !== 'name') throw new Error(`bad dotted key '${key}.'`); key += '.' + part.v }
        return { t: 'str', v: key }
      }
      return { t: 'ref', ref: this.ref() }
    }
    throw new Error(`unexpected token in outcome args: '${show(t)}'`)
  }

  private refTarget(): Ref | { role: Role } {
    const t = this.peek()
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
