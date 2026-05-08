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

import db from "@/server/db";
import { nextId } from "@/server/models/ids";
import type { Fact } from "@/types/entities";

interface FactRow {
  id: string;
  key: string;
  value: string;
  related_entity_ids: string;
  related_plot_ids: string;
  related_scene: number;
  related_time: number;
  is_valid: number;
  created_at: string;
  updated_at: string;
}

function rowToFact(row: FactRow): Fact {
  return {
    id: row.id,
    key: row.key,
    value: row.value,
    relatedEntityIds: JSON.parse(row.related_entity_ids || "[]"),
    relatedPlotIds: JSON.parse(row.related_plot_ids || "[]"),
    relatedScene: row.related_scene === 1,
    relatedTime: row.related_time === 1,
    isValid: row.is_valid === 1,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  };
}

export function addFact(input: {
  key: string;
  value: string;
  relatedEntityIds?: string[];
  relatedPlotIds?: string[];
  relatedScene?: boolean;
  relatedTime?: boolean;
}): Fact {
  const id = `fact_${nextId()}`;
  const now = new Date().toISOString();
  db.prepare(
    `INSERT INTO facts (id, key, value, related_entity_ids, related_plot_ids, related_scene, related_time, is_valid, created_at, updated_at)
     VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)`,
  ).run(
    id,
    input.key,
    input.value,
    JSON.stringify(input.relatedEntityIds ?? []),
    JSON.stringify(input.relatedPlotIds ?? []),
    input.relatedScene ? 1 : 0,
    input.relatedTime ? 1 : 0,
    now,
    now,
  );
  return getFactById(id)!;
}

export function getFactById(id: string): Fact | undefined {
  const row = db.prepare("SELECT * FROM facts WHERE id = ?").get(id) as FactRow | undefined;
  return row ? rowToFact(row) : undefined;
}

export interface FactFilter {
  relatedEntityId?: string;
  relatedPlotId?: string;
  relatedScene?: boolean;
  relatedTime?: boolean;
  includeInvalid?: boolean;
}

export function getFacts(filter?: FactFilter): Fact[] {
  const conditions: string[] = [];
  const params: unknown[] = [];

  if (!filter?.includeInvalid) {
    conditions.push("is_valid = 1");
  }

  if (filter?.relatedEntityId) {
    conditions.push("related_entity_ids LIKE ?");
    params.push(`%"${filter.relatedEntityId}"%`);
  }

  if (filter?.relatedPlotId) {
    conditions.push("related_plot_ids LIKE ?");
    params.push(`%"${filter.relatedPlotId}"%`);
  }

  if (filter?.relatedScene !== undefined) {
    conditions.push("related_scene = ?");
    params.push(filter.relatedScene ? 1 : 0);
  }

  if (filter?.relatedTime !== undefined) {
    conditions.push("related_time = ?");
    params.push(filter.relatedTime ? 1 : 0);
  }

  const where = conditions.length > 0 ? `WHERE ${conditions.join(" AND ")}` : "";
  const rows = db
    .prepare(`SELECT * FROM facts ${where} ORDER BY created_at DESC`)
    .all(...params) as FactRow[];
  return rows.map(rowToFact);
}

export function getFactsByIds(ids: string[]): Fact[] {
  const placeholders = ids.map(() => "?").join(", ");
  const rows = db
    .prepare(
      `SELECT * FROM facts WHERE id IN (${placeholders}) AND is_valid = 1 ORDER BY created_at DESC`,
    )
    .all(...ids) as FactRow[];
  return rows.map(rowToFact);
}

export function updateFact(
  id: string,
  changes: {
    key?: string;
    value?: string;
    relatedEntityIds?: string[];
    relatedPlotIds?: string[];
    relatedScene?: boolean;
    relatedTime?: boolean;
  },
): { ok: true; fact: Fact } | { ok: false; error: string } {
  const existing = db.prepare("SELECT * FROM facts WHERE id = ? AND is_valid = 1").get(id) as
    | FactRow
    | undefined;
  if (!existing) {
    return { ok: false, error: `Fact '${id}' not found.` };
  }

  const setters: string[] = ["updated_at = CURRENT_TIMESTAMP"];
  const params: unknown[] = [];

  if (changes.key !== undefined) {
    setters.push("key = ?");
    params.push(changes.key);
  }
  if (changes.value !== undefined) {
    setters.push("value = ?");
    params.push(changes.value);
  }
  if (changes.relatedEntityIds !== undefined) {
    setters.push("related_entity_ids = ?");
    params.push(JSON.stringify(changes.relatedEntityIds));
  }
  if (changes.relatedPlotIds !== undefined) {
    setters.push("related_plot_ids = ?");
    params.push(JSON.stringify(changes.relatedPlotIds));
  }
  if (changes.relatedScene !== undefined) {
    setters.push("related_scene = ?");
    params.push(changes.relatedScene ? 1 : 0);
  }
  if (changes.relatedTime !== undefined) {
    setters.push("related_time = ?");
    params.push(changes.relatedTime ? 1 : 0);
  }

  params.push(id);
  db.prepare(`UPDATE facts SET ${setters.join(", ")} WHERE id = ?`).run(...params);
  return { ok: true, fact: getFactById(id)! };
}

export function removeFact(id: string): { ok: true } | { ok: false; error: string } {
  const existing = db.prepare("SELECT * FROM facts WHERE id = ? AND is_valid = 1").get(id) as
    | FactRow
    | undefined;
  if (!existing) {
    return { ok: false, error: `Fact '${id}' not found.` };
  }
  db.prepare("UPDATE facts SET is_valid = 0, updated_at = CURRENT_TIMESTAMP WHERE id = ?").run(id);
  return { ok: true };
}

export function getFactsSnapshot(): Fact[] {
  return getFacts();
}
