import { searchWorld } from "@/server/llm/tools/searchWorld";
import { exec, parseToolOutput, isEmbedderAvailable, resetDb } from "../helpers";

describe("searchWorld", () => {
  let embedderAvailable = false;

  beforeAll(async () => {
    await resetDb();
    embedderAvailable = await isEmbedderAvailable();
  });

  it("searches entities by keyword", async () => {
    if (!embedderAvailable) return;
    const result = await exec(searchWorld, {
      query: "player amnesia identity",
      labels: ["Entity"],
      limit: 5,
    });
    const data = parseToolOutput(result);
    expect(Array.isArray(data.Entity)).toBe(true);
  });

  it("searches multiple types at once", async () => {
    if (!embedderAvailable) return;
    const result = await exec(searchWorld, {
      query: "murder investigation",
      labels: ["Entity", "Plot"],
      limit: 5,
    });
    const data = parseToolOutput(result);
    expect(data).toHaveProperty("Entity");
    expect(data).toHaveProperty("Plot");
  });

  it("handles empty results gracefully", async () => {
    if (!embedderAvailable) return;
    const result = await exec(searchWorld, {
      query: "zzxyznonexistentword98765",
      labels: ["Entity"],
      limit: 2,
    });
    const data = parseToolOutput(result);
    expect(Array.isArray(data.Entity)).toBe(true);
  });

  it("returns results for all searchable labels by default", async () => {
    if (!embedderAvailable) return;
    const result = await exec(searchWorld, {
      query: "glass cage murder",
      limit: 3,
    });
    const data = parseToolOutput(result);
    const keys = Object.keys(data);
    expect(keys.length).toBeGreaterThan(0);
  });

  it("succeeds without embedder (returns empty or minimal results)", async () => {
    const result = await exec(searchWorld, {
      query: "test query",
      labels: ["Entity"],
      limit: 2,
    });
    const data = parseToolOutput(result);
    expect(Array.isArray(data.Entity)).toBe(true);
  });
});
