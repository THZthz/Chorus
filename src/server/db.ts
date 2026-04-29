import Database from "better-sqlite3";
import fs from "fs";
import path from "path";

const dbPath = path.join(process.cwd(), "game.db");
const db = new Database(dbPath);

db.pragma('journal_mode = WAL');

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
    opinions TEXT
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
    triggerCondition TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'PENDING'
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

  CREATE TABLE IF NOT EXISTS console_logs (
    id TEXT PRIMARY KEY,
    level TEXT NOT NULL,
    message TEXT NOT NULL,
    args TEXT NOT NULL,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
  );
`);

export default db;
