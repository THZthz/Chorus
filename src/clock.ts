// Clock & durations — spec §5.1, §9.5. (day, halfhour) is raw mechanics
// time: the clock ticks in halfhours (0..47 per day, each 30 minutes) while
// the engine tracks absolute minutes, so 20m offsets stay exact.
import type { Clock } from './types.js'

export type { Clock }

/** minutes per clock tick */
export const MIN_PER_HALFHOUR = 30
/** clock ticks per day */
export const HALFHOURS_PER_DAY = 48

export const absMin = (c: Clock): number =>
  (c.day * HALFHOURS_PER_DAY + c.halfhour) * MIN_PER_HALFHOUR

export const clockFromAbsMin = (m: number): Clock => {
  const total = Math.floor(m / MIN_PER_HALFHOUR)
  return { day: Math.floor(total / HALFHOURS_PER_DAY), halfhour: total % HALFHOURS_PER_DAY }
}

/** the earliest startable step clock at or after absolute minute m (§9.28):
 *  step clocks are halfhour-quantized, so a sub-halfhour action leaves the
 *  rest of the tick as dead air and play resumes on the next halfhour */
export const nextStepClock = (m: number): Clock => {
  const rem = m % MIN_PER_HALFHOUR
  return clockFromAbsMin(rem === 0 ? m : m + MIN_PER_HALFHOUR - rem)
}

export const formatClock = (c: Clock): string => `${c.day}:${c.halfhour}`

export function parseClock(s: string): Clock {
  const m = /^(\d+):(\d+)$/.exec(s)
  if (!m) throw new Error(`bad clock '${s}' (expected "day:halfhour")`)
  const halfhour = Number(m[2])
  if (halfhour < 0 || halfhour > 47) throw new Error(`bad halfhour '${s}' (0..47)`)
  return { day: Number(m[1]), halfhour }
}

/** Duration literal grammar: int ( "halfhour" | "h" | "m" | "d" ) — spec §5.1.
 *  1 halfhour = 30 minutes. */
export function parseDuration(s: string): number {
  const m = /^(\d+)(halfhour|h|m|d)$/.exec(s)
  if (!m) throw new Error(`bad duration '${s}' (expected e.g. 1halfhour, 2h, 90m, 1d)`)
  const n = Number(m[1])
  switch (m[2]) {
    case 'halfhour': return n * MIN_PER_HALFHOUR
    case 'h': return n * 60
    case 'm': return n
    case 'd': return n * 24 * 60
  }
  throw new Error('unreachable')
}

/** canonical duration rendering: prefers the shorter unit — d, then h, then
 *  halfhour, then raw minutes */
export function formatDuration(minutes: number): string {
  if (minutes === 0) return '0m'
  if (minutes % (24 * 60) === 0) return `${minutes / (24 * 60)}d`
  if (minutes % 60 === 0) return `${minutes / 60}h`
  if (minutes % MIN_PER_HALFHOUR === 0) return `${minutes / MIN_PER_HALFHOUR}halfhour`
  return `${minutes}m`
}

/** halfhour ticks strictly crossed when advancing from `from` to `to`
 *  (absolute minutes), in order */
export function crossedSlots(from: number, to: number): number[] {
  const out: number[] = []
  const first = Math.floor(from / MIN_PER_HALFHOUR) + 1
  const last = Math.floor(to / MIN_PER_HALFHOUR)
  for (let s = first; s <= last; s++) out.push(s)
  return out
}
