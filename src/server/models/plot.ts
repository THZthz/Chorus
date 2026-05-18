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

import type { SceneObserver } from "@/server/llm/sceneObserver";

export interface PlotRef {
  name: string;
  description: string;
  brief: string | null;
  status: string;
  triggerCondition?: string | null;
  children: PlotRef[];
}

export function parseFlags(flags: unknown): Array<{ flagId: string; description: string }> {
  if (typeof flags === "string") {
    try {
      return JSON.parse(flags) as Array<{ flagId: string; description: string }>;
    } catch {
      return [];
    }
  }
  return (flags as Array<{ flagId: string; description: string }>) || [];
}

export function buildPlotTree(
  plots: PlotRef[],
  observer?: SceneObserver,
): { tree: string; unseenDescriptions: string } {
  const activeNames = new Set(plots.map((p) => p.name));
  const childNames = new Set<string>();
  for (const p of plots) {
    for (const c of p.children) {
      if (activeNames.has(c.name)) childNames.add(c.name);
    }
  }
  const roots = plots.filter((p) => !childNames.has(p.name));

  const visited = new Set<string>();
  const treeLines: string[] = [];
  const fullDescs: string[] = [];

  function renderNode(plot: PlotRef, prefix: string, isLast: boolean, connector: string) {
    if (visited.has(plot.name)) return;
    visited.add(plot.name);

    const brief = plot.brief || (plot.description || "").slice(0, 120);
    treeLines.push(`${prefix}${connector} ${plot.name} (${plot.status}): ${brief}`);

    if (observer && !observer.wasSeen("plot", plot.name)) {
      fullDescs.push(`### ${plot.name}\n${plot.description}`);
      observer.markSeen("plot", plot.name);
    }

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
