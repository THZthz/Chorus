/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias
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

import type { Neo4jClient } from "@/server/memory/neo4j";
import { GAME_ID } from "@/server/gameState";

/**
 * 32-bit Feistel cipher – maps an unsigned integer < 2^32
 * to a unique pseudo-random number in the same range.
 *
 * @param x      Input integer (0 … 2^32-1)
 * @param key    Array of numbers used as secret key material
 * @returns      Permuted integer, always unique for each x with the same key
 */
export function feistelEncrypt(x: number, key: number[]): number {
  // Split into two 16-bit halves
  let left = (x >>> 16) & 0xffff;
  let right = x & 0xffff;

  // Number of rounds (8 is usually enough for good mixing)
  const ROUNDS = 8;

  for (let i = 0; i < ROUNDS; i++) {
    // Simple round function: mix right half, round index and key
    const roundKey = key[i % key.length] & 0xffff;
    let f = (right * roundKey + i) & 0xffff; // not cryptographic, but works for obfuscation
    f = ((f << 5) | (f >>> 11)) & 0xffff;     // rotate bits a little

    const newRight = left ^ f;
    left = right;
    right = newRight;
  }

  // Reassemble the halves
  return ((left << 16) | right) >>> 0; // force unsigned
}

/**
 * Reverse of feistelEncrypt – recovers the original integer.
 * Same key, same number of rounds, but in reverse order.
 */
export function feistelDecrypt(y: number, key: number[]): number {
  let left = (y >>> 16) & 0xffff;
  let right = y & 0xffff;

  const ROUNDS = 8;

  for (let i = ROUNDS - 1; i >= 0; i--) {
    const roundKey = key[i % key.length] & 0xffff;
    let f = (left * roundKey + i) & 0xffff;
    f = ((f << 5) | (f >>> 11)) & 0xffff;

    const newLeft = right ^ f;
    right = left;
    left = newLeft;
  }

  return ((left << 16) | right) >>> 0;
}

// Secret key for Feistel cipher.
export const SECRET = [0xa3b5, 0x2c7d, 0x8f1e, 0x4402];

const CHARS = "1fER78GIDVbh95ngu6adzmkjZy2sSQoJTL0vXrx3MCtcPeKYUBWAiFpl4HqOwN"; // 62 characters

function encodeBase62(n: number): string {
  if (!Number.isFinite(n) || n < 0) throw new Error(`encodeBase62: invalid input ${n}`);
  let result = "";
  for (let i = 0; i < 4; i++) {
    result = CHARS[n % 62] + result;
    n = Math.floor(n / 62);
  }
  return result;
}

/**
 * Generate the next short ID for a given counter key.
 * Uses an :IdCounter node in Neo4j to atomically increment a counter
 * and returns its base62-encoded value as a 4-character string.
 * This is used especially for :Message, since GM may reference its ID, 4 characters is easy to remember.
 */
export async function nextId(client: Neo4jClient): Promise<string> {
  const rows = await client.executeWrite(
    `MERGE (c:IdCounter {session_id: $id})
     ON CREATE SET c.value = 0
     SET c.value = c.value + 1
     RETURN c.value AS value`,
    { id: GAME_ID },
  );
  const value = Number(rows[0].value);
  if (!Number.isFinite(value)) throw new Error(`nextId: invalid counter value ${rows[0].value}`);
  return encodeBase62(feistelEncrypt(value - 1, SECRET));
}

/**
 * Generate a batch of short IDs for a given counter key. Atomically reserves `count` values and
 * returns them.
 */
export async function nextIdBatch(client: Neo4jClient, count: number): Promise<string[]> {
  const rows = await client.executeWrite(
    `MERGE (c:IdCounter {session_id: $id})
     ON CREATE SET c.value = 0
     SET c.value = c.value + $count
     RETURN c.value AS value`,
    { id: GAME_ID, count },
  );
  const endValue = Number(rows[0].value);
  if (!Number.isFinite(endValue))
    throw new Error(`nextIdBatch: invalid counter value ${rows[0].value}`);
  const startValue = endValue - count;
  const ids: string[] = [];
  for (let i = startValue; i < endValue; i++) {
    ids.push(encodeBase62(feistelEncrypt(i, SECRET)));
  }
  return ids;
}
