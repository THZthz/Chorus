import { tool } from "ai";
import { z } from "zod";
import { SKILL_NAMES } from "@/shared/constants";
import { MemoryClient } from "@/server/memory/client";
import { evaluateConditions } from "@/server/llm/conditionEvaluator";
import type { EventEmitter } from "@/server/llm/events";
import { wrapSafe } from "@/server/llm/tools/shared";

const inputSchema = z.object({
  skill: z.enum(SKILL_NAMES).describe("The skill to check (e.g. 'LOGIC')"),
  difficulty: z.number().describe("Numerical difficulty target (e.g. 10)"),
  diceCount: z.number().int().min(1).max(10).default(2).describe("Number of d6 dice to roll"),
  conditions: z
    .array(
      z.object({
        expression: z.string().describe("JS expression e.g. 'success' or 'total < difficulty'"),
        label: z.string().optional(),
        color: z.string().optional(),
        stepId: z.string().optional(),
      }),
    )
    .optional()
    .default([]),
  reason: z.string().optional().describe("Brief narrative context for this roll"),
});

export function createRollSkillCheckTool(events: EventEmitter) {
  return tool({
    description:
      "Roll dice for a skill check. Rolls diceCount d6, adds the player's relevant stat bonus, and determines success (total >= difficulty). Call this BEFORE narrating the outcome when a player's chosen action has a skill check.",
    inputSchema,
    execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
      const client = MemoryClient.getCachedInstance();

      // Look up player stats from Neo4j (keys are lowercase in TOML/metadata)
      const statKey = args.skill.toLowerCase();
      let statBonus = 0;
      const playerStats = await client.longTerm.getPlayerStats("Player");
      if (playerStats && typeof playerStats[statKey] === "number") {
        statBonus = playerStats[statKey];
      }

      // Roll dice
      const dice = Array.from({ length: args.diceCount }, () =>
        Math.floor(Math.random() * 6 + 1),
      );
      const diceSum = dice.reduce((a, b) => a + b, 0);
      const total = diceSum + statBonus;
      const success = total >= args.difficulty;

      // Evaluate conditions
      const matchedConditions = evaluateConditions(args.conditions ?? [], {
        success,
        total,
        difficulty: args.difficulty,
        statBonus,
      });

      // Emit SSE event for console rendering
      events.emitRollResult({
        skill: args.skill,
        difficulty: args.difficulty,
        dice,
        total,
        statBonus,
        success,
        matchedConditions,
      });

      // Persist ROLL message
      const rollText = [
        `Rolled ${args.diceCount}d6 + ${args.skill}(${statBonus})`,
        `Dice: [${dice.join(", ")}]`,
        `Total: ${total} vs Difficulty: ${args.difficulty}`,
        `Result: ${success ? "SUCCESS" : "FAILURE"}`,
      ].join(" | ");

      await client.shortTerm.addMessage("system", rollText, {
        speaker: args.skill,
        type: "ROLL",
        rollResult: {
          skill: args.skill,
          difficulty: args.difficulty,
          dice,
          total,
          success,
        },
      });

      // Build response for the LLM
      const parts: string[] = [
        `SKILL CHECK — ${args.skill} vs Difficulty ${args.difficulty}`,
        `Dice: [${dice.join(", ")}] = ${diceSum} + ${args.skill}(${statBonus}) = ${total} vs ${args.difficulty}`,
        `Result: ${success ? "SUCCESS" : "FAILURE"}`,
      ];

      if (args.reason) {
        parts.splice(1, 0, `Context: ${args.reason}`);
      }

      if (matchedConditions.length > 0) {
        const matched = matchedConditions.filter((c) => c.matched);
        if (matched.length > 0) {
          parts.push(
            `\nMatched conditions: ${matched.map((c) => c.label ?? c.expression).join(", ")}`,
          );
        } else {
          parts.push(`\nNo conditions matched. Default outcome: ${success ? "success" : "failure"}.`);
        }
        const unmatched = matchedConditions.filter((c) => !c.matched);
        if (unmatched.length > 0) {
          parts.push(
            `Unmatched conditions: ${unmatched.map((c) => c.label ?? c.expression).join(", ")}`,
          );
        }
      }

      parts.push(
        `\nNarrate this ${success ? "success" : "failure"} naturally via generateDialogueStep.${success ? " The player's skill shines through." : " Make the failure interesting but keep the story moving."}`,
      );

      return parts.join("\n");
    }, "rollSkillCheck"),
  });
}
