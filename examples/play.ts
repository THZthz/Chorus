// Play a game script in the terminal:  npm run play -- scripts/the-forgotten-house.txt
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { Game } from '../src/play.js'
import { runTui } from '../src/tui.js'

const path = process.argv[2] ?? fileURLToPath(new URL('../scripts/the-forgotten-house.txt', import.meta.url))
const text = readFileSync(path, 'utf8')
const game = Game.fromScript(text)
await runTui(game)
