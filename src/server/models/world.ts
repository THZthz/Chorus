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
import {
  WorldEntity,
  WorldState,
  Character,
  Location,
  WorldObject,
  EntityType,
} from "@/types/entities";
import { getActiveSeedStory } from "@/server/seed-stories";

export function seedDatabase() {
  const count = db.prepare("SELECT COUNT(*) as count FROM entities").get() as { count: number };
  if (count.count === 0) {
    const story = getActiveSeedStory();

    const insert = db.prepare(
      "INSERT INTO entities (id, type, displayName, shortDescription, longDescription, attributes, stats, opinions) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    );
    const insertEntity = (entity: any) => {
      insert.run(
        entity.id,
        entity.type,
        entity.displayName,
        entity.shortDescription,
        entity.longDescription,
        JSON.stringify(entity.attributes || {}),
        entity.type === "CHARACTER" ? JSON.stringify(entity.stats || {}) : null,
        entity.type === "CHARACTER" ? JSON.stringify(entity.opinions || {}) : null,
      );
      console.log(`Inserted ${entity.displayName}.`);
    };

    Object.values(story.objects).forEach(insertEntity);
    Object.values(story.locations).forEach(insertEntity);
    Object.values(story.characters).forEach(insertEntity);

    const rp = story.rootPlot;
    db.prepare(
      `INSERT INTO plots (id, title, description, status, involved_locations, involved_characters, parent_plot_id, parent_option_id, child_plots, plot_flags)
       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
    ).run(
      rp.id,
      rp.title,
      rp.description,
      rp.status,
      JSON.stringify(rp.involvedLocations),
      JSON.stringify(rp.involvedCharacters),
      null,
      null,
      JSON.stringify(rp.childPlots),
      "{}",
    );
    console.log(`Seeded initial plot: ${rp.title}.`);

    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
      "game_time_day",
      String(story.initialTime.day),
    );
    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
      "game_time_segment",
      String(story.initialTime.segment),
    );

    db.prepare("INSERT OR REPLACE INTO system_state (key, value) VALUES (?, ?)").run(
      "current_scene",
      JSON.stringify(story.initialScene),
    );
  }
}

function rowToEntity(row: any): WorldEntity {
  return {
    id: row.id,
    type: row.type,
    displayName: row.displayName,
    shortDescription: row.shortDescription,
    longDescription: row.longDescription,
    attributes: JSON.parse(row.attributes),
    ...(row.type === "CHARACTER" && {
      stats: JSON.parse(row.stats ?? "{}"),
      opinions: JSON.parse(row.opinions ?? "{}"),
    }),
  } as WorldEntity;
}

export function getAllEntities(): WorldState {
  const rows = db.prepare("SELECT * FROM entities").all() as any[];
  const state: WorldState = { objects: {}, locations: {}, characters: {} };

  for (const entity of rows.map(rowToEntity)) {
    if (entity.type === "OBJECT") state.objects[entity.id] = entity as WorldObject;
    else if (entity.type === "LOCATION") state.locations[entity.id] = entity as Location;
    else if (entity.type === "CHARACTER") state.characters[entity.id] = entity as Character;
  }

  return state;
}

export function updateEntity(entity: Partial<WorldEntity> & { id: string }) {
  const existing = db.prepare("SELECT * FROM entities WHERE id = ?").get(entity.id) as any;
  if (!existing) return;

  const currentAttrs = JSON.parse(existing.attributes);
  const currentStats = existing.stats ? JSON.parse(existing.stats) : {};
  const currentOpinions = existing.opinions ? JSON.parse(existing.opinions) : {};

  // For update, we merge properties
  const newAttrs = entity.attributes ? { ...currentAttrs, ...entity.attributes } : currentAttrs;
  let newStats = currentStats;
  let newOpinions = currentOpinions;

  if (existing.type === "CHARACTER" && (entity as any).stats) {
    newStats = { ...currentStats, ...(entity as any).stats };
  }
  if (existing.type === "CHARACTER" && (entity as any).opinions) {
    newOpinions = { ...currentOpinions, ...(entity as any).opinions };
  }

  db.prepare(
    `
    UPDATE entities SET 
      displayName = COALESCE(?, displayName),
      shortDescription = COALESCE(?, shortDescription),
      longDescription = COALESCE(?, longDescription),
      attributes = ?,
      stats = ?,
      opinions = ?
    WHERE id = ?
  `,
  ).run(
    entity.displayName || null,
    entity.shortDescription || null,
    entity.longDescription || null,
    JSON.stringify(newAttrs),
    existing.type === "CHARACTER" ? JSON.stringify(newStats) : null,
    existing.type === "CHARACTER" ? JSON.stringify(newOpinions) : null,
    entity.id,
  );
  console.log(`Updated ${entity.displayName}.`);
}

export function upsertEntity(entity: WorldEntity) {
  db.prepare(
    `
    INSERT INTO entities (id, type, displayName, shortDescription, longDescription, attributes, stats, opinions)
    VALUES (?, ?, ?, ?, ?, ?, ?, ?)
    ON CONFLICT(id) DO UPDATE SET
      type = excluded.type,
      displayName = excluded.displayName,
      shortDescription = excluded.shortDescription,
      longDescription = excluded.longDescription,
      attributes = excluded.attributes,
      stats = excluded.stats,
      opinions = excluded.opinions
  `,
  ).run(
    entity.id,
    entity.type,
    entity.displayName,
    entity.shortDescription,
    entity.longDescription,
    JSON.stringify(entity.attributes || {}),
    entity.type === "CHARACTER" ? JSON.stringify((entity as any).stats || {}) : null,
    entity.type === "CHARACTER" ? JSON.stringify((entity as any).opinions || {}) : null,
  );
}

export function getAllEntitySummaries(
  typeFilter?: EntityType,
): { id: string; displayName: string; type: EntityType; shortDescription: string }[] {
  const rows = typeFilter
    ? (db
        .prepare("SELECT id, type, displayName, shortDescription FROM entities WHERE type = ?")
        .all(typeFilter) as any[])
    : (db.prepare("SELECT id, type, displayName, shortDescription FROM entities").all() as any[]);
  return rows.map((r) => ({
    id: r.id,
    displayName: r.displayName,
    type: r.type as EntityType,
    shortDescription: r.shortDescription,
  }));
}

export function getEntitiesByIds(ids: string[]): WorldEntity[] {
  if (ids.length === 0) return [];
  const placeholders = ids.map(() => "?").join(", ");
  const rows = db
    .prepare(`SELECT * FROM entities WHERE id IN (${placeholders})`)
    .all(...ids) as any[];
  const entityMap = new Map(rows.map((row) => [row.id, row]));
  return ids.filter((id) => entityMap.has(id)).map((id) => rowToEntity(entityMap.get(id)!));
}

export function getEntityById(id: string): WorldEntity | null {
  const row = db.prepare("SELECT * FROM entities WHERE id = ?").get(id) as any;
  return row ? rowToEntity(row) : null;
}

export function getEntitiesByText(query: string): WorldEntity[] {
  const lower = query.toLowerCase();
  const rows = db.prepare("SELECT * FROM entities").all() as any[];
  return rows
    .filter(
      (r) =>
        r.displayName.toLowerCase().includes(lower) ||
        r.shortDescription.toLowerCase().includes(lower),
    )
    .slice(0, 5)
    .map(rowToEntity);
}
