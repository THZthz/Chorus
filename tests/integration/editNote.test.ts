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

import { editNote } from "@/server/llm/tools/editNote";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { exec, parseToolOutput, resetDb } from "../helpers";
import { MemoryClient } from "@/server/memory/client";

describe("editNote", () => {
  const TEST_NOTE = "test_note_editNote";

  beforeAll(async () => {
    await resetDb();
  });

  afterEach(async () => {
    try {
      await exec(editNote, { noteName: TEST_NOTE, action: "DELETE" });
    } catch {
      // Ignore cleanup failures
    }
  });

  it("CREATEs a note", async () => {
    const result = await exec(editNote, {
      noteName: TEST_NOTE,
      action: "CREATE",
      content: "Test note content",
    });
    expect(result).toContain("created");

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (n:Note {name: '${TEST_NOTE}'}) RETURN n.name, n.content`,
    });
    const data = parseToolOutput(verify);
    expect(data.rowCount).toBe(1);
  });

  it("rejects CREATE without content", async () => {
    const result = await exec(editNote, {
      noteName: TEST_NOTE,
      action: "CREATE",
    });
    expect(result).toContain('"content" is required');
  });

  it("UPDATEs content", async () => {
    await exec(editNote, {
      noteName: TEST_NOTE,
      action: "CREATE",
      content: "Original content",
    });

    const result = await exec(editNote, {
      noteName: TEST_NOTE,
      action: "UPDATE",
      content: "Updated content",
    });
    expect(result).toContain("updated");

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (n:Note {name: '${TEST_NOTE}'}) RETURN n.content`,
    });
    const data = parseToolOutput(verify);
    const row = data.rows[0] as Record<string, unknown>;
    expect(row["n.content"]).toBe("Updated content");
  });

  it("UPDATEs with aboutEntities linking", async () => {
    const noteName = "test_note_entities";
    try {
      await exec(editNote, {
        noteName,
        action: "CREATE",
        content: "Note about entities",
      });

      const result = await exec(editNote, {
        noteName,
        action: "UPDATE",
        aboutEntities: ["Player", "Elias Crowne"],
      });
      expect(result).toContain("updated");
      expect(result).toContain("all entities links");

      const verify = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Note {name: '${noteName}'})-[:ABOUT_ENTITY]->(e:Entity) RETURN e.name ORDER BY e.name`,
      });
      const data = parseToolOutput(verify);
      expect(data.rowCount).toBe(2);
    } finally {
      await exec(editNote, { noteName, action: "DELETE" }).catch(() => {});
    }
  });

  it("UPDATEs with aboutMessages linking", async () => {
    const noteName = "test_note_messages";
    try {
      await exec(editNote, {
        noteName,
        action: "CREATE",
        content: "Note about messages",
      });

      const client = MemoryClient.getCachedInstance();
      const msg = await client.shortTerm.addMessage("A test message for linking");

      const result = await exec(editNote, {
        noteName,
        action: "UPDATE",
        aboutMessages: [msg.id],
      });
      expect(result).toContain("updated");
      expect(result).toContain("all messages links");

      const verify = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Note {name: '${noteName}'})-[:ABOUT_MESSAGE]->(m:Message) RETURN m.id`,
      });
      const data = parseToolOutput(verify);
      expect(data.rowCount).toBe(1);
    } finally {
      await exec(editNote, { noteName, action: "DELETE" }).catch(() => {});
    }
  });

  it("UPDATE clears links when empty array passed", async () => {
    const noteName = "test_note_clear_links";
    try {
      await exec(editNote, {
        noteName,
        action: "CREATE",
        content: "Note with links to clear",
        aboutEntities: ["Player"],
      });

      // Verify link exists
      const before = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Note {name: '${noteName}'})-[:ABOUT_ENTITY]->(e:Entity) RETURN e.name`,
      });
      expect(parseToolOutput(before).rowCount).toBe(1);

      // Clear links with empty array
      const result = await exec(editNote, {
        noteName,
        action: "UPDATE",
        aboutEntities: [],
      });
      expect(result).toContain("updated");

      const verify = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Note {name: '${noteName}'})-[:ABOUT_ENTITY]->(e:Entity) RETURN e.name`,
      });
      expect(parseToolOutput(verify).rowCount).toBe(0);
    } finally {
      await exec(editNote, { noteName, action: "DELETE" }).catch(() => {});
    }
  });

  it("UPDATE partial — only content, links unchanged", async () => {
    const noteName = "test_note_partial";
    try {
      await exec(editNote, {
        noteName,
        action: "CREATE",
        content: "Original",
        aboutEntities: ["Player"],
      });

      // Update only content, don't touch links
      const result = await exec(editNote, {
        noteName,
        action: "UPDATE",
        content: "Revised",
      });
      expect(result).toContain("updated");

      // Links still there
      const verify = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Note {name: '${noteName}'})-[:ABOUT_ENTITY]->(e:Entity) RETURN e.name`,
      });
      expect(parseToolOutput(verify).rowCount).toBe(1);
    } finally {
      await exec(editNote, { noteName, action: "DELETE" }).catch(() => {});
    }
  });

  it("DELETEs a note", async () => {
    await exec(editNote, {
      noteName: TEST_NOTE,
      action: "CREATE",
      content: "To be deleted",
    });

    const result = await exec(editNote, {
      noteName: TEST_NOTE,
      action: "DELETE",
    });
    expect(result).toContain("deleted");

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (n:Note {name: '${TEST_NOTE}'}) RETURN n`,
    });
    expect(parseToolOutput(verify).rowCount).toBe(0);
  });

  it("rejects DELETE on non-existent note", async () => {
    const result = await exec(editNote, {
      noteName: "nonexistent_note_xyz_999",
      action: "DELETE",
    });
    expect(result).toContain("not found");
  });

  it("rejects UPDATE on non-existent note", async () => {
    const result = await exec(editNote, {
      noteName: "nonexistent_note_xyz_999",
      action: "UPDATE",
      content: "nope",
    });
    expect(result).toContain("not found");
  });
});
