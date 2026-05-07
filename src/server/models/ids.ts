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

const CHARS = "1fER78GIDVbh95ngu6adzmkjZy2sSQoJTL0vXrx3MCtcPeKYUBWAiFpl4HqOwN";

function encodeBase62(n: number): string {
  let result = "";
  for (let i = 0; i < 4; i++) {
    result = CHARS[n % 62] + result;
    n = Math.floor(n / 62);
  }
  return result;
}

export function nextId(): string {
  const val = db.transaction(() => {
    const row = db.prepare("SELECT value FROM system_state WHERE key = 'id_counter'").get() as
      | { value: string }
      | undefined;
    const current = row ? parseInt(row.value, 10) : 0;
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES ('id_counter', ?)").run(
      String(current + 1),
    );
    return current;
  })();
  return encodeBase62(val);
}

export function nextIdBatch(count: number): string[] {
  return db.transaction(() => {
    const row = db.prepare("SELECT value FROM system_state WHERE key = 'id_counter'").get() as
      | { value: string }
      | undefined;
    const current = row ? parseInt(row.value, 10) : 0;
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES ('id_counter', ?)").run(
      String(current + count),
    );
    const ids: string[] = [];
    for (let i = 0; i < count; i++) {
      ids.push(encodeBase62(current + i));
    }
    return ids;
  })();
}
