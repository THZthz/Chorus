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

import { tool } from "ai";
import { z } from "zod";
import { MemoryClient } from "@/server/memory/client";
import { RelationshipManager, RELATIONSHIP_PROPERTY_TAGS } from "@/server/relationshipManager";
import type { RelationshipPropertyDef } from "@/server/relationshipManager";
import { NODE_PROPERTY_TAGS, NodeManager } from "@/server/nodeManager";
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

Relationship types — provide a description and required sourceLabel/targetLabel
to constrain which node types can sit at each endpoint.

Only GM_DEFINED types can be unregistered. PREDEFINED and INTERNAL types are permanent.
`.trim(),
  inputSchema: z.object({
    target: z
      .enum(["NODE", "RELATIONSHIP"])
      .describe("Whether to register a node type (label) or a relationship type."),
    action: z
      .enum(["REGISTER", "UNREGISTER"])
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
          tags: z
            .array(z.enum(NODE_PROPERTY_TAGS))
            .describe(
              "Comma-separated tags describing the property. " +
                "For nodes: 'string', 'number', 'number[]', 'json', 'embedded', 'unique', 'index', 'composite_unique_1', 'composite_unique_2', 'composite_unique_3', 'composite_index_1', 'composite_index_2', 'composite_index_3'. " +
                "For relationships: same tags except 'unique' and 'composite_unique_X' (not supported by Neo4j for relationship properties).",
            ),
        }),
      )
      .nullable()
      .optional()
      .describe(
        "For action=register: the property schema for the new type (nodes or relationships).",
      ),
    sourceLabel: z
      .string()
      .optional()
      .describe(
        "The node label that sits at the source (tail) of this relationship. E.g. 'Entity'. Required for relationship registration.",
      ),
    targetLabel: z
      .string()
      .optional()
      .describe(
        "The node label that sits at the target (head) of this relationship. E.g. 'Location'. Required for relationship registration.",
      ),
  }),
  execute: wrapSafe(async (args) => {
    if (args.action === "REGISTER") {
      if (args.target === "NODE") {
        const nodeManager = NodeManager.getCachedInstance();
        const existing = nodeManager.get(args.name);
        if (existing && existing.type !== "GM_DEFINED") {
          return `ERROR: Cannot register "${args.name}": it is a ${existing.type} type and cannot be modified.`;
        }

        const props = (args.properties ?? []).filter((p) => !!p?.name);

        if (existing) {
          // Update existing GM_DEFINED type
          const updated = nodeManager.updateDefinition(args.name, {
            description: args.description ?? undefined,
            properties: props.length > 0 ? props : undefined,
          });
          if (!updated) return `ERROR: Failed to update "${args.name}".`;
        } else {
          nodeManager.register(
            args.name,
            args.description ?? "No description provided.",
            props,
            "GM_DEFINED",
          );
        }

        const client = MemoryClient.getCachedInstance();
        // TODO: This function will sync everything by default, incremental in future?
        await nodeManager.syncToNeo4j(client.neo4j);

        const propSummary =
          props.length > 0
            ? ` with ${props.length} property(s): ${props.map((p) => p.name).join(", ")}`
            : "";
        return `Registered node type "${args.name}"${propSummary}. It is now available for use via ${TOOL_NAMES.QUERY_WORLD} (WRITE action).`;
      }

      if (args.target === "RELATIONSHIP") {
        const manager = RelationshipManager.getCachedInstance();
        const srcLabel = args.sourceLabel;
        const tgtLabel = args.targetLabel;
        if (!srcLabel || !tgtLabel) {
          return `ERROR: sourceLabel and targetLabel are required for relationship registration.`;
        }

        const existing = manager.get(args.name, srcLabel, tgtLabel);
        if (existing && existing.type !== "GM_DEFINED") {
          return `Cannot register "${args.name}" (${srcLabel}→${tgtLabel}): it is a ${existing.type} type and cannot be modified.`;
        }

        const relProps: RelationshipPropertyDef[] = (args.properties ?? [])
          .filter((p) => !!p?.name)
          .map((p) => ({
            name: p.name,
            description: p.description,
            tags: p.tags.filter((t) =>
              (RELATIONSHIP_PROPERTY_TAGS as readonly string[]).includes(t),
            ) as RelationshipPropertyDef["tags"],
          }));

        if (existing) {
          const updated = manager.updateDefinition(args.name, srcLabel, tgtLabel, {
            description: args.description ?? undefined,
            properties: relProps.length > 0 ? relProps : undefined,
          });
          if (!updated) return `Failed to update "${args.name}".`;
        } else {
          manager.register(
            args.name,
            args.description ?? "No description provided.",
            "GM_DEFINED",
            srcLabel,
            tgtLabel,
            relProps,
          );
        }

        const client = MemoryClient.getCachedInstance();
        await manager.syncToNeo4j(client.neo4j);

        const endpoints = `(${srcLabel})→(${tgtLabel})`;
        const propSummary =
          relProps.length > 0
            ? ` with ${relProps.length} property(s): ${relProps.map((p) => p.name).join(", ")}`
            : "";
        return `Registered relationship type "${args.name}"${endpoints}${propSummary}. It is now available for use via ${TOOL_NAMES.QUERY_WORLD} (WRITE action).`;
      }
    }

    if (args.action === "UNREGISTER") {
      if (args.target === "NODE") {
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

      if (args.target === "RELATIONSHIP") {
        const manager = RelationshipManager.getCachedInstance();
        const srcLabel = args.sourceLabel;
        const tgtLabel = args.targetLabel;
        if (!srcLabel || !tgtLabel) {
          return `ERROR: sourceLabel and targetLabel are required for relationship unregistration.`;
        }
        const removed = manager.unregister(args.name, srcLabel, tgtLabel);
        if (!removed) {
          return `Cannot unregister "${args.name}" (${srcLabel}→${tgtLabel}): it is not a GM_DEFINED type.`;
        }
        const client = MemoryClient.getCachedInstance();
        // Remove the corresponding :RelationshipType node from Neo4j
        await client.neo4j.executeWrite(
          `MATCH (rt:RelationshipType {name: $name, source_label: $srcLabel, target_label: $tgtLabel}) DETACH DELETE rt`,
          { name: args.name, srcLabel, tgtLabel },
        );
        return `Unregistered relationship type "${args.name}" (${srcLabel}→${tgtLabel}).`;
      }
    }

    return "Invalid action. Use 'register' or 'unregister'.";
  }, TOOL_NAMES.MANAGE_SCHEMA),
});
