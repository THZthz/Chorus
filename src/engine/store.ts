// Append-only log + JSONL persistence — spec §7. The log is the source of
// truth; entity/relation tables are projections built by fold.
import { readFileSync, writeFileSync } from 'node:fs'
import type { ActionRecord } from './engine.js'
import type { State } from '../core/types.js'

export class Log {
  records: ActionRecord[] = []
  constructor(public initialState: State) {}

  get nextRecordId(): number {
    return this.records.length + 1
  }

  appendAll(records: ActionRecord[]): void {
    this.records.push(...records)
  }

  journal(): string[] {
    return this.records.map(r => r.sentence)
  }
}

export function saveLog(path: string, records: ActionRecord[]): void {
  const body = records.map(r => JSON.stringify(r)).join('\n') + '\n'
  writeFileSync(path, body, 'utf8')
}

export function loadLog(path: string): ActionRecord[] {
  const lines = readFileSync(path, 'utf8').split('\n').filter(Boolean)
  return lines.map(line => JSON.parse(line) as ActionRecord)
}
