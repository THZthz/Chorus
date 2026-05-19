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
import { v4 as uuidv4 } from "uuid";
import { MemoryClient } from "@/server/memory/client";
import { NodeDef, NodeManager } from "@/server/memory/nodeManager";
import { extractInternalAndUnknownKeys, wrapSafe } from "@/server/llm/tools/shared";
import { getObserver } from "@/server/llm/sceneObserver";
import { getEmbedder } from "@/server/memory/embedder";
import { TOOL_NAMES } from "@/shared/constants";

const NODE_ACTIONS = ["CREATE", "UPDATE", "DELETE"] as const;

function visibleProps(node: Record<string, unknown> | undefined): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  if (!node) return out;
  for (const [k, v] of Object.entries(node)) {
    if (!k.startsWith("_")) out[k] = v;
  }
  return out;
}

const inputSchema = z.object({
  nodeLabel: z.string().describe(
    `
Node label to operate on (e.g. \`Entity\`, \`Character\`, \`Location\`, or a GM-defined label).
Must be registered in the world schema and writable.
Query \`NodeType\` nodes via ${TOOL_NAMES.QUERY_WORLD} to discover available types and their property schemas.
`.trim(), // TODO: Should use getContext.
  ),
  action: z.enum(NODE_ACTIONS).default("CREATE").describe("Action to perform."),
  match: z
    .record(z.string(), z.string())
    .nullable()
    .optional()
    .describe(
      `
Key-value pairs to locate exactly one node. Required for UPDATE/DELETE.
e.g. { name: 'Tavern' } for an Entity, or { npc_name: 'Guard', target_name: 'Player' } for an NPCDisposition.
`.trim(),
    ),
  properties: z
    .record(z.string(), z.unknown())
    .nullable()
    .optional()
    .describe(
      `
Key-value pairs to set on the node. Must match the property schema for this node type.
CREATE: sets initial properties. UPDATE: only include properties you want to change.
System properties (_id, _created_at, _updated_at, _embedding) are managed automatically.
`.trim(),
    ),
});

export const editNode = tool({
  title: TOOL_NAMES.EDIT_NODE,
  description: `
CREATE, UPDATE, or DELETE a **single** node in the world archive using a registered node type.

CREATE — Add a new entity, note, plot, or custom node type. Properties are validated
against the type's schema. WARNING: This tool does NOT check for duplicates — search first
via ${TOOL_NAMES.SEARCH_WORLD} or ${TOOL_NAMES.QUERY_WORLD} (READ) to verify the node doesn't
already exist. Use label \`Note\` to create your own scratchpad notes. Use label \`Plot\` to
create plot tree in advance of dialogue steps.

UPDATE — Change properties on an existing node. Only include fields you want to change.
Use for: entity descriptions, plot statuses/flags, note contents, dispositions. This also supports
partial JSON properties update although the property may actually stored as "string" in database
(when creating schema by \`${TOOL_NAMES.MANAGE_SCHEMA}\`, the type of that property MUST be
specified as "json").

DELETE — Remove a node and all its relationships (DETACH DELETE). Requires exact match criteria.
Verify you're targeting the right node.
`.trim(),
  inputSchema,
  execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
    const client = MemoryClient.getCachedInstance();
    const nodeManager = NodeManager.getCachedInstance();

    // Validate node label ever registered
    const nodeDef = nodeManager.get(args.nodeLabel);
    if (!nodeDef) {
      const available = nodeManager
        .getAll()
        .filter((n) => n.type !== "INTERNAL")
        .map((n) => n.name)
        .join(", ");
      return `ERROR: Node label "${args.nodeLabel}" is not registered. Available labels: ${available}.`;
    }
    if (!nodeManager.isAllowedForWrite(args.nodeLabel)) {
      // TODO: GM may try to use queryWorld, that should also block unauthorized writes.
      return `ERROR: Node label "${args.nodeLabel}" is ${nodeDef.type} and cannot be written to.`;
    }

    // Build allowed property names from the schema.
    // PREDEFINED and INTERNAL types accept any non-_-prefixed property.
    // GM_DEFINED types have explicit schemas and must be validated strictly.
    const schemaProps = new Set(
      nodeDef.properties.map((p) => p.name).filter((name) => !name.startsWith("_")),
    );
    const hasSchema = nodeDef.type === "GM_DEFINED";

    // Functions are defined inline to use cached variables.

    function isPropsKeyExistAndNotInternal(props: Record<string, unknown>): string | null {
      const {internalKeys, unknownKeys} = extractInternalAndUnknownKeys(schemaProps, hasSchema, props);
      const errorTextParts: string[] = [];
      if (internalKeys.length > 0)
        errorTextParts.push(
          `Property "${internalKeys.join("/")}" is internal (prefixed with '_') and cannot be set (managed internally by the engine).`,
        );
      if (unknownKeys.length > 0)
        errorTextParts.push(
          `Unknown property "${unknownKeys.join("/")}" for node type "${args.nodeLabel}". Allowed: ${[...schemaProps].join(", ")}`,
        );
      return errorTextParts.length > 0 ? errorTextParts.join(" ") : null;
    }

    function isMatchKeysInternal(match: Record<string, string>): string | null {
      const errorKeys: string[] = [];
      for (const key of Object.keys(match)) {
        if (key.startsWith("_")) errorKeys.push(key);
      }
      return errorKeys.length > 0
        ? `Parameter \`match\` contain invalid key "${errorKeys.join("/")}", which ${errorKeys.length > 1 ? "are" : "is"} internal and managed by the engine.`
        : null;
    }

    const wantsEmbedding = nodeDef.properties.some((p) => p.name === "_embedding") ?? false;

    async function computeEmbedding(props: Record<string, unknown>): Promise<number[] | null> {
      if (!wantsEmbedding) return null;
      const nodeManager = NodeManager.getCachedInstance();
      const embedText = nodeManager.getEmbeddingText(args.nodeLabel, props);
      if (!embedText) return null;
      try {
        const embedder = getEmbedder();
        return await embedder.embed(embedText);
      } catch {
        console.warn(`[editNode] embedding failed for "${args.nodeLabel}"`);
        return null;
      }
    }

    // Serialize plain objects to JSON strings for Neo4j compatibility.
    // Neo4j properties must be primitives or arrays of primitives — nested
    // objects (e.g. flags, metadata) are rejected as Maps.
    function toPropertyValue(v: unknown): unknown {
      if (v === null || v === undefined) return v;
      if (typeof v === "object" && !Array.isArray(v)) return JSON.stringify(v);
      return v;
    }

    function buildWhere(match: Record<string, string>, params: Record<string, unknown>): string {
      const parts = Object.entries(match).map(([key, value], i) => {
        const pName = `mk${i}`;
        params[pName] = value;
        return `n.\`${key}\` = $${pName}`;
      });
      return parts.join(" AND ");
    }

    // ── DELETE ──
    if (args.action === "DELETE") {
      if (!args.match || Object.keys(args.match).length === 0) {
        return "ERROR: Parameter `match` is required for DELETE.";
      }
      const matchErr = isMatchKeysInternal(args.match);
      if (matchErr) return `ERROR: ${matchErr}`;

      const params: Record<string, unknown> = {};
      const where = buildWhere(args.match, params);
      const result = await client.neo4j.executeWrite(
        `MATCH (n:\`${args.nodeLabel}\`) WHERE ${where} DETACH DELETE n RETURN count(n) AS deleted`,
        params,
      );
      return (result[0]?.deleted as number) > 0
        ? `Node "${args.nodeLabel}" matched by ${JSON.stringify(args.match)} deleted.`
        : `ERROR: No "${args.nodeLabel}" node found matching ${JSON.stringify(args.match)}.`;
    }

    const allSchemaProps = new Set(nodeDef.properties.map((p) => p.name));

    // ── CREATE ──
    if (args.action === "CREATE") {
      if (!args.properties || Object.keys(args.properties).length === 0) {
        return "ERROR: Parameter `properties` is required for CREATE and must not be empty.";
      }
      const propErr = isPropsKeyExistAndNotInternal(args.properties);
      if (propErr) return `ERROR: ${propErr}`;

      const id = uuidv4();
      const params: Record<string, unknown> = { id };
      const setters = ["n._id = $id"];
      if (allSchemaProps.has("_created_at")) setters.push("n._created_at = datetime()");
      if (allSchemaProps.has("_updated_at")) setters.push("n._updated_at = datetime()");
      for (const [key, value] of Object.entries(args.properties)) {
        const pName = `p_${key}`;
        params[pName] = toPropertyValue(value);
        setters.push(`n.\`${key}\` = $${pName}`);
      }

      if (wantsEmbedding) {
        const embedding = await computeEmbedding(args.properties);
        const embeddingParam = `p__embedding`;
        params[embeddingParam] = embedding ?? [];
        setters.push(`n._embedding = $${embeddingParam}`);
      }

      // TODO: NodeManager, specify unique property name to enable auto de-duplication.
      const rows = await client.neo4j.executeWrite(
        `CREATE (n:\`${args.nodeLabel}\`) SET ${setters.join(", ")} RETURN n`,
        params,
      );
      const created = rows[0]?.n as Record<string, unknown> | undefined;
      const v = visibleProps(created);
      const propSummary =
        Object.keys(v).length > 0 ? ` with keys: ${Object.keys(v).join(", ")}` : "";
      return `Node "${args.nodeLabel}" created${propSummary}.`;
    }

    // ── UPDATE ──
    if (args.action === "UPDATE" && (!args.match || Object.keys(args.match).length === 0)) {
      return "ERROR: Parameter `match` is required for UPDATE.";
    }
    const matchErr = isMatchKeysInternal(args.match);
    if (matchErr) return `ERROR: ${matchErr}`;

    const matchParams: Record<string, unknown> = {};
    const where = buildWhere(args.match, matchParams);

    const existing = await client.neo4j.executeRead(
      `MATCH (n:\`${args.nodeLabel}\`) WHERE ${where} RETURN n`,
      matchParams,
    );
    if (existing.length === 0) {
      return `ERROR: No "${args.nodeLabel}" node found matching ${JSON.stringify(args.match)}.`;
    }
    if (existing.length !== 1) {
      return `ERROR: There are ${existing.length} matching results. The tool is designed to edit **single** node only.`;
    }

    if (!args.properties || Object.keys(args.properties).length === 0) {
      return "ERROR: No properties to update. Nothing is edited inside the database";
    }

    const propErr = isPropsKeyExistAndNotInternal(args.properties);
    if (propErr) return `ERROR: ${propErr}`;

    const existingNode = existing[0]?.n as Record<string, unknown> | undefined;

    // Properties tagged "json" are stored as JSON strings in Neo4j.
    // A plain SET would overwrite the entire property, silently dropping fields
    // the GM didn't mention. Read the existing value and shallow-merge the
    // incoming ones so partial updates don't clobber.
    const propertiesToSet = { ...args.properties };
    const jsonPropNames = new Set(
      nodeDef.properties.filter((p) => p.tags.includes("json")).map((p) => p.name),
    );
    for (const key of Object.keys(args.properties)) {
      if (!jsonPropNames.has(key)) continue;
      const incoming = args.properties[key] as Record<string, unknown>;
      const existingRaw = existingNode?.[key];
      let existing: Record<string, unknown> = {};
      if (typeof existingRaw === "string") {
        try {
          const parsed = JSON.parse(existingRaw);
          if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
            existing = parsed;
          }
        } catch {
          // unparseable — overwrite with incoming
        }
      } else if (existingRaw && typeof existingRaw === "object" && !Array.isArray(existingRaw)) {
        existing = existingRaw as Record<string, unknown>;
      }
      propertiesToSet[key] = { ...existing, ...incoming };
    }

    const setParams: Record<string, unknown> = { ...matchParams };
    const setters: string[] = [];
    if (allSchemaProps.has("_updated_at")) setters.push("n._updated_at = datetime()");
    for (const [key, value] of Object.entries(propertiesToSet)) {
      const pName = `s_${key}`;
      setParams[pName] = toPropertyValue(value);
      setters.push(`n.\`${key}\` = $${pName}`);
    }

    if (wantsEmbedding) {
      const embeddedNames = new Set(
        nodeDef.properties.filter((p) => p.tags.includes("embedded")).map((p) => p.name),
      );
      const textChanged = Object.keys(args.properties).some((k) => embeddedNames.has(k));
      if (textChanged) {
        const merged: Record<string, unknown> = { ...existingNode, ...args.properties };
        const embedding = await computeEmbedding(merged);
        setters.push(`n._embedding = $s__embedding`);
        setParams["s__embedding"] = embedding ?? [];
      }
    }

    await client.neo4j.executeWrite(
      `MATCH (n:\`${args.nodeLabel}\`) WHERE ${where} SET ${setters.join(", ")}`,
      setParams,
    );

    // Reset scene observer for entities whose description/brief changed
    // TODO: This is fragile.
    if (args.properties.description !== undefined || args.properties.brief !== undefined) {
      const entityName = existingNode?.name as string | undefined;
      if (entityName) {
        try {
          getObserver().resetEntity(entityName);
        } catch {
          // Best-effort
        }
      }
    }

    return `Node "${args.nodeLabel}" updated properties: ${Object.keys(args.properties).join(", ")}.`;
  }, TOOL_NAMES.EDIT_NODE),
});
