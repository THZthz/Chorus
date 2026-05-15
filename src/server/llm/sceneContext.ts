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

import { getGameTime, describeTime } from "@/server/models/time";
import { MemoryClient } from "@/server/memory/client";
import { RelationshipManager } from "@/server/memory/relationshipManager";
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
    const kids = (plot.children || []).filter((c) => c.name && !visited.has(c.name));
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

// ── Full world state dump (developer debug) ──

async function sampleProps(db: import("@/server/memory/neo4j").Neo4jClient, label: string): Promise<string[]> {
  try {
    const rows = await db.executeRead(`MATCH (n:\`${label}\`) RETURN n LIMIT 1`);
    if (rows.length > 0) {
      const n = rows[0].n as Record<string, unknown>;
      return Object.keys(n).filter((k) => k !== "_elementId" && k !== "_labels").sort();
    }
  } catch { /* label may not support direct matching */ }
  return [];
}

export async function buildFullSceneContext(): Promise<string> {
  const client = MemoryClient.getCachedInstance();
  const db = client.neo4j;

  const extractAliases = (metadata: unknown): string[] => {
    if (typeof metadata === "string") {
      try { return ((JSON.parse(metadata) as Record<string, unknown>).aliases as string[]) || []; }
      catch { return []; }
    }
    if (metadata && typeof metadata === "object") {
      return ((metadata as Record<string, unknown>).aliases as string[]) || [];
    }
    return [];
  };
  const extractConditions = (metadata: unknown): string[] => {
    try {
      const m = typeof metadata === "string"
        ? JSON.parse(metadata) as Record<string, unknown>
        : (metadata as Record<string, unknown> | null);
      if (!m?.conditions) return [];
      return Object.entries(m.conditions as Record<string, Record<string, unknown>>).map(
        ([id, c]) => `${id}: ${c.description as string}`,
      );
    } catch { return []; }
  };
  const parseFlags = (flags: unknown): Array<{ flagId: string; description: string }> => {
    if (typeof flags === "string") {
      try { return JSON.parse(flags) as Array<{ flagId: string; description: string }>; }
      catch { return []; }
    }
    return (flags as Array<{ flagId: string; description: string }>) || [];
  };

  const [entityRows, plotRows, noteRows, dispRows, time, relRows, timeChain,
    labelRows, relTypeRows] = await Promise.all([
    db.executeRead("MATCH (e:Entity) RETURN e ORDER BY e.name"),
    db.executeRead("MATCH (p:Plot) RETURN p ORDER BY p.name"),
    db.executeRead("MATCH (n:Note) RETURN n ORDER BY n.name"),
    db.executeRead("MATCH (d:NPCDisposition) RETURN d ORDER BY d.npc_name"),
    getGameTime(),
    db.executeRead(
      `MATCH (a)-[r]->(b)
       WHERE NOT a:Message AND NOT a:Conversation AND NOT a:GMTurnMessage
         AND NOT b:Message AND NOT b:Conversation AND NOT b:GMTurnMessage
         AND NOT a:IdCounter AND NOT b:IdCounter
         AND NOT a:RelationshipType AND NOT b:RelationshipType
       RETURN labels(a) AS sourceLabels,
              COALESCE(a.name, a._id) AS sourceName,
              type(r) AS type,
              labels(b) AS targetLabels,
              COALESCE(b.name, b._id) AS targetName`,
    ),
    db.executeRead(
      `MATCH (a:TimeAnchor {_id: 'anchor'})-[:CURRENT_TIMEPOINT]->(current:TimePoint)
       OPTIONAL MATCH (current)-[:NEXT_TIMEPOINT*]->(future:TimePoint)
       RETURN current, collect(DISTINCT future) AS future`,
    ),
    db.executeRead("CALL db.labels() YIELD label RETURN label ORDER BY label"),
    db.executeRead("CALL db.relationshipTypes() YIELD relationshipType RETURN relationshipType ORDER BY relationshipType"),
  ]);

  const parts: string[] = [];
  parts.push("## World State");
  parts.push(`**Time**: ${describeTime(time)}`);
  parts.push("");

  // Entities
  parts.push("## Entities");
  for (const row of entityRows) {
    const e = row.e as Record<string, unknown>;
    const name = e.name as string;
    const type = e.type as string;
    const desc = (e.description as string) || "";
    const brief = (e.brief as string) || "";
    const subtype = e.subtype ? `:${e.subtype}` : "";
    const aliases = extractAliases(e.metadata);
    const aliasStr = aliases.length > 0 ? ` (aka ${aliases.join(", ")})` : "";
    const conditions = extractConditions(e.metadata);

    parts.push(`### ${name} (${type}${subtype})${aliasStr}`);
    if (brief) parts.push(`*${brief}*`);
    if (desc && desc !== brief) parts.push(desc);
    if (conditions.length > 0) {
      for (const c of conditions) {
        parts.push(`- Condition: ${c}`);
      }
    }
    parts.push("");
  }

  // Plots
  parts.push("## Plots");
  for (const row of plotRows) {
    const p = row.p as Record<string, unknown>;
    const name = p.name as string;
    const status = p.status as string;
    const desc = (p.description as string) || "";
    const brief = (p.brief as string) || "";
    const trigger = p.trigger_condition as string | undefined;
    const flags = parseFlags(p.flags);

    parts.push(`### ${name} (${status})`);
    if (brief) parts.push(`*${brief}*`);
    if (desc && desc !== brief) parts.push(desc);
    if (trigger) parts.push(`Trigger: \`${trigger}\``);
    if (flags.length > 0) {
      parts.push(`Flags: ${flags.map((f) => `\`${f.flagId}\` — ${f.description}`).join(", ")}`);
    }
    parts.push("");
  }

  // Notes
  parts.push("## Notes");
  for (const row of noteRows) {
    const n = row.n as Record<string, unknown>;
    const name = n.name as string;
    const content = (n.content as string) || "";
    parts.push(`### ${name}`);
    parts.push(content);
    parts.push("");
  }

  // NPCDispositions
  parts.push("## NPCDispositions");
  for (const row of dispRows) {
    const d = row.d as Record<string, unknown>;
    const npcName = d.npc_name as string;
    const targetName = d.target_name as string;
    const sentiment = d.sentiment as string;
    const summary = d.summary as string;
    parts.push(`- **${npcName}** → ${targetName}: ${sentiment} — "${summary}"`);
  }
  parts.push("");

  // Relationships
  parts.push("## Relationships");
  const rels = relRows as Array<{
    sourceName: string;
    type: string;
    targetName: string;
  }>;
  const manager = RelationshipManager.getCachedInstance();
  // INTERNAL types (GM message bookkeeping) — always hidden
  const internalTypeNames = new Set(manager.getByType("INTERNAL").map((r) => r.name));
  // Types covered by dedicated dump sections (Dispositions, Time Chain, Plots)
  for (const name of ["HAS_DISPOSITION", "CURRENT_TIMEPOINT", "NEXT_TIMEPOINT",
    "STARTED_AT", "ACTIVE_AT", "COMPLETED_AT", "BRANCHES_TO",
    "ABOUT_ENTITY", "ABOUT_MESSAGE"]) {
    internalTypeNames.add(name);
  }
  const visible = rels.filter((r) => !internalTypeNames.has(r.type));
  const byType = new Map<string, typeof visible>();
  for (const r of visible) {
    const group = byType.get(r.type) || [];
    group.push(r);
    if (!byType.has(r.type)) byType.set(r.type, group);
  }
  const sortedTypes = [...byType.keys()].sort();
  for (const type of sortedTypes) {
    const group = byType.get(type)!;
    parts.push(`### ${type}`);
    const seen = new Set<string>();
    for (const r of group) {
      const src = r.sourceName;
      const tgt = r.targetName;
      if (type === "LOCATED_AT" || type === "LOCATED_IN") {
        const key = tgt;
        if (!seen.has(key)) {
          seen.add(key);
          const occupants = group
            .filter((o) => o.targetName === tgt)
            .map((o) => o.sourceName)
            .join(", ");
          parts.push(`- **${tgt}**: ${occupants}`);
        }
      } else {
        parts.push(`- ${src} → ${tgt}`);
      }
    }
    parts.push("");
  }
  if (visible.length === 0) parts.push("(none)");

  // Schema
  parts.push("");
  parts.push("## Schema");

  const labels = labelRows.map((r) => r.label as string);
  const schemaLabels = labels.filter(
    (l) => !["GMTurnMessage", "IdCounter", "RelationshipType", "Conversation"].includes(l),
  );

  const labelProps = new Map<string, string[]>();
  for (const label of schemaLabels) {
    labelProps.set(label, await sampleProps(db, label));
  }

  const allRelTypes = relTypeRows.map((r) => r.relationshipType as string);

  parts.push("### Node Labels");
  for (const [label, props] of [...labelProps.entries()].sort((a, b) => a[0].localeCompare(b[0]))) {
    const propList = props.length > 0 ? props.join(", ") : "(no nodes)";
    parts.push(`- **${label}**: ${propList}`);
  }
  parts.push("");

  parts.push("### Relationship Types");
  for (const rt of allRelTypes) {
    const samples = (rels as typeof visible).filter((r) => r.type === rt).slice(0, 2);
    if (samples.length > 0) {
      const ctx = samples.map((s) => `(${s.sourceName})→(${s.targetName})`).join(", ");
      parts.push(`- **${rt}**: ${ctx}`);
    } else {
      parts.push(`- **${rt}**`);
    }
  }
  parts.push("");

  // Time Chain
  parts.push("## Time Chain");
  const nodes = timeChain.flatMap((r) => {
    const current = r.current as Record<string, unknown>;
    const future = (r.future as Record<string, unknown>[]) || [];
    return [current, ...future];
  });
  for (const tp of nodes) {
    if (!tp) continue;
    const day = tp.day as number;
    const segment = tp.segment as number;
    const label = tp.label as string;
    const created = tp.created_at as string;
    parts.push(`- Day ${day}, Segment ${segment} (${label}) — ${created}`);
  }
  parts.push("");

  parts.push("---");
  return parts.join("\n");
}
