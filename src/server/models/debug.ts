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
}

export interface ConsoleLog {
  id: string;
  level: string;
  message: string;
  args: string;
  timestamp: string;
}

export function addLlmLog(request: any, parentId?: string, label?: string): string {
  const id = uuidv4();
  db.prepare(`
    INSERT INTO llm_logs (id, request, status, parent_id, label)
    VALUES (?, ?, ?, ?, ?)
  `).run(id, JSON.stringify(request), 'PENDING', parentId ?? null, label ?? null);
  return id;
}

export function updateLlmLog(id: string, response: any, duration: number, status: string = 'SUCCESS') {
  db.prepare(`
    UPDATE llm_logs
    SET response = ?, duration = ?, status = ?
    WHERE id = ?
  `).run(JSON.stringify(response), duration, status, id);
}

export function addLlmStep(step: Omit<LlmStep, 'id' | 'timestamp'>): string {
  const id = uuidv4();
  db.prepare(`
    INSERT INTO llm_steps (id, log_id, step_number, finish_reason, usage, tool_calls, tool_results, text, duration_ms)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
  `).run(id, step.log_id, step.step_number, step.finish_reason, step.usage, step.tool_calls, step.tool_results, step.text, step.duration_ms);
  return id;
}

export function getLlmLogs(limit: number = 50): LlmLog[] {
  const logs = db.prepare(`
    SELECT * FROM llm_logs
    ORDER BY timestamp DESC
    LIMIT ?
  `).all(limit) as LlmLog[];

  for (const log of logs) {
    log.steps = db.prepare(`
      SELECT * FROM llm_steps WHERE log_id = ? ORDER BY step_number ASC
    `).all(log.id) as LlmStep[];
  }
  return logs;
}

export function clearLlmLogs() {
  db.prepare("DELETE FROM llm_steps").run();
  db.prepare("DELETE FROM llm_logs").run();
}

export function addConsoleLog(level: string, message: string, args: any[]): string {
  const id = uuidv4();
  db.prepare(`
    INSERT INTO console_logs (id, level, message, args)
    VALUES (?, ?, ?, ?)
  `).run(id, level, message, JSON.stringify(args));
  return id;
}

export function getConsoleLogs(limit: number = 200): ConsoleLog[] {
  return db.prepare(`
    SELECT * FROM console_logs
    ORDER BY timestamp DESC
    LIMIT ?
  `).all(limit) as ConsoleLog[];
}

export function clearConsoleLogs() {
  db.prepare("DELETE FROM console_logs").run();
}
