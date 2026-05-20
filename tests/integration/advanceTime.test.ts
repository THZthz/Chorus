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

import { createAdvanceTimeTool } from "@/server/llm/tools/advanceTime";
import { exec, createMockEventEmitter, resetDb } from "../helpers";

describe("advanceTime", () => {
  beforeEach(async () => {
    await resetDb();
  });

  it("advances by hours and emits time update event", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      hours: 6,
      reason: "Short rest",
    });
    expect(result).toContain("Time advanced");
    expect(result).toContain("6 hour(s)");

    const timeEvent = mockEvents.events.find((e) => e.event === "time_update");
    expect(timeEvent).toBeDefined();
    expect((timeEvent!.data as { hoursAdvanced: number }).hoursAdvanced).toBe(6);
  });

  it("advances by fractional hours (30 minutes)", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      hours: 0.5,
      reason: "Brief pause",
    });
    expect(result).toContain("Time advanced");
    expect(result).toContain("0.5 hours");

    const timeEvent = mockEvents.events.find((e: { event: string }) => e.event === "time_update");
    expect(timeEvent).toBeDefined();
    expect((timeEvent!.data as { hoursAdvanced: number }).hoursAdvanced).toBe(0.5);
  });

  it("advances by days", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      days: 2,
      reason: "Multi-day travel",
    });
    expect(result).toContain("2 day(s)");
    expect(result).toContain("Time advanced");

    const timeEvent = mockEvents.events.find((e: { event: string }) => e.event === "time_update");
    expect(timeEvent).toBeDefined();
    expect((timeEvent!.data as { hoursAdvanced: number }).hoursAdvanced).toBe(48); // 2 days * 24 hours
  });

  it("combines days and hours", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      days: 1,
      hours: 2.5,
      reason: "Travel + short rest",
    });
    expect(result).toContain("1 day(s)");
    expect(result).toContain("2.5 hours");

    const timeEvent = mockEvents.events.find((e) => e.event === "time_update");
    expect(timeEvent).toBeDefined();
    expect((timeEvent!.data as { hoursAdvanced: number }).hoursAdvanced).toBe(26.5); // 24 + 2.5
  });

  it("reports no change when both days and hours are zero or absent", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      hours: 0,
      reason: "Checking time",
    });
    expect(result).toContain("Time unchanged");
  });

  it("stores reason on NEXT_TIMEPOINT relationship", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    await exec(advanceTime, {
      hours: 1,
      reason: "Walking to the next carriage",
    });

    // Verify reason is stored on the NEXT_TIMEPOINT relationship
    const { MemoryClient } = await import("@/server/memory/client");
    const client = MemoryClient.getCachedInstance();
    const rows = await client.neo4j.executeRead(
      `MATCH (:TimePoint)-[r:NEXT_TIMEPOINT]->(:TimePoint) RETURN r.reason AS reason`,
    );
    expect(rows.length).toBeGreaterThan(0);
    expect(rows[0].reason).toBe("Walking to the next carriage");
  });

  it("still works when reason is provided (reason stored but not shown in tool output)", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      hours: 1,
      reason: "Walking to the next carriage",
    });
    expect(result).toContain("Time advanced");
    expect(result).toContain("1 hour(s)");
    expect(result).not.toContain("Walking to the next carriage");
  });

  it("does not break when reason is omitted", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, { hours: 1 });
    expect(result).toContain("Time advanced");
  });
});
