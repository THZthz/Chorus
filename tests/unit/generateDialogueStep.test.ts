import {
  validateDialogueArgs,
  createGenerateDialogueStepTool,
} from "@/server/llm/tools/generateDialogueStep";
import { exec } from "../helpers";

describe("generateDialogueStep validation", () => {
  it("rejects empty messages", () => {
    const result = validateDialogueArgs({
      messages: [],
      options: [{ text: "Option 1" }, { text: "Option 2" }],
    });
    expect(result.errors.some((e) => e.includes("No messages"))).toBe(true);
  });

  it("rejects null messages", () => {
    const result = validateDialogueArgs({
      options: [{ text: "A" }, { text: "B" }],
    });
    expect(result.errors.some((e) => e.includes("No messages"))).toBe(true);
  });

  it("rejects INNER_VOICE as a speaker name", () => {
    const result = validateDialogueArgs({
      messages: [{ speaker: "INNER_VOICE", type: "INNER_VOICE", text: "Wrong" }],
      options: [{ text: "A" }, { text: "B" }],
    });
    expect(result.errors.some((e) => e.includes("INNER_VOICE"))).toBe(true);
  });

  it("rejects non-skill speaker with INNER_VOICE type", () => {
    const result = validateDialogueArgs({
      messages: [{ speaker: "RandomName", type: "INNER_VOICE", text: "Hello" }],
      options: [{ text: "A" }, { text: "B" }],
    });
    expect(result.errors.some((e) => e.includes("not a valid skill name"))).toBe(true);
  });

  it("allows valid skill name as speaker for INNER_VOICE type", () => {
    const result = validateDialogueArgs({
      messages: [
        { speaker: "LOGIC", type: "INNER_VOICE", text: "This doesn't add up." },
        { speaker: "EMPATHY", type: "INNER_VOICE", text: "She's hiding something." },
      ],
      options: [{ text: "A" }, { text: "B" }],
    });
    expect(result.errors).toHaveLength(0);
  });

  it("rejects too few options (less than 2)", () => {
    const result = validateDialogueArgs({
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "Scene description." }],
      options: [{ text: "Only one" }],
    });
    expect(result.errors.some((e) => e.includes("Too few options"))).toBe(true);
  });

  it("rejects too many options (more than 5)", () => {
    const result = validateDialogueArgs({
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "Scene description." }],
      options: [
        { text: "1" }, { text: "2" }, { text: "3" },
        { text: "4" }, { text: "5" }, { text: "6" },
      ],
    });
    expect(result.errors.some((e) => e.includes("Too many options"))).toBe(true);
  });

  it("rejects option with both check and hintBefore", () => {
    const result = validateDialogueArgs({
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "Scene description." }],
      options: [
        {
          text: "Try to persuade",
          check: { skill: "RHETORIC", difficulty: 10, difficultyText: "Hard", diceCount: 2, conditions: [] },
          hintBefore: "[Rhetoric]",
        },
        { text: "Walk away" },
      ],
    });
    expect(
      result.errors.some((e) => e.includes("check") && e.includes("hintBefore")),
    ).toBe(true);
  });

  it("validates message text length", () => {
    const longText = "A".repeat(701);
    const result = validateDialogueArgs({
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: longText }],
      options: [{ text: "A" }, { text: "B" }],
    });
    expect(result.errors.some((e) => e.includes("too long"))).toBe(true);
  });

  it("accepts valid complete input", () => {
    const result = validateDialogueArgs({
      messages: [
        { speaker: "NARRATOR", type: "SYSTEM", text: "The Aethon Conveyor hums steadily." },
        { speaker: "LOGIC", type: "INNER_VOICE", text: "The ledger doesn't match the manifest." },
        { speaker: "Elias Crowne", type: "CHARACTER", text: "You've been asking questions." },
      ],
      options: [
        { text: "Show him the ledger", hintBefore: "[Logic]" },
        { text: "Deflect with a question about his whereabouts" },
        { text: "Appeal to his sense of duty" },
      ],
    });
    expect(result.errors).toHaveLength(0);
  });

  it("accepts an option with a complete skill check definition", () => {
    const result = validateDialogueArgs({
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "The door is locked." }],
      options: [
        {
          text: "[SORCERY] Sense magical wards",
          check: {
            skill: "SORCERY",
            difficulty: 12,
            difficultyText: "Challenging",
            diceCount: 2,
            conditions: [
              { expression: "success", label: "You sense the ward", color: "green" },
            ],
          },
        },
        { text: "Find another way around" },
      ],
    });
    expect(result.errors).toHaveLength(0);
  });

  it("accumulates multiple validation errors", () => {
    const result = validateDialogueArgs({
      messages: [
        { speaker: "INNER_VOICE", type: "INNER_VOICE", text: "Bad speaker name" },
      ],
      options: [{ text: "Only option" }],
    });
    expect(result.errors.length).toBeGreaterThanOrEqual(2);
  });
});

describe("generateDialogueStep correction flow", () => {
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

  it("marks state invalid after a failed call", async () => {
    const r1 = await exec(tool, {
      messages: [],
      options: [{ text: "A" }, { text: "B" }],
    });
    expect(r1).toContain("VALIDATION FAILED");
    expect(wasValid()).toBe(false);
  });

  it("marks state valid after a successful call", async () => {
    const r1 = await exec(tool, {
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "The train rumbles on." }],
      options: [{ text: "Investigate" }, { text: "Stay put" }],
    });
    expect(r1).toContain("Dialogue successfully streamed");
    expect(wasValid()).toBe(true);
  });

  it("handles correction that fixes invalid first call", async () => {
    // First call - invalid (too few options)
    const r1 = await exec(tool, {
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "Scene opens." }],
      options: [{ text: "Only one" }],
    });
    expect(r1).toContain("VALIDATION FAILED");

    // Second call - correction adds a second option
    const r2 = await exec(tool, {
      messages: undefined,
      options: [{ index: 1, text: "A second choice" }],
      isCorrection: true,
    });
    expect(r2).toContain("Correction applied");
    expect(wasValid()).toBe(true);
  });

  it("tells the LLM to retry fresh when correcting with no stored state", async () => {
    // Correction without a prior call - no state to merge against
    const r1 = await exec(tool, {
      messages: [{ speaker: "NARRATOR", type: "SYSTEM", text: "Direct correction attempt." }],
      options: [{ text: "A" }, { text: "B" }],
      isCorrection: true,
    });
    expect(r1).toContain("VALIDATION FAILED");
    expect(r1).toContain("isCorrection: true");
    expect(r1).toContain("No stored state");
  });
});
