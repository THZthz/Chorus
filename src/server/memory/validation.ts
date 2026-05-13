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

const READ_ALLOWED_LABELS = new Set(["Entity", "Message", "NPCDisposition", "GameTime", "TimePoint", "TimeAnchor"]);

const WRITE_ALLOWED_LABELS = new Set(["Entity", "Message", "NPCDisposition", "GameTime", "TimePoint", "TimeAnchor"]);

const ALLOWED_RELATIONSHIPS = new Set([
  "LOCATED_AT",
  "CARRIES",
  "ALLIED_WITH",
  "HOSTILE_TOWARDS",
  "LOCATED_IN",
  "HAS_DISPOSITION",
  "HAS_MESSAGE",
  "FIRST_MESSAGE",
  "NEXT_MESSAGE",
  "NEXT_TIMEPOINT",
  "CURRENT_TIMEPOINT",
  "AT_TIME",
  "STARTED_AT",
  "ACTIVE_AT",
  "COMPLETED_AT",
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
        "Query contains write clause (CREATE/MERGE/DELETE/SET/REMOVE/DETACH DELETE/DROP). queryWorld is read-only.",
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
      if (!ALLOWED_RELATIONSHIPS.has(relType)) {
        errors.push(
          `Query references forbidden relationship type \`[:${relType}]\`. Allowed: ${[...ALLOWED_RELATIONSHIPS].join(", ")}`,
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
  private extractRelationshipTypes(query: string): string[] {
    const cleaned = query.replace(/"[^"]*"/g, '""').replace(/'[^']*'/g, "''");
    const matches = cleaned.matchAll(/\[:([A-Z][A-Za-z0-9_]+)/g);
    return [...new Set([...matches].map((m) => m[1]))];
  }

  private hasQualifiedMatch(query: string): boolean {
    return /\bMATCH\b.*\bWHERE\b/i.test(query) || /\bMATCH\b[^}]*\{[^}]*:/i.test(query);
  }
}
