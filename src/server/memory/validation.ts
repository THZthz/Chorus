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

import { TOOL_NAMES } from "@/shared/constants";
import { RelationshipManager } from "@/server/memory/relationshipManager";

const READ_ALLOWED_LABELS = new Set([
  "Entity",
  "Message",
  "NPCDisposition",
  "GameTime",
  "TimePoint",
  "TimeAnchor",
]);

const WRITE_ALLOWED_LABELS = new Set([
  "Entity",
  "Message",
  "NPCDisposition",
  "GameTime",
  "TimePoint",
  "TimeAnchor",
]);

const BLOCKED_READ_CLAUSES = /\b(CREATE|MERGE|DELETE|SET|REMOVE|DETACH\s+DELETE|DROP)\b/i;
const BLOCKED_DDL =
  /\b(CREATE\s+INDEX|CREATE\s+CONSTRAINT|DROP\s+(INDEX|CONSTRAINT|DATABASE|GRAPH)|ALTER|USING\s+PERIODIC\s+COMMIT)\b/i;
const UNBOUNDED_PATH = /\[(\*|\\*)\.\.\]|\[\\*\]/;

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

    for (const label of this.extractNodeLabels(query)) {
      if (!READ_ALLOWED_LABELS.has(label)) {
        errors.push(
          `Query references forbidden label \`:${label}\`. Allowed read labels: ${[...READ_ALLOWED_LABELS].join(", ")}`,
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

    for (const label of this.extractNodeLabels(query)) {
      if (!WRITE_ALLOWED_LABELS.has(label)) {
        errors.push(
          `Query references forbidden label \`:${label}\`. Allowed write labels: ${[...WRITE_ALLOWED_LABELS].join(", ")}`,
        );
      }
    }

    for (const relType of this.extractRelationshipTypes(query)) {
      const manager = RelationshipManager.getCachedInstance();
      // Auto-register unknown types as GM_DEFINED
      if (!manager.get(relType)) {
        manager.register(
          relType,
          "Created by GM via mutateWorld",
          "GM_DEFINED",
        );
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

    // Check: new relationships must include a description property
    for (const err of this.checkRelationshipDescriptions(query)) {
      errors.push(err);
    }

    return { valid: errors.length === 0, errors };
  }

  // Check that every CREATE/MERGE relationship includes a description property.
  // Limitation: inline property regex uses {[^}]*}, so a `}` inside a description
  // string value (e.g. {description: "a } b"}) won't be detected. Detection via
  // SET r.description scans the full query and works across clauses.
  private checkRelationshipDescriptions(query: string): string[] {
    const errors: string[] = [];

    // Quick bail: no relationship creation
    const hasRelCreation =
      /(?:CREATE|MERGE)\s.*-\[.*:\w+.*\]->/i.test(query) ||
      /(?:CREATE|MERGE)\s.*<-\[.*:\w+.*\]-/i.test(query);
    if (!hasRelCreation) return errors;

    // Match relationship creation: -[var:REL_TYPE {props}]-> or <-[var:REL_TYPE {props}]-
    const relPattern =
      /-\[(\w*)\s*:\s*(\w+)\s*(\{[^}]*\})?\s*\]\s*->|<-\[(\w*)\s*:\s*(\w+)\s*(\{[^}]*\})?\s*\]\s*-/g;

    let match: RegExpExecArray | null;
    while ((match = relPattern.exec(query)) !== null) {
      const relVar = match[1] || match[4];
      const relType = match[2] || match[5];
      const inlineProps = match[3] || match[6];

      const hasInlineDesc = inlineProps ? /\bdescription\s*:/.test(inlineProps) : false;

      let hasSetDesc = false;
      if (relVar) {
        hasSetDesc = new RegExp(`SET\\s+${relVar}\\.description\\s*=`, "i").test(query);
      }

      if (!hasInlineDesc && !hasSetDesc) {
        errors.push(
          `New relationship [:${relType}] must include a description property. Add {description: "why"} inline or SET ${relVar ? relVar : "r"}.description. (Note: inline detection can miss values containing "}" — use SET if your description includes braces.)`,
        );
        break;
      }
    }

    return errors;
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
  private extractRelationshipTypes(query: string): string[] {
    const cleaned = query.replace(/"[^"]*"/g, '""').replace(/'[^']*'/g, "''");
    const matches = cleaned.matchAll(/\[:([A-Z][A-Za-z0-9_]+)/g);
    return [...new Set([...matches].map((m) => m[1]))];
  }

  private hasQualifiedMatch(query: string): boolean {
    return /\bMATCH\b.*\bWHERE\b/i.test(query) || /\bMATCH\b[^}]*\{[^}]*:/i.test(query);
  }
}
