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

const dbPath = path.join(process.cwd(), "game.db");
const db = new Database(dbPath);

db.pragma("journal_mode = WAL");

// Define schema
db.exec(`
  CREATE TABLE IF NOT EXISTS entities (
    id TEXT PRIMARY KEY,
    type TEXT NOT NULL,
    displayName TEXT NOT NULL,
    shortDescription TEXT NOT NULL,
    longDescription TEXT NOT NULL,
    attributes TEXT NOT NULL,
    stats TEXT,
    opinions TEXT,
    conditions TEXT
  );

  CREATE TABLE IF NOT EXISTS history_messages (
    id TEXT PRIMARY KEY,
    speaker TEXT NOT NULL,
    type TEXT NOT NULL,
    text TEXT NOT NULL,
    metadata TEXT,
    skillCheck TEXT,
    rollResult TEXT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
  );

  CREATE TABLE IF NOT EXISTS plots (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'PENDING',
    involved_locations TEXT NOT NULL DEFAULT '[]',
    involved_characters TEXT NOT NULL DEFAULT '[]',
    parent_plot_id TEXT,
    parent_option_id INTEGER,
    child_plots TEXT NOT NULL DEFAULT '[]',
    FOREIGN KEY (parent_plot_id) REFERENCES plots(id)
  );

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
    status TEXT
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
    FOREIGN KEY (log_id) REFERENCES llm_logs(id)
  );

  CREATE TABLE IF NOT EXISTS dialogue_steps (
    id TEXT PRIMARY KEY,
    parent_step_id TEXT,
    parent_option_id TEXT,
    messages TEXT NOT NULL,
    options TEXT NOT NULL,
    world_snapshot TEXT,
    is_generated INTEGER DEFAULT 0,
    is_active INTEGER DEFAULT 1,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (parent_step_id) REFERENCES dialogue_steps(id)
  );

  CREATE TABLE IF NOT EXISTS dialogue_alternatives (
    id TEXT PRIMARY KEY,
    step_id TEXT NOT NULL,
    messages TEXT NOT NULL,
    options TEXT NOT NULL,
    sequence_num INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (step_id) REFERENCES dialogue_steps(id)
  );

  CREATE TABLE IF NOT EXISTS facts (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    related_entity_ids TEXT NOT NULL DEFAULT '[]',
    related_plot_ids TEXT NOT NULL DEFAULT '[]',
    related_scene INTEGER NOT NULL DEFAULT 0,
    related_time INTEGER NOT NULL DEFAULT 0,
    is_valid INTEGER NOT NULL DEFAULT 1,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
  );
`);

// Idempotent migrations for columns added after initial schema
try {
  db.exec("ALTER TABLE llm_logs ADD COLUMN parent_id TEXT");
} catch {}
try {
  db.exec("ALTER TABLE llm_logs ADD COLUMN label TEXT");
} catch {}
try {
  db.exec("ALTER TABLE llm_steps ADD COLUMN user_prompt TEXT");
} catch {}
try {
  db.exec("ALTER TABLE llm_steps ADD COLUMN reasoning TEXT");
} catch {}
try {
  db.exec("ALTER TABLE plots ADD COLUMN plot_flags TEXT DEFAULT '{}'");
} catch {}
try {
  db.exec("ALTER TABLE entities ADD COLUMN conditions TEXT");
} catch {}

export default db;
