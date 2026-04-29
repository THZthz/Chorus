import db from "../database";
import { v4 as uuidv4 } from "uuid";

export interface LlmLog {
  id: string;
  timestamp: string;
  request: string;
  response: string | null;
  duration: number | null;
  status: string;
}

export interface ConsoleLog {
  id: string;
  level: string;
  message: string;
  args: string;
  timestamp: string;
}

export function addLlmLog(request: any): string {
  const id = uuidv4();
  db.prepare(`
    INSERT INTO llm_logs (id, request, status)
    VALUES (?, ?, ?)
  `).run(id, JSON.stringify(request), 'PENDING');
  return id;
}

export function updateLlmLog(id: string, response: any, duration: number, status: string = 'SUCCESS') {
  db.prepare(`
    UPDATE llm_logs
    SET response = ?, duration = ?, status = ?
    WHERE id = ?
  `).run(JSON.stringify(response), duration, status, id);
}

export function getLlmLogs(limit: number = 50): LlmLog[] {
  return db.prepare(`
    SELECT * FROM llm_logs
    ORDER BY timestamp DESC
    LIMIT ?
  `).all(limit) as LlmLog[];
}

export function clearLlmLogs() {
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
