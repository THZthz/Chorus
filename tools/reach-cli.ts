// Card-reachability CLI — "can the player obtain card X?" over a bundled
// script. Both semantics: default is existence; --usable also requires the
// card to be bindable into some committable verb slot.
//
// Every PROVEN acquisition path is printed (best-effort: guarded 'candidate'
// sources are annotated, never drawn as paths), each recursively expanded
// through its slot fillers' own provenance.
//
//   npm run reach -- scripts/the-nightjar-flyer.txt THE_WAX_CYLINDER [--usable]
//   npm run reach -- scripts/the-nightjar-flyer.txt --all
//   npm run reach -- scripts/the-nightjar-flyer.txt --demand "aspect:secret >= 3"
import { readFileSync } from 'node:fs'
import { coreDictionary } from '../src/core/dict.js'
import { analyzeCard, type ReachPath } from '../src/reach/reach.js'
import { compileScript } from '../src/content/script.js'

const args = process.argv.slice(2)
const usable = args.includes('--usable')
const demandIdx = args.indexOf('--demand')
const demand = demandIdx >= 0 ? args[demandIdx + 1] : undefined
const rest: string[] = []
for (let i = 0; i < args.length; i++) {
  if (args[i] === '--usable') continue
  if (args[i] === '--demand') { i++; continue }
  rest.push(args[i]!)
}
const file = rest[0]
const card = rest[1]
const all = rest.includes('--all')
if (!file || (!card && !all && !demand)) {
  console.error('usage: tsx tools/reach-cli.ts <script.txt> <CARD_NAME> [--usable] [--demand "<query>"]\n       tsx tools/reach-cli.ts <script.txt> --all')
  process.exit(2)
}

const dict = coreDictionary()
const { content, state } = compileScript(readFileSync(file, 'utf8'), dict)

const renderPath = (p: ReachPath, depth: number): void => {
  const pad = '    '.repeat(depth + 1)
  const deck = p.deck
    ? ` — deck ${p.deck.id}${p.deck.position !== undefined ? ` order[${p.deck.position}]` : ' defaultcard'}${p.deck.capacity !== undefined ? ` (draw capacity ${p.deck.capacity})` : ''}`
    : ''
  const derived = p.derivedName ? ' (derived-mint name — `${PROTO}_${OBJECT}`)' : ''
  console.log(`${pad}· ${p.op === 'initial' ? 'initial' : p.op} '${p.mints}'${derived}${deck} — via ${p.via}`)
  for (const g of p.guards) console.log(`${pad}    guard: ${g}`)
  for (const s of p.supports) console.log(`${pad}    · ${s}`)
  for (const r of p.requires) {
    console.log(`${pad}    requires ${r.name} — ${r.how}`)
    if (r.paths.length === 0) console.log(`${pad}      (no proven source on this branch — cycle guard / depth cap)`)
    for (const sub of r.paths) renderPath(sub, depth + 2)
  }
}

const report = (name: string): void => {
  const r = analyzeCard(content, state, name, { usable, demand })
  console.log(`${file} · card ${demand ? `matching '${demand}'` : name}${usable ? '  [usable semantics]' : ''}`)
  console.log(`  verdict: ${r.verdict.toUpperCase()}  (existence: ${r.existence})`)
  if (r.paths.length === 0 && r.existence === 'proven') console.log('  (no path exported — unexpected)')
  r.paths.forEach((p, i) => {
    console.log(`  path ${i + 1}/${r.paths.length}:`)
    renderPath(p, 0)
  })
  for (const c of r.candidates) console.log(`  candidate source: ${c.via} — guarded (${c.reason})`)
  for (const u of r.usableVia) console.log(`  bindable via ${u}`)
  for (const n of r.notes) console.log(`  note: ${n}`)
  console.log()
}

if (all) {
  const names = new Set<string>(content.prototypes.keys())
  for (const e of state.entities.values()) if (e.deadAction === null) names.add(e.name)
  for (const d of content.decks.values()) for (const c of d.order) names.add(c.name)
  for (const n of [...names].sort()) report(n)
} else {
  report(card!)
}
