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

import { getGameTime, describeTime } from "@/server/models/time";
import { MemoryClient } from "@/server/memory/client";
import { getObserver } from "@/server/llm/sceneObserver";

// ── Query result types ──

interface SceneEntityRef {
  name: string;
  type: string;
  description: string | null;
  brief: string | null;
  subtype?: string | null;
}

interface SceneRow {
  player: Record<string, unknown> | null;
  loc: Record<string, unknown> | null;
  inventory: SceneEntityRef[] | null;
  npcs: SceneEntityRef[] | null;
  objects: SceneEntityRef[] | null;
}

interface DispositionRow {
  npcName: string;
  sentiment: string;
  summary: string;
}

interface PlotRef {
  name: string;
  description: string;
  brief: string | null;
  status: string;
  triggerCondition: string | null;
  children: PlotRef[];
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
ORDER BY d.updated_at DESC
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
ORDER BY p.updated_at DESC
`;

// ── Formatters ──

function formatEntityCompact(e: SceneEntityRef): string {
  const brief = e.brief || e.description?.slice(0, 120) || "";
  return `**${e.name}** (${e.type}) — ${brief}`;
}

function formatEntityFull(e: SceneEntityRef): string {
  if (!e.description) return "";
  return `### ${e.name}\n${e.description}`;
}

function formatDisposition(d: DispositionRow): string {
  return `- **${d.npcName}**: ${d.sentiment} — "${d.summary}"`;
}

function buildPlotTree(plots: PlotRef[]): { tree: string; unseenDescriptions: string } {
  // Identify roots: plots with no incoming BRANCHES_TO from the active set
  const activeNames = new Set(plots.map((p) => p.name));
  const childNames = new Set<string>();
  for (const p of plots) {
    for (const c of p.children) {
      if (activeNames.has(c.name)) childNames.add(c.name);
    }
  }
  const roots = plots.filter((p) => !childNames.has(p.name));

  // Also include plots that were loaded but whose parent wasn't in the result
  // (this shouldn't happen normally, but be safe)
  const visited = new Set<string>();
  const observer = getObserver();
  const treeLines: string[] = [];
  const fullDescs: string[] = [];

  function renderNode(plot: PlotRef, prefix: string, isLast: boolean, connector: string) {
    if (visited.has(plot.name)) return;
    visited.add(plot.name);

    const brief = plot.brief || (plot.description || "").slice(0, 120);
    treeLines.push(`${prefix}${connector} ${plot.name} (${plot.status}): ${brief}`);

    // Track unseen
    if (!observer.wasSeen("plot", plot.name)) {
      fullDescs.push(`### ${plot.name}\n${plot.description}`);
      observer.markSeen("plot", plot.name);
    }

    // Render children
    const kids = plot.children.filter((c) => activeNames.has(c.name) && !visited.has(c.name));
    const childPrefix = prefix + (isLast ? "    " : "│   ");
    kids.forEach((child, i) => {
      renderNode(child, childPrefix, i === kids.length - 1, i === kids.length - 1 ? "└──" : "├──");
    });
  }

  roots.forEach((root, i) => {
    renderNode(root, "", i === roots.length - 1, "");
  });

  return {
    tree: treeLines.join("\n"),
    unseenDescriptions: fullDescs.join("\n\n"),
  };
}

// ── Main export ──

export async function buildSceneContext(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const observer = getObserver();

  const [gameTime, sceneRows, dispositionRows, plotRows] = await Promise.all([
    getGameTime().catch((err) => {
      console.error(
        "[sceneContext] getGameTime failed:",
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
  const allEntities: SceneEntityRef[] = [];

  // Location
  const loc = scene.loc as Record<string, unknown> | null;
  if (loc) {
    const locRef: SceneEntityRef = {
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
    const { tree, unseenDescriptions } = buildPlotTree(plotRefs);
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
