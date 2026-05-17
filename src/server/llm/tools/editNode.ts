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
import { v4 as uuidv4 } from "uuid";
import { MemoryClient } from "@/server/memory/client";
import { NodeManager } from "@/server/memory/nodeManager";
import { wrapSafe } from "@/server/llm/tools/shared";
import { getObserver } from "@/server/llm/sceneObserver";
import { getEmbedder } from "@/server/memory/embedder";
import { TOOL_NAMES } from "@/shared/constants";

const NODE_ACTIONS = ["CREATE", "UPDATE", "DELETE"] as const;

const SYSTEM_PROPS = new Set(["_id", "_created_at", "_updated_at", "_embedding"]);

function visibleProps(node: Record<string, unknown> | undefined): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  if (!node) return out;
  for (const [k, v] of Object.entries(node)) {
    if (!k.startsWith("_")) out[k] = v;
  }
  return out;
}

const inputSchema = z.object({
  nodeLabel: z
    .string()
    .describe(
      "Node label to operate on (e.g. 'Entity', 'Character', 'Location', or a GM-defined label). " +
        "Must be registered in the world schema and writable. " +
        `Query :NodeType nodes via ${TOOL_NAMES.QUERY_WORLD} to discover available types and their property schemas.`,
    ),
  action: z.enum(NODE_ACTIONS).default("CREATE").describe("Action to perform."),
  match: z
    .record(z.string(), z.string())
    .nullable()
    .optional()
    .describe(
      "Key-value pairs to locate exactly one node. Required for UPDATE/DELETE. " +
        "e.g. { name: 'Tavern' } for an Entity, or { npc_name: 'Guard', target_name: 'Player' } for an NPCDisposition.",
    ),
  properties: z
    .record(z.string(), z.unknown())
    .nullable()
    .optional()
    .describe(
      "Key-value pairs to set on the node. Must match the property schema for this node type. " +
        "CREATE: sets initial properties. UPDATE: only include properties you want to change. " +
        "System properties (_id, _created_at, _updated_at, _embedding) are managed automatically.",
    ),
});

export const editNode = tool({
  title: TOOL_NAMES.EDIT_NODE,
  description: `
CREATE, UPDATE, or DELETE a node in the world archive using a registered node type.

CREATE — Add a new entity, note, plot, or custom node type. Properties are validated
against the type's schema. WARNING: This tool does NOT check for duplicates — search first
via searchWorld or queryWorld (READ) to verify the node doesn't already exist.
Use label "Note" to create your own scratchpad notes.

UPDATE — Change properties on an existing node. Only include fields you want to change.
Use for: entity descriptions, plot statuses/flags, note contents, dispositions.

DELETE — Remove a node and all its relationships (DETACH DELETE).
Requires exact match criteria. Verify you're targeting the right node.
`.trim(),
  inputSchema,
  execute: wrapSafe(async (args: z.infer<typeof inputSchema>) => {
    const client = MemoryClient.getCachedInstance();
    const nodeManager = NodeManager.getCachedInstance();

    // Validate node label
    const nodeDef = nodeManager.get(args.nodeLabel);
    if (!nodeDef) {
      const available = nodeManager
        .getAll()
        .filter((n) => n.type !== "INTERNAL")
        .map((n) => n.name)
        .join(", ");
      return `ERROR: Node label "${args.nodeLabel}" is not registered. Available labels: ${available}`;
    }
    if (!nodeManager.isAllowedForWrite(args.nodeLabel)) {
      return `ERROR: Node label "${args.nodeLabel}" is ${nodeDef.type} and cannot be written to via ${TOOL_NAMES.EDIT_NODE}.`;
    }

    // Build allowed property names from the schema.
    // Predefined types have empty schemas (by design) — for those, accept any
    // non-_-prefixed property except system-managed ones.  GM_DEFINED types
    // have explicit schemas and must be validated strictly.
    const schemaProps = new Set(
      nodeDef.properties
        .map((p) => p.name)
        .filter((name) => !SYSTEM_PROPS.has(name) && !name.startsWith("_")),
    );
    const hasSchema = nodeDef.properties.length > 0;

    function checkProps(props: Record<string, unknown>): string | null {
      for (const key of Object.keys(props)) {
        if (SYSTEM_PROPS.has(key)) {
          return `Property "${key}" is system-managed and cannot be set directly.`;
        }
        if (key.startsWith("_")) {
          return `Property "${key}" is internal (prefixed with '_') and cannot be set.`;
        }
        if (hasSchema && !schemaProps.has(key)) {
          return `Unknown property "${key}" for node type "${args.nodeLabel}". Allowed: ${[...schemaProps].join(", ")}`;
        }
      }
      return null;
    }

    function checkMatchKeys(match: Record<string, string>): string | null {
      for (const key of Object.keys(match)) {
        if (SYSTEM_PROPS.has(key)) return `Match key "${key}" is system-managed.`;
        if (key.startsWith("_")) return `Match key "${key}" is internal.`;
      }
      return null;
    }

    // NodeManager discards properties for PREDEFINED types, so check the
    // known embeddable labels directly (all have Neo4j vector indexes).
    const EMBEDDABLE = new Set([
      "Entity",
      "Character",
      "Object",
      "Location",
      "Organization",
      "Event",
      "Note",
      "Plot",
      "Message",
    ]);
    const wantsEmbedding = EMBEDDABLE.has(args.nodeLabel);

    async function computeEmbedding(props: Record<string, unknown>): Promise<number[] | null> {
      const name = String(props.name ?? "");
      const text = String(props.description ?? props.content ?? "");
      const embedText = name && text ? `${name}: ${text}` : name || text;
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
        return "ERROR: match is required for DELETE.";
      }
      const matchErr = checkMatchKeys(args.match);
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

    // ── CREATE ──
    if (args.action === "CREATE") {
      if (!args.properties || Object.keys(args.properties).length === 0) {
        return "ERROR: properties is required for CREATE and must not be empty.";
      }
      const propErr = checkProps(args.properties);
      if (propErr) return `ERROR: ${propErr}`;

      const id = uuidv4();
      const params: Record<string, unknown> = { id };
      const setters = ["n._id = $id", "n._created_at = datetime()"];
      for (const [key, value] of Object.entries(args.properties)) {
        const pName = `p_${key}`;
        params[pName] = toPropertyValue(value);
        setters.push(`n.\`${key}\` = $${pName}`);
      }

      if (wantsEmbedding) {
        const emb = await computeEmbedding(args.properties);
        const embParam = `p__embedding`;
        params[embParam] = emb ?? [];
        setters.push(`n._embedding = $${embParam}`);
      }

      const rows = await client.neo4j.executeWrite(
        `CREATE (n:\`${args.nodeLabel}\`) SET ${setters.join(", ")} RETURN n`,
        params,
      );
      const created = rows[0]?.n as Record<string, unknown> | undefined;
      const v = visibleProps(created);
      const propSummary = Object.keys(v).length > 0 ? ` with properties: ${JSON.stringify(v)}` : "";
      return `Node "${args.nodeLabel}" created${propSummary}.`;
    }

    // ── UPDATE ──
    if (!args.match || Object.keys(args.match).length === 0) {
      return "ERROR: match is required for UPDATE.";
    }
    const matchErr = checkMatchKeys(args.match);
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

    if (!args.properties || Object.keys(args.properties).length === 0) {
      return "No properties to update.";
    }

    const propErr = checkProps(args.properties);
    if (propErr) return `ERROR: ${propErr}`;

    const existingNode = existing[0]?.n as Record<string, unknown> | undefined;

    // WARNING: Plot flags are serialized as a single JSON property in Neo4j.
    // The GM passes flags as an object map (e.g. {memory_professionally_removed: true}),
    // and a plain SET would overwrite the entire property, silently dropping flags
    // the GM didn't mention. For Plot nodes we read the existing flags and
    // shallow-merge the incoming ones so partial updates don't clobber.
    const propertiesToSet = { ...args.properties };
    if (args.nodeLabel === "Plot" && args.properties.flags !== undefined) {
      const incomingFlags = args.properties.flags as Record<string, unknown>;
      const existingFlagsRaw = existingNode?.flags;
      let existingFlags: Record<string, unknown> = {};
      if (typeof existingFlagsRaw === "string") {
        try {
          const parsed = JSON.parse(existingFlagsRaw);
          if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
            existingFlags = parsed;
          }
        } catch {
          // unparseable — overwrite with incoming
        }
      } else if (
        existingFlagsRaw &&
        typeof existingFlagsRaw === "object" &&
        !Array.isArray(existingFlagsRaw)
      ) {
        existingFlags = existingFlagsRaw as Record<string, unknown>;
      }
      propertiesToSet.flags = { ...existingFlags, ...incomingFlags };
    }

    const setParams: Record<string, unknown> = { ...matchParams };
    const setters = ["n._updated_at = datetime()"];
    for (const [key, value] of Object.entries(propertiesToSet)) {
      const pName = `s_${key}`;
      setParams[pName] = toPropertyValue(value);
      setters.push(`n.\`${key}\` = $${pName}`);
    }

    if (wantsEmbedding) {
      const embKeys = new Set(["name", "description", "content", "brief"]);
      const textChanged = Object.keys(args.properties).some((k) => embKeys.has(k));
      if (textChanged) {
        const merged: Record<string, unknown> = { ...existingNode, ...args.properties };
        const emb = await computeEmbedding(merged);
        setters.push(`n._embedding = $s__embedding`);
        setParams["s__embedding"] = emb ?? [];
      }
    }

    await client.neo4j.executeWrite(
      `MATCH (n:\`${args.nodeLabel}\`) WHERE ${where} SET ${setters.join(", ")}`,
      setParams,
    );

    // Reset scene observer for entities whose description/brief changed
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
