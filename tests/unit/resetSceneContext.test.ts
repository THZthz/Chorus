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
