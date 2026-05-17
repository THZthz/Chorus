import { createAdvanceTimeTool } from "@/server/llm/tools/advanceTime";
import { exec } from "../helpers";
import { createMockEventEmitter, resetDb } from "../helpers";

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
    expect(
      (timeEvent!.data as { segmentsAdvanced: number }).segmentsAdvanced,
    ).toBe(3);
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
    expect(
      (timeEvent!.data as { segmentsAdvanced: number }).segmentsAdvanced,
    ).toBe(24); // 2 days * 12 segments
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

  it("includes the reason in output when provided", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, {
      segments: 1,
      reason: "Walking to the next carriage",
    });
    expect(result).toContain("Walking to the next carriage");
  });

  it("does not break when reason is omitted", async () => {
    const mockEvents = createMockEventEmitter();
    const advanceTime = createAdvanceTimeTool(mockEvents);

    const result = await exec(advanceTime, { segments: 1 });
    expect(result).toContain("Time advanced");
  });
});
