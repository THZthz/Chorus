import db from "@/server/db";
import type { Plot, PlotOption, PlotPatch } from "@/types/plot";
import { nextId } from "@/server/models/ids";

function rowToPlot(row: any): Plot {
  return {
    id: row.id,
    title: row.title,
    description: row.description,
    status: row.status,
    involvedLocations: JSON.parse(row.involved_locations ?? "[]"),
    involvedCharacters: JSON.parse(row.involved_characters ?? "[]"),
    parentPlotId: row.parent_plot_id ?? null,
    parentOptionId: row.parent_option_id ?? null,
    childPlots: JSON.parse(row.child_plots ?? "[]"),
  };
}

export function getAllPlots(): Plot[] {
  return (db.prepare("SELECT * FROM plots").all() as any[]).map(rowToPlot);
}

export function getPlotById(id: string): Plot | null {
  const row = db.prepare("SELECT * FROM plots WHERE id = ?").get(id) as any;
  return row ? rowToPlot(row) : null;
}

export function getPlotsByIds(ids: string[]): Plot[] {
  if (ids.length === 0) return [];
  const placeholders = ids.map(() => "?").join(", ");
  return (db.prepare(`SELECT * FROM plots WHERE id IN (${placeholders})`).all(...ids) as any[]).map(
    rowToPlot,
  );
}

export function getActivePlots(): Plot[] {
  return (
    db.prepare("SELECT * FROM plots WHERE status IN ('PENDING', 'IN_PROGRESS')").all() as any[]
  ).map(rowToPlot);
}

export function getRootPlot(): Plot | null {
  const row = db.prepare("SELECT * FROM plots WHERE parent_plot_id IS NULL LIMIT 1").get() as any;
  return row ? rowToPlot(row) : null;
}

type AddPlotInput = Omit<Plot, "id"> & { id?: string };

export function addPlot(input: AddPlotInput): { ok: true; id: string } | { ok: false; error: string } {
  const id = input.id ?? `plot_${nextId()}`;
  const isRoot = input.parentPlotId === null;

  if (isRoot) {
    const existingRoot = getRootPlot();
    if (existingRoot) {
      return {
        ok: false,
        error: `A root plot already exists: "${existingRoot.title}" (${existingRoot.id}). New plots must link to a parent via parentPlotId and parentOptionId.`,
      };
    }
  } else {
    const parent = getPlotById(input.parentPlotId!);
    if (!parent) {
      return {
        ok: false,
        error: `Parent plot "${input.parentPlotId}" not found. Use getPlot() to see available plots.`,
      };
    }
    if (parent.status === "RESOLVED") {
      return {
        ok: false,
        error: `Parent plot "${parent.title}" (${input.parentPlotId}) is RESOLVED and cannot have new child plots.`,
      };
    }
    if (input.parentOptionId !== null && input.parentOptionId !== undefined) {
      if (input.parentOptionId < 0 || input.parentOptionId >= parent.childPlots.length) {
        return {
          ok: false,
          error: `parentOptionId ${input.parentOptionId} is out of range. Parent plot "${parent.title}" has ${parent.childPlots.length} childPlots (indices 0–${parent.childPlots.length - 1}).`,
        };
      }
    }
  }

  db.prepare(
    `INSERT INTO plots (id, title, description, status, involved_locations, involved_characters, parent_plot_id, parent_option_id, child_plots)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)`,
  ).run(
    id,
    input.title,
    input.description,
    input.status ?? "PENDING",
    JSON.stringify(input.involvedLocations ?? []),
    JSON.stringify(input.involvedCharacters ?? []),
    input.parentPlotId ?? null,
    input.parentOptionId ?? null,
    JSON.stringify(input.childPlots ?? []),
  );

  // Auto-link: update parent's childPlots[parentOptionId].plotId
  if (input.parentPlotId && input.parentOptionId !== null && input.parentOptionId !== undefined) {
    const parent = getPlotById(input.parentPlotId)!;
    const updatedChildren = [...parent.childPlots];
    updatedChildren[input.parentOptionId] = {
      ...updatedChildren[input.parentOptionId],
      plotId: id,
    };
    db.prepare("UPDATE plots SET child_plots = ? WHERE id = ?").run(
      JSON.stringify(updatedChildren),
      input.parentPlotId,
    );
  }

  const treeError = validatePlotTree();
  if (treeError) {
    // Roll back
    db.prepare("DELETE FROM plots WHERE id = ?").run(id);
    if (input.parentPlotId && input.parentOptionId !== null && input.parentOptionId !== undefined) {
      const parent = getPlotById(input.parentPlotId)!;
      const restored = [...parent.childPlots];
      restored[input.parentOptionId!] = { ...restored[input.parentOptionId!], plotId: null };
      db.prepare("UPDATE plots SET child_plots = ? WHERE id = ?").run(
        JSON.stringify(restored),
        input.parentPlotId,
      );
    }
    return { ok: false, error: `Plot tree validation failed after insert: ${treeError}` };
  }

  return { ok: true, id };
}

export function updatePlot(
  id: string,
  patch: PlotPatch,
): { ok: true } | { ok: false; error: string } {
  const existing = getPlotById(id);
  if (!existing) {
    return { ok: false, error: `Plot "${id}" not found. Use getPlot() to list available plots.` };
  }

  if (existing.status === "RESOLVED") {
    return {
      ok: false,
      error: `Plot "${existing.title}" (${id}) is RESOLVED and cannot be altered. Only PENDING or IN_PROGRESS plots may be modified.`,
    };
  }

  const updated: Plot = {
    ...existing,
    title: patch.title ?? existing.title,
    status: patch.status ?? existing.status,
    description: patch.description ?? existing.description,
    involvedLocations: patch.involvedLocations ?? existing.involvedLocations,
    involvedCharacters: patch.involvedCharacters ?? existing.involvedCharacters,
    childPlots: patch.childPlots ?? existing.childPlots,
  };

  db.prepare(
    `UPDATE plots SET title = ?, status = ?, description = ?, involved_locations = ?, involved_characters = ?, child_plots = ? WHERE id = ?`,
  ).run(
    updated.title,
    updated.status,
    updated.description,
    JSON.stringify(updated.involvedLocations),
    JSON.stringify(updated.involvedCharacters),
    JSON.stringify(updated.childPlots),
    id,
  );

  const treeError = validatePlotTree();
  if (treeError) {
    // Restore original
    db.prepare(
      `UPDATE plots SET title = ?, status = ?, description = ?, involved_locations = ?, involved_characters = ?, child_plots = ? WHERE id = ?`,
    ).run(
      existing.title,
      existing.status,
      existing.description,
      JSON.stringify(existing.involvedLocations),
      JSON.stringify(existing.involvedCharacters),
      JSON.stringify(existing.childPlots),
      id,
    );
    return { ok: false, error: `Plot tree validation failed: ${treeError}` };
  }

  return { ok: true };
}

export function validatePlotTree(): string | null {
  const plots = getAllPlots();
  const plotMap = new Map(plots.map((p) => [p.id, p]));

  const roots = plots.filter((p) => p.parentPlotId === null);
  if (roots.length > 1) {
    return `Multiple root plots found: ${roots.map((r) => `"${r.title}" (${r.id})`).join(", ")}. Only one root is allowed.`;
  }

  for (const plot of plots) {
    if (plot.parentPlotId !== null && !plotMap.has(plot.parentPlotId)) {
      return `Plot "${plot.title}" (${plot.id}) references non-existent parent "${plot.parentPlotId}".`;
    }
    for (const opt of plot.childPlots) {
      if (opt.plotId !== null && !plotMap.has(opt.plotId)) {
        return `Plot "${plot.title}" (${plot.id}) has a childPlot option referencing non-existent plot "${opt.plotId}".`;
      }
    }
  }

  return null;
}

export function buildActivePlotTree(): string {
  const allPlots = getAllPlots();
  if (allPlots.length === 0) return "(no plots yet)";

  const plotMap = new Map(allPlots.map((p) => [p.id, p]));
  const activePlots = allPlots.filter((p) => p.status !== "RESOLVED");

  function renderPlot(plot: Plot, depth: number): string {
    if (!activePlots.includes(plot)) return "";
    const indent = "  ".repeat(depth);
    const statusTag = `[${plot.status.replace("_", " ")}]`;
    const locations = plot.involvedLocations.join(", ");
    const characters = plot.involvedCharacters.join(", ");

    const lines: string[] = [];
    const marker = depth === 0 ? "▶" : "└─";
    lines.push(`${indent}${marker} ${plot.id} ${statusTag} "${plot.title}"`);
    if (locations) lines.push(`${indent}   locations_ids [${locations}]`);
    if (characters) lines.push(`${indent}   characters_ids [${characters}]`);
    lines.push(`${indent}   ${plot.description}`);

    if (plot.childPlots.length > 0) {
      lines.push(`${indent}   Options:`);
      for (let i = 0; i < plot.childPlots.length; i++) {
        const opt = plot.childPlots[i];
        const childLabel = opt.plotId
          ? (() => {
              const child = plotMap.get(opt.plotId);
              return child
                ? `→ ${opt.plotId} [${child.status.replace("_", " ")}] "${child.title}"`
                : `→ ${opt.plotId} (not found)`;
            })()
          : "→ (plot not created yet)";
        lines.push(`${indent}     [${i}] ${childLabel} if "${opt.triggerCondition}"`);
      }
    }

    for (const opt of plot.childPlots) {
      if (opt.plotId) {
        const child = plotMap.get(opt.plotId);
        if (child) {
          const childText = renderPlot(child, depth + 1);
          if (childText) lines.push(childText);
        }
      }
    }

    return lines.join("\n");
  }

  const root = allPlots.find((p) => p.parentPlotId === null);
  if (!root) return "(no root plot found — use createPlot without parentPlotId to start the story)";

  return (
    renderPlot(root, 0) +
    "\n\nUse getPlot(id) for full details. Entity IDs above map to entities in the WORLD ENTITIES section."
  );
}
