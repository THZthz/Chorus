// Clock & durations — spec §5.1, §9.5. (day,hour) is raw mechanics time;
// the engine tracks absolute minutes so 90m durations stay exact.
import type { Clock } from './types.js'

export type { Clock }

export const absMin = (c: Clock): number => (c.day * 24 + c.hour) * 60

export const clockFromAbsMin = (m: number): Clock => {
  const total = Math.floor(m / 60)
  return { day: Math.floor(total / 24), hour: total % 24 }
}

export const formatClock = (c: Clock): string => `${c.day}:${c.hour}`

export function parseClock(s: string): Clock {
  const m = /^(\d+):(\d+)$/.exec(s)
  if (!m) throw new Error(`bad clock '${s}' (expected "day:hour")`)
  const hour = Number(m[2])
  if (hour < 0 || hour > 23) throw new Error(`bad hour '${s}' (0..23)`)
  return { day: Number(m[1]), hour }
}

/** Duration literal grammar: int ("h" | "m" | "d") — spec §5.1 */
export function parseDuration(s: string): number {
  const m = /^(\d+)(h|m|d)$/.exec(s)
  if (!m) throw new Error(`bad duration '${s}' (expected e.g. 2h, 90m, 1d)`)
  const n = Number(m[1])
  switch (m[2]) {
    case 'h': return n * 60
    case 'm': return n
    case 'd': return n * 24 * 60
  }
  throw new Error('unreachable')
}

export function formatDuration(minutes: number): string {
  if (minutes % (24 * 60) === 0) return `${minutes / (24 * 60)}d`
  if (minutes % 60 === 0) return `${minutes / 60}h`
  return `${minutes}m`
}

/** hours strictly crossed when advancing from `from` to `to` (absolute minutes), in order */
export function crossedHours(from: number, to: number): number[] {
  const out: number[] = []
  for (let h = Math.floor(from / 60) + 1; h <= Math.floor(to / 60); h++) out.push(h)
  return out
}
