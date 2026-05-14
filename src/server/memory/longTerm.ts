/**
 * Chorus — cinematic RPG-style dialogue engine
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

import { v4 as uuidv4 } from "uuid";
import { int } from "neo4j-driver";
import { Neo4jClient } from "@/server/memory/neo4j";
import { Embedder, getEmbedder } from "@/server/memory/embedder";
import type {
  EntityType,
  MemoryEntity,
  NPCDisposition,
  PlayerCondition,
} from "@/server/memory/types";

// ── Helpers ──

// Convert a string to PascalCase, matching Python's to_pascal_case.
//  Handles snake_case and simple uppercase inputs.
//  e.g. "OBJECT" -> "Object", "snake_case" -> "SnakeCase"
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

// ── LongTermMemory ──

export class LongTermMemory {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  // ═══════════════════════════════════════════════════════════════
  // Entities
  // ═══════════════════════════════════════════════════════════════

  async addEntity(
    name: string,
    entityType: EntityType | string,
    options?: {
      subtype?: string;
      description?: string;
      brief?: string;
      aliases?: string[];
      metadata?: Record<string, unknown>;
      generateEmbedding?: boolean;
    },
  ): Promise<MemoryEntity> {
    const {
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

    const entityId = uuidv4();

    // Build dynamic labels: e.g. :Entity:Character
    const typeLabel = pascalCase(finalType);
    const subtypeLabel = finalSubtype ? pascalCase(finalSubtype) : null;

    let embedding: number[] | undefined;
    if (generateEmbedding) {
      embedding = await this.embedder.embed(name);
    }

    // Store aliases inside metadata (Python convention)
    const storageMetadata: Record<string, unknown> = { ...metadata };
    if (aliases && aliases.length > 0) {
      storageMetadata["aliases"] = aliases;
    }

    const rows = await this.client.executeWrite(
      `MERGE (e:Entity {name: $name})
       ON CREATE SET
         e.id = $id,
         e.created_at = datetime()
       SET
         e.type = $type,
         e.subtype = $subtype,
         e.brief = $brief,
         e.description = $description,
         e._embedding = $embedding,
         e.metadata = $metadata
       SET e:${typeLabel}
       ${subtypeLabel ? `SET e:${subtypeLabel}` : ""}
       RETURN e, e.id = $id AS isNew`,
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
    const persistedNode = result?.e as Record<string, unknown> | undefined;
    const persistedId = (persistedNode?.id as string) || entityId;
    const persistedCreatedAt = persistedNode?.created_at
      ? new Date(persistedNode.created_at as string | number)
      : new Date();

    return {
      id: persistedId,
      name,
      type: finalType as EntityType,
      subtype: finalSubtype,
      brief,
      description,
      aliases: aliases || [],
      metadata: metadata || {},
      _embedding: embedding,
      createdAt: persistedCreatedAt,
      isNew,
    };
  }

  async getEntity(name: string, type?: string): Promise<MemoryEntity | null> {
    let query = "MATCH (e:Entity {name: $name})";
    const params: Record<string, unknown> = { name };

    if (type) {
      query += " WHERE e.type = $type";
      params["type"] = type.toUpperCase();
    }

    query += " RETURN e LIMIT 1";

    const rows = await this.client.executeRead(query, params);
    if (rows.length === 0) return null;
    return this.parseEntity(rows[0].e as Record<string, unknown>);
  }

  async searchEntities(
    query: string,
    options?: {
      entityTypes?: string[];
      limit?: number;
      threshold?: number;
    },
  ): Promise<Array<MemoryEntity & { similarity: number }>> {
    const { entityTypes, limit = 10, threshold = 0.7 } = options || {};

    const queryEmbedding = await this.embedder.embed(query);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('entity_embedding_idx', $limit, $embedding)
       YIELD node AS e, score WHERE score >= $threshold
       RETURN e, score ORDER BY score DESC`,
      { embedding: queryEmbedding, limit: int(limit * 2), threshold },
    );

    const filterTypes = entityTypes ? new Set(entityTypes.map((t) => t.toUpperCase())) : null;

    const results: Array<MemoryEntity & { similarity: number }> = [];
    for (const row of rows) {
      const entity = this.parseEntity(row.e as Record<string, unknown>);
      if (filterTypes && !filterTypes.has(entity.type)) continue;
      if (results.length >= limit) break;
      results.push({ ...entity, similarity: row.score as number });
    }
    return results;
  }

  // ═══════════════════════════════════════════════════════════════
  // Relationships
  // ═══════════════════════════════════════════════════════════════

  async addRelationship(
    sourceName: string,
    targetName: string,
    relationshipType: string,
    options?: {
      description?: string;
      confidence?: number;
    },
  ): Promise<{ created: boolean }> {
    const { description, confidence = 1.0 } = options || {};
    const rows = await this.client.mergeRelationship(
      "Entity", "name", sourceName,
      "Entity", "name", targetName,
      relationshipType,
      { description, onCreateProps: { confidence } },
    );
    const created = rows.length > 0;
    return { created };
  }

  // ═══════════════════════════════════════════════════════════════
  // Player Conditions
  // ═══════════════════════════════════════════════════════════════

  async updatePlayerCondition(
    playerName: string,
    conditionId: string,
    condition: PlayerCondition | null,
  ): Promise<void> {
    const entity = await this.getEntity(playerName);
    if (!entity) throw new Error(`Player entity "${playerName}" not found`);

    const existingConditions = (entity.metadata.conditions as Record<string, unknown>) || {};
    if (condition === null) {
      delete existingConditions[conditionId];
    } else {
      existingConditions[conditionId] = condition;
    }

    // Preserve all existing metadata — addEntity replaces e.metadata entirely
    const fullMetadata: Record<string, unknown> = {
      ...entity.metadata,
      conditions: existingConditions,
    };
    await this.addEntity(playerName, entity.type, {
      subtype: entity.subtype,
      description: entity.description,
      brief: entity.brief,
      metadata: fullMetadata,
    });
  }

  async getPlayerStats(playerName: string = "Player"): Promise<Record<string, number> | null> {
    const entity = await this.getEntity(playerName);
    if (!entity?.metadata.stats) return null;
    return entity.metadata.stats as Record<string, number>;
  }

  // ═══════════════════════════════════════════════════════════════
  // Dispositions
  // ═══════════════════════════════════════════════════════════════

  async setDisposition(
    npcName: string,
    targetName: string,
    sentiment: string,
    summary: string,
  ): Promise<NPCDisposition> {
    const id = uuidv4();
    const now = new Date().toISOString();
    const rows = await this.client.executeWrite(
      `MATCH (npc:Entity {name: $npcName})
       MERGE (npc)-[r:HAS_DISPOSITION]->(d:NPCDisposition {npc_name: $npcName, target_name: $targetName})
       ON CREATE SET d.id = $id, d.created_at = datetime($now), r.description = $dispDesc, r.created_at = datetime()
       SET d.sentiment = $sentiment, d.summary = $summary, d.updated_at = datetime($now)
       RETURN d, d.id = $id AS isNew`,
      { npcName, targetName, sentiment, summary, id, now, dispDesc: null },
    );
    if (rows.length === 0) {
      throw new Error(`NPC entity "${npcName}" not found`);
    }
    return this.parseDisposition(rows[0].d as Record<string, unknown>);
  }

  async getDisposition(npcName: string, targetName: string): Promise<NPCDisposition | null> {
    const rows = await this.client.executeRead(
      `MATCH (d:NPCDisposition {npc_name: $npcName, target_name: $targetName})
       RETURN d LIMIT 1`,
      { npcName, targetName },
    );
    if (rows.length === 0) return null;
    return this.parseDisposition(rows[0].d as Record<string, unknown>);
  }

  async getDispositionsToward(targetName: string): Promise<NPCDisposition[]> {
    const rows = await this.client.executeRead(
      `MATCH (d:NPCDisposition {target_name: $targetName})
       RETURN d ORDER BY d.updated_at DESC`,
      { targetName },
    );
    return rows.map((r) => this.parseDisposition(r.d as Record<string, unknown>));
  }

  // ═══════════════════════════════════════════════════════════════
  // Parsers
  // ═══════════════════════════════════════════════════════════════

  private parseEntity(data: Record<string, unknown>): MemoryEntity {
    const meta =
      typeof data.metadata === "string"
        ? (JSON.parse(data.metadata) as Record<string, unknown>)
        : {};
    const aliases = (meta.aliases as string[]) || [];
    delete meta.aliases;
    return {
      id: data.id as string,
      name: data.name as string,
      type: data.type as EntityType,
      subtype: (data.subtype as string) || undefined,
      brief: (data.brief as string) || undefined,
      description: (data.description as string) || undefined,
      aliases,
      metadata: meta,
      _embedding: data._embedding as number[] | undefined,
      createdAt: new Date((data.created_at as string | number) || Date.now()),
    };
  }

  private parseDisposition(data: Record<string, unknown>): NPCDisposition {
    return {
      id: data.id as string,
      npcName: data.npc_name as string,
      targetName: data.target_name as string,
      sentiment: data.sentiment as string,
      summary: data.summary as string,
      createdAt: new Date((data.created_at as string | number) || Date.now()),
      updatedAt: new Date((data.updated_at as string | number) || Date.now()),
    };
  }
}
