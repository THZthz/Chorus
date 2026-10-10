// Tokenizer for the sentence grammar — spec §5.1.
// Quote-aware and paren-aware: strings and outcome args carry commas/brackets.
export type Tok =
  | { t: 'name'; v: string }
  | { t: 'int'; v: number }
  | { t: 'dur'; v: number }      // minutes
  | { t: 'str'; v: string }
  | { t: 'idref'; v: string }    // #c7f3 (without '#')
  | { t: 'punct'; v: string }

const NAME_RE = /^[A-Za-z_][A-Za-z0-9_]*/
const NUM_RE = /^[0-9]+/
const DUR_RE = /^[0-9]+(h|m|d)/
const PUNCT = new Set([',', ';', ':', '(', ')', '@', '=', '!', '>', '<'])

export function lex(src: string): Tok[] {
  const toks: Tok[] = []
  let i = 0
  while (i < src.length) {
    const c = src[i]
    if (/\s/.test(c)) { i++; continue }
    if (src.startsWith('->', i)) { toks.push({ t: 'punct', v: '->' }); i += 2; continue }
    if (c === '"') {
      let j = i + 1, out = ''
      while (j < src.length && src[j] !== '"') {
        if (src[j] === '\\') { out += src[j + 1]; j += 2 } else { out += src[j]; j++ }
      }
      if (j >= src.length) throw new Error('unterminated string literal')
      toks.push({ t: 'str', v: out })
      i = j + 1
      continue
    }
    if (c === '#') {
      const m = NAME_RE.exec(src.slice(i + 1))
      if (!m) throw new Error(`bad id ref at offset ${i} (expected #id)`)
      toks.push({ t: 'idref', v: m[0] })
      i += 1 + m[0].length
      continue
    }
    if (/[0-9]/.test(c)) {
      const rest = src.slice(i)
      const dur = DUR_RE.exec(rest)
      if (dur && NUM_RE.exec(rest)![0] === dur[0].slice(0, dur[0].length - 1)) {
        toks.push({ t: 'dur', v: durToMinutes(dur[0]) })
        i += dur[0].length
        continue
      }
      const num = NUM_RE.exec(rest)!
      toks.push({ t: 'int', v: Number(num[0]) })
      i += num[0].length
      continue
    }
    const nm = NAME_RE.exec(src.slice(i))
    if (nm) { toks.push({ t: 'name', v: nm[0] }); i += nm[0].length; continue }
    const two = src.slice(i, i + 2)
    if (two === '>=' || two === '<=') { toks.push({ t: 'punct', v: two }); i += 2; continue }
    if (PUNCT.has(c)) { toks.push({ t: 'punct', v: c }); i++; continue }
    throw new Error(`unexpected character '${c}' at offset ${i}`)
  }
  return toks
}

function durToMinutes(s: string): number {
  const n = Number(s.slice(0, -1))
  const unit = s[s.length - 1]
  return unit === 'h' ? n * 60 : unit === 'd' ? n * 24 * 60 : n
}
