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

  it("advances by segments and emits time update event", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      segments: 3,
      reason: "Short rest",
    });
    expect(result).toContain("Time advanced");
    expect(result).toContain("3 segment(s)");

    const timeEvent = mockEvents.events.find((e) => e.event === "time_update");
    expect(timeEvent).toBeDefined();
    expect((timeEvent!.data as { segmentsAdvanced: number }).segmentsAdvanced).toBe(3);
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

    const timeEvent = mockEvents.events.find((e) => e.event === "time_update");
    expect(timeEvent).toBeDefined();
    expect((timeEvent!.data as { segmentsAdvanced: number }).segmentsAdvanced).toBe(24); // 2 days * 12 segments
  });

  it("reports no change when both days and segments are zero or absent", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      segments: 0,
      reason: "Checking time",
    });
    expect(result).toContain("Time unchanged");
  });

  it("still works when reason is provided (reason is logged but not shown in tool output)", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      segments: 1,
      reason: "Walking to the next carriage",
    });
    expect(result).toContain("Time advanced");
    expect(result).toContain("1 segment(s)");
    expect(result).not.toContain("Walking to the next carriage");
  });

  it("does not break when reason is omitted", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, { segments: 1 });
    expect(result).toContain("Time advanced");
  });
});
