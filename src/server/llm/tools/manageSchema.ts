/**
 * Chorus — cinematic dialogue engine
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
import { NodeManager } from "@/server/memory/nodeManager";
import { wrapSafe } from "@/server/llm/tools/shared";
import { TOOL_NAMES } from "@/shared/constants";

export const manageSchema = tool({
  title: TOOL_NAMES.MANAGE_SCHEMA,
  description: `
Register or unregister node types and relationship types in the world schema.

Use this BEFORE creating a node with a NEW label (e.g., 'Artifact', 'Faction') or a
relationship with a NEW type (e.g., 'WORSHIPS', 'OWNS'). PREDEFINED types
(Entity, Plot, Note, NPCDisposition, etc.) are already registered — you don't need
to re-register them.

Node types — provide a description and optional property schema (name, description, type).
The schema is enforced: editNode will reject unknown property names for GM_DEFINED types.

Relationship types — provide a description and optional sourceLabels/targetLabels
to constrain which node types can sit at each endpoint.

Only GM_DEFINED types can be unregistered. PREDEFINED and INTERNAL types are permanent.
`.trim(),
  inputSchema: z.object({
    target: z
      .enum(["node", "relationship"])
      .describe("Whether to register a node type (label) or a relationship type."),
    action: z
      .enum(["register", "unregister"])
      .describe(
        "Register a new type or remove an existing one. Only GM-defined types can be unregistered.",
      ),
    name: z
      .string()
      .describe(
        "The name of the node label (e.g. 'Artifact') or relationship type (e.g. 'CONNECTED_TO'). Use PascalCase for node labels, UPPER_SNAKE for relationships.",
      ),
    description: z
      .string()
      .nullable()
      .optional()
      .describe(
        "For register: describes what the node type represents or what the relationship type means. For unregister: not needed.",
      ),
    properties: z
      .array(
        z.object({
          name: z.string().describe("Property name (snake_case, e.g. 'power_level')."),
          description: z.string().describe("What this property stores."),
          type: z
            .enum(["string", "number", "boolean", "json"])
            .nullable()
            .optional()
            .describe("The property's data type. Omit if uncertain."),
        }),
      )
      .nullable()
      .optional()
      .describe(
        "For target=node, action=register: the property schema for the new node type. For target=relationship: not needed.",
      ),
    sourceLabels: z
      .array(z.string())
      .nullable()
      .optional()
      .describe(
        "For target=relationship, action=register: which node labels can be the source (tail) of this relationship. E.g. ['Entity', 'Character'].",
      ),
    targetLabels: z
      .array(z.string())
      .nullable()
      .optional()
      .describe(
        "For target=relationship, action=register: which node labels can be the target (head) of this relationship. E.g. ['Location'].",
      ),
  }),
  execute: wrapSafe(async (args) => {
    if (args.action === "register") {
      if (args.target === "node") {
        const nodeManager = NodeManager.getCachedInstance();
        const existing = nodeManager.get(args.name);
        if (existing && existing.type !== "GM_DEFINED") {
          return `Cannot register "${args.name}": it is a ${existing.type} type and cannot be modified.`;
        }

        const props = (args.properties ?? []).filter((p) => !!p?.name);

        if (existing) {
          // Update existing GM_DEFINED type
          const updated = nodeManager.updateDefinition(args.name, {
            description: args.description ?? undefined,
            properties: props.length > 0 ? props : undefined,
          });
          if (!updated) return `Failed to update "${args.name}".`;
        } else {
          nodeManager.register(
            args.name,
            args.description ?? "No description provided.",
            props,
            "GM_DEFINED",
          );
        }

        const client = MemoryClient.getCachedInstance();
        await nodeManager.syncToNeo4j(client.neo4j);

        const propSummary =
          props.length > 0
            ? ` with ${props.length} property(s): ${props.map((p) => p.name).join(", ")}`
            : "";
        return `Registered node type "${args.name}"${propSummary}. It is now available for use via ${TOOL_NAMES.QUERY_WORLD} (WRITE action).`;
      }

      if (args.target === "relationship") {
        const manager = RelationshipManager.getCachedInstance();
        const existing = manager.get(args.name);
        if (existing && existing.type !== "GM_DEFINED") {
          return `Cannot register "${args.name}": it is a ${existing.type} type and cannot be modified.`;
        }

        const srcLabels = args.sourceLabels ?? undefined;
        const tgtLabels = args.targetLabels ?? undefined;

        if (existing) {
          const updated = manager.updateDefinition(args.name, {
            description: args.description ?? undefined,
            sourceLabels: srcLabels,
            targetLabels: tgtLabels,
          });
          if (!updated) return `Failed to update "${args.name}".`;
        } else {
          manager.register(
            args.name,
            args.description ?? "No description provided.",
            "GM_DEFINED",
            srcLabels,
            tgtLabels,
          );
        }

        const client = MemoryClient.getCachedInstance();
        await manager.syncToNeo4j(client.neo4j);

        const endpoints =
          srcLabels && tgtLabels ? ` (${srcLabels.join("|")})→(${tgtLabels.join("|")})` : "";
        return `Registered relationship type "${args.name}"${endpoints}. It is now available for use via ${TOOL_NAMES.QUERY_WORLD} (WRITE action).`;
      }
    }

    if (args.action === "unregister") {
      if (args.target === "node") {
        const nodeManager = NodeManager.getCachedInstance();
        const removed = nodeManager.unregister(args.name);
        if (!removed) {
          return `Cannot unregister "${args.name}": it is not a GM_DEFINED type.`;
        }
        const client = MemoryClient.getCachedInstance();
        // Remove the corresponding :NodeType node from Neo4j
        await client.neo4j.executeWrite(`MATCH (nt:NodeType {name: $name}) DETACH DELETE nt`, {
          name: args.name,
        });
        return `Unregistered node type "${args.name}".`;
      }

      if (args.target === "relationship") {
        const manager = RelationshipManager.getCachedInstance();
        const removed = manager.unregister(args.name);
        if (!removed) {
          return `Cannot unregister "${args.name}": it is not a GM_DEFINED type.`;
        }
        const client = MemoryClient.getCachedInstance();
        // Remove the corresponding :RelationshipType node from Neo4j
        await client.neo4j.executeWrite(
          `MATCH (rt:RelationshipType {name: $name}) DETACH DELETE rt`,
          { name: args.name },
        );
        return `Unregistered relationship type "${args.name}".`;
      }
    }

    return "Invalid action. Use 'register' or 'unregister'.";
  }, TOOL_NAMES.MANAGE_SCHEMA),
});
