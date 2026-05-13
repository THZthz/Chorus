import { getGameTime, describeTime } from "@/server/models/time";
import { MemoryClient } from "@/server/memory/client";

// ── Query result types ──

interface SceneEntityRef {
  name: string;
  type: string;
  description: string | null;
  subtype?: string | null;
  metadata?: string | null;
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

interface PlotChild {
  name: string;
  status: string;
}

interface PlotRow {
  name: string;
  description: string;
  status: string;
  triggerCondition: string | null;
  flags: string | null;
  children: PlotChild[] | null;
}

// ── Queries ──

const SCENE_QUERY = `
MATCH (player:Entity {name: "Player"})
OPTIONAL MATCH (player)-[:LOCATED_AT]->(loc:Entity)
WITH player, loc
OPTIONAL MATCH (player)-[:CARRIES]->(inv:Entity)
WITH player, loc, COLLECT(DISTINCT {name: inv.name, type: inv.type, description: inv.description}) AS inventory
OPTIONAL MATCH (npc:Entity)-[:LOCATED_AT]->(loc)
  WHERE npc.type = "PERSON" AND npc.name <> "Player"
WITH player, loc, inventory, COLLECT(DISTINCT {name: npc.name, type: npc.type, description: npc.description, subtype: npc.subtype, metadata: npc.metadata}) AS npcs
OPTIONAL MATCH (obj:Entity)-[:LOCATED_AT]->(loc)
  WHERE obj.type = "OBJECT"
RETURN player, loc, inventory, npcs, COLLECT(DISTINCT {name: obj.name, type: obj.type, description: obj.description}) AS objects
`;

const DISPOSITIONS_QUERY = `
MATCH (d:NPCDisposition {target_name: "Player"})
RETURN d.npc_name AS npcName, d.sentiment AS sentiment, d.summary AS summary
ORDER BY d.updated_at DESC
`;

const PLOTS_QUERY = `
MATCH (p:Plot)
WHERE p.status IN ["ACTIVE", "IN_PROGRESS"]
OPTIONAL MATCH (p)-[:BRANCHES_TO]->(child:Plot)
WITH p, COLLECT(DISTINCT {name: child.name, status: child.status}) AS children
RETURN p.name AS name, p.description AS description, p.status AS status,
       p.trigger_condition AS triggerCondition, p.flags AS flags, children
ORDER BY p.updated_at DESC
`;

// ── Formatters ──

function formatEntityBrief(e: SceneEntityRef | null): string {
  if (!e) return "";
  const desc = e.description ? ` — ${e.description}` : "";
  return `**${e.name}** (${e.type})${desc}`;
}

function formatDisposition(d: DispositionRow): string {
  return `- **${d.npcName}**: ${d.sentiment} — "${d.summary}"`;
}

function formatPlot(p: PlotRow): string {
  let line = `- **${p.name}** (${p.status}): ${p.description}`;
  if (p.triggerCondition) line += ` [Trigger: ${p.triggerCondition}]`;
  const children = p.children?.filter((c) => c.name);
  if (children && children.length > 0) {
    line += `\n  Children: ${children.map((c) => `${c.name} (${c.status})`).join(", ")}`;
  }
  return line;
}

// ── Main export ──

export async function buildSceneContext(): Promise<string> {
  const client = MemoryClient.getCachedInstance();

  const [gameTime, sceneRows, dispositionRows, plotRows] = await Promise.all([
    getGameTime().catch(() => null),
    client.neo4j.executeRead(SCENE_QUERY).catch(() => [] as SceneRow[]),
    client.neo4j.executeRead(DISPOSITIONS_QUERY).catch(() => [] as DispositionRow[]),
    client.neo4j.executeRead(PLOTS_QUERY).catch(() => [] as PlotRow[]),
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

  // Location
  const loc = scene.loc as Record<string, unknown> | null;
  if (loc) {
    const locName = (loc.name as string) ?? "Unknown";
    const locDesc = loc.description ? ` — ${loc.description}` : "";
    const locType = loc.type ? ` (${loc.type})` : "";
    parts.push(`**Location**: **${locName}**${locType}${locDesc}`);
  }

  // Inventory
  if (scene.inventory && scene.inventory.length > 0) {
    parts.push(`**Carrying**: ${scene.inventory.map((i) => i.name).join(", ")}`);
  }

  // NPCs at location
  if (scene.npcs && scene.npcs.length > 0) {
    parts.push("**Nearby NPCs**:");
    for (const npc of scene.npcs) {
      parts.push(formatEntityBrief(npc));
    }
  }

  // Objects at location
  if (scene.objects && scene.objects.length > 0) {
    parts.push("**Nearby Objects**:");
    for (const obj of scene.objects) {
      parts.push(formatEntityBrief(obj));
    }
  }

  // Dispositions
  if (dispositionRows.length > 0) {
    parts.push("**NPC Dispositions toward Player**:");
    for (const d of dispositionRows) {
      parts.push(formatDisposition(d as DispositionRow));
    }
  }

  // Active plots
  if (plotRows.length > 0) {
    parts.push("**Active Plots**:");
    for (const p of plotRows) {
      parts.push(formatPlot(p as PlotRow));
    }
  }

  return parts.join("\n");
}
