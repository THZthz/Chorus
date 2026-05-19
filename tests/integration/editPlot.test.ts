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

import { editPlot } from "@/server/llm/tools/editPlot";
import { queryWorld } from "@/server/llm/tools/queryWorld";
import { exec, parseToolOutput, resetDb } from "../helpers";

describe("editPlot", () => {
  const TEST_PLOT = "test_plot_editPlot";
  const TEST_CHILD = "test_plot_child";

  beforeAll(async () => {
    await resetDb();
  });

  afterEach(async () => {
    try {
      await exec(editPlot, { plotName: TEST_PLOT, action: "DELETE" });
    } catch {
      // Ignore
    }
    try {
      await exec(editPlot, { plotName: TEST_CHILD, action: "DELETE" });
    } catch {
      // Ignore
    }
  });

  it("CREATEs a plot", async () => {
    const result = await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "A test plot for integration testing",
    });
    expect(result).toContain("created");

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'}) RETURN p.name, p.description, p.status`,
    });
    const data = parseToolOutput(verify);
    expect(data.rowCount).toBe(1);
    const row = data.rows[0] as Record<string, unknown>;
    expect(row["p.status"]).toBe("PENDING");
  });

  it("rejects CREATE without description", async () => {
    const result = await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
    });
    expect(result).toContain('"description" is required');
  });

  it("UPDATEs description, brief, and status", async () => {
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "Original description",
    });

    const result = await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      description: "Updated description",
      brief: "Updated brief",
      status: "ACTIVE",
    });
    expect(result).toContain("updated");

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'}) RETURN p.description, p.brief, p.status`,
    });
    const data = parseToolOutput(verify);
    const row = data.rows[0] as Record<string, unknown>;
    expect(row["p.description"]).toBe("Updated description");
    expect(row["p.brief"]).toBe("Updated brief");
    expect(row["p.status"]).toBe("ACTIVE");
  });

  it("UPDATEs with setFlag", async () => {
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "Plot with flags",
    });

    const result = await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      setFlag: { flagId: "alarm_raised", description: "The alarm was triggered" },
    });
    expect(result).toContain("updated");
    expect(result).toContain('flag "alarm_raised"');

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'}) RETURN p.flags`,
    });
    const data = parseToolOutput(verify);
    const row = data.rows[0] as Record<string, unknown>;
    const flags = JSON.parse(row["p.flags"] as string) as Array<Record<string, unknown>>;
    expect(flags).toHaveLength(1);
    expect(flags[0].flagId).toBe("alarm_raised");
  });

  it("UPDATEs with removeFlag", async () => {
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "Plot with flags",
    });
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      setFlag: { flagId: "temp_flag", description: "Will be removed" },
    });

    const result = await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      removeFlag: "temp_flag",
    });
    expect(result).toContain("updated");
    expect(result).toContain('flag "temp_flag" removed');

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'}) RETURN p.flags`,
    });
    const data = parseToolOutput(verify);
    const row = data.rows[0] as Record<string, unknown>;
    const flags = row["p.flags"];
    // Flags is null when empty (serialized null → Neo4j null)
    expect(flags).toBeNull();
  });

  it("UPDATEs with branchTo", async () => {
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "Parent plot",
    });
    await exec(editPlot, {
      plotName: TEST_CHILD,
      action: "CREATE",
      description: "Child plot",
    });

    const result = await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      branchTo: TEST_CHILD,
    });
    expect(result).toContain("updated");
    expect(result).toContain(`branched to "${TEST_CHILD}"`);

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'})-[r:BRANCHES_TO]->(c:Plot {name: '${TEST_CHILD}'}) RETURN c.name`,
    });
    expect(parseToolOutput(verify).rowCount).toBe(1);
  });

  it("UPDATEs with unbranch", async () => {
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "Parent plot",
    });
    await exec(editPlot, {
      plotName: TEST_CHILD,
      action: "CREATE",
      description: "Child plot",
    });
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      branchTo: TEST_CHILD,
    });

    const result = await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      unbranch: TEST_CHILD,
    });
    expect(result).toContain("updated");
    expect(result).toContain(`unbranched "${TEST_CHILD}"`);

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'})-[r:BRANCHES_TO]->(c:Plot {name: '${TEST_CHILD}'}) RETURN c.name`,
    });
    expect(parseToolOutput(verify).rowCount).toBe(0);
  });

  it("auto-wires time relationships on PENDING → ACTIVE transition", async () => {
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "Plot for status transition",
    });

    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      status: "ACTIVE",
    });

    // Verify STARTED_AT and ACTIVE_AT relationships exist
    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'})-[r:STARTED_AT]->(tp:TimePoint) RETURN count(r) AS cnt`,
    });
    expect(parseToolOutput(verify).rows[0].cnt).toBeGreaterThanOrEqual(1);

    const verify2 = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'})-[r:ACTIVE_AT]->(tp:TimePoint) RETURN count(r) AS cnt`,
    });
    expect(parseToolOutput(verify2).rows[0].cnt).toBeGreaterThanOrEqual(1);
  });

  it("auto-wires time relationship on → COMPLETED transition", async () => {
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "Plot for completion",
    });

    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "UPDATE",
      status: "COMPLETED",
    });

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'})-[r:COMPLETED_AT]->(tp:TimePoint) RETURN count(r) AS cnt`,
    });
    expect(parseToolOutput(verify).rows[0].cnt).toBeGreaterThanOrEqual(1);
  });

  it("DELETEs a plot", async () => {
    await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "CREATE",
      description: "To be deleted",
    });

    const result = await exec(editPlot, {
      plotName: TEST_PLOT,
      action: "DELETE",
    });
    expect(result).toContain("deleted");

    const verify = await exec(queryWorld, {
      action: "READ",
      query: `MATCH (p:Plot {name: '${TEST_PLOT}'}) RETURN p`,
    });
    expect(parseToolOutput(verify).rowCount).toBe(0);
  });

  it("rejects DELETE on non-existent plot", async () => {
    const result = await exec(editPlot, {
      plotName: "nonexistent_plot_xyz_999",
      action: "DELETE",
    });
    expect(result).toContain("not found");
  });

  it("rejects UPDATE on non-existent plot", async () => {
    const result = await exec(editPlot, {
      plotName: "nonexistent_plot_xyz_999",
      action: "UPDATE",
      description: "nope",
    });
    expect(result).toContain("not found");
  });
});
