import type { Neo4jClient } from "@/server/memory/neo4j";

const CHARS = "1fER78GIDVbh95ngu6adzmkjZy2sSQoJTL0vXrx3MCtcPeKYUBWAiFpl4HqOwN";

function encodeBase62(n: number): string {
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
 */
export async function nextId(
  client: Neo4jClient,
  key: string = "message_id",
): Promise<string> {
  const rows = await client.executeWrite(
    `MERGE (c:IdCounter {key: $key})
     ON CREATE SET c.value = 0
     SET c.value = c.value + 1
     RETURN c.value AS value`,
    { key },
  );
  const value = rows[0].value as number;
  return encodeBase62(value - 1);
}

/**
 * Generate a batch of short IDs for a given counter key.
 * Atomically reserves `count` values and returns them.
 */
export async function nextIdBatch(
  client: Neo4jClient,
  count: number,
  key: string = "message_id",
): Promise<string[]> {
  const rows = await client.executeWrite(
    `MERGE (c:IdCounter {key: $key})
     ON CREATE SET c.value = 0
     SET c.value = c.value + $count
     RETURN c.value AS value`,
    { key, count },
  );
  const endValue = rows[0].value as number;
  const startValue = endValue - count;
  const ids: string[] = [];
  for (let i = startValue; i < endValue; i++) {
    ids.push(encodeBase62(i));
  }
  return ids;
}
