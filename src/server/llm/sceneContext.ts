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

import { describeTime, getCurrentTimePoint } from "@/server/models/time";
import { MemoryClient } from "@/server/memory/client";
import { RelationshipManager } from "@/server/memory/relationshipManager";
import { NodeManager } from "@/server/memory/nodeManager";
import { getObserver } from "@/server/llm/sceneObserver";
import type { EntityRef } from "@/server/models/entity";
import { formatEntityCompact, formatEntityFull } from "@/server/models/entity";
import type { PlotRef } from "@/server/models/plot";
import { buildPlotTree, parseFlags } from "@/server/models/plot";

// ── Query result types ──

interface SceneRow {
  player: Record<string, unknown> | null;
  loc: Record<string, unknown> | null;
  inventory: EntityRef[] | null;
  npcs: EntityRef[] | null;
  objects: EntityRef[] | null;
}

interface DispositionRow {
  npcName: string;
  sentiment: string;
  summary: string;
}

// ── Queries ──

const SCENE_QUERY = `
MATCH (player:Entity {name: "Player"})
OPTIONAL MATCH (player)-[:LOCATED_AT]->(loc:Entity)
RETURN player, loc,
  COLLECT { MATCH (player)-[:CARRIES]->(inv:Entity)
            RETURN { name: inv.name, type: inv.type, description: inv.description,
                     brief: inv.brief } } AS inventory,
  COLLECT { MATCH (npc:Entity)-[:LOCATED_AT]->(loc)
            WHERE npc.type = "CHARACTER" AND npc.name <> "Player"
            RETURN { name: npc.name, type: npc.type, description: npc.description,
                     brief: npc.brief, subtype: npc.subtype } } AS npcs,
  COLLECT { MATCH (obj:Entity)-[:LOCATED_AT]->(loc)
            WHERE obj.type = "OBJECT"
            RETURN { name: obj.name, type: obj.type, description: obj.description,
                     brief: obj.brief } } AS objects
`;

const DISPOSITIONS_QUERY = `
MATCH (d:NPCDisposition {target_name: "Player"})
RETURN d.npc_name AS npcName, d.sentiment AS sentiment, d.summary AS summary
ORDER BY d._updated_at DESC
`;

const PLOTS_QUERY = `
MATCH (p:Plot)
WHERE p.status IN ["ACTIVE", "IN_PROGRESS"]
RETURN p.name AS name, p.description AS description, p.brief AS brief,
       p.status AS status, p.trigger_condition AS triggerCondition,
       COLLECT { MATCH (p)-[:BRANCHES_TO]->(child:Plot)
                 WHERE child.status IN ["ACTIVE", "IN_PROGRESS", "PENDING"]
                 RETURN { name: child.name, description: child.description,
                          brief: child.brief, status: child.status } } AS children
ORDER BY p._updated_at DESC
`;

// ── Formatters ──

function formatDisposition(d: DispositionRow): string {
  return `- **${d.npcName}**: ${d.sentiment} — "${d.summary}"`;
}

// ── SCENE_CONTEXT ──

export async function buildSceneContext(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const observer = getObserver();

  const [gameTime, sceneRows, dispositionRows, plotRows] = await Promise.all([
    getCurrentTimePoint().catch((err) => {
      console.error(
        "[sceneContext] getCurrentTimePoint failed:",
        err instanceof Error ? err.message : String(err),
      );
      return null;
    }),
    client.neo4j.executeRead(SCENE_QUERY).catch((err) => {
      console.error(
        "[sceneContext] scene query failed:",
        err instanceof Error ? err.message : String(err),
      );
      return [] as SceneRow[];
    }),
    client.neo4j.executeRead(DISPOSITIONS_QUERY).catch((err) => {
      console.error(
        "[sceneContext] dispositions query failed:",
        err instanceof Error ? err.message : String(err),
      );
      return [] as DispositionRow[];
    }),
    client.neo4j.executeRead(PLOTS_QUERY).catch((err) => {
      console.error(
        "[sceneContext] plots query failed:",
        err instanceof Error ? err.message : String(err),
      );
      return [] as PlotRef[];
    }),
  ]);

  const parts: string[] = [];
  parts.push("## SCENE CONTEXT (pre-loaded)");

  // Game time
  if (gameTime) {
    parts.push(`**Time**: ${describeTime(gameTime)}`);
  }

  const scene = sceneRows[0] as SceneRow | undefined;

  if (!scene || !scene.player) {
    parts.push("(No scene data available — player entity not found.)");
    return parts.join("\n");
  }

  const compactLines: string[] = [];
  const fullSections: string[] = [];
  const allEntities: EntityRef[] = [];

  // Location
  const loc = scene.loc as Record<string, unknown> | null;
  if (loc) {
    const locRef: EntityRef = {
      name: (loc.name as string) ?? "Unknown",
      type: (loc.type as string) ?? "LOCATION",
      description: (loc.description as string) || null,
      brief: (loc.brief as string) || null,
    };
    compactLines.push(`**Location**: ${formatEntityCompact(locRef)}`);
    allEntities.push(locRef);
  }

  // Inventory — names only
  if (scene.inventory && scene.inventory.length > 0) {
    compactLines.push(`**Carrying**: ${scene.inventory.map((i) => i.name).join(", ")}`);
  }

  // NPCs
  if (scene.npcs && scene.npcs.length > 0) {
    compactLines.push("**Nearby NPCs**:");
    for (const npc of scene.npcs) {
      compactLines.push(formatEntityCompact(npc));
      allEntities.push(npc);
    }
  }

  // Objects
  if (scene.objects && scene.objects.length > 0) {
    compactLines.push("**Nearby Objects**:");
    for (const obj of scene.objects) {
      compactLines.push(formatEntityCompact(obj));
      allEntities.push(obj);
    }
  }

  // Dispositions — always compact
  if (dispositionRows.length > 0) {
    compactLines.push("**NPC Dispositions toward Player**:");
    for (const d of dispositionRows) {
      compactLines.push(formatDisposition(d as DispositionRow));
    }
  }

  // Active plots
  if (plotRows.length > 0) {
    const plotRefs: PlotRef[] = plotRows.map((p: any) => ({
      name: p.name,
      description: p.description ?? "",
      brief: p.brief || null,
      status: p.status,
      triggerCondition: p.triggerCondition,
      children: (p.children || []).filter((c: any) => c && c.name),
    }));
    const { tree, unseenDescriptions } = buildPlotTree(plotRefs, observer);
    compactLines.push("**Active Plots**:");
    compactLines.push(tree);
    if (unseenDescriptions) {
      fullSections.push(unseenDescriptions);
    }
  }

  // Build compact section
  parts.push(compactLines.join("\n"));

  // Build ### full descriptions for unseen entities
  for (const ref of allEntities) {
    if (!observer.wasSeen("entity", ref.name)) {
      const full = formatEntityFull(ref);
      if (full) {
        fullSections.push(full);
      }
      observer.markSeen("entity", ref.name);
    }
  }

  // Append full descriptions section
  if (fullSections.length > 0) {
    parts.push("");
    parts.push(fullSections.join("\n\n"));
  }

  parts.push("");
  parts.push("---");

  return parts.join("\n");
}

// ── CHARACTERS_BRIEF ──

const CHARACTERS_QUERY = `
MATCH (c:Entity)
WHERE c.type = "CHARACTER"
OPTIONAL MATCH (c)-[:LOCATED_AT]->(loc:Entity)
OPTIONAL MATCH (c)-[:HAS_DISPOSITION]->(d:NPCDisposition {target_name: "Player"})
RETURN c.name AS name, c.brief AS brief, c.description AS description,
       loc.name AS location, d.sentiment AS disposition
ORDER BY name
`;

interface CharacterRow {
  name: string;
  brief: string | null;
  description: string | null;
  location: string | null;
  disposition: string | null;
}

export async function buildCharactersBrief(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const rows = (await client.neo4j.executeRead(CHARACTERS_QUERY)) as unknown as CharacterRow[];

  if (rows.length === 0) return "## CHARACTERS\n\n(none)\n";

  const lines: string[] = ["## CHARACTERS", ""];
  for (const c of rows) {
    const brief = c.brief || (c.description || "").slice(0, 120) || "";
    const loc = c.location ? ` (${c.location})` : "";
    const disp = c.disposition ? ` [${c.disposition}]` : "";
    lines.push(`- **${c.name}**${loc}: ${brief}${disp}`);
  }
  lines.push("");
  return lines.join("\n");
}

// ── LOCATIONS_BRIEF ──

const LOCATIONS_QUERY = `
MATCH (l:Entity)
WHERE l.type = "LOCATION"
RETURN l.name AS name, l.brief AS brief, l.description AS description
ORDER BY name
`;

interface LocationRow {
  name: string;
  brief: string | null;
  description: string | null;
}

export async function buildLocationsBrief(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const rows = (await client.neo4j.executeRead(LOCATIONS_QUERY)) as unknown as LocationRow[];

  if (rows.length === 0) return "## LOCATIONS\n\n(none)\n";

  const lines: string[] = ["## LOCATIONS", ""];
  for (const l of rows) {
    const brief = l.brief || (l.description || "").slice(0, 120) || "";
    lines.push(`- **${l.name}**: ${brief}`);
  }
  lines.push("");
  return lines.join("\n");
}

// ── OBJECTS_BRIEF ──

const OBJECTS_QUERY = `
MATCH (o:Entity)
WHERE o.type = "OBJECT"
OPTIONAL MATCH (carrier:Entity)-[:CARRIES]->(o)
OPTIONAL MATCH (o)-[:LOCATED_AT]->(loc:Entity)
  WHERE carrier IS NULL
RETURN o.name AS name, o.brief AS brief, o.description AS description,
       carrier.name AS carrier, loc.name AS location
ORDER BY name
`;

interface ObjectRow {
  name: string;
  brief: string | null;
  description: string | null;
  carrier: string | null;
  location: string | null;
}

export async function buildObjectsBrief(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const rows = (await client.neo4j.executeRead(OBJECTS_QUERY)) as unknown as ObjectRow[];

  if (rows.length === 0) return "## OBJECTS\n\n(none)\n";

  const lines: string[] = ["## OBJECTS", ""];
  for (const o of rows) {
    const context = o.carrier
      ? `Carried by: ${o.carrier}`
      : o.location
        ? `Located at: ${o.location}`
        : "location unknown";
    lines.push(`- **${o.name}** — ${context}`);
  }
  lines.push("");
  return lines.join("\n");
}

// ── PLOTS_BRIEF ──

export async function buildPlotsBrief(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const rows = (await client.neo4j.executeRead(
    `MATCH (p:Plot) RETURN p ORDER BY p.name`,
  )) as Array<{ p: Record<string, unknown> }>;

  if (rows.length === 0) return "## PLOTS\n\n(none)\n";

  const lines: string[] = ["## PLOTS", ""];
  for (const row of rows) {
    const p = row.p;
    const name = p.name as string;
    const status = p.status as string;
    const brief = (p.brief as string) || ((p.description as string) || "").slice(0, 120);
    const flags = parseFlags(p.flags);
    const flagStr = flags.length > 0 ? `\n  Flags: ${flags.map((f) => f.flagId).join(", ")}` : "";
    lines.push(`- **${name}** (${status}): ${brief}${flagStr}`);
  }
  lines.push("");
  return lines.join("\n");
}

// ── RELATIONSHIP_DUMP ──

export async function buildRelationshipDump(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const manager = RelationshipManager.getCachedInstance();
  const nodeManager = NodeManager.getCachedInstance();

  const excluded = new Set(nodeManager.getByType("INTERNAL").map((n) => n.name));
  for (const name of ["Message", "RelationshipType", "NodeType"]) {
    excluded.add(name);
  }
  const aClauses = [...excluded].map((l) => `NOT a:\`${l}\``).join(" AND ");
  const bClauses = [...excluded].map((l) => `NOT b:\`${l}\``).join(" AND ");

  const relRows = (await client.neo4j.executeRead(
    `MATCH (a)-[r]->(b)
     WHERE ${aClauses} AND ${bClauses}
     RETURN labels(a) AS sourceLabels,
            COALESCE(a.name, a._id) AS sourceName,
            type(r) AS type,
            labels(b) AS targetLabels,
            COALESCE(b.name, b._id) AS targetName`,
  )) as Array<{
    sourceName: string;
    type: string;
    targetName: string;
  }>;

  // Filter INTERNAL and dedicated-section types
  const internalTypeNames = new Set(manager.getByType("INTERNAL").map((r) => r.name));
  for (const name of [
    "HAS_DISPOSITION",
    "CURRENT_TIMEPOINT",
    "NEXT_TIMEPOINT",
    "STARTED_AT",
    "ACTIVE_AT",
    "COMPLETED_AT",
    "BRANCHES_TO",
    "ABOUT_ENTITY",
    "ABOUT_MESSAGE",
  ]) {
    internalTypeNames.add(name);
  }
  const visible = relRows.filter((r) => !internalTypeNames.has(r.type));

  if (visible.length === 0) return "## RELATIONSHIPS\n\n(none)\n";

  const byType = new Map<string, typeof visible>();
  for (const r of visible) {
    const group = byType.get(r.type) || [];
    group.push(r);
    if (!byType.has(r.type)) byType.set(r.type, group);
  }

  const lines: string[] = ["## RELATIONSHIPS", ""];
  const sortedTypes = [...byType.keys()].sort();
  for (const type of sortedTypes) {
    const group = byType.get(type)!;
    lines.push(`### ${type}`);
    if (type === "LOCATED_AT" || type === "LOCATED_IN") {
      const seen = new Set<string>();
      for (const r of group) {
        const tgt = r.targetName;
        if (!seen.has(tgt)) {
          seen.add(tgt);
          const occupants = group
            .filter((o) => o.targetName === tgt)
            .map((o) => o.sourceName)
            .join(", ");
          lines.push(`- **${tgt}**: ${occupants}`);
        }
      }
    } else {
      for (const r of group) {
        lines.push(`- ${r.sourceName} → ${r.targetName}`);
      }
    }
    lines.push("");
  }
  return lines.join("\n");
}
