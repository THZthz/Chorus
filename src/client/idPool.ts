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

let pool: string[] = [];
let fetching: Promise<void> | null = null;

async function refill(): Promise<void> {
  const res = await fetch("/api/ids/batch?count=20");
  if (!res.ok) throw new Error(`Failed to fetch IDs: ${res.status}`);
  const { ids } = await res.json();
  pool.push(...ids);
}

export async function nextId(): Promise<string> {
  if (pool.length < 5 && !fetching) {
    fetching = refill().finally(() => {
      fetching = null;
    });
  }
  if (pool.length === 0) {
    await fetching;
  }
  return pool.pop()!;
}

export async function initIdPool(): Promise<void> {
  await refill();
}
