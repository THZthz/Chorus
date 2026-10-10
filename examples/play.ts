// Play a game script in the terminal:  npm run play -- scripts/the-forgotten-house.txt
import { readFileSync } from 'node:fs'
import { Game, runRepl } from '../src/play.js'

const path = process.argv[2] ?? new URL('../scripts/the-forgotten-house.txt', import.meta.url).pathname
const text = readFileSync(path, 'utf8')
const game = Game.fromScript(text)
await runRepl(game)
