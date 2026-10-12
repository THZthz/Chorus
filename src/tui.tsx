// Terminal UI (Ink) — the component front end for a Game.
// Turns append permanently to the terminal scrollback via <Static>; the live
// area at the bottom shows everything the player can do: the verb vocabulary,
// what is here, and the meta actions as number buttons. Free-form input
// completes verbs and card names with tab.
import { useRef, useState, useEffect } from 'react'
import { Box, Static, Text, render, useInput, useWindowSize } from 'ink'
import type { Instance } from 'ink'
import { createInterface } from 'node:readline/promises'
import { stdin, stdout } from 'node:process'
import type { Game } from './play.js'
import { clockFromAbsMin, formatClock } from './clock.js'
import { pretty, endingBanner } from './render.js'

type EntryKind = 'action' | 'info' | 'error'

interface Entry {
  id: number
  kind: EntryKind
  lines: string[]
}

const BUTTONS = ['look', 'cards', 'story', 'journal', 'undo', 'quit'] as const

/** the script's verb vocabulary, display form */
function verbList(game: Game): string[] {
  return [...game.content.verbs.keys()].map(v => v.toLowerCase().replace(/_/g, ' '))
}

function truncate(s: string, width: number): string {
  return s.length > width ? s.slice(0, width - 1) + '…' : s
}

/** one-line summary of where the player stands, from the visible projection */
function statusOf(game: Game): string {
  const visible = game.visible()
  const bits: string[] = []
  const me = game.meta.player ? visible.find(e => e.name === game.meta.player) : undefined
  if (game.meta.player) bits.push(`you are ${pretty(game.meta.player)}`)
  const here = me?.attrs['container']
  const place = typeof here === 'string' ? game.state.entities.get(here) : undefined
  if (place) bits.push(`at ${pretty(place.name)}`)
  bits.push(`day ${formatClock(clockFromAbsMin(game.state.now))}`)
  bits.push(`${visible.length} cards`)
  return bits.join(' · ')
}

/** the cards sharing the player's location — the immediate options */
function hereOf(game: Game): string {
  const visible = game.visible()
  const me = game.meta.player ? visible.find(e => e.name === game.meta.player) : undefined
  const container = me?.attrs['container']
  const things = visible.filter(e => e.attrs['container'] === container && e.name !== game.meta.player)
  if (things.length === 0) return ''
  return `here: ${things.map(e => pretty(e.name)).join(' · ')}`
}

/** the canonical « … » echo: shown whole when it fits on one line, else as a
 *  summary — the full trace stays available through the story view */
function quoteLine(line: string, width: number): string {
  if (line.length <= width - 2) return line
  const content = line.replace(/^«\s*/, '').replace(/\s*»$/, '')
  const clock = content.match(/\d+:\d+/)?.[0] ?? '…'
  const records = content.split(' | ').length
  return `« ${clock} · ${records} record${records === 1 ? '' : 's'} · see story »`
}

function EntryView({ entry }: { entry: Entry }) {
  const { columns } = useWindowSize()
  if (entry.kind === 'error') {
    return (
      <Box flexDirection="column" paddingBottom={1}>
        {entry.lines.map((l, i) =>
          l.startsWith('» ')
            ? <Text key={i} color="cyan">{l}</Text>
            : <Text key={i} color="red">✗ {l}</Text>,
        )}
      </Box>
    )
  }
  // action and info turns: the echoed command cyan, the engine's canonical
  // « … » echo dim (collapsed when it would wrap), everything else plain
  const width = columns ?? 80
  return (
    <Box flexDirection="column" paddingBottom={1}>
      {entry.lines.map((l, i) =>
        l.startsWith('«')
          ? <Text key={i} dimColor>{quoteLine(l, width)}</Text>
          : <Text key={i} color={l.startsWith('» ') ? 'cyan' : undefined}>{l}</Text>,
      )}
    </Box>
  )
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

function Prompt({ onSubmit, requestExit, candidates, onHint }: {
  onSubmit: (line: string) => void
  requestExit: () => void
  candidates: string[]
  onHint: (lines: string[]) => void
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

  // tab: complete the word before the cursor against verbs and card names;
  // ambiguous matches extend to the common prefix and list the options
  const completeLeft = (left: string): { text: string; hint?: string } => {
    const m = /[A-Za-z0-9_']+$/.exec(left)
    if (!m) return { text: left }
    const word = m[0]
    const pool = candidates.filter(c => c.toLowerCase().startsWith(word.toLowerCase()) && c.toLowerCase() !== word.toLowerCase())
    if (pool.length === 0) return { text: left }
    let prefix = pool[0]!
    for (const c of pool.slice(1)) {
      while (prefix && !c.toLowerCase().startsWith(prefix.toLowerCase())) prefix = prefix.slice(0, -1)
    }
    if (prefix.length <= word.length) return { text: left, hint: pool.join(' · ') }
    return { text: left.slice(0, m.index) + prefix, hint: pool.length > 1 ? pool.join(' · ') : undefined }
  }

  // ink does not split \t out of coalesced chunks either, so tabs in the
  // chunk are completed in sequence, not inserted literally
  const runTabs = (input: string, solo: boolean) => {
    const parts = input.split('\t')
    if (solo && parts.length === 1) parts.push('')
    let left = value.slice(0, cursor) + (parts[0] ?? '')
    let hint: string | undefined
    for (const part of parts.slice(1)) {
      const r = completeLeft(left)
      left = r.text
      if (r.hint) hint = r.hint
      left += part
    }
    edited()
    setValue(left + value.slice(cursor))
    setCursor(left.length)
    if (hint) onHint([hint])
  }

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
    if (key.tab || input.includes('\t')) return runTabs(input, key.tab)
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
    if (key.meta || key.escape) return
    edited()
    // cursor is a UTF-16 index; astral characters would split, fine for game commands
    setValue(value.slice(0, cursor) + input + value.slice(cursor))
    setCursor(cursor + input.length)
  })

  return <InputLine value={value} cursor={cursor} />
}

export interface ScriptChoice {
  path: string
  name: string
  title?: string
}

function ScriptPicker({ choices, leave }: {
  choices: ScriptChoice[]
  leave: (value: string | null) => void
}) {
  const { columns } = useWindowSize()
  const [idx, setIdx] = useState(0)

  useInput((input, key) => {
    // ink does not split \r out of pasted/coalesced chunks — a newline-bearing
    // chunk means "confirm the current choice"
    if (key.return || /[\r\n]/.test(input)) return leave(choices[idx]!.path)
    if (key.upArrow) return setIdx(i => Math.max(0, i - 1))
    if (key.downArrow) return setIdx(i => Math.min(choices.length - 1, i + 1))
    if (key.escape || (key.ctrl && input === 'c') || input === 'q') return leave(null)
    const n = Number(input)
    if (Number.isInteger(n) && n >= 1 && n <= choices.length) return leave(choices[n - 1]!.path)
  })

  return (
    <Box flexDirection="column">
      <Text bold color="cyan">◈ chorus</Text>
      <Text dimColor>choose a script · ↑↓ move · enter plays · 1–{choices.length} jump · q quits</Text>
      <Text dimColor>{'─'.repeat(Math.max(10, (columns ?? 80) - 2))}</Text>
      {choices.map((c, i) => (
        <Text key={c.path} color={i === idx ? 'cyan' : undefined}>
          {i === idx ? '› ' : '  '}{c.name}{c.title ? ` — ${c.title}` : ''}
        </Text>
      ))}
    </Box>
  )
}

/** interactive script picker; resolves the chosen path, or null on quit.
 *  Without a TTY (or with nothing to choose) it resolves the first choice. */
export async function pickScript(choices: ScriptChoice[]): Promise<string | null> {
  if (choices.length === 0) return null
  if (!stdin.isTTY || !stdout.isTTY) {
    console.log(`(non-interactive — defaulting to ${choices[0]!.name})`)
    return choices[0]!.path
  }
  return new Promise((resolve, reject) => {
    let instance!: Instance
    let done = false
    let picked: string | null = null
    // wipe the menu off the screen once a choice is made — it should not linger
    const leave = (value: string | null) => {
      if (done) return
      done = true
      picked = value
      instance.clear()
      instance.unmount()
    }
    instance = render(<ScriptPicker choices={choices} leave={leave} />, { exitOnCtrlC: false })
    instance.waitUntilExit().then(() => resolve(picked), reject)
  })
}

function App({ game, requestExit }: { game: Game; requestExit: () => void }) {
  const { columns } = useWindowSize()
  const [entries, setEntries] = useState<Entry[]>([])
  const [exiting, setExiting] = useState(false)
  const nextId = useRef(0)
  const width = Math.max(10, (columns ?? 80) - 2)

  const addEntry = (kind: EntryKind, lines: string[]) => {
    const id = nextId.current++
    setEntries(prev => [...prev, { id, kind, lines }])
  }

  // the welcome mat: the goal the script declares, and how to act
  useEffect(() => {
    const lines: string[] = []
    if (game.meta.goal) lines.push(`§ ${game.meta.goal}`)
    lines.push('type what happens — a verb plus what you see here; tab completes · inspect <card> reads its face · number + enter opens the views below')
    addEntry('info', lines)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  // dropping the live area before unmount lets the session end on the
  // transcript instead of a dangling empty prompt
  const doExit = () => {
    setExiting(true)
    requestExit()
  }

  const runButton = (n: number) => {
    const label = BUTTONS[n - 1]!
    // buttons bypass the prompt, so the transcript records which was pressed
    const echo = `» ${n} ${label}`
    if (label === 'quit') {
      addEntry('info', [echo, 'Goodbye.'])
      doExit()
      return
    }
    const turn = label === 'undo' ? game.undo()
      : { kind: 'info' as const, lines:
          label === 'look' ? game.look()
        : label === 'cards' ? game.cards()
        : label === 'story' ? game.story()
        : game.journal() }
    addEntry(turn.kind, [echo, ...turn.lines])
  }

  const submit = (line: string) => {
    const n = Number(line)
    if (Number.isInteger(n) && n >= 1 && n <= BUTTONS.length) {
      runButton(n)
      return
    }
    const inspect = /^inspect\b\s*(.*)$/.exec(line)
    if (inspect) {
      addEntry('info', [`» ${line}`, ...game.inspect(inspect[1] ?? '')])
      return
    }
    const turn = game.handleLine(line)
    addEntry(turn.kind, [`» ${line}`, ...turn.lines])
  }

  const verbs = verbList(game)
  const here = hereOf(game)
  const suggestions = game.suggestions()
  const candidates = [...verbs, 'inspect', ...game.visible().map(e => pretty(e.name))]

  return (
    <>
      <Static items={entries}>
        {entry => <EntryView key={entry.id} entry={entry} />}
      </Static>
      {exiting ? null : (
        <Box flexDirection="column">
          <Text bold color="cyan">◈ {game.title}</Text>
          {game.state.ended ? (
            // §9.35: the story is over — the banner takes the status area;
            // the views and buttons below keep working (undo reopens, quit quits)
            <Text bold color="magenta">{endingBanner(game.state.ended)}</Text>
          ) : null}
          <Text dimColor>{statusOf(game)}</Text>
          {here ? <Text dimColor>{truncate(here, width)}</Text> : null}
          {suggestions.length > 0 ? (
            <Text>
              <Text color="cyan">try: </Text>
              <Text>{truncate(suggestions.join(' · '), width - 5)}</Text>
            </Text>
          ) : null}
          <Text dimColor>{truncate(`verbs: ${verbs.join(' · ')}`, width)}</Text>
          <Text>
            {BUTTONS.map((b, i) => (
              <Text key={b}>
                <Text dimColor>[</Text><Text color="cyan">{i + 1}</Text><Text dimColor> {b}] </Text>
              </Text>
            ))}
            <Text dimColor>← number + enter</Text>
          </Text>
          {/* never write to the terminal's last column: a full-width line
              desyncs Ink's row accounting from the screen (deferred wrap) */}
          <Text dimColor>{'─'.repeat(width)}</Text>
          <Prompt
            onSubmit={submit}
            requestExit={doExit}
            candidates={candidates}
            onHint={lines => addEntry('info', lines)}
          />
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

/** non-TTY fallback: a plain line loop; EOF ends the session */
async function runPlain(game: Game): Promise<void> {
  console.log(`── ${game.title} ──`)
  console.log(`verbs: ${verbList(game).join(' · ')}`)
  const rl = createInterface({ input: stdin, output: stdout, prompt: '» ' })
  try {
    rl.prompt()
    for await (const line of rl) {
      const inspect = /^inspect\b\s*(.*)$/.exec(line.trim())
      if (inspect) {
        for (const l of game.inspect(inspect[1] ?? '')) console.log(l)
      } else {
        const turn = game.handleLine(line)
        for (const l of turn.lines) console.log(l)
      }
      rl.prompt()
    }
  } finally {
    rl.close()
  }
}
