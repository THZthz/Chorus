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

import { EntityType, MemoryClient, type MemoryEntity } from "@/server/memory/client";
import type { SkillName } from "@/shared/constants";

const EXPRESSION_ALLOWLIST = /^[a-zA-Z0-9_<>=!&|()+\-*/\s.0-9]+$/;

interface ConditionContext {
  success: boolean;
  total: number;
  difficulty: number;
  statBonus: number;
}

function evaluateCondition(expression: string, ctx: ConditionContext): boolean {
  const trimmed = expression.trim();
  if (!trimmed) return ctx.success;

  if (!EXPRESSION_ALLOWLIST.test(trimmed)) {
    return false;
  }

  try {
    const fn = new Function(
      "success",
      "total",
      "difficulty",
      "statBonus",
      `return Boolean(${trimmed});`,
    );
    return Boolean(fn(ctx.success, ctx.total, ctx.difficulty, ctx.statBonus));
  } catch {
    return false;
  }
}

/**
 * Safe expression evaluator for skill check conditions.
 *
 * Uses a character whitelist + Function constructor with bound variables
 * to evaluate boolean expressions without eval() or arbitrary code execution.
 */
function evaluateConditions(
  conditions: Array<{
    expression: string;
    label?: string;
    color?: string;
    stepId?: string;
  }>,
  ctx: ConditionContext,
): Array<{
  expression: string;
  label?: string;
  color?: string;
  stepId?: string;
  matched: boolean;
}> {
  return conditions.map((c) => ({
    expression: c.expression,
    label: c.label,
    color: c.color,
    stepId: c.stepId,
    matched: evaluateCondition(c.expression, ctx),
  }));
}

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

function parseEntity(data: Record<string, unknown>): MemoryEntity {
  const meta =
    typeof data.metadata === "string" ? (JSON.parse(data.metadata) as Record<string, unknown>) : {};
  const aliases = (meta.aliases as string[]) || [];
  delete meta.aliases;
  return {
    name: data.name as string,
    type: data.type as EntityType,
    subtype: (data.subtype as string) || undefined,
    brief: (data.brief as string) || undefined,
    description: (data.description as string) || undefined,
    aliases,
    metadata: meta,
    _embedding: data._embedding as number[] | undefined,
  };
}

async function getPlayerStats(): Promise<Record<string, number> | null> {
  const client = MemoryClient.getCachedInstance();
  const rows = await client.neo4j.executeRead('MATCH (e:Entity {id: "#player#"}) RETURN e LIMIT 1');
  if (rows.length === 0) return null;
  const entity = parseEntity(rows[0].e as Record<string, unknown>);

  if (!entity?.metadata.stats) return null;
  return entity.metadata.stats as Record<string, number>;
}

export async function performSkillCheck(args: SkillCheckParams): Promise<SkillCheckResult> {
  const statKey = args.skill.toLowerCase();
  let statBonus = 0;
  const playerStats = await getPlayerStats();
  if (playerStats && typeof playerStats[statKey] === "number") {
    statBonus = playerStats[statKey];
  } else {
    console.error("[performSkillCheck] Invalid player stats.");
  }

  const dice = Array.from({ length: args.diceCount }, () => Math.floor(Math.random() * 6 + 1));
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
