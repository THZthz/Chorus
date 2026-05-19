import { editNode } from "@/server/llm/tools/editNode";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { exec, parseToolOutput, resetDb } from "../helpers";

describe("editNode", () => {
  const TEST_NOTE = `test_note_editNode`;

  beforeAll(async () => {
    await resetDb();
  });

  afterEach(async () => {
    // Clean up test note
    try {
      await exec(editNode, {
        nodeLabel: "Note",
        action: "DELETE",
        match: { name: TEST_NOTE },
      });
    } catch {
      // Ignore cleanup failures
    }
  });

  it("CREATEs a Note", async () => {
    const result = await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: TEST_NOTE, content: "Test note content" },
    });
    expect(result).toContain("created");

    // Verify via queryWorld
    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (n:Note {name: '${TEST_NOTE}'}) RETURN n.name, n.content`,
    });
    const data = parseToolOutput(verify);
    expect(data.rowCount).toBe(1);
  });

  it("UPDATEs a Note", async () => {
    // Create first
    await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: TEST_NOTE, content: "Original content" },
    });

    // Update
    const result = await exec(editNode, {
      nodeLabel: "Note",
      action: "UPDATE",
      match: { name: TEST_NOTE },
      properties: { content: "Updated content" },
    });
    expect(result).toContain("updated");

    // Verify
    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (n:Note {name: '${TEST_NOTE}'}) RETURN n.name, n.content`,
    });
    const data = parseToolOutput(verify);
    const row = data.rows[0] as Record<string, unknown>;
    expect(row["n.content"]).toBe("Updated content");
    expect(row["n.name"]).toBe(TEST_NOTE);
  });

  it("DELETEs a Note", async () => {
    // Create first
    await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: TEST_NOTE, content: "To be deleted" },
    });

    const result = await exec(editNode, {
      nodeLabel: "Note",
      action: "DELETE",
      match: { name: TEST_NOTE },
    });
    expect(result).toContain("deleted");

    // Verify deletion
    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (n:Note {name: '${TEST_NOTE}'}) RETURN n`,
    });
    const data = parseToolOutput(verify);
    expect(data.rowCount).toBe(0);
  });

  it("rejects unknown node label", async () => {
    const result = await exec(editNode, {
      nodeLabel: "FakeLabelXYZ999",
      action: "CREATE",
      properties: { x: 1 },
    });
    expect(result).toContain("is not registered");
  });

  it("rejects CREATE with empty properties", async () => {
    const result = await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: {},
    });
    expect(result).toContain("ERROR: Parameter `properties` is required for CREATE and must not be empty.");
  });

  it("rejects system property _id", async () => {
    const result = await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: "bad_note", content: "x", _id: "hack" },
    });
    expect(result).toContain(" is internal (prefixed with '_') and cannot be set (managed internally by the engine).");
  });

  it("rejects UPDATE on non-existent node", async () => {
    const result = await exec(editNode, {
      nodeLabel: "Note",
      action: "UPDATE",
      match: { name: "nonexistent_note_xyz_999" },
      properties: { content: "nope" },
    });
    expect(result).toContain("No");
  });

  it("reports no properties to update when properties is empty", async () => {
    // Create first
    await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: TEST_NOTE, content: "Some content" },
    });

    const result = await exec(editNode, {
      nodeLabel: "Note",
      action: "UPDATE",
      match: { name: TEST_NOTE },
      properties: {},
    });
    expect(result).toContain("No properties to update");
  });

  describe("json partial update", () => {
    const TEST_ENTITY = `test_json_entity_editNode`;

    beforeAll(async () => {
      // Create an Entity with initial metadata for JSON merge tests
      await exec(editNode, {
        nodeLabel: "Entity",
        action: "CREATE",
        properties: {
          name: TEST_ENTITY,
          type: "OBJECT",
          description: "A test entity for JSON merge",
          brief: "JSON merge test",
          metadata: { stats: { power: 10 }, attributes: { Color: "red" } },
        },
      });
    });

    afterAll(async () => {
      await exec(editNode, {
        nodeLabel: "Entity",
        action: "DELETE",
        match: { name: TEST_ENTITY },
      });
    });

    it("shallow-merges json-tagged property and preserves existing keys", async () => {
      // Update only one top-level key inside metadata
      const result = await exec(editNode, {
        nodeLabel: "Entity",
        action: "UPDATE",
        match: { name: TEST_ENTITY },
        properties: { metadata: { stats: { power: 99, speed: 5 } } },
      });
      expect(result).toContain("updated");

      // Verify: metadata.stats should reflect the update,
      // and metadata.attributes should still exist (not clobbered)
      const verify = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Entity {name: '${TEST_ENTITY}'}) RETURN n.metadata`,
      });
      const data = parseToolOutput(verify);
      const row = data.rows[0] as Record<string, unknown>;
      const metadata = JSON.parse(row["n.metadata"] as string) as Record<string, unknown>;

      expect(metadata.stats).toEqual({ power: 99, speed: 5 });
      expect(metadata.attributes).toEqual({ Color: "red" });
    });

    it("shallow-merges a new top-level key into json-tagged property", async () => {
      const result = await exec(editNode, {
        nodeLabel: "Entity",
        action: "UPDATE",
        match: { name: TEST_ENTITY },
        properties: { metadata: { conditions: { Broken: true } } },
      });
      expect(result).toContain("updated");

      const verify = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Entity {name: '${TEST_ENTITY}'}) RETURN n.metadata`,
      });
      const data = parseToolOutput(verify);
      const row = data.rows[0] as Record<string, unknown>;
      const metadata = JSON.parse(row["n.metadata"] as string) as Record<string, unknown>;

      // New key merged in
      expect(metadata.conditions).toEqual({ Broken: true });
      // Existing keys still present
      expect(metadata.stats).toEqual({ power: 99, speed: 5 });
      expect(metadata.attributes).toEqual({ Color: "red" });
    });
  });
});
