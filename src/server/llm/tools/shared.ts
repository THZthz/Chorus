/**
 * Elysian Dialogue — cinematic RPG-style dialogue engine
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

import type { DialogueOption } from "@/types/dialogue";
import type { WorldEntity, Character, Note } from "@/types/entities";
import type { Plot } from "@/types/plot";

// ── Markdown formatters ──

export function formatEntityMarkdown(entity: WorldEntity): string {
  const lines: string[] = [];
  lines.push(`## Entity: ${entity.displayName}`);
  lines.push("");
  lines.push(`**ID:** \`${entity.id}\` | **Type:** ${entity.type}`);
  lines.push(`**Short Description:** ${entity.shortDescription}`);
  lines.push("");
  lines.push(`### Long Description`);
  lines.push("");
  lines.push(entity.longDescription);
  lines.push("");

  if (Object.keys(entity.attributes).length > 0) {
    lines.push("### Attributes");
    lines.push("");
    lines.push("| Key | Value |");
    lines.push("|---|---|");
    for (const [k, v] of Object.entries(entity.attributes)) {
      lines.push(`| ${k} | ${String(v)} |`);
    }
    lines.push("");
  }

  if (entity.type === "CHARACTER") {
    const char = entity as Character;

    if (Object.keys(char.stats).length > 0) {
      lines.push("### Stats");
      lines.push("");
      lines.push("| Skill | Value |");
      lines.push("|---|---|");
      for (const [k, v] of Object.entries(char.stats)) {
        lines.push(`| ${k} | ${v} |`);
      }
      lines.push("");
    }

    if (Object.keys(char.opinions).length > 0) {
      lines.push("### Opinions");
      lines.push("");
      for (const [targetId, opinion] of Object.entries(char.opinions)) {
        lines.push(`- **${targetId}:** ${opinion}`);
      }
      lines.push("");
    }

    if (Object.keys(char.conditions).length > 0) {
      lines.push("### Conditions");
      lines.push("");
      lines.push("| Key | Value |");
      lines.push("|---|---|");
      for (const [k, v] of Object.entries(char.conditions)) {
        lines.push(`| ${k} | ${String(v)} |`);
      }
      lines.push("");
    }
  }

  return lines.join("\n").trim();
}

export function formatPlotMarkdown(plot: Plot): string {
  const lines: string[] = [];
  lines.push(`## Plot: ${plot.title}`);
  lines.push("");
  lines.push(`**ID:** \`${plot.id}\` | **Status:** ${plot.status}`);
  lines.push(`**Parent Plot:** ${plot.parentPlotId ? `\`${plot.parentPlotId}\`` : "None (root plot)"}`);
  lines.push("");
  lines.push(`### Description`);
  lines.push("");
  lines.push(plot.description);
  lines.push("");

  if (plot.involvedCharacters.length > 0) {
    lines.push("### Involved Characters");
    lines.push("");
    for (const c of plot.involvedCharacters) {
      lines.push(`- \`${c}\``);
    }
    lines.push("");
  }

  if (plot.involvedLocations.length > 0) {
    lines.push("### Involved Locations");
    lines.push("");
    for (const loc of plot.involvedLocations) {
      lines.push(`- \`${loc}\``);
    }
    lines.push("");
  }

  lines.push("### Child Plots");
  lines.push("");
  if (plot.childPlots.length > 0) {
    lines.push("| # | Plot ID | Trigger Condition |");
    lines.push("|---|---|---|");
    for (let i = 0; i < plot.childPlots.length; i++) {
      const cp = plot.childPlots[i];
      lines.push(`| ${i} | \`${cp.plotId ?? "—"}\` | ${cp.triggerCondition} |`);
    }
    lines.push("");
  } else {
    lines.push("*None*");
    lines.push("");
  }

  if (Object.keys(plot.flags).length > 0) {
    lines.push("### Flags");
    lines.push("");
    lines.push("| Key | Value |");
    lines.push("|---|---|");
    for (const [k, v] of Object.entries(plot.flags)) {
      lines.push(`| ${k} | ${String(v)} |`);
    }
    lines.push("");
  }

  return lines.join("\n").trim();
}

export function formatNoteMarkdown(note: Note): string {
  const lines: string[] = [];
  lines.push(`## Note: ${note.key}`);
  lines.push("");
  lines.push(`**ID:** \`${note.id}\``);
  lines.push(`**Value:** ${note.value}`);
  lines.push(`**Related Entities:** ${note.relatedEntityIds.length > 0 ? note.relatedEntityIds.map((id) => `\`${id}\``).join(", ") : "None"}`);
  lines.push(`**Related Plots:** ${note.relatedPlotIds.length > 0 ? note.relatedPlotIds.map((id) => `\`${id}\``).join(", ") : "None"}`);
  lines.push(`**Related Scene:** ${note.relatedScene ? "Yes" : "No"} | **Related Time:** ${note.relatedTime ? "Yes" : "No"}`);
  lines.push(`**Created:** ${note.createdAt} | **Updated:** ${note.updatedAt}`);
  return lines.join("\n").trim();
}

// ── Text verification ──

const disallowedRe =
  /[\p{Emoji}\p{Script=Han}\p{Script=Arabic}\p{Script=Cyrillic}\p{Script=Hiragana}\p{Script=Katakana}]/u;

function isAllowedText(str: string): boolean {
  if (!disallowedRe.test(str)) return true;
  // \p{Emoji} matches ASCII digits 0-9, #, * (emoji keycap bases) — allow them
  return /^[0-9#*]$/.test(str);
}

export function checkText(value: unknown, context: string): string | null {
  const str = typeof value === "string" ? value : JSON.stringify(value);
  const disallowed = [...str].filter((c) => !isAllowedText(c));
  if (disallowed.length !== 0) {
    const unique = [...new Set(disallowed)].slice(0, 10);
    return `TEXT VERIFICATION FAILED in ${context}: disallowed characters detected [${unique.join(",")}]. Only Latin-script text and typographic punctuation (no emoji, no non-Latin scripts) is allowed. Please retry with allowed content.`;
  }
  return null;
}

// ── Error-handling wrapper ──

export function wrapSafe<T>(
  fn: (args: T) => Promise<string>,
  toolName: string,
): (args: T) => Promise<string> {
  return async (args: T) => {
    const inputError = checkText(args, `${toolName} input`);
    if (inputError) return inputError;

    try {
      const result = await fn(args);
      const outputError = checkText(result, `${toolName} output`);
      if (outputError) return outputError;
      return result;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`[${toolName}] execute error:`, err);
      return `ERROR: Tool "${toolName}" failed unexpectedly: ${msg}. Please retry or use a different approach.`;
    }
  };
}

// ── Dialogue option mapping ──

export function mapToDialogueOption(
  o: Record<string, unknown>,
  i: number,
  baseId: string,
): DialogueOption {
  const optId = (o.id as string) || `opt_${baseId}_${i}`;
  const check = o.check as Record<string, unknown> | undefined;
  return {
    id: optId,
    text: (o.text as string) || "",
    selectionMessage: o.selectionMessage as string | undefined,
    hintBefore: o.hintBefore as string | undefined,
    hintAfter: o.hintAfter as string | undefined,
    check: check
      ? {
          skill: check.skill as string,
          difficulty: check.difficulty as number,
          difficultyText: (check.difficultyText as string) || "",
          diceCount: (check.diceCount as number) ?? 2,
          isRed: check.isRed as boolean | undefined,
          conditions: ((check.conditions as unknown[]) || []).map((c: unknown, ci: number) => {
            const cond = c as Record<string, unknown>;
            return {
              expression: cond.expression as string,
              label: cond.label as string | undefined,
              color: cond.color as string | undefined,
              stepId: (cond.stepId as string) || `step_${optId}_res_${ci}`,
            };
          }),
        }
      : undefined,
  };
}
