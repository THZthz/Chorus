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

import { createGenerateDialogueStepTool } from "@/server/llm/tools/generateDialogueStep";
import { exec } from "../helpers";

describe("Correction Workflow Scenario", () => {
  let tool: ReturnType<typeof createGenerateDialogueStepTool>["tool"];
  let wasValid: () => boolean;
  let resetForTurn: () => void;

  beforeEach(() => {
    const instance = createGenerateDialogueStepTool();
    tool = instance.tool;
    wasValid = instance.wasValid;
    resetForTurn = instance.resetForTurn;
    resetForTurn();
  });

  it("handles three corrections in sequence: invalid -> fix -> still invalid -> fix -> success", async () => {
    // Turn 1: First attempt fails — too few options
    const r1 = await exec(tool, {
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "The train carriage is dimly lit." }],
      options: [{ text: "Look around" }],
    });
    expect(r1).toContain("VALIDATION FAILED");
    expect(r1).toContain("Too few options");
    expect(wasValid()).toBe(false);

    // Turn 2: Correction attempt — adds an option but it also has check+hintBefore conflict
    const r2 = await exec(tool, {
      options: [
        {
          index: 1,
          text: "Force the door",
          check: { skill: "MIGHT", difficulty: 10, difficultyText: "Hard" },
          hintBefore: "[Might]",
        },
      ],
      isCorrection: true,
    });
    expect(r2).toContain("VALIDATION FAILED");
    expect(r2).toContain("check");
    expect(r2).toContain("hintBefore");

    // Turn 3: Fix the check+hintBefore conflict — remove hintBefore
    const r3 = await exec(tool, {
      isCorrection: true,
      options: [
        {
          index: 1,
          text: "Force the door with your shoulder",
          check: { skill: "MIGHT", difficulty: 10, difficultyText: "Hard" },
        },
      ],
    });
    expect(r3).toContain("Correction applied");
    expect(wasValid()).toBe(true);
  });

  it("correction with fresh messages replaces nothing when no index provided", async () => {
    // First call - invalid
    await exec(tool, {
      messages: [],
      options: [{ text: "A" }, { text: "B" }],
    });

    // Second call - sends all fresh messages (no index on any)
    const r2 = await exec(tool, {
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "A fresh start." }],
      options: [{ text: "Go left" }, { text: "Go right" }],
      isCorrection: true,
    });
    // When no items have index, it's treated as a full replacement
    expect(r2).toContain("Correction applied");
  });
});
