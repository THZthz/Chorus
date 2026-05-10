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
import { Message } from "@/types/dialogue";
import { safeJsonParse } from "@/server/models/shared";

export function getHistory(): Message[] {
  const rows = db.prepare("SELECT * FROM history_messages ORDER BY timestamp ASC").all() as any[];
  return rows.map((r) => ({
    id: r.id,
    speaker: r.speaker,
    type: r.type,
    text: r.text,
    metadata: safeJsonParse(r.metadata, undefined),
    skillCheck: safeJsonParse(r.skillCheck, undefined),
    rollResult: safeJsonParse(r.rollResult, undefined),
  }));
}

export function addMessage(msg: Message) {
  db.prepare(
    `
    INSERT INTO history_messages (id, speaker, type, text, metadata, skillCheck, rollResult)
    VALUES (?, ?, ?, ?, ?, ?, ?)
  `,
  ).run(
    msg.id,
    msg.speaker,
    msg.type,
    msg.text,
    msg.metadata ? JSON.stringify(msg.metadata) : null,
    msg.skillCheck ? JSON.stringify(msg.skillCheck) : null,
    msg.rollResult ? JSON.stringify(msg.rollResult) : null,
  );
}

export function clearHistory() {
  db.prepare("DELETE FROM history_messages").run();
}

export function setHistory(messages: Message[]) {
  db.transaction(() => {
    clearHistory();
    for (const msg of messages) {
      addMessage(msg);
    }
  })();
}
