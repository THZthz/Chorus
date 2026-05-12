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

import neo4j, { isNode, isRelationship } from "neo4j-driver";
import type { Driver } from "neo4j-driver";

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

export class Neo4jClient {
  private driver: Driver;

  constructor(
    uri: string = process.env.NEO4J_URI || "bolt://localhost:7687",
    user: string = process.env.NEO4J_USER || "neo4j",
    password: string = process.env.NEO4J_PASSWORD || "12345678",
  ) {
    this.driver = neo4j.driver(uri, neo4j.auth.basic(user, password));
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
    } finally {
      await session.close();
    }
  }

  async close(): Promise<void> {
    await this.driver.close();
  }
}
