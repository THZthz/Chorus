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

import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { RelationshipManager } from "@/server/memory/relationshipManager";
import { wrapSafe } from "@/server/llm/tools/shared";
import { TOOL_NAMES } from "@/shared/constants";

const REL_ACTIONS = ["CREATE", "DELETE"] as const;

const inputSchema = z.object({
  action: z.enum(REL_ACTIONS).default("CREATE").describe("Action to perform."),
  relationshipType: z
    .string()
    .describe(
      "The relationship type (e.g. 'LOCATED_AT', 'ALLIED_WITH', 'HOSTILE_TOWARDS', or GM-defined). " +
        "Must be registered in the world schema and writable. " +
        `Query :RelationshipType nodes via ${TOOL_NAMES.QUERY_WORLD} to discover available types.`,
    ),
  sourceLabel: z
    .string()
    .describe("Label of the source node (e.g. 'Entity', 'Character', 'Location')."),
  sourceMatch: z
    .record(z.string(), z.string())
    .describe("Key-value pairs to locate the source node (e.g. { name: 'Tavern' } for an Entity)."),
  targetLabel: z.string().describe("Label of the target node."),
  targetMatch: z
    .record(z.string(), z.string())
    .describe("Key-value pairs to locate the target node (e.g. { name: 'Town Square' })."),
  properties: z
    .record(z.string(), z.unknown())
    .nullable()
    .optional()
    .describe(
      "Properties to set on the relationship (CREATE only). _created_at is auto-managed. " +
        "No _-prefixed keys allowed.",
    ),
});

export const editRelationship = tool({
  title: TOOL_NAMES.EDIT_RELATIONSHIP,
  description: `
CREATE or DELETE a relationship between two nodes in the world archive.

CREATE — Link two existing nodes. The relationship type must be registered (PREDEFINED or
GM_DEFINED). Creating the same relationship twice is safe (MERGE semantics).
Both endpoint nodes must already exist — if either is missing, the call fails.
Use for: moving entities (delete old LOCATED_AT, create new), transferring items
(delete old CARRIES, create new), setting alliances/hostilities, linking notes to entities.

DELETE — Remove a relationship. Use when entities move, items transfer, or
relationships change.
`.trim(),
  inputSchema,
  execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
    const client = MemoryClient.getCachedInstance();
    const relManager = RelationshipManager.getCachedInstance();

    // Validate relationship type
    const relDef = relManager.get(args.relationshipType);
    if (!relDef) {
      const available = relManager
        .getAll()
        .filter((r) => r.type !== "INTERNAL")
        .map((r) => r.name)
        .join(", ");
      return `ERROR: Relationship type "${args.relationshipType}" is not registered. Available types: ${available}`;
    }
    if (!relManager.isAllowedForWrite(args.relationshipType)) {
      return `ERROR: Relationship type "${args.relationshipType}" is ${relDef.type} and cannot be written to.`;
    }

    const srcEntries = Object.entries(args.sourceMatch);
    const tgtEntries = Object.entries(args.targetMatch);
    if (srcEntries.length === 0) return "ERROR: sourceMatch must not be empty.";
    if (tgtEntries.length === 0) return "ERROR: targetMatch must not be empty.";

    // Extract first key-value pair for the neo4j layer (covers common single-key lookups)
    const [srcKey, srcVal] = srcEntries[0];
    const [tgtKey, tgtVal] = tgtEntries[0];

    // ── CREATE ──
    if (args.action === "CREATE") {
      if (args.properties) {
        for (const key of Object.keys(args.properties)) {
          if (key.startsWith("_")) {
            return `ERROR: Property "${key}" is system-managed and cannot be set directly.`;
          }
        }
      }
      // Block _-prefixed match keys on endpoints
      for (const key of Object.keys(args.sourceMatch)) {
        if (key.startsWith("_")) return `ERROR: sourceMatch key "${key}" is internal.`;
      }
      for (const key of Object.keys(args.targetMatch)) {
        if (key.startsWith("_")) return `ERROR: targetMatch key "${key}" is internal.`;
      }

      const rows = await client.neo4j.mergeRelationship(
        args.sourceLabel,
        srcKey,
        srcVal,
        args.targetLabel,
        tgtKey,
        tgtVal,
        args.relationshipType,
        { onCreateProps: (args.properties ?? {}) as Record<string, unknown> },
      );

      if (rows.length === 0) {
        return (
          `ERROR: Could not create relationship. One or both endpoint nodes may not exist — ` +
          `source: (:\`${args.sourceLabel}\` ${JSON.stringify(args.sourceMatch)}), ` +
          `target: (:\`${args.targetLabel}\` ${JSON.stringify(args.targetMatch)}).`
        );
      }

      return `Relationship (:\`${args.sourceLabel}\`)-[:${args.relationshipType}]->(:\`${args.targetLabel}\`) created successfully.`;
    }

    // ── DELETE ──
    const deleted = await client.neo4j.deleteRelationship(
      args.sourceLabel,
      srcKey,
      srcVal,
      args.targetLabel,
      tgtKey,
      tgtVal,
      args.relationshipType,
    );

    return deleted
      ? `Relationship (:\`${args.sourceLabel}\`)-[:${args.relationshipType}]->(:\`${args.targetLabel}\`) deleted.`
      : `ERROR: Relationship not found — (:\`${args.sourceLabel}\` ${JSON.stringify(args.sourceMatch)})-[:${args.relationshipType}]->(:\`${args.targetLabel}\` ${JSON.stringify(args.targetMatch)}).`;
  }, TOOL_NAMES.EDIT_RELATIONSHIP),
});
