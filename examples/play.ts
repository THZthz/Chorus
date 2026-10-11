// Play a game script in the terminal:  npm run play -- scripts/the-forgotten-house.txt
// With no argument, lists the bundled scripts to choose from.
import { readFileSync, readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { join } from 'node:path'
import { Game } from '../src/play.js'
import { pickScript, runTui, type ScriptChoice } from '../src/tui.js'

const scriptsDir = fileURLToPath(new URL('../scripts', import.meta.url))

function listScripts(dirs: string[]): ScriptChoice[] {
  const seen = new Set<string>()
  const out: ScriptChoice[] = []
  for (const dir of dirs) {
    let entries: string[] = []
    try { entries = readdirSync(dir) } catch { continue }
    for (const name of entries.filter(f => f.endsWith('.txt')).sort()) {
      const path = join(dir, name)
      if (seen.has(path)) continue
      seen.add(path)
      const title = /^title:\s*(.+)$/m.exec(readFileSync(path, 'utf8'))?.[1]?.trim()
      out.push({ path, name, title })
    }
  }
  return out
}

const arg = process.argv[2]
const choices = arg ? [] : listScripts([scriptsDir])
if (!arg && choices.length === 0) {
  console.error(`no scripts found in ${scriptsDir} — pass one: npm run play -- <script.txt>`)
  process.exit(1)
}

const path = arg ?? await pickScript(choices)
if (!path) process.exit(0)

let text: string
try {
  text = readFileSync(path, 'utf8')
} catch {
  console.error(`cannot read script: ${path}`)
  process.exit(1)
}
const game = Game.fromScript(text)
await runTui(game)
