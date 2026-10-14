// Card-reachability CLI — "can the player obtain card X?" over a bundled
// script. Both semantics: default is existence; --usable also requires the
// card to be bindable into some committable verb slot.
//
//   npm run reach -- scripts/the-nightjar-flyer.txt THE_WAX_CYLINDER [--usable]
//   npm run reach -- scripts/the-nightjar-flyer.txt --all
import { readFileSync } from 'node:fs'
import { coreDictionary } from '../src/core/dict.js'
import { analyzeCard } from '../src/reach/reach.js'
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
  console.error('usage: tsx src/reach-cli.ts <script.txt> <CARD_NAME> [--usable] [--demand "<query>"]\n       tsx src/reach-cli.ts <script.txt> --all')
  process.exit(2)
}

const dict = coreDictionary()
const { content, state } = compileScript(readFileSync(file, 'utf8'), dict)

const report = (name: string): void => {
  const r = analyzeCard(content, state, name, { usable, demand })
  console.log(`${file} · card ${demand ? `matching '${demand}'` : name}${usable ? '  [usable semantics]' : ''}`)
  console.log(`  verdict: ${r.verdict.toUpperCase()}  (existence: ${r.existence})`)
  for (const l of r.chain) console.log(`  ${l}`)
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
