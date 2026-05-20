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
import { TOOL_NAMES } from "@/shared/constants";
import { wrapSafe } from "@/server/llm/tools/shared";
import {
  buildSceneContext,
  buildCharactersBrief,
  buildLocationsBrief,
  buildObjectsBrief,
  buildPlotsBrief,
  buildRelationshipDump,
} from "@/server/llm/sceneContext";
import { NodeManager } from "@/server/nodeManager";
import { RelationshipManager } from "@/server/relationshipManager";

const CONTEXT_TYPES = [
  "SCENE_CONTEXT",
  "CHARACTERS_BRIEF",
  "LOCATIONS_BRIEF",
  "OBJECTS_BRIEF",
  "PLOTS_BRIEF",
  "SCHEMA_DUMP",
  "RELATIONSHIP_DUMP",
] as const;

type ContextType = (typeof CONTEXT_TYPES)[number];

async function buildSchemaDump(): Promise<string> {
  const nodeManager = NodeManager.getCachedInstance();
  const relManager = RelationshipManager.getCachedInstance();

  const lines: string[] = [];
  lines.push("## Schema (from in-memory registry)");
  lines.push("");
  lines.push(
    "A list of nodes/relationships, with their list of properties, tags of the property is displayed before its name.",
  );
  lines.push("");

  // ── Node types ──
  lines.push("### Node Types");
  lines.push("");
  const nodes = nodeManager
    .getAll()
    .filter((n) => n.type !== "INTERNAL")
    .sort((a, b) => a.name.localeCompare(b.name));
  for (const node of nodes) {
    const category = node.type === "GM_DEFINED" ? " [GM_DEFINED]" : "";
    lines.push(`- **${node.name}**${category}: ${node.description}`);
    if (node.properties.length > 0) {
      const visible = node.properties.filter((p) => !p.name.startsWith("_"));
      for (const prop of visible) {
        const tagStr = prop.tags.length > 0 ? ` (${prop.tags.join(", ")})` : "";
        lines.push(`  -${tagStr} \`${prop.name}\`: ${prop.description}`);
      }
    }
    lines.push("");
  }

  // ── Relationship types ──
  lines.push("### Relationship Types");
  lines.push("");
  const rels = relManager
    .getAll()
    .filter((r) => r.type !== "INTERNAL")
    .sort((a, b) => a.name.localeCompare(b.name));
  for (const rel of rels) {
    const src = rel.sourceLabel || "?";
    const tgt = rel.targetLabel || "?";
    const category = rel.type === "GM_DEFINED" ? " [GM_DEFINED]" : "";
    lines.push(`- **${rel.name}** (${src}→${tgt})${category}: ${rel.description}`);
    if (rel.properties.length > 0) {
      const visible = rel.properties.filter((p) => !p.name.startsWith("_"));
      for (const prop of visible) {
        const tagStr = prop.tags.length > 0 ? ` (${prop.tags.join(", ")})` : "";
        lines.push(`  -${tagStr} \`${prop.name}\`: ${prop.description}`);
      }
    }
    lines.push("");
  }

  return lines.join("\n");
}

export const getContext = tool({
  title: TOOL_NAMES.GET_CONTEXT,
  description: `
Pull pre-built context from the world. Nothing is auto-loaded — you choose what you need.

Types:
- SCENE_CONTEXT — Time, your location, nearby NPCs/objects, inventory, NPC dispositions, active plots. Full descriptions on first encounter, compact briefs after.
- CHARACTERS_BRIEF — All characters with location and disposition toward player.
- LOCATIONS_BRIEF — All locations with brief descriptions.
- OBJECTS_BRIEF — All objects with carrier or location.
- PLOTS_BRIEF — All plots with status, brief, and flags.
- SCHEMA_DUMP — All registered node types (with full property schemas: names, tags, descriptions) and relationship types (with endpoint constraints and property schemas) in Neo4j.
- RELATIONSHIP_DUMP — All active relationships grouped by type. LOCATED_AT/LOCATED_IN are grouped by location showing occupants and access details.

Default (no types specified): SCENE_CONTEXT only.
`.trim(),
  inputSchema: z.object({
    types: z
      .array(z.enum(CONTEXT_TYPES))
      .default(["SCENE_CONTEXT"])
      .describe("Which context sections to return. Default: SCENE_CONTEXT only."),
  }),
  execute: wrapSafe(async (args: { types: ContextType[] }) => {
    const sections = args.types.length > 0 ? args.types : ["SCENE_CONTEXT"];

    const builders: Record<ContextType, () => Promise<string>> = {
      SCENE_CONTEXT: buildSceneContext,
      CHARACTERS_BRIEF: buildCharactersBrief,
      LOCATIONS_BRIEF: buildLocationsBrief,
      OBJECTS_BRIEF: buildObjectsBrief,
      PLOTS_BRIEF: buildPlotsBrief,
      SCHEMA_DUMP: buildSchemaDump,
      RELATIONSHIP_DUMP: buildRelationshipDump,
    };

    // TODO: Rewrite in Promise.all?
    const results: string[] = [];
    for (const type of sections) {
      try {
        const section = await builders[type]();
        if (section) results.push(section);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        results.push(`## ${type}\n\nError: ${msg}\n`);
      }
    }

    return results.join("\n");
  }, TOOL_NAMES.GET_CONTEXT),
});
