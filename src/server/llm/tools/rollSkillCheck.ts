import { SKILL_NAMES } from "@/shared/constants";
import { MemoryClient } from "@/server/memory/client";
import { evaluateConditions } from "@/server/llm/conditionEvaluator";
import type { SkillName } from "@/shared/constants";

export interface SkillCheckParams {
  skill: SkillName;
  difficulty: number;
  difficultyText: string;
  diceCount: number;
  conditions?: Array<{
    expression: string;
    label?: string;
    color?: string;
    stepId?: string;
  }>;
}

export interface SkillCheckResult {
  skill: string;
  difficulty: number;
  dice: number[];
  total: number;
  statBonus: number;
  success: boolean;
  matchedConditions: Array<{
    expression: string;
    label?: string;
    color?: string;
    stepId?: string;
    matched: boolean;
  }>;
  narrativeSummary: string;
}

export async function performSkillCheck(
  args: SkillCheckParams,
): Promise<SkillCheckResult> {
  const client = MemoryClient.getCachedInstance();

  const statKey = args.skill.toLowerCase();
  let statBonus = 0;
  const playerStats = await client.longTerm.getPlayerStats("Player");
  if (playerStats && typeof playerStats[statKey] === "number") {
    statBonus = playerStats[statKey];
  }

  const dice = Array.from({ length: args.diceCount }, () =>
    Math.floor(Math.random() * 6 + 1),
  );
  const diceSum = dice.reduce((a, b) => a + b, 0);
  const total = diceSum + statBonus;
  const success = total >= args.difficulty;

  const matchedConditions = evaluateConditions(args.conditions ?? [], {
    success,
    total,
    difficulty: args.difficulty,
    statBonus,
  });

  const resultParts: string[] = [
    `Skill Check: ${args.skill} vs Difficulty ${args.difficulty}`,
    `Roll: ${args.diceCount}d6 = [${dice.join(", ")}] = ${diceSum} + ${args.skill}(${statBonus}) = ${total}`,
    `Result: ${success ? "SUCCESS" : "FAILURE"} (${total} vs ${args.difficulty})`,
  ];

  if (matchedConditions.length > 0) {
    const matched = matchedConditions.filter((c) => c.matched);
    if (matched.length > 0) {
      resultParts.push(
        `Matched conditions: ${matched.map((c) => c.label ?? c.expression).join(", ")}`,
      );
    }
    const unmatched = matchedConditions.filter((c) => !c.matched);
    if (unmatched.length > 0) {
      resultParts.push(
        `Unmatched conditions: ${unmatched.map((c) => c.label ?? c.expression).join(", ")}`,
      );
    }
  }

  return {
    skill: args.skill,
    difficulty: args.difficulty,
    dice,
    total,
    statBonus,
    success,
    matchedConditions,
    narrativeSummary: resultParts.join("\n"),
  };
}
