/**
 * Chorus — cinematic dialogue engine
 * Copyright (C) 2026 Amias 1289941679@qq.com
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

import { TOOL_NAMES } from "@/shared/constants";
import { RelationshipManager } from "@/server/memory/relationshipManager";
import { NodeManager } from "@/server/memory/nodeManager";
import type { Neo4jClient } from "@/server/memory/neo4j";

const BLOCKED_READ_CLAUSES = /\b(CREATE|MERGE|DELETE|SET|REMOVE|DETACH\s+DELETE|DROP)\b/i;
const BLOCKED_DDL =
  /\b(CREATE\s+INDEX|CREATE\s+CONSTRAINT|DROP\s+(INDEX|CONSTRAINT|DATABASE|GRAPH)|ALTER|USING\s+PERIODIC\s+COMMIT)\b/i;
const UNBOUNDED_PATH = /\[(\*|\\*)(\.\.\d*)?\]/;

export interface ValidationResult {
  valid: boolean;
  errors: string[];
}

export class CypherValidator {
  validateRead(query: string): ValidationResult {
    const errors: string[] = [];

    if (BLOCKED_READ_CLAUSES.test(query)) {
      errors.push(
        `Query contains write clause (CREATE/MERGE/DELETE/SET/REMOVE/DETACH DELETE/DROP). ${TOOL_NAMES.QUERY_WORLD} is read-only.`,
      );
    }

    if (BLOCKED_DDL.test(query)) {
      errors.push("Query contains DDL statement. Schema changes are not allowed through GM tools.");
    }

    if (UNBOUNDED_PATH.test(query)) {
      errors.push(
        "Query contains unbounded variable-length path (*). Use a fixed upper bound like [*1..3].",
      );
    }

    const nodeManager = NodeManager.getCachedInstance();
    for (const label of this.extractNodeLabels(query)) {
      if (!nodeManager.isAllowedForRead(label)) {
        const allowed = nodeManager
          .getAll()
          .filter((n) => n.type !== "INTERNAL")
          .map((n) => n.name);
        errors.push(
          `Query references forbidden label \`:${label}\`. Allowed read labels: ${allowed.join(", ")}`,
        );
      }
    }

    return { valid: errors.length === 0, errors };
  }

  validateWrite(query: string): ValidationResult {
    const errors: string[] = [];

    if (BLOCKED_DDL.test(query)) {
      errors.push("Query contains DDL statement. Schema changes are not allowed through GM tools.");
    }

    // Require specific MATCH before DELETE
    const hasDelete = /\bDELETE\b/i.test(query);
    const hasDetachDelete = /\bDETACH\s+DELETE\b/i.test(query);
    if ((hasDelete || hasDetachDelete) && !this.hasQualifiedMatch(query)) {
      errors.push(
        "DELETE/DETACH DELETE must be preceded by a specific MATCH with at least one property condition (WHERE clause or property specification).",
      );
    }

    const nodeManager = NodeManager.getCachedInstance();
    for (const label of this.extractNodeLabels(query)) {
      if (!nodeManager.isAllowedForWrite(label)) {
        if (!nodeManager.get(label)) {
          errors.push(
            `Unknown label \`:${label}\`. Use \`${TOOL_NAMES.MANAGE_SCHEMA}\` (target: "node", action: "register") to register it with a description and property schema first.`,
          );
        } else {
          const allowed = nodeManager
            .getAll()
            .filter((n) => nodeManager.isAllowedForWrite(n.name))
            .map((n) => n.name);
          errors.push(
            `Label \`:${label}\` is read-only or internal. Allowed write labels: ${allowed.join(", ")}`,
          );
        }
      }
    }

    for (const relType of this.extractRelationshipTypes(query)) {
      const manager = RelationshipManager.getCachedInstance();
      // Auto-register unknown types as GM_DEFINED
      if (!manager.get(relType)) {
        manager.register(relType, "Created by GM via ${TOOL_NAMES.QUERY_WORLD}", "GM_DEFINED");
      }
      if (!manager.isAllowedForWrite(relType)) {
        const allowed = [
          ...manager.getByType("PREDEFINED").map((r) => r.name),
          ...manager.getByType("GM_DEFINED").map((r) => r.name),
        ];
        errors.push(
          `Query references forbidden relationship type \`[:${relType}]\`. Allowed: ${allowed.join(", ")}`,
        );
      }
    }

    return { valid: errors.length === 0, errors };
  }

  // Extract node labels from a Cypher query (e.g. :Entity, :Message).
  // Only matches labels outside of square brackets to avoid conflating relationship types.
  private extractNodeLabels(query: string): string[] {
    const cleaned = query.replace(/"[^"]*"/g, '""').replace(/'[^']*'/g, "''");
    const outsideBrackets = cleaned.replace(/\[[^\]]*\]/g, "");
    const matches = outsideBrackets.matchAll(/:([A-Z][A-Za-z0-9_]*)/g);
    return [...new Set([...matches].map((m) => m[1]))];
  }

  // Extract relationship types from a Cypher query (e.g. [:LOCATED_AT], [:CARRIES]).
  extractRelationshipTypes(query: string): string[] {
    const cleaned = query.replace(/"[^"]*"/g, '""').replace(/'[^']*'/g, "''");
    const matches = cleaned.matchAll(/\[:([A-Z][A-Za-z0-9_]+)/g);
    return [...new Set([...matches].map((m) => m[1]))];
  }

  private hasQualifiedMatch(query: string): boolean {
    return /\bMATCH\b.*\bWHERE\b/i.test(query) || /\bMATCH\b[^}]*\{[^}]*:/i.test(query);
  }

  // Diagnostic tool: queries all relationship types in the database and checks
  // whether each has a corresponding :RelationshipType node. Logs warnings for
  // any types missing a :RelationshipType entry. Never blocks writes.
  async auditRelationshipDescriptions(client: Neo4jClient): Promise<void> {
    try {
      const types = await client.executeRead(
        "CALL db.relationshipTypes() YIELD relationshipType RETURN relationshipType",
      );
      for (const row of types) {
        const relType = row.relationshipType as string;
        const nodes = await client.executeRead(
          "MATCH (rt:RelationshipType {name: $name}) RETURN rt LIMIT 1",
          { name: relType },
        );
        if (nodes.length === 0) {
          console.error(
            `[CypherValidator] relationship type ":${relType}" exists in graph but has no :RelationshipType node`,
          );
        }
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      console.error(`[CypherValidator] auditRelationshipDescriptions failed: ${msg}`);
    }
  }
}
