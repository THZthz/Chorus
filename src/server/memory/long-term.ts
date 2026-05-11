import { v4 as uuidv4 } from "uuid";
import { Neo4jClient } from "./neo4j";
import { Embedder, getEmbedder } from "./embedder";
import type {
  EntityType,
  MemoryEntity,
  MemoryPreference,
  MemoryFact,
} from "./types";

// ── Helpers ──

/** Convert a string to PascalCase, matching Python's to_pascal_case.
 *  Handles snake_case and simple uppercase inputs.
 *  e.g. "OBJECT" -> "Object", "snake_case" -> "SnakeCase" */
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
function parseEntityType(
  typeStr: string,
): { type: string; subtype: string | null } {
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
      aliases?: string[];
      metadata?: Record<string, unknown>;
      generateEmbedding?: boolean;
    },
  ): Promise<MemoryEntity> {
    const {
      subtype,
      description,
      aliases,
      metadata,
      generateEmbedding = true,
    } = options || {};

    // Support "TYPE:SUBTYPE" in entityType (Python compat)
    const parsed = parseEntityType(String(entityType));
    const finalType = parsed.type;
    const finalSubtype = subtype || parsed.subtype || undefined;

    const entityId = uuidv4();

    // Build dynamic labels: e.g. :Entity:Person:Character
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
         e.description = $description,
         e.embedding = $embedding,
         e.metadata = $metadata
       SET e:${typeLabel}
       ${subtypeLabel ? `SET e:${subtypeLabel}` : ""}
       RETURN e, e.id = $id AS isNew`,
      {
        id: entityId,
        name,
        type: finalType,
        subtype: finalSubtype || null,
        description: description || null,
        embedding: embedding || null,
        metadata: Object.keys(storageMetadata).length > 0
          ? JSON.stringify(storageMetadata)
          : null,
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
      description,
      aliases: aliases || [],
      metadata: metadata || {},
      embedding,
      createdAt: persistedCreatedAt,
      isNew,
    };
  }

  async getEntity(
    name: string,
    type?: string,
  ): Promise<MemoryEntity | null> {
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
      { embedding: queryEmbedding, limit: limit * 2, threshold },
    );

    const filterTypes = entityTypes
      ? new Set(entityTypes.map((t) => t.toUpperCase()))
      : null;

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
  // Preferences
  // ═══════════════════════════════════════════════════════════════

  async addPreference(
    category: string,
    preference: string,
    options?: {
      context?: string;
      confidence?: number;
      metadata?: Record<string, unknown>;
      generateEmbedding?: boolean;
    },
  ): Promise<MemoryPreference> {
    const {
      context,
      confidence = 1.0,
      metadata,
      generateEmbedding = true,
    } = options || {};

    const prefId = uuidv4();

    let embedding: number[] | undefined;
    if (generateEmbedding) {
      const text = context
        ? `${category}: ${preference} (${context})`
        : `${category}: ${preference}`;
      embedding = await this.embedder.embed(text);
    }

    await this.client.executeWrite(
      `CREATE (p:Preference {
         id: $id,
         category: $category,
         preference: $preference,
         context: $context,
         confidence: $confidence,
         embedding: $embedding,
         metadata: $metadata,
         created_at: datetime(),
         valid_from: datetime()
       })`,
      {
        id: prefId,
        category,
        preference,
        context: context || null,
        confidence,
        embedding: embedding || null,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    return {
      id: prefId,
      category,
      preference,
      context,
      confidence,
      metadata: metadata || {},
      createdAt: new Date(),
    };
  }

  async getPreferences(
    category?: string,
    limit: number = 100,
  ): Promise<MemoryPreference[]> {
    const query = category
      ? `MATCH (p:Preference {category: $category}) RETURN p ORDER BY p.created_at DESC LIMIT $limit`
      : `MATCH (p:Preference) RETURN p ORDER BY p.created_at DESC LIMIT $limit`;
    const rows = await this.client.executeRead(query, {
      category: category || null,
      limit,
    });
    return rows.map((r) =>
      this.parsePreference(r.p as Record<string, unknown>),
    );
  }

  // ═══════════════════════════════════════════════════════════════
  // Facts
  // ═══════════════════════════════════════════════════════════════

  async addFact(
    subject: string,
    predicate: string,
    objectValue: string,
    options?: {
      confidence?: number;
      validFrom?: Date;
      validUntil?: Date;
      metadata?: Record<string, unknown>;
      generateEmbedding?: boolean;
    },
  ): Promise<MemoryFact> {
    const {
      confidence = 1.0,
      validFrom,
      validUntil,
      metadata,
      generateEmbedding = true,
    } = options || {};

    const factId = uuidv4();

    let embedding: number[] | undefined;
    if (generateEmbedding) {
      embedding = await this.embedder.embed(
        `${subject} ${predicate} ${objectValue}`,
      );
    }

    await this.client.executeWrite(
      `CREATE (f:Fact {
         id: $id,
         subject: $subject,
         predicate: $predicate,
         object: $object,
         confidence: $confidence,
         embedding: $embedding,
         valid_from: $validFrom,
         valid_until: $validUntil,
         metadata: $metadata,
         created_at: datetime()
       })`,
      {
        id: factId,
        subject,
        predicate,
        object: objectValue,
        confidence,
        embedding: embedding || null,
        validFrom: validFrom?.toISOString() || null,
        validUntil: validUntil?.toISOString() || null,
        metadata: metadata ? JSON.stringify(metadata) : null,
      },
    );

    return {
      id: factId,
      subject,
      predicate,
      object: objectValue,
      confidence,
      validFrom,
      validUntil,
      metadata: metadata || {},
      createdAt: new Date(),
    };
  }

  async getFacts(
    subject?: string,
    limit: number = 100,
  ): Promise<MemoryFact[]> {
    const query = subject
      ? `MATCH (f:Fact {subject: $subject}) RETURN f ORDER BY f.created_at DESC LIMIT $limit`
      : `MATCH (f:Fact) RETURN f ORDER BY f.created_at DESC LIMIT $limit`;
    const rows = await this.client.executeRead(query, {
      subject: subject || null,
      limit,
    });
    return rows.map((r) =>
      this.parseFact(r.f as Record<string, unknown>),
    );
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
    // Sanitize relationship type for use as a Cypher relationship type
    const safeType = relationshipType.replace(/[^A-Za-z0-9_]/g, "_");
    const rows = await this.client.executeWrite(
      `MATCH (a:Entity {name: $src}), (b:Entity {name: $tgt})
       MERGE (a)-[r:${safeType}]->(b)
       ON CREATE SET
         r.description = $desc,
         r.confidence = $conf,
         r.created_at = datetime()
       RETURN r, r.created_at IS NOT NULL AS isNew`,
      {
        src: sourceName,
        tgt: targetName,
        desc: description || null,
        conf: confidence,
      },
    );
    const created = rows.length > 0 && (rows[0]?.isNew as boolean || false);
    return { created };
  }

  // ═══════════════════════════════════════════════════════════════
  // Parsers
  // ═══════════════════════════════════════════════════════════════

  private parseEntity(data: Record<string, unknown>): MemoryEntity {
    const meta = typeof data.metadata === "string"
      ? (JSON.parse(data.metadata) as Record<string, unknown>)
      : {};
    const aliases = (meta.aliases as string[]) || [];
    delete meta.aliases;
    return {
      id: data.id as string,
      name: data.name as string,
      type: data.type as EntityType,
      subtype: (data.subtype as string) || undefined,
      description: (data.description as string) || undefined,
      aliases,
      metadata: meta,
      embedding: data.embedding as number[] | undefined,
      createdAt: new Date(
        (data.created_at as string | number) || Date.now(),
      ),
    };
  }

  private parsePreference(data: Record<string, unknown>): MemoryPreference {
    return {
      id: data.id as string,
      category: data.category as string,
      preference: data.preference as string,
      context: (data.context as string) || undefined,
      confidence: (data.confidence as number) || 1.0,
      metadata:
        typeof data.metadata === "string"
          ? (JSON.parse(data.metadata) as Record<string, unknown>)
          : {},
      createdAt: new Date(
        (data.created_at as string | number) || Date.now(),
      ),
    };
  }

  private parseFact(data: Record<string, unknown>): MemoryFact {
    return {
      id: data.id as string,
      subject: data.subject as string,
      predicate: data.predicate as string,
      object: data.object as string,
      confidence: (data.confidence as number) || 1.0,
      validFrom: data.valid_from
        ? new Date(data.valid_from as string)
        : undefined,
      validUntil: data.valid_until
        ? new Date(data.valid_until as string)
        : undefined,
      metadata:
        typeof data.metadata === "string"
          ? (JSON.parse(data.metadata) as Record<string, unknown>)
          : {},
      createdAt: new Date(
        (data.created_at as string | number) || Date.now(),
      ),
    };
  }
}
