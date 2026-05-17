/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026  Amias
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

/**
 * Safe expression evaluator for skill check conditions.
 *
 * Uses a character whitelist + Function constructor with bound variables
 * to evaluate boolean expressions without eval() or arbitrary code execution.
 */

const EXPRESSION_ALLOWLIST = /^[a-zA-Z0-9_<>=!&|()+\-*/\s.0-9]+$/;

export interface ConditionContext {
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

export function evaluateConditions(
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
