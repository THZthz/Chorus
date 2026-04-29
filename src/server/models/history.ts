import db from "@/server/db";
import { Message } from "@/types/dialogue";

export function getHistory(): Message[] {
  const rows = db.prepare("SELECT * FROM history_messages ORDER BY timestamp ASC").all() as any[];
  return rows.map(r => ({
    id: r.id,
    speaker: r.speaker,
    type: r.type,
    text: r.text,
    metadata: r.metadata ? JSON.parse(r.metadata) : undefined,
    skillCheck: r.skillCheck ? JSON.parse(r.skillCheck) : undefined,
    rollResult: r.rollResult ? JSON.parse(r.rollResult) : undefined,
  }));
}

export function addMessage(msg: Message) {
  db.prepare(`
    INSERT INTO history_messages (id, speaker, type, text, metadata, skillCheck, rollResult)
    VALUES (?, ?, ?, ?, ?, ?, ?)
  `).run(
    msg.id,
    msg.speaker,
    msg.type,
    msg.text,
    msg.metadata ? JSON.stringify(msg.metadata) : null,
    msg.skillCheck ? JSON.stringify(msg.skillCheck) : null,
    msg.rollResult ? JSON.stringify(msg.rollResult) : null
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
