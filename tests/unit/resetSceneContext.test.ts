import { resetSceneContext } from "@/server/llm/tools/resetSceneContext";
import { getObserver } from "@/server/llm/sceneObserver";
import { exec } from "../helpers";

describe("resetSceneContext", () => {
  beforeEach(() => {
    getObserver().markSeen("entity", "Player");
    getObserver().markSeen("entity", "Veyla");
    getObserver().markSeen("plot", "The Glass Cage");
  });

  it("resets the observer and returns success message", async () => {
    const result = await exec(resetSceneContext, {});
    expect(result).toContain("reset");
    expect(result).toContain("Scene context observer");
  });

  it("clears all seen entities and plots", async () => {
    await exec(resetSceneContext, {});
    expect(getObserver().isEmpty()).toBe(true);
  });

  it("can be called with extra properties (ignored)", async () => {
    const result = await exec(resetSceneContext, { extra: "ignored" } as any);
    expect(result).toContain("reset");
  });
});
