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

import {
  type EntityType,
  MemoryClient,
  type MemoryEntity,
  type Disposition,
} from "@/server/memory/client";
import { RelationshipManager } from "@/server/relationshipManager";
import { NodeManager } from "@/server/nodeManager";
import { CypherValidator } from "@/server/memory/validation";
import { setInitialTime } from "@/server/models/time";
import { getActiveSeedStory } from "@/server/stories";
import { v4 as uuidv4 } from "uuid";
import { Neo4jClient } from "@/server/memory/neo4j.ts";
import { Embedder, getEmbedder } from "@/server/memory/embedder.ts";

// ── Helpers ──

/**
 * Convert a string to PascalCase, matching Python's to_pascal_case.
 * Handles snake_case and simple uppercase inputs. e.g. "OBJECT" -> "Object", "snake_case" -> "SnakeCase"
 * @param str
 */
function pascalCase(str: string): string {
  if (!str) return str;
  return str
    .split("_")
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
    .join("");
}

/**
 * Parse an entity type string that may include a subtype.
 * Matches Python's parse_entity_type: "TYPE:SUBTYPE" -> ("TYPE", "SUBTYPE")
 */
function parseEntityType(typeStr: string): { type: string; subtype: string | null } {
  if (typeStr.includes(":")) {
    const parts = typeStr.toUpperCase().split(":", 2);
    return { type: parts[0], subtype: parts[1] || null };
  }
  return { type: typeStr.toUpperCase(), subtype: null };
}

async function addEntity(
  name: string,
  entityType: EntityType | string,
  options?: {
    id?: string; // Only used to identify player, all other entities should not have this ID.
    subtype?: string;
    description?: string;
    brief?: string;
    aliases?: string[];
    metadata?: Record<string, unknown>;
    generateEmbedding?: boolean;
  },
): Promise<MemoryEntity> {
  const client = MemoryClient.getCachedInstance();

  const {
    id,
    subtype,
    description,
    brief,
    aliases,
    metadata,
    generateEmbedding = true,
  } = options || {};

  // Support "TYPE:SUBTYPE" in entityType (Python compat)
  const parsed = parseEntityType(String(entityType));
  const finalType = parsed.type;
  const finalSubtype = subtype || parsed.subtype || undefined;

  const entityId = id === "#player#" ? id : uuidv4();

  // Build dynamic labels: e.g. :Entity:Character
  const typeLabel = pascalCase(finalType);
  const subtypeLabel = finalSubtype ? pascalCase(finalSubtype) : null;

  let embedding: number[] | undefined;
  if (generateEmbedding) {
    const nodeManager = NodeManager.getCachedInstance();
    const embedText = nodeManager.getEmbeddingText("Entity", {
      name,
      type: finalType,
      description: description ?? "",
      brief: brief ?? "",
    });
    const embedder = getEmbedder();
    embedding = embedText ? await embedder.embed(embedText) : undefined;
  }

  // Store aliases inside metadata (Python convention)
  const storageMetadata: Record<string, unknown> = { ...metadata };
  if (aliases && aliases.length > 0) {
    storageMetadata["aliases"] = aliases;
  }

  const rows = await client.neo4j.executeWrite(
    `MERGE (e:Entity {name: $name})
       ON CREATE SET
         e._id = $id,
         e._created_at = datetime()
       SET
         e.type = $type,
         e.subtype = $subtype,
         e.brief = $brief,
         e.description = $description,
         e._embedding = $embedding,
         e.metadata = $metadata
       SET e:${typeLabel}
       ${subtypeLabel ? `SET e:${subtypeLabel}` : ""}
       RETURN e, e._id = $id AS isNew`,
    {
      id: entityId,
      name,
      type: finalType,
      subtype: finalSubtype || null,
      brief: brief || null,
      description: description || null,
      embedding: embedding || null,
      metadata: Object.keys(storageMetadata).length > 0 ? JSON.stringify(storageMetadata) : null,
    },
  );

  const result = rows[0];
  const isNew = (result?.isNew as boolean) || false;
  return {
    name,
    type: finalType as EntityType,
    subtype: finalSubtype,
    brief,
    description,
    aliases: aliases || [],
    metadata: metadata || {},
    _embedding: embedding,
    isNew,
  };
}

function parseDisposition(data: Record<string, unknown>): Disposition {
  return {
    npcName: data.source_name as string,
    targetName: data.target_name as string,
    sentiment: data.sentiment as string,
    summary: data.summary as string,
  };
}

async function setDisposition(
  npcName: string,
  targetName: string,
  sentiment: string,
  summary: string,
): Promise<Disposition> {
  const neo4j = MemoryClient.getCachedInstance().neo4j;
  const id = uuidv4();
  const now = new Date().toISOString();
  const rows = await neo4j.executeWrite(
    `MATCH (npc:Entity {name: $npcName})
       MERGE (npc)-[r:HAS_DISPOSITION]->(d:Disposition {source_name: $npcName, target_name: $targetName})
       ON CREATE SET d._id = $id, d._created_at = datetime($now), r._created_at = datetime()
       SET d.sentiment = $sentiment, d.summary = $summary, d._updated_at = datetime($now)
       RETURN d, d._id = $id AS isNew`,
    { npcName, targetName, sentiment, summary, id, now },
  );
  if (rows.length === 0) {
    throw new Error(`NPC entity "${npcName}" not found`);
  }
  return parseDisposition(rows[0].d as Record<string, unknown>);
}

export async function seedDatabase(): Promise<void> {
  const story = getActiveSeedStory();
  const client = await MemoryClient.getInstance();

  // Always sync INTERNAL + PREDEFINED relationship types to Neo4j on startup
  await RelationshipManager.getCachedInstance().syncToNeo4j(client.neo4j);

  // Sync INTERNAL + PREDEFINED node types to Neo4j on startup
  await NodeManager.getCachedInstance().syncToNeo4j(client.neo4j);

  // Audit: log warnings for any relationship types in the graph missing a :RelationshipType node
  const validator = new CypherValidator();
  await validator.auditRelationshipDescriptions(client.neo4j);

  // Skip if database already has data (prevents duplicate injection on restart)
  const existing = await client.neo4j.executeRead("MATCH (e:Entity) RETURN count(e) AS count");
  if ((existing[0]?.count as number) > 0) {
    console.log(`[seedDatabase] database already has ${existing[0].count} entities, skipping`);
    return;
  }

  await setInitialTime(story.initialDay, story.initialSegment);

  console.log(`[seedDatabase] seeding ${story.entities.length} entities from "${story.id}"`);

  for (const entity of story.entities) {
    const cleanMetadata = entity.metadata ? { ...entity.metadata } : {};
    await addEntity(entity.name, entity.type, {
      id: entity.id ? entity.id : undefined,
      subtype: entity.subtype,
      description: entity.description,
      brief: entity.brief,
      metadata: Object.keys(cleanMetadata).length > 0 ? cleanMetadata : undefined,
    });
  }

  // Register relationship types from seed story before creating instances
  if (story.relationshipTypes) {
    const manager = RelationshipManager.getCachedInstance();
    for (const rt of story.relationshipTypes) {
      manager.register(rt.name, rt.description, "GM_DEFINED", rt.sourceLabel, rt.targetLabel);
    }
    console.log(
      `[seedDatabase] registered ${story.relationshipTypes.length} relationship types from "${story.id}"`,
    );
    // Sync to Neo4j so seed story's custom types are discoverable via :RelationshipType nodes
    await manager.syncToNeo4j(client.neo4j);
  }

  for (const rel of story.relationships) {
    await client.neo4j.mergeRelationship(
      "Entity",
      "name",
      rel.sourceName,
      "Entity",
      "name",
      rel.targetName,
      rel.type,
      { onCreateProps: rel.description ? { description: rel.description } : null },
    );
  }

  // Seed initial NPC dispositions from story configuration
  let dispositionCount = 0;
  for (const disp of story.dispositions || []) {
    await setDisposition(disp.sourceName, disp.targetName, disp.sentiment, disp.summary);
    dispositionCount++;
  }

  // Seed plots from story
  for (const plot of story.plots || []) {
    await client.plots.createPlot(plot.name, {
      description: plot.description,
      brief: plot.brief,
      status: plot.status,
      triggerCondition: plot.triggerCondition,
      flags: plot.flags,
    });
  }

  // Seed plot branches
  for (const plot of story.plots || []) {
    if (plot.branchesTo) {
      for (const childName of plot.branchesTo) {
        await client.plots.branchTo(plot.name, childName);
      }
    }
  }

  // Seed notes
  let noteCount = 0;
  for (const note of story.notes || []) {
    await client.notes.createNote(note.name, note.content);
    if (note.aboutEntities) {
      for (const entityName of note.aboutEntities) {
        await client.notes.linkToEntity(note.name, entityName);
      }
    }
    if (note.aboutPlots) {
      for (const plotName of note.aboutPlots) {
        await client.notes.linkToPlot(note.name, plotName);
      }
    }
    noteCount++;
  }

  console.log(
    `[seedDatabase] done — ${story.entities.length} entities, ${story.relationships.length} relationships, ${dispositionCount} dispositions, ${noteCount} notes`,
  );
}
