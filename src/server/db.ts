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

import Database from "better-sqlite3";
import path from "path";

const dbPath = path.join(process.cwd(), process.env.SQLITE_PATH ?? "data/sqlite/game.db");
const db = new Database(dbPath);

db.pragma("journal_mode = WAL");

db.exec(`
  CREATE TABLE IF NOT EXISTS system_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
  );

  CREATE TABLE IF NOT EXISTS llm_logs (
    id TEXT PRIMARY KEY,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    request TEXT NOT NULL,
    response TEXT,
    duration INTEGER,
    status TEXT,
    parent_id TEXT,
    label TEXT
  );

  CREATE TABLE IF NOT EXISTS llm_steps (
    id TEXT PRIMARY KEY,
    log_id TEXT NOT NULL,
    step_number INTEGER NOT NULL,
    finish_reason TEXT,
    usage TEXT,
    tool_calls TEXT,
    tool_results TEXT,
    text TEXT,
    duration_ms INTEGER,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    user_prompt TEXT,
    reasoning TEXT,
    FOREIGN KEY (log_id) REFERENCES llm_logs(id)
  );
`);

// Drop old tables if they exist (idempotent migration)
db.exec(`
  DROP TABLE IF EXISTS entities;
  DROP TABLE IF EXISTS history_messages;
  DROP TABLE IF EXISTS plots;
  DROP TABLE IF EXISTS dialogue_steps;
  DROP TABLE IF EXISTS dialogue_alternatives;
  DROP TABLE IF EXISTS notes;
`);

export default db;
