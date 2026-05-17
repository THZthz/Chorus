import { editRelationship } from "@/server/llm/tools/editRelationship";
import { exec } from "../helpers";
import { editNode } from "@/server/llm/tools/editNode";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { parseToolOutput, resetDb } from "../helpers";

describe("editRelationship", () => {
  const NOTE_A = "test_rel_note_a";
  const NOTE_B = "test_rel_note_b";

  beforeAll(async () => {
    await resetDb();
  });

  afterEach(async () => {
    // Clean up test notes
    for (const name of [NOTE_A, NOTE_B]) {
      try {
        await exec(editNode, {
          nodeLabel: "Note",
          action: "DELETE",
          match: { name },
        });
      } catch {
        // Ignore
      }
    }
  });

  async function createTestNotes() {
    await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: NOTE_A, content: "Note A" },
    });
    await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: NOTE_B, content: "Note B" },
    });
  }

  it("CREATEs a relationship between two existing nodes", async () => {
    await createTestNotes();

    const result = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "ABOUT_ENTITY",
      sourceLabel: "Note",
      sourceMatch: { name: NOTE_A },
      targetLabel: "Entity",
      targetMatch: { name: "Player" },
    });
    expect(result).toContain("created successfully");

    // Verify
    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (n:Note {name: '${NOTE_A}'})-[r:ABOUT_ENTITY]->(e:Entity {name: 'Player'}) RETURN type(r)`,
    });
    const data = parseToolOutput(verify);
    expect(data.rowCount).toBe(1);
  });

  it("DELETEs a relationship", async () => {
    await createTestNotes();
    // Create relationship first
    await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "ABOUT_ENTITY",
      sourceLabel: "Note",
      sourceMatch: { name: NOTE_A },
      targetLabel: "Entity",
      targetMatch: { name: "Player" },
    });

    const result = await exec(editRelationship, {
      action: "DELETE",
      relationshipType: "ABOUT_ENTITY",
      sourceLabel: "Note",
      sourceMatch: { name: NOTE_A },
      targetLabel: "Entity",
      targetMatch: { name: "Player" },
    });
    expect(result).toContain("deleted");

    // Verify deletion
    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (n:Note {name: '${NOTE_A}'})-[r:ABOUT_ENTITY]->(e:Entity {name: 'Player'}) RETURN r`,
    });
    const data = parseToolOutput(verify);
    expect(data.rowCount).toBe(0);
  });

  it("rejects unknown relationship type", async () => {
    const result = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "FAKE_TYPE_XYZ",
      sourceLabel: "Entity",
      sourceMatch: { name: "Player" },
      targetLabel: "Entity",
      targetMatch: { name: "Veyla" },
    });
    expect(result).toContain("is not registered");
  });

  it("reports error when source node does not exist", async () => {
    const result = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "ALLIED_WITH",
      sourceLabel: "Entity",
      sourceMatch: { name: "NonexistentEntityXYZ" },
      targetLabel: "Entity",
      targetMatch: { name: "Player" },
    });
    expect(result).toContain("ERROR");
  });

  it("rejects empty sourceMatch", async () => {
    const result = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "ALLIED_WITH",
      sourceLabel: "Entity",
      sourceMatch: {},
      targetLabel: "Entity",
      targetMatch: { name: "Player" },
    });
    expect(result).toContain("sourceMatch must not be empty");
  });

  it("rejects empty targetMatch", async () => {
    const result = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "ALLIED_WITH",
      sourceLabel: "Entity",
      sourceMatch: { name: "Player" },
      targetLabel: "Entity",
      targetMatch: {},
    });
    expect(result).toContain("targetMatch must not be empty");
  });
});
