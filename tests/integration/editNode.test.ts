import { editNode } from "@/server/llm/tools/editNode";
import { exec } from "../helpers";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { parseToolOutput, resetDb } from "../helpers";

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
      query: `MATCH (n:Note {name: '${TEST_NOTE}'}) RETURN n.content`,
    });
    const data = parseToolOutput(verify);
    const row = data.rows[0] as Record<string, unknown>;
    expect(row["n.content"]).toBe("Updated content");
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
    expect(result).toContain("properties is required");
  });

  it("rejects system property _id", async () => {
    const result = await exec(editNode, {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: "bad_note", content: "x", _id: "hack" },
    });
    expect(result).toContain("system-managed");
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
});
