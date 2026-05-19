import { queryWorld } from "@/server/llm/tools/queryWorld";
import { MemoryClient } from "@/server/memory/client";
import { exec, parseToolOutput, resetDb } from "../helpers";

describe("queryWorld READ", () => {
  beforeAll(async () => {
    await resetDb();
  });

  it("reads entities from seed data", async () => {
    const result = await exec(queryWorld, {
      action: "READ",
      query: "MATCH (e:Entity) RETURN e.name, e.type LIMIT 3",
    });
    const data = parseToolOutput(result);
    expect(data.rowCount).toBeGreaterThan(0);
    expect(Array.isArray(data.rows)).toBe(true);
  });

  it("auto-applies LIMIT when missing", async () => {
    const result = await exec(queryWorld, {
      action: "READ",
      query: "MATCH (e:Entity) RETURN e.name",
    });
    const data = parseToolOutput(result);
    expect((data.rows as unknown[]).length).toBeLessThanOrEqual(50);
  });

  it("respects explicit LIMIT", async () => {
    const result = await exec(queryWorld, {
      action: "READ",
      query: "MATCH (e:Entity) RETURN e.name LIMIT 2",
    });
    const data = parseToolOutput(result);
    expect(data.rowCount).toBe(2);
  });

  it("hides internal _-prefixed properties", async () => {
    const result = await exec(queryWorld, {
      action: "READ",
      query: "MATCH (e:Entity {name: 'Player'}) RETURN e LIMIT 1",
    });
    const data = parseToolOutput(result);
    const entity = data.rows[0] as Record<string, unknown>;
    const e = entity.e as Record<string, unknown>;
    // _embedding, _id, etc. should be stripped
    for (const key of Object.keys(e)) {
      expect(key).not.toMatch(/^_/);
    }
  });

  it("rejects invalid Cypher syntax", async () => {
    const result = await exec(queryWorld, {
      action: "READ",
      query: "MATCH BROKEN GARBAGE",
    });
    expect(result).toContain("CYPHER SYNTAX ERROR");
  });

  it("rejects write clause in READ mode", async () => {
    const result = await exec(queryWorld, {
      action: "READ",
      query: "CREATE (n:Entity {name: 'hack'})",
    });
    expect(result).toContain("VALIDATION FAILED");
  });

  it("rejects unbounded path patterns", async () => {
    const result = await exec(queryWorld, {
      action: "READ",
      query: "MATCH (e:Entity)-[*]->(o) RETURN e, o",
    });
    expect(result).toContain("VALIDATION FAILED");
    expect(result).toContain("unbounded");
  });
});

describe("queryWorld WRITE", () => {
  const TEST_NAME = "TestWriteEntity";

  afterEach(async () => {
    const client = MemoryClient.getCachedInstance();
    try {
      await client.neo4j.executeWrite(
        `MATCH (e:Entity {name: '${TEST_NAME}'}) DETACH DELETE e`,
      );
    } catch {
      // Ignore cleanup failures
    }
  });

  it("creates and verifies an entity via MERGE", async () => {
    const writeResult = await exec(queryWorld, {
      action: "WRITE",
      query: `MERGE (e:Entity {name: '${TEST_NAME}'}) SET e.type = 'CHARACTER', e.brief = 'Test entity'`,
    });
    expect(writeResult).toContain("Success");
    expect(writeResult).toContain("row(s) affected");

    // Verify via READ
    const readResult = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (e:Entity {name: '${TEST_NAME}'}) RETURN e.name, e.type`,
    });
    const data = parseToolOutput(readResult);
    expect(data.rowCount).toBe(1);
  });

  it("rejects DELETE without WHERE clause", async () => {
    const result = await exec(queryWorld, {
      action: "WRITE",
      query: "MATCH (n:Entity) DETACH DELETE n",
    });
    expect(result).toContain("VALIDATION FAILED");
  });

  it("rejects unregistered node label", async () => {
    const result = await exec(queryWorld, {
      action: "WRITE",
      query: "CREATE (n:FakeLabelXYZ123 {x: 1})",
    });
    expect(result).toContain("Unknown label");
  });

  it("allows UPDATE via SET on existing entity", async () => {
    await exec(queryWorld, {
      action: "WRITE",
      query: `MERGE (e:Entity {name: '${TEST_NAME}'}) SET e.type = 'OBJECT', e.brief = 'Before update'`,
    });

    const updateResult = await exec(queryWorld, {
      action: "WRITE",
      query: `MATCH (e:Entity {name: '${TEST_NAME}'}) SET e.brief = 'After update'`,
    });
    expect(updateResult).toContain("Success");

    // Verify update
    const readResult = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (e:Entity {name: '${TEST_NAME}'}) RETURN e.brief`,
    });
    const data = parseToolOutput(readResult);
    const row = data.rows[0] as Record<string, unknown>;
    expect(row["e.brief"]).toBe("After update");
  });

  it("reports CYPHER SYNTAX ERROR for malformed write query", async () => {
    const result = await exec(queryWorld, {
      action: "WRITE",
      query: "MOOCH (e:Entity) SET e.x = 1",
    });
    expect(result).toContain("CYPHER SYNTAX ERROR");
  });
});
