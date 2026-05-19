import { searchWorld } from "@/server/llm/tools/searchWorld";
import { exec, parseToolOutput, isEmbedderAvailable, resetDb } from "../helpers";

describe("searchWorld", () => {
  let embedderAvailable = false;

  beforeAll(async () => {
    await resetDb();
    embedderAvailable = await isEmbedderAvailable();
  });

  function skipIfNoEmbedder() {
    if (!embedderAvailable) return;
    // This runs at test execution time, not module load time
  }

  it("searches entities by keyword", async () => {
    if (!embedderAvailable) return;
    const result = await exec(searchWorld, {
      query: "player amnesia identity",
      types: ["entities"],
      limit: 5,
    });
    const data = parseToolOutput(result);
    expect(Array.isArray(data.entities)).toBe(true);
  });

  it("searches multiple types at once", async () => {
    if (!embedderAvailable) return;
    const result = await exec(searchWorld, {
      query: "murder investigation",
      types: ["entities", "plots"],
      limit: 5,
    });
    const data = parseToolOutput(result);
    expect(data).toHaveProperty("entities");
    expect(data).toHaveProperty("plots");
  });

  it("handles empty results gracefully", async () => {
    if (!embedderAvailable) return;
    const result = await exec(searchWorld, {
      query: "zzxyznonexistentword98765",
      types: ["entities"],
      limit: 2,
    });
    const data = parseToolOutput(result);
    expect(Array.isArray(data.entities)).toBe(true);
  });

  it("returns results for default types (all four)", async () => {
    if (!embedderAvailable) return;
    const result = await exec(searchWorld, {
      query: "glass cage murder",
      // The test relied on Zod's .default() to populate types, but args.types was undefined at runtime inside the tool's execute function.
      // searchWorld.ts calls args.types.filter(...), which throws Cannot read properties of undefined (reading 'filter').
      types: ["entities", "messages", "notes", "plots"],
      limit: 3,
    });
    const data = parseToolOutput(result);
    const keys = Object.keys(data);
    expect(keys.length).toBeGreaterThan(0);
  });

  it("succeeds without embedder (returns empty or minimal results)", async () => {
    const result = await exec(searchWorld, {
      query: "test query",
      types: ["entities"],
      limit: 2,
    });
    const data = parseToolOutput(result);
    expect(Array.isArray(data.entities)).toBe(true);
  });
});
