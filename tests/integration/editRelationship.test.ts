import { editRelationship } from "@/server/llm/tools/editRelationship";
import { editNode } from "@/server/llm/tools/editNode";
import { manageSchema } from "@/server/llm/tools/manageSchema";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { exec, resetDb, parseToolOutput } from "../helpers";

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

  it("rejects CREATE when endpoint labels don't match the registered definition", async () => {
    // Register TEST_STRICT_REL with (Entity→Entity)
    await exec(manageSchema, {
      target: "relationship",
      action: "register",
      name: "TEST_STRICT_REL",
      description: "Strict endpoint relationship",
      sourceLabel: "Entity",
      targetLabel: "Entity",
    });

    await createTestNotes();
    const result = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "TEST_STRICT_REL",
      sourceLabel: "Note",
      sourceMatch: { name: NOTE_A },
      targetLabel: "Entity",
      targetMatch: { name: "Player" },
    });
    expect(result).toContain("not registered");

    // But it should work with the correct labels
    const ok = await exec(editRelationship, {
      action: "CREATE",
      relationshipType: "TEST_STRICT_REL",
      sourceLabel: "Entity",
      sourceMatch: { name: "Player" },
      targetLabel: "Entity",
      targetMatch: { name: "Player" },
    });
    expect(ok).toContain("created successfully");

    // Cleanup
    await exec(manageSchema, {
      target: "relationship",
      action: "unregister",
      name: "TEST_STRICT_REL",
      sourceLabel: "Entity",
      targetLabel: "Entity",
    });
  });

  describe("UPDATE", () => {
    beforeAll(async () => {
      await createTestNotes();
    });

    it("UPDATEs properties on a relationship", async () => {
      // Create relationship with two properties to verify partial update preservation
      await exec(editRelationship, {
        action: "CREATE",
        relationshipType: "ABOUT_ENTITY",
        sourceLabel: "Note",
        sourceMatch: { name: NOTE_A },
        targetLabel: "Entity",
        targetMatch: { name: "Player" },
        properties: { confidence: 0.8, reason: "initial reason" },
      });

      const result = await exec(editRelationship, {
        action: "UPDATE",
        relationshipType: "ABOUT_ENTITY",
        sourceLabel: "Note",
        sourceMatch: { name: NOTE_A },
        targetLabel: "Entity",
        targetMatch: { name: "Player" },
        properties: { confidence: 0.9 },
      });
      expect(result).toContain("updated properties");

      // Verify: confidence updated, reason preserved
      const verify = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Note {name: '${NOTE_A}'})-[r:ABOUT_ENTITY]->(e:Entity {name: 'Player'}) RETURN r.confidence, r.reason`,
      });
      const data = parseToolOutput(verify);
      const row = data.rows[0] as Record<string, unknown>;
      expect(row["r.confidence"]).toBe(0.9);
      expect(row["r.reason"]).toBe("initial reason");
    });

    it("reports error when UPDATE relationship not found", async () => {
      const result = await exec(editRelationship, {
        action: "UPDATE",
        relationshipType: "ALLIED_WITH",
        sourceLabel: "Entity",
        sourceMatch: { name: "Player" },
        targetLabel: "Entity",
        targetMatch: { name: "NonexistentEntityXYZ" },
        properties: { confidence: 1 },
      });
      expect(result).toContain("ERROR");
    });

    it("rejects UPDATE with empty properties", async () => {
      const result = await exec(editRelationship, {
        action: "UPDATE",
        relationshipType: "ABOUT_ENTITY",
        sourceLabel: "Note",
        sourceMatch: { name: NOTE_A },
        targetLabel: "Entity",
        targetMatch: { name: "Player" },
        properties: {},
      });
      expect(result).toContain("No properties to update");
    });

    it("rejects UPDATE with internal property", async () => {
      const result = await exec(editRelationship, {
        action: "UPDATE",
        relationshipType: "ABOUT_ENTITY",
        sourceLabel: "Note",
        sourceMatch: { name: NOTE_A },
        targetLabel: "Entity",
        targetMatch: { name: "Player" },
        properties: { _hack: "bad" },
      });
      expect(result).toContain("internal");
    });
  });

  describe("GM_DEFINED schema validation", () => {
    const GM_REL = "TEST_TRUSTS";
    const ENT_A = "test_rel_schema_entity_a";
    const ENT_B = "test_rel_schema_entity_b";

    beforeAll(async () => {
      // Register a GM_DEFINED relationship type with a property schema
      await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: GM_REL,
        description: "Test trust relationship with confidence level",
        properties: [
          { name: "confidence", description: "Trust confidence (0-1)", tags: ["number"] },
          { name: "reason", description: "Why this trust exists", tags: ["string"] },
        ],
        sourceLabel: "Entity",
        targetLabel: "Entity",
      });

      // Create two test entities
      for (const name of [ENT_A, ENT_B]) {
        await exec(editNode, {
          nodeLabel: "Entity",
          action: "CREATE",
          properties: {
            name,
            type: "CHARACTER",
            description: `Schema validation test entity ${name}`,
            brief: "Test entity",
          },
        });
      }

      // Create the relationship
      await exec(editRelationship, {
        action: "CREATE",
        relationshipType: GM_REL,
        sourceLabel: "Entity",
        sourceMatch: { name: ENT_A },
        targetLabel: "Entity",
        targetMatch: { name: ENT_B },
        properties: { confidence: 0.7, reason: "shared history" },
      });
    });

    afterAll(async () => {
      // Clean up relationship
      await exec(editRelationship, {
        action: "DELETE",
        relationshipType: GM_REL,
        sourceLabel: "Entity",
        sourceMatch: { name: ENT_A },
        targetLabel: "Entity",
        targetMatch: { name: ENT_B },
      });
      // Clean up entities
      for (const name of [ENT_A, ENT_B]) {
        await exec(editNode, {
          nodeLabel: "Entity",
          action: "DELETE",
          match: { name },
        });
      }
      await exec(manageSchema, {
        target: "relationship",
        action: "unregister",
        name: GM_REL,
      });
    });

    it("rejects unknown property on GM_DEFINED relationship CREATE", async () => {
      const result = await exec(editRelationship, {
        action: "CREATE",
        relationshipType: GM_REL,
        sourceLabel: "Entity",
        sourceMatch: { name: ENT_A },
        targetLabel: "Entity",
        targetMatch: { name: "Player" },
        properties: { confidence: 0.5, bad_prop: "should fail" },
      });
      expect(result).toContain("Unknown property");
    });

    it("rejects unknown property on GM_DEFINED relationship UPDATE", async () => {
      const result = await exec(editRelationship, {
        action: "UPDATE",
        relationshipType: GM_REL,
        sourceLabel: "Entity",
        sourceMatch: { name: ENT_A },
        targetLabel: "Entity",
        targetMatch: { name: ENT_B },
        properties: { bad_prop: "should fail" },
      });
      expect(result).toContain("Unknown property");
    });

    it("allows known properties on GM_DEFINED relationship UPDATE", async () => {
      const result = await exec(editRelationship, {
        action: "UPDATE",
        relationshipType: GM_REL,
        sourceLabel: "Entity",
        sourceMatch: { name: ENT_A },
        targetLabel: "Entity",
        targetMatch: { name: ENT_B },
        properties: { confidence: 0.95 },
      });
      expect(result).toContain("updated properties");
    });
  });

  describe("JSON partial merge on UPDATE", () => {
    beforeAll(async () => {
      await createTestNotes();
    });

    it("shallow-merges json-tagged property on relationship", async () => {
      // Register a GM_DEFINED type with a JSON property
      await exec(manageSchema, {
        target: "relationship",
        action: "register",
        name: "HAS_METADATA",
        description: "Relationship with JSON metadata",
        properties: [
          { name: "meta", description: "JSON metadata blob", tags: ["json"] },
        ],
        sourceLabel: "Note",
        targetLabel: "Entity",
      });

      // Create with initial JSON
      await exec(editRelationship, {
        action: "CREATE",
        relationshipType: "HAS_METADATA",
        sourceLabel: "Note",
        sourceMatch: { name: NOTE_A },
        targetLabel: "Entity",
        targetMatch: { name: "Player" },
        properties: { meta: { x: 1, y: 2 } },
      });

      // Update only one key
      const result = await exec(editRelationship, {
        action: "UPDATE",
        relationshipType: "HAS_METADATA",
        sourceLabel: "Note",
        sourceMatch: { name: NOTE_A },
        targetLabel: "Entity",
        targetMatch: { name: "Player" },
        properties: { meta: { y: 99 } },
      });
      expect(result).toContain("updated properties");

      // Verify: x should still be 1, y should be 99
      const verify = await exec(queryWorld, {
        action: "READ",
        query: `MATCH (n:Note {name: '${NOTE_A}'})-[r:HAS_METADATA]->(e:Entity {name: 'Player'}) RETURN r.meta`,
      });
      const data = parseToolOutput(verify);
      const row = data.rows[0] as Record<string, unknown>;
      const meta = JSON.parse((row as Record<string, string>)["r.meta"]);
      expect(meta).toEqual({ x: 1, y: 99 });
    });
  });
});
