// Terminal UI (Ink) — the component front end for a Game.
// Turns append permanently to the terminal scrollback via <Static>; the live
// area at the bottom keeps the title, a status line and the input prompt.
// handleLine stays the whole turn loop; this file only renders and feeds it.
import { useRef, useState } from 'react'
import { Box, Static, Text, render, useInput, useWindowSize } from 'ink'
import type { Instance } from 'ink'
import { createInterface } from 'node:readline/promises'
import { stdin, stdout } from 'node:process'
import type { Game } from './play.js'
import { clockFromAbsMin, formatClock } from './clock.js'

type EntryKind = 'action' | 'info' | 'error'

interface Entry {
  id: number
  kind: EntryKind
  lines: string[]
}

// shown in the status slot until the first turn, then replaced by statusOf
const HINT = 'describe what happens — try “wait” or “read <thing>” · /help has every verb'

function EntryView({ entry }: { entry: Entry }) {
  const { columns } = useWindowSize()
  if (entry.kind === 'error') {
    return (
      <Box flexDirection="column" paddingBottom={1}>
        {entry.lines.map((l, i) => (
          <Text key={i} color="red">✗ {l}</Text>
        ))}
      </Box>
    )
  }
  // action and info turns: plain prose; the engine's canonical « … » echo dims,
  // and collapses to a one-line summary when it would wrap
  const width = columns ?? 80
  return (
    <Box flexDirection="column" paddingBottom={1}>
      {entry.lines.map((l, i) =>
        l.startsWith('«')
          ? <Text key={i} dimColor>{quoteLine(l, width)}</Text>
          : <Text key={i}>{l}</Text>,
      )}
    </Box>
  )
}

/** the canonical « … » echo: shown whole when it fits on one line, else as a
 *  summary — the full trace stays available through /story */
function quoteLine(line: string, width: number): string {
  if (line.length <= width - 2) return line
  const content = line.replace(/^«\s*/, '').replace(/\s*»$/, '')
  const clock = content.match(/\d+:\d+/)?.[0] ?? '…'
  const records = content.split(' | ').length
  return `« ${clock} · ${records} record${records === 1 ? '' : 's'} · see /story »`
}

function InputLine({ value, cursor }: { value: string; cursor: number }) {
  return (
    <Text>
      <Text color="cyan">» </Text>
      {value.slice(0, cursor)}
      <Text inverse>{value[cursor] ?? ' '}</Text>
      {value.slice(cursor + 1)}
    </Text>
  )
}

function Prompt({ onSubmit, requestExit }: {
  onSubmit: (line: string) => void
  requestExit: () => void
}) {
  const [value, setValue] = useState('')
  const [cursor, setCursor] = useState(0)
  const [histIdx, setHistIdx] = useState<number | null>(null)
  const historyRef = useRef<string[]>([])
  const draftRef = useRef('')

  const runLine = (line: string) => {
    const t = line.trim()
    if (!t) return
    historyRef.current = [...historyRef.current, t]
    onSubmit(t)
  }

  // ink does not split \r out of pasted/coalesced chunks (it may occur inside
  // paste text), so newline-bearing input is submitted line by line; whatever
  // was already typed joins the first line and the tail stays in the buffer
  const runChunk = (input: string) => {
    const segments = input.split(/\r\n|\r|\n/)
    const trailing = segments.pop() ?? ''
    runLine(value.slice(0, cursor) + (segments[0] ?? ''))
    for (const seg of segments.slice(1)) runLine(seg)
    setValue(trailing + value.slice(cursor))
    setCursor(trailing.length)
  }

  // an edit invalidates the history entry being browsed
  const edited = () => { if (histIdx !== null) setHistIdx(null) }

  const browse = (dir: -1 | 1) => {
    const history = historyRef.current
    if (history.length === 0) return
    if (histIdx === null) {
      if (dir === 1) return
      draftRef.current = value
      const idx = history.length - 1
      setHistIdx(idx)
      setValue(history[idx]!)
      setCursor(history[idx]!.length)
      return
    }
    const idx = histIdx + dir
    if (idx >= history.length) {
      setHistIdx(null)
      setValue(draftRef.current)
      setCursor(draftRef.current.length)
      return
    }
    const clamped = Math.max(0, idx)
    setHistIdx(clamped)
    setValue(history[clamped]!)
    setCursor(history[clamped]!.length)
  }

  useInput((input, key) => {
    if (key.return) {
      setHistIdx(null)
      runLine(value)
      setValue('')
      setCursor(0)
      return
    }
    if (/[\r\n]/.test(input)) {
      setHistIdx(null)
      runChunk(input)
      return
    }
    if (key.ctrl) {
      if (input === 'c') return requestExit()
      if (input === 'a') return setCursor(0)
      if (input === 'e') return setCursor(value.length)
      if (input === 'u') { edited(); setValue(''); setCursor(0); return }
      if (input === 'w') {
        edited()
        const kept = value.slice(0, cursor).replace(/\S*\s*$/, '')
        setValue(kept + value.slice(cursor))
        setCursor(kept.length)
      }
      return
    }
    if (key.upArrow) return browse(-1)
    if (key.downArrow) return browse(1)
    if (key.leftArrow) return setCursor(c => Math.max(0, c - 1))
    if (key.rightArrow) return setCursor(c => Math.min(value.length, c + 1))
    if (key.home) return setCursor(0)
    if (key.end) return setCursor(value.length)
    if (key.backspace) {
      if (cursor === 0) return
      edited()
      setValue(value.slice(0, cursor - 1) + value.slice(cursor))
      setCursor(cursor - 1)
      return
    }
    if (key.delete) {
      if (cursor >= value.length) return
      edited()
      setValue(value.slice(0, cursor) + value.slice(cursor + 1))
      return
    }
    if (key.meta || key.tab || key.escape) return
    edited()
    // cursor is a UTF-16 index; astral characters would split, fine for game commands
    setValue(value.slice(0, cursor) + input + value.slice(cursor))
    setCursor(cursor + input.length)
  })

  return <InputLine value={value} cursor={cursor} />
}

/** display form of an authored name: THE_FORGETTING → THE FORGETTING */
function pretty(name: string): string {
  return name.replace(/_/g, ' ')
}

/** one-line summary of where the player stands, from public state alone */
function statusOf(game: Game): string {
  const bits: string[] = []
  const alive = [...game.state.entities.values()].filter(e => e.deadAction === null)
  const me = game.meta.player ? alive.find(e => e.name === game.meta.player) : undefined
  if (game.meta.player) bits.push(`you are ${pretty(game.meta.player)}`)
  const here = me?.attrs['container']
  const place = typeof here === 'string' ? game.state.entities.get(here) : undefined
  if (place) bits.push(`at ${pretty(place.name)}`)
  bits.push(`day ${formatClock(clockFromAbsMin(game.state.now))}`)
  bits.push(`${alive.length} cards`)
  return bits.join(' · ')
}

function App({ game, requestExit }: { game: Game; requestExit: () => void }) {
  const { columns } = useWindowSize()
  const [entries, setEntries] = useState<Entry[]>([])
  const [turns, setTurns] = useState(0)
  const [exiting, setExiting] = useState(false)
  const nextId = useRef(0)

  // dropping the live area before unmount lets the session end on the
  // transcript instead of a dangling empty prompt
  const doExit = () => {
    setExiting(true)
    requestExit()
  }

  const submit = (line: string) => {
    const turn = game.handleLine(line)
    if (turn.lines.length > 0) {
      const id = nextId.current++
      setEntries(prev => [...prev, { id, kind: turn.kind === 'quit' ? 'info' : turn.kind, lines: turn.lines }])
    }
    setTurns(t => t + 1)
    if (turn.kind === 'quit') doExit()
  }

  return (
    <>
      <Static items={entries}>
        {entry => <EntryView key={entry.id} entry={entry} />}
      </Static>
      {exiting ? null : (
        <Box flexDirection="column">
          <Text>
            <Text bold color="cyan">◈ {game.title}</Text>
            <Text dimColor> · /help · ctrl+c quits</Text>
          </Text>
          <Text dimColor>{turns > 0 ? statusOf(game) : HINT}</Text>
          <Text dimColor>{'─'.repeat(columns ?? 80)}</Text>
          <Prompt onSubmit={submit} requestExit={doExit} />
        </Box>
      )}
    </>
  )
}

/** run a Game in the Ink TUI; falls back to plain readline without a real TTY */
export async function runTui(game: Game): Promise<void> {
  if (!stdin.isTTY || !stdout.isTTY) return runPlain(game)

  return new Promise((resolve, reject) => {
    let instance!: Instance
    const requestExit = () => {
      setTimeout(() => {
        void instance.waitUntilRenderFlush().then(() => instance.unmount())
      }, 0)
    }
    instance = render(<App game={game} requestExit={requestExit} />, { exitOnCtrlC: false })
    instance.waitUntilExit().then(() => resolve(), reject)
  })
}

/** non-TTY fallback: same loop the old readline REPL ran */
async function runPlain(game: Game): Promise<void> {
  console.log(`── ${game.title} ──  (type /help)`)
  const rl = createInterface({ input: stdin, output: stdout, prompt: '» ' })
  try {
    rl.prompt()
    for await (const line of rl) {
      const turn = game.handleLine(line)
      for (const l of turn.lines) console.log(l)
      if (turn.kind === 'quit') break
      rl.prompt()
    }
  } finally {
    rl.close()
  }
}
