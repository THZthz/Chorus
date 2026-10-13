// Lint bundled scripts — the load-time warnings of lintScript (§10's "fail
// loudly", warn-loudly sibling). Exit code stays 0: warnings advise, errors
// are compileScript's job.
//
//   npm run lint:scripts                 # everything under scripts/
//   npm run lint:scripts -- my-game.txt  # or name files
import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { coreDictionary } from './dict.js'
import { lintScript } from './script.js'

const dict = coreDictionary()
const args = process.argv.slice(2)
const files = args.length > 0
  ? args
  : readdirSync('scripts').filter(f => f.endsWith('.txt')).map(f => join('scripts', f))

let findings = 0
for (const file of files) {
  const warnings = lintScript(readFileSync(file, 'utf8'), dict)
  if (warnings.length === 0) {
    console.log(`${file}: clean`)
    continue
  }
  findings += warnings.length
  console.log(`${file}:`)
  for (const w of warnings) console.log(`  - ${w}`)
}
console.log(`\n${files.length} file(s), ${findings} finding(s)`)
