import db from "@/server/db";

const CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

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
    const row = db
      .prepare("SELECT value FROM system_state WHERE key = 'id_counter'")
      .get() as { value: string } | undefined;
    const current = row ? parseInt(row.value, 10) : 0;
    db.prepare(
      "INSERT OR REPLACE INTO system_state (key, value) VALUES ('id_counter', ?)"
    ).run(String(current + 1));
    return current;
  })();
  return encodeBase62(val);
}

export function nextIdBatch(count: number): string[] {
  return db.transaction(() => {
    const row = db
      .prepare("SELECT value FROM system_state WHERE key = 'id_counter'")
      .get() as { value: string } | undefined;
    const current = row ? parseInt(row.value, 10) : 0;
    db.prepare(
      "INSERT OR REPLACE INTO system_state (key, value) VALUES ('id_counter', ?)"
    ).run(String(current + count));
    const ids: string[] = [];
    for (let i = 0; i < count; i++) {
      ids.push(encodeBase62(current + i));
    }
    return ids;
  })();
}
