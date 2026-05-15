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

import neo4j, { isNode, isRelationship, Neo4jError } from "neo4j-driver";
import type { Driver } from "neo4j-driver";

// Properties whose key starts with "_" are internal/hidden and must never be
// exposed to the LLM (e.g. _embedding vectors, _elementId, _labels).
export function stripHiddenProperties(obj: any): any {
  if (obj === null || obj === undefined) return obj;
  if (Array.isArray(obj)) return obj.map((item) => stripHiddenProperties(item));
  if (typeof obj === "object") {
    const out: Record<string, unknown> = {};
    for (const [key, val] of Object.entries(obj as Record<string, unknown>)) {
      if (key.startsWith("_")) continue;
      out[key] = stripHiddenProperties(val);
    }
    return out;
  }
  return obj;
}

// Recursively convert BigInt values to Number — neo4j-driver 6.x returns integer properties as BigInt.
function toPlainValue(v: unknown): unknown {
  if (typeof v === "bigint") return Number(v);
  if (Array.isArray(v)) return v.map(toPlainValue);
  if (v && typeof v === "object") {
    // Neo4j temporal types (DateTime, LocalDateTime, Date, Time, LocalTime, Duration) have
    // BigInt components. Flatten them to ISO strings to avoid downstream BigInt mixing.
    if ("year" in (v as Record<string, unknown>) || "months" in (v as Record<string, unknown>)) {
      try {
        return (v as { toString(): string }).toString();
      } catch {
        // fall through to generic object handling
      }
    }
    // Neo4j Point2D/Point3D
    if ("srid" in (v as Record<string, unknown>) && "x" in (v as Record<string, unknown>)) {
      try {
        return (v as { toString(): string }).toString();
      } catch {
        // fall through
      }
    }
    // Generic object — recurse into its values
    const out: Record<string, unknown> = {};
    for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
      out[k] = toPlainValue(val);
    }
    return out;
  }
  return v;
}

// Unwrap Neo4j Node/Relationship objects from toObject() results — 6.x returns graph types, not plain maps.
function unwrapRecord(obj: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(obj)) {
    if (val && typeof val === "object") {
      if (isNode(val)) {
        out[key] = toPlainValue({
          ...val.properties,
          _elementId: val.elementId,
          _labels: val.labels,
        });
      } else if (isRelationship(val)) {
        out[key] = toPlainValue({
          ...val.properties,
          _elementId: val.elementId,
          _type: val.type,
          _startNodeElementId: val.startNodeElementId,
          _endNodeElementId: val.endNodeElementId,
        });
      } else {
        out[key] = toPlainValue(val);
      }
    } else {
      out[key] = typeof val === "bigint" ? Number(val) : val;
    }
  }
  return out;
}

function logQueryError(error: unknown, query: string, kind: "read" | "write"): void {
  if (error instanceof Neo4jError) {
    console.error(
      `[neo4j] execute${kind === "read" ? "Read" : "Write"} failed: ${error.code} - ${error.message}`,
      {
        code: error.code,
        retriable: error.retriable,
        query: query.length > 300 ? query.slice(0, 300) + "..." : query,
      },
    );
  } else {
    const msg = error instanceof Error ? error.message : String(error);
    console.error(
      `[neo4j] execute${kind === "read" ? "Read" : "Write"} failed (non-Neo4j): ${msg}`,
      { query: query.length > 300 ? query.slice(0, 300) + "..." : query },
    );
  }
}

export class Neo4jClient {
  private driver: Driver;

  constructor(
    uri: string = process.env.NEO4J_URI || "bolt://localhost:7687",
    user: string = process.env.NEO4J_USER || process.env.NEO4J_USERNAME || "neo4j",
    password: string = process.env.NEO4J_PASSWORD || "12345678",
  ) {
    this.driver = neo4j.driver(uri, neo4j.auth.basic(user, password), {
      maxTransactionRetryTime: 30_000,
      logging: {
        logger: (_level, message) => {
          console.error(`[neo4j-driver] ${message}`);
        },
      },
    });
  }

  async verifyConnectivity(): Promise<void> {
    await this.driver.verifyConnectivity();
  }

  async executeRead(
    query: string,
    parameters?: Record<string, unknown>,
  ): Promise<Record<string, unknown>[]> {
    const session = this.driver.session();
    try {
      const result = await session.executeRead((tx) => tx.run(query, parameters));
      return result.records.map((r) => unwrapRecord(r.toObject()));
    } catch (error) {
      logQueryError(error, query, "read");
      throw error;
    } finally {
      await session.close();
    }
  }

  async executeWrite(
    query: string,
    parameters?: Record<string, unknown>,
  ): Promise<Record<string, unknown>[]> {
    const session = this.driver.session();
    try {
      const result = await session.executeWrite((tx) => tx.run(query, parameters));
      return result.records.map((r) => unwrapRecord(r.toObject()));
    } catch (error) {
      logQueryError(error, query, "write");
      throw error;
    } finally {
      await session.close();
    }
  }

  /**
   * MERGE a relationship between two existing nodes, setting created_at.
   * Extra properties are set only ON CREATE.
   * The source/target nodes must already exist and be matchable by a single
   * key-value pair.
   */
  async mergeRelationship(
    sourceLabel: string,
    sourceKey: string,
    sourceValue: string,
    targetLabel: string,
    targetKey: string,
    targetValue: string,
    relType: string,
    options?: { onCreateProps?: Record<string, unknown> },
  ): Promise<Record<string, unknown>[]> {
    const { onCreateProps } = options || {};
    const safeType = relType.replace(/[^A-Za-z0-9_]/g, "_");
    const params: Record<string, unknown> = {
      srcVal: sourceValue,
      tgtVal: targetValue,
    };
    const onCreateSetters = ["r.created_at = datetime()"];
    if (onCreateProps) {
      for (const [key, val] of Object.entries(onCreateProps)) {
        const paramKey = `_onCreate_${key}`;
        params[paramKey] = val;
        onCreateSetters.push(`r.${key} = $${paramKey}`);
      }
    }
    return this.executeWrite(
      `MATCH (src:${sourceLabel} {${sourceKey}: $srcVal})
       MATCH (tgt:${targetLabel} {${targetKey}: $tgtVal})
       MERGE (src)-[r:${safeType}]->(tgt)
       ON CREATE SET ${onCreateSetters.join(", ")}
       RETURN r`,
      params,
    );
  }

  /**
   * CREATE a relationship between two existing nodes, setting created_at.
   * The source/target nodes must already exist and be matchable
   * by a single key-value pair.
   */
  async createRelationship(
    sourceLabel: string,
    sourceKey: string,
    sourceValue: string,
    targetLabel: string,
    targetKey: string,
    targetValue: string,
    relType: string,
  ): Promise<void> {
    const safeType = relType.replace(/[^A-Za-z0-9_]/g, "_");
    await this.executeWrite(
      `MATCH (src:${sourceLabel} {${sourceKey}: $srcVal})
       MATCH (tgt:${targetLabel} {${targetKey}: $tgtVal})
       CREATE (src)-[r:${safeType}]->(tgt)
       SET r.created_at = datetime()`,
      {
        srcVal: sourceValue,
        tgtVal: targetValue,
      },
    );
  }

  async close(): Promise<void> {
    await this.driver.close();
  }
}
