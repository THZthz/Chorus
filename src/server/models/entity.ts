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

export interface EntityRef {
  name: string;
  type: string;
  description: string | null;
  brief: string | null;
  subtype?: string | null;
}

export function formatEntityCompact(e: EntityRef): string {
  const brief = e.brief || e.description?.slice(0, 120) || "";
  return `**${e.name}** (${e.type}) — ${brief}`;
}

export function formatEntityFull(e: EntityRef): string {
  if (!e.description) return "";
  return `### ${e.name}\n${e.description}`;
}

export function extractAliases(metadata: unknown): string[] {
  if (typeof metadata === "string") {
    try {
      return ((JSON.parse(metadata) as Record<string, unknown>).aliases as string[]) || [];
    } catch {
      return [];
    }
  }
  if (metadata && typeof metadata === "object") {
    return ((metadata as Record<string, unknown>).aliases as string[]) || [];
  }
  return [];
}

export function extractConditions(metadata: unknown): string[] {
  try {
    const m =
      typeof metadata === "string"
        ? (JSON.parse(metadata) as Record<string, unknown>)
        : (metadata as Record<string, unknown> | null);
    if (!m?.conditions) return [];
    return Object.entries(m.conditions as Record<string, Record<string, unknown>>).map(
      ([id, c]) => `${id}: ${c.description as string}`,
    );
  } catch {
    return [];
  }
}
