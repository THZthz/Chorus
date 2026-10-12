// Refs — spec §5.1, §9.14. Canonical output is the bare name (unique among the
// living); `kind:NAME` and `#id` are accepted input sugar; reserved words and
// unsafe names must be quoted strings.
export type Ref =
  | { form: 'name'; kind?: string; name: string }
  | { form: 'id'; id: string }
  | { form: 'param'; name: string }  // F3 authored-only: a function parameter standing in a whole-ref
                                     // position (§9.40); calls expand at load, so this never formats,
                                     // resolves, or round-trips

/** minimal dictionary surface the formatter needs (avoids a circular import) */
export interface SafeDict {
  reserved: Set<string>
}

const SAFE_NAME = /^[A-Za-z_][A-Za-z0-9_]*$/

export function nameRef(name: string, kind?: string): Ref {
  return kind ? { form: 'name', kind, name } : { form: 'name', name }
}

export function idRef(id: string): Ref {
  return { form: 'id', id }
}

export function formatRef(r: Ref, dict?: SafeDict): string {
  switch (r.form) {
    case 'id': return `#${r.id}`
    case 'name': {
      const safe = SAFE_NAME.test(r.name) && !(dict && dict.reserved.has(r.name))
      return safe ? r.name : `"${r.name.replace(/"/g, '\\"')}"`
    }
    case 'param': throw new Error('a function parameter is authored form only — calls expand at load (§9.40)')
  }
}

export function refSortKey(r: Ref): string {
  return r.form === 'id' ? `#${r.id}` : r.name
}

export function sortRefs(refs: Ref[]): Ref[] {
  return [...refs].sort((a, b) => refSortKey(a) < refSortKey(b) ? -1 : refSortKey(a) > refSortKey(b) ? 1 : 0)
}

export function sameRef(a: Ref, b: Ref): boolean {
  return formatRef(a) === formatRef(b)
}
