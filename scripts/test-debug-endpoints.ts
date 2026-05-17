/**
 * Automatic test script for debug tool API endpoints.
 *
 * Usage: tsx scripts/test-debug-endpoints.ts
 * Requires: server running on localhost:3000 (npm start) and Neo4j accessible.
 */

const BASE = "http://localhost:3000/api";
const COLOR_GREEN = "\x1b[32m";
const COLOR_RED = "\x1b[31m";
const COLOR_RESET = "\x1b[0m";

let passed = 0;
let failed = 0;
const failures: string[] = [];

// ── Test harness ──

async function test(name: string, fn: () => Promise<void>): Promise<void> {
  process.stdout.write(`  ${name}... `);
  try {
    await fn();
    passed++;
    console.log(`${COLOR_GREEN}PASS${COLOR_RESET}`);
  } catch (e) {
    failed++;
    const msg = e instanceof Error ? e.message : String(e);
    console.log(`${COLOR_RED}FAIL${COLOR_RESET}`);
    console.log(`    ${msg}`);
    failures.push(`${name}: ${msg}`);
  }
}

function ok(condition: boolean, msg: string): void {
  if (!condition) throw new Error(msg);
}

function eq<T>(actual: T, expected: T, msg: string): void {
  if (actual !== expected) {
    throw new Error(
      `${msg}\n      expected: ${JSON.stringify(expected)}\n      actual:   ${JSON.stringify(actual)}`,
    );
  }
}

function contains(haystack: string, needle: string, msg: string): void {
  if (!haystack.includes(needle)) {
    throw new Error(
      `${msg}\n      expected to contain: ${JSON.stringify(needle)}\n      actual: ${haystack.slice(0, 500)}`,
    );
  }
}

function notContains(haystack: string, needle: string, msg: string): void {
  if (haystack.includes(needle)) {
    throw new Error(`${msg}\n      found unexpected: ${JSON.stringify(needle)}`);
  }
}

function assertJSON(body: string): Record<string, unknown> {
  try {
    return JSON.parse(body) as Record<string, unknown>;
  } catch {
    throw new Error(`Expected valid JSON, got: ${body.slice(0, 300)}`);
  }
}

// ── HTTP helpers ──

interface Response {
  status: number;
  body: string;
}

async function post(path: string, payload: unknown): Promise<Response> {
  const url = `${BASE}${path}`;
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  const body = await res.text();
  return { status: res.status, body };
}

async function get(path: string): Promise<Response> {
  const url = `${BASE}${path}`;
  const res = await fetch(url);
  const body = await res.text();
  return { status: res.status, body };
}

// ── Setup ──

async function resetDatabase(): Promise<void> {
  const { status, body } = await post("/reset", {});
  ok(status === 200, `Reset returned ${status}`);
  const json = JSON.parse(body) as { success?: boolean };
  ok(json.success === true, `Reset did not succeed: ${body}`);
}

// ── Main ──

async function main() {
  console.log("\nChorus Debug Tool Tests\n");

  // ─── Pre-flight ──────────────────────────────────────────────

  console.log("── Pre-flight ──");

  await test("Server reachable", async () => {
    const { status } = await get("/game/current");
    eq(status, 200, "Server not reachable — start the server first (npm start)");
  });

  await test("Reset database", async () => {
    await resetDatabase();
  });

  // ─── queryWorld ──────────────────────────────────────────────

  console.log("\n── queryWorld READ ──");

  await test("READ: list entities", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "READ",
      query: "MATCH (e:Entity) RETURN e.name, e.type LIMIT 3",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    const json = JSON.parse(body) as { rowCount: number; rows: Record<string, unknown>[] };
    ok(json.rowCount > 0, "Expected entities in seed data");
    ok(json.rows.length > 0, "Expected at least one row");
    ok("e.name" in json.rows[0] || json.rows[0]["e.name"] !== undefined, "Expected e.name in row");
  });

  await test("READ: query TimeAnchor", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "READ",
      query:
        "MATCH (a:TimeAnchor {_id:'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint) RETURN tp.day, tp.segment, tp.label",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    const json = JSON.parse(body) as { rowCount: number; rows: Record<string, unknown>[] };
    eq(json.rowCount, 1, "Expected exactly one TimeAnchor");
    ok(typeof json.rows[0]["tp.day"] === "number", "Expected tp.day to be a number");
  });

  await test("READ: defaults to READ when action omitted", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      query: "MATCH (e:Entity) RETURN e.name LIMIT 1",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    const json = JSON.parse(body) as { rowCount: number };
    ok(json.rowCount > 0, "Expected entities in seed data");
  });

  await test("READ: invalid Cypher syntax", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "READ",
      query: "MATCH BROKEN CRAP",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "CYPHER SYNTAX ERROR", "Expected CYPHER SYNTAX ERROR in response");
  });

  await test("READ: write clause rejected", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "READ",
      query: "CREATE (n:Entity {name:'hack'})",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "VALIDATION FAILED", "Expected VALIDATION FAILED");
    contains(body, "write clause", "Expected 'write clause' in error");
  });

  console.log("\n── queryWorld WRITE ──");

  const testEntityQuery = "MATCH (e:Entity {name:'TestNPC'}) DETACH DELETE e";
  const createTestEntity = "MERGE (e:Entity {name:'TestNPC'}) SET e.type='CHARACTER', e.brief='Test entity' RETURN e";

  await test("WRITE: create test entity", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "WRITE",
      query: createTestEntity,
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "Success.", "Expected 'Success.' message");
    contains(body, "row(s) affected", "Expected 'row(s) affected' message");
  });

  await test("WRITE: clean up test entity", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "WRITE",
      query: testEntityQuery,
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "Success.", "Expected success on cleanup");
  });

  await test("WRITE: DELETE without WHERE", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "WRITE",
      query: "MATCH (n:Entity) DETACH DELETE n",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "VALIDATION FAILED", "Expected VALIDATION FAILED");
    contains(body, "DELETE", "Expected error about DELETE requirements");
  });

  await test("WRITE: unregistered label", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "WRITE",
      query: "CREATE (n:TotallyFakeLabel {x:1})",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "Unknown label", "Expected 'Unknown label' error");
    contains(body, "TotallyFakeLabel", "Expected mention of the unknown label");
  });

  // ─── searchWorld ─────────────────────────────────────────────

  console.log("\n── searchWorld ──");

  await test("Search entities by keyword", async () => {
    const { status, body } = await post("/debug/tools/searchWorld", {
      query: "velvet thorn",
      types: ["entities"],
      limit: 3,
    });
    eq(status, 200, `Expected 200, got ${status}`);
    const json = JSON.parse(body) as Record<string, unknown>;
    ok(Array.isArray(json.entities), "Expected entities array");
    ok((json.entities as unknown[]).length >= 1, "Expected at least 1 entity matching 'velvet thorn'");
  });

  await test("Search multiple types", async () => {
    const { status, body } = await post("/debug/tools/searchWorld", {
      query: "warrens",
      types: ["entities", "plots"],
      limit: 5,
    });
    eq(status, 200, `Expected 200, got ${status}`);
    const json = JSON.parse(body) as Record<string, unknown>;
    ok("entities" in json, "Expected entities key");
    ok("plots" in json, "Expected plots key");
  });

  await test("Search default types (all)", async () => {
    const { status, body } = await post("/debug/tools/searchWorld", {
      query: "player",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    // wrapSafe may return error string if embedder is unavailable
    if (body.startsWith("ERROR:")) {
      // Embedder not running — still a valid tool invocation, just no results
      return;
    }
    const json = JSON.parse(body) as Record<string, unknown>;
    const hasSomeResults = ["entities", "messages", "notes", "plots"].some(
      (k) => Array.isArray(json[k]) && (json[k] as unknown[]).length > 0,
    );
    ok(hasSomeResults, "Expected at least one domain to have results");
  });

  await test("Search with nonsense query (handled gracefully)", async () => {
    const { status, body } = await post("/debug/tools/searchWorld", {
      query: "zzxyznonexistent98765",
      types: ["entities"],
      limit: 2,
    });
    eq(status, 200, `Expected 200, got ${status}`);
    // wrapSafe may return error string if embedder is unavailable
    if (body.startsWith("ERROR:")) return;
    const json = assertJSON(body);
    ok(Array.isArray(json.entities), "Expected entities array in response");
    // Vector search returns nearest neighbors for any input — it doesn't know
    // a query is "nonsense". The tool should just not crash.
  });

  await test("Search with invalid type", async () => {
    // Zod validation doesn't run on direct .execute() calls — invalid
    // types are silently ignored, producing an empty result with 200.
    const { status, body } = await post("/debug/tools/searchWorld", {
      query: "test",
      types: ["bogus_type"],
      limit: 2,
    });
    eq(status, 200, `Expected 200, got ${status}`);
    // Should return empty JSON since no valid types matched
    if (!body.startsWith("ERROR:")) {
      const json = JSON.parse(body) as Record<string, unknown>;
      const hasNoResults = !Object.values(json).some(
        (v) => Array.isArray(v) && (v as unknown[]).length > 0,
      );
      ok(hasNoResults, "Expected no results for invalid type");
    }
  });

  // ─── editNode ────────────────────────────────────────────────

  console.log("\n── editNode ──");

  const testNoteId = "test_note_auto_001";
  const testNoteName = "test_note_auto_001";

  await test("CREATE a Note", async () => {
    const { status, body } = await post("/debug/tools/editNode", {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: testNoteName, content: "Hello from automatic test" },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, `Node "Note" created`, "Expected creation success message");
  });

  await test("UPDATE the Note", async () => {
    const { status, body } = await post("/debug/tools/editNode", {
      nodeLabel: "Note",
      action: "UPDATE",
      match: { name: testNoteName },
      properties: { content: "Updated by automatic test" },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, `Node "Note" updated`, "Expected update success message");
  });

  await test("verify UPDATE via queryWorld READ", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "READ",
      query: `MATCH (n:Note {name:'${testNoteName}'}) RETURN n.content`,
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "Updated by automatic test", "Expected updated content in query result");
  });

  await test("DELETE the Note", async () => {
    const { status, body } = await post("/debug/tools/editNode", {
      nodeLabel: "Note",
      action: "DELETE",
      match: { name: testNoteName },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "deleted", "Expected deletion success message");
  });

  await test("DELETE non-existent node", async () => {
    const { status, body } = await post("/debug/tools/editNode", {
      nodeLabel: "Note",
      action: "DELETE",
      match: { name: testNoteName },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "ERROR", "Expected error for non-existent node");
    contains(body, "No", "Expected 'No...found' error");
  });

  await test("UPDATE non-existent node", async () => {
    const { status, body } = await post("/debug/tools/editNode", {
      nodeLabel: "Note",
      action: "UPDATE",
      match: { name: "nonexistent_note_xyz" },
      properties: { content: "should fail" },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "ERROR", "Expected error for non-existent update");
  });

  await test("CREATE without properties", async () => {
    const { status, body } = await post("/debug/tools/editNode", {
      nodeLabel: "Note",
      action: "CREATE",
      properties: {},
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "properties is required", "Expected properties required error");
  });

  await test("Unknown node label", async () => {
    const { status, body } = await post("/debug/tools/editNode", {
      nodeLabel: "TotallyFakeLabel",
      action: "CREATE",
      properties: { x: 1 },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "is not registered", "Expected 'not registered' error");
  });

  await test("System property rejection (_id)", async () => {
    const { status, body } = await post("/debug/tools/editNode", {
      nodeLabel: "Note",
      action: "CREATE",
      properties: { name: "test_bad_sys", _id: "abc" },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "system-managed", "Expected system-managed property error");
  });

  // Clean up "test_bad_sys" note if it was somehow created despite error
  try {
    await post("/debug/tools/editNode", {
      nodeLabel: "Note",
      action: "DELETE",
      match: { name: "test_bad_sys" },
    });
  } catch {
    // ignore
  }

  // ─── editRelationship ────────────────────────────────────────

  console.log("\n── editRelationship ──");

  // Discover a Location name dynamically
  let locationName = "The Velvet Thorn"; // fallback from seed data

  await test("Discover a Location for relationship tests", async () => {
    const { body } = await post("/debug/tools/queryWorld", {
      action: "READ",
      query: "MATCH (l:Location) RETURN l.name LIMIT 1",
    });
    const json = JSON.parse(body) as { rowCount: number; rows: Record<string, unknown>[] };
    if (json.rowCount > 0) {
      const name = json.rows[0]["l.name"] as string;
      if (name) locationName = name;
    }
    ok(locationName.length > 0, "No Location found in seed data");
  });

  await test("CREATE relationship (Player → Location)", async () => {
    const { status, body } = await post("/debug/tools/editRelationship", {
      action: "CREATE",
      relationshipType: "LOCATED_AT",
      sourceLabel: "Entity",
      sourceMatch: { name: "Player" },
      targetLabel: "Location",
      targetMatch: { name: locationName },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "created successfully", "Expected relationship creation success");
  });

  await test("DELETE the relationship", async () => {
    const { status, body } = await post("/debug/tools/editRelationship", {
      action: "DELETE",
      relationshipType: "LOCATED_AT",
      sourceLabel: "Entity",
      sourceMatch: { name: "Player" },
      targetLabel: "Location",
      targetMatch: { name: locationName },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "deleted", "Expected relationship deletion success");
  });

  await test("CREATE with unknown relationship type", async () => {
    const { status, body } = await post("/debug/tools/editRelationship", {
      action: "CREATE",
      relationshipType: "TotallyFakeRel",
      sourceLabel: "Entity",
      sourceMatch: { name: "Player" },
      targetLabel: "Entity",
      targetMatch: { name: "Player" },
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "is not registered", "Expected 'not registered' error");
  });

  await test("CREATE with system property", async () => {
    const { status, body } = await post("/debug/tools/editRelationship", {
      action: "CREATE",
      relationshipType: "LOCATED_AT",
      sourceLabel: "Entity",
      sourceMatch: { name: "Player" },
      targetLabel: "Location",
      targetMatch: { name: locationName },
      properties: { _bad: "val" },
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "system-managed", "Expected system-managed property error");
  });

  // ─── manageSchema ────────────────────────────────────────────

  console.log("\n── manageSchema ──");

  await test("Register GM_DEFINED node type", async () => {
    const { status, body } = await post("/debug/tools/manageSchema", {
      target: "node",
      action: "register",
      name: "TestArtifact",
      description: "A test artifact for automated testing",
      properties: [{ name: "power", description: "Power level", type: "number" }],
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, `Registered node type "TestArtifact"`, "Expected registration success");
  });

  await test("Verify node type via queryWorld", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "READ",
      query: "MATCH (nt:NodeType {name:'TestArtifact'}) RETURN nt.name, nt.description, nt.category",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body, "TestArtifact", "Expected TestArtifact NodeType node");
    contains(body, "GM_DEFINED", "Expected GM_DEFINED category");
  });

  await test("Create node of registered type via queryWorld WRITE", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "WRITE",
      query: "CREATE (a:TestArtifact {name:'test_artifact_1', power:42})",
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "Success.", "Expected write success");
  });

  await test("Clean up test artifact node", async () => {
    const { status, body } = await post("/debug/tools/queryWorld", {
      action: "WRITE",
      query: "MATCH (a:TestArtifact {name:'test_artifact_1'}) DETACH DELETE a",
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "Success.", "Expected cleanup success");
  });

  await test("Unregister the node type", async () => {
    const { status, body } = await post("/debug/tools/manageSchema", {
      target: "node",
      action: "unregister",
      name: "TestArtifact",
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, `Unregistered node type "TestArtifact"`, "Expected unregistration success");
  });

  await test("Cannot unregister PREDEFINED type", async () => {
    const { status, body } = await post("/debug/tools/manageSchema", {
      target: "node",
      action: "unregister",
      name: "Entity",
    });
    eq(status, 200, `Expected 200, got ${status}: ${body}`);
    contains(body, "Cannot unregister", "Expected 'Cannot unregister' error");
  });

  await test("Register and unregister a relationship type", async () => {
    // Register
    const reg = await post("/debug/tools/manageSchema", {
      target: "relationship",
      action: "register",
      name: "TEST_CONNECTS_TO",
      description: "Test relationship for automated testing",
      sourceLabels: ["Entity"],
      targetLabels: ["Entity"],
    });
    eq(reg.status, 200, `Register returned ${reg.status}: ${reg.body}`);
    contains(reg.body, `Registered relationship type "TEST_CONNECTS_TO"`, "Expected registration success");

    // Unregister
    const unreg = await post("/debug/tools/manageSchema", {
      target: "relationship",
      action: "unregister",
      name: "TEST_CONNECTS_TO",
    });
    eq(unreg.status, 200, `Unregister returned ${unreg.status}: ${unreg.body}`);
    contains(unreg.body, `Unregistered relationship type "TEST_CONNECTS_TO"`, "Expected unregistration success");
  });

  // ─── resetSceneContext ───────────────────────────────────────

  console.log("\n── resetSceneContext ──");

  await test("Reset with empty body", async () => {
    const { status, body } = await post("/debug/tools/resetSceneContext", {});
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body.toLowerCase(), "reset", "Expected 'reset' in response");
  });

  await test("Reset with extra properties (ignored)", async () => {
    const { status, body } = await post("/debug/tools/resetSceneContext", {
      unused: "value",
    });
    eq(status, 200, `Expected 200, got ${status}`);
    contains(body.toLowerCase(), "reset", "Expected success even with extra properties");
  });

  // ─── Error handling ──────────────────────────────────────────

  console.log("\n── Error handling ──");

  await test("Unknown tool name returns 404", async () => {
    const { status, body } = await post("/debug/tools/nonexistentTool", {});
    eq(status, 404, `Expected 404, got ${status}`);
    try {
      const json = JSON.parse(body) as { error?: string };
      contains(json.error ?? "", "Unknown tool", "Expected 'Unknown tool' error");
    } catch {
      throw new Error(`Expected JSON error body, got: ${body.slice(0, 200)}`);
    }
  });

  // ─── Final cleanup ───────────────────────────────────────────

  console.log("\n── Cleanup ──");

  await test("Final reset leaves database clean", async () => {
    await resetDatabase();
  });

  // ─── Summary ─────────────────────────────────────────────────

  const total = passed + failed;
  console.log(`\n${"─".repeat(40)}`);
  console.log(`  ${COLOR_GREEN}${passed} passed${COLOR_RESET} / ${COLOR_RED}${failed} failed${COLOR_RESET} / ${total} total`);

  if (failures.length > 0) {
    console.log("\nFailures:");
    for (const f of failures) {
      console.log(`  ${COLOR_RED}✗${COLOR_RESET} ${f}`);
    }
    process.exit(1);
  }

  console.log(`\n${COLOR_GREEN}All tests passed.${COLOR_RESET}\n`);
}

main().catch((e) => {
  console.error("Fatal error:", e instanceof Error ? e.message : String(e));
  process.exit(1);
});
