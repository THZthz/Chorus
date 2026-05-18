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
import {
  getSchemaVisualization,
  getRelationshipTypeDescriptions,
  formatSchemaMarkdown,
} from "@/server/models/schema";
import { MemoryClient } from "@/server/memory/client";

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
  const db = MemoryClient.getCachedInstance().neo4j;
  const [schemaVis, relTypeDescs] = await Promise.all([
    getSchemaVisualization(db).catch((err) => {
      console.error("[getContext] schema visualization failed:", err);
      return { nodes: [], relationships: [] };
    }),
    getRelationshipTypeDescriptions(db).catch((err) => {
      console.error("[getContext] relationship type descriptions failed:", err);
      return [] as { name: string; description: string; category: string }[];
    }),
  ]);
  return formatSchemaMarkdown(schemaVis, relTypeDescs);
}

export const getContext = tool({
  title: TOOL_NAMES.GET_CONTEXT,
  description: `
Pull context from the world archive on demand. Nothing is auto-loaded — you decide what you need.

Types:
- SCENE_CONTEXT — Your immediate surroundings: time, location, nearby NPCs/objects,
  inventory, NPC dispositions, active plots. Shows full descriptions for unseen entities/plots;
  compact briefs thereafter.
- CHARACTERS_BRIEF — All characters with current location and disposition toward player.
- LOCATIONS_BRIEF — All locations with brief descriptions.
- OBJECTS_BRIEF — All objects with who carries them or where they are.
- PLOTS_BRIEF — All plots with status, brief, and flags.
- SCHEMA_DUMP — Available node types (with property schemas) and relationship types
  (with endpoint constraints).
- RELATIONSHIP_DUMP — All non-internal relationships, grouped by type. LOCATED_AT and
  LOCATED_IN are grouped by location showing occupants.

After the first call, entities and plots show compact briefs in SCENE_CONTEXT instead of full descriptions.
Call \`${TOOL_NAMES.RESET_SCENE_CONTEXT}\` if you really need the full descriptions again.

Default (no types specified): SCENE_CONTEXT only.
Call with additional types when you need global awareness beyond your immediate scene.
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
