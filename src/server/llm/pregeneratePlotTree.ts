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

import { generateText } from "ai";
import { getAllEntitySummaries } from "@/server/models/world";
import { getActiveSeedStory } from "@/server/seed-stories";
import { getModel } from "@/server/llm/model";

export interface PlotDef {
  index: number;
  title: string;
  description: string;
  status: string;
  involvedLocations: string[];
  involvedCharacters: string[];
  childPlots: Array<{ childPlotIndex: number | null; triggerCondition: string }>;
}

export async function generatePlotDefs(size: number): Promise<PlotDef[]> {
  const seedStory = getActiveSeedStory();
  const summaries = getAllEntitySummaries();

  const entityLines = summaries
    .map((e) => `  ${e.id} — "${e.displayName}" (${e.type}) — ${e.shortDescription}`)
    .join("\n");

  const prompt = [
    `You are designing a complete plot tree for a narrative RPG.`,
    ``,
    `SETTING: ${seedStory.settingDescription}`,
    `TONE: ${seedStory.toneDescription}`,
    ``,
    `Available entities (use these exact IDs):`,
    entityLines,
    ``,
    `Generate a plot tree with approximately ${size} nodes. Follow these rules:`,
    ``,
    `1. Plot index 0 is the ROOT — it has no parent. It represents the overarching story.`,
    `2. Each plot is a BROAD narrative arc (chapter/quest), not a single scene or dialogue beat.`,
    `3. childPlots define narrative branch directions — each triggerCondition describes a story-level choice ("Player sides with the rebels"), not a specific dialogue line.`,
    `4. Status must be "PENDING" for all nodes.`,
    `5. The tree should be SHALLOW and WIDE — prefer 2-3 levels with many branches over deep nesting.`,
    `6. Use exact entity IDs from the list above for involvedLocations and involvedCharacters.`,
    `7. Every non-root plot must be referenced by exactly one parent's childPlots via childPlotIndex.`,
    `8. Each non-leaf node should have 2-5 childPlots (some with childPlotIndex set to a valid index, some null for future expansion).`,
    `9. The tree must be connected — every node must be reachable from the root.`,
    ``,
    `Output ONLY valid JSON in this exact format (no markdown, no explanation):`,
    `{"plots":[{"index":0,"title":"string","description":"string","status":"PENDING","involvedLocations":["id"],"involvedCharacters":["id"],"childPlots":[{"childPlotIndex":1,"triggerCondition":"string"},{"childPlotIndex":null,"triggerCondition":"string"}]}]}`,
  ].join("\n");

  const { model } = getModel();
  const result = await generateText({ model, messages: [{ role: "user", content: prompt }] });

  // Strip markdown code fences before extracting JSON — the regex /\{[\s\S]*\}/
  // would otherwise match across fences and include non-JSON text.
  let text = result.text.trim();
  text = text.replace(/^```(?:json)?\s*\n?/gm, "").replace(/```\s*$/gm, "").trim();
  const jsonMatch = text.match(/\{[\s\S]*\}/);
  if (!jsonMatch) throw new Error("Failed to parse plot tree JSON from LLM response");

  let parsed: { plots: PlotDef[] };
  try {
    parsed = JSON.parse(jsonMatch[0]);
  } catch {
    throw new Error("Invalid JSON in LLM response for plot tree");
  }

  if (!Array.isArray(parsed.plots) || parsed.plots.length === 0) {
    throw new Error("No plots in generated tree");
  }

  const plotDefs = parsed.plots;

  // ── Validate LLM output before returning ─────────────────────────────────

  // Duplicate indices would silently corrupt the index→ID map
  const indexSet = new Set<number>();
  for (const def of plotDefs) {
    if (indexSet.has(def.index)) {
      throw new Error(`Duplicate plot index ${def.index} in generated tree`);
    }
    indexSet.add(def.index);
  }

  // Root (index 0) must exist
  if (!indexSet.has(0)) {
    throw new Error("Root plot (index 0) is required but missing from generated tree");
  }

  // Every childPlotIndex must reference an existing plot index
  for (const def of plotDefs) {
    for (const cp of def.childPlots) {
      if (cp.childPlotIndex !== null && !indexSet.has(cp.childPlotIndex)) {
        throw new Error(
          `Plot ${def.index} references non-existent childPlotIndex ${cp.childPlotIndex}`,
        );
      }
    }
  }

  // Every non-root plot must be reachable from root via childPlots
  const referenced = new Set<number>();
  for (const def of plotDefs) {
    for (const cp of def.childPlots) {
      if (cp.childPlotIndex !== null) referenced.add(cp.childPlotIndex);
    }
  }
  for (const def of plotDefs) {
    if (def.index !== 0 && !referenced.has(def.index)) {
      throw new Error(
        `Plot ${def.index} ("${def.title}") is not referenced by any parent's childPlots — orphans are not allowed`,
      );
    }
  }

  return plotDefs;
}
