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
Use this BEFORE creating nodes with a new label or relationships with a new type in mutateWorld.

**Node types**: When registering, provide a description of what the node type represents and a property schema (name, description, optional type for each property). The LLM can query :NodeType nodes via queryWorld to discover available node types and their schemas.

**Relationship types**: When registering, provide a description of what the relationship type means. The LLM can query :RelationshipType nodes via queryWorld to discover available relationship types.
`.trim(),
  inputSchema: z.object({
    target: z
      .enum(["node", "relationship"])
      .describe("Whether to register a node type (label) or a relationship type."),
    action: z
      .enum(["register", "unregister"])
      .describe("Register a new type or remove an existing one. Only GM-defined types can be unregistered."),
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
        return `Registered node type "${args.name}"${propSummary}. It is now available for use in mutateWorld.`;
      }

      if (args.target === "relationship") {
        const manager = RelationshipManager.getCachedInstance();
        const existing = manager.get(args.name);
        if (existing && existing.type !== "GM_DEFINED") {
          return `Cannot register "${args.name}": it is a ${existing.type} type and cannot be modified.`;
        }

        if (existing) {
          if (args.description) {
            manager.updateDescription(args.name, args.description);
          }
        } else {
          manager.register(
            args.name,
            args.description ?? "No description provided.",
            "GM_DEFINED",
          );
        }

        const client = MemoryClient.getCachedInstance();
        await manager.syncToNeo4j(client.neo4j);

        return `Registered relationship type "${args.name}". It is now available for use in mutateWorld.`;
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
        await client.neo4j.executeWrite(
          `MATCH (nt:NodeType {name: $name}) DETACH DELETE nt`,
          { name: args.name },
        );
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
