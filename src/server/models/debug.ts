/**
 * Elysian Dialogue — cinematic RPG-style dialogue engine
 * Copyright (C) 2026  Amias
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * This program is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU Affero General Public License for more details.
 *
 * You should have received a copy of the GNU Affero General Public License
 * along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

import db from "@/server/db";
import { v4 as uuidv4 } from "uuid";

export interface LlmLog {
  id: string;
  timestamp: string;
  request: string;
  response: string | null;
  duration: number | null;
  status: string;
  parent_id: string | null;
  label: string | null;
  steps?: LlmStep[];
}

export interface LlmStep {
  id: string;
  log_id: string;
  step_number: number;
  finish_reason: string | null;
  usage: string | null;
  tool_calls: string | null;
  tool_results: string | null;
  text: string | null;
  duration_ms: number | null;
  timestamp: string;
  user_prompt: string | null;
  reasoning: string | null;
}

export function addLlmLog(request: any, parentId?: string, label?: string): string {
  const id = uuidv4();
  db.prepare(
    `
    INSERT INTO llm_logs (id, request, status, parent_id, label)
    VALUES (?, ?, ?, ?, ?)
  `,
  ).run(id, JSON.stringify(request), "PENDING", parentId ?? null, label ?? null);
  return id;
}

export function updateLlmLog(
  id: string,
  response: any,
  duration: number,
  status: string = "SUCCESS",
) {
  db.prepare(
    `
    UPDATE llm_logs
    SET response = ?, duration = ?, status = ?
    WHERE id = ?
  `,
  ).run(JSON.stringify(response), duration, status, id);
}

export function addLlmStep(step: Omit<LlmStep, "id" | "timestamp">): string {
  const id = uuidv4();
  db.prepare(
    `
    INSERT INTO llm_steps (id, log_id, step_number, finish_reason, usage, tool_calls, tool_results, text, duration_ms, user_prompt, reasoning)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
  `,
  ).run(
    id,
    step.log_id,
    step.step_number,
    step.finish_reason,
    step.usage,
    step.tool_calls,
    step.tool_results,
    step.text,
    step.duration_ms,
    step.user_prompt ?? null,
    step.reasoning ?? null,
  );
  return id;
}

export function getLlmLogs(limit: number = 50): LlmLog[] {
  const logs = db
    .prepare(
      `
    SELECT * FROM llm_logs
    ORDER BY timestamp DESC
    LIMIT ?
  `,
    )
    .all(limit) as LlmLog[];

  for (const log of logs) {
    log.steps = db
      .prepare(
        `
      SELECT * FROM llm_steps WHERE log_id = ? ORDER BY step_number ASC
    `,
      )
      .all(log.id) as LlmStep[];
  }
  return logs;
}

export function clearLlmLogs() {
  db.prepare("DELETE FROM llm_steps").run();
  db.prepare("DELETE FROM llm_logs").run();
}

