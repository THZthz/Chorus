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

import { v4 as uuidv4 } from "uuid";
import { int } from "neo4j-driver";
import { Neo4jClient } from "@/server/memory/neo4j";
import { Embedder, getEmbedder } from "@/server/memory/embedder";
import { getReranker, extractSearchTexts, applyRerank } from "@/server/memory/reranker";
import type { MemoryPlot, PlotFlag, PlotStatus } from "@/server/memory/types";

export class Plots {
  private client: Neo4jClient;
  private embedder: Embedder;

  constructor(client: Neo4jClient) {
    this.client = client;
    this.embedder = getEmbedder();
  }

  async createPlot(
    name: string,
    options?: {
      description?: string;
      brief?: string;
      status?: PlotStatus;
      triggerCondition?: string;
      flags?: PlotFlag[];
    },
  ): Promise<MemoryPlot> {
    const id = uuidv4();
    const description = options?.description ?? "";
    const brief = options?.brief ?? null;
    const status = options?.status ?? "PENDING";
    const triggerCondition = options?.triggerCondition ?? null;
    const flags = options?.flags ?? [];
    const { NodeManager: NM } = await import("@/server/memory/nodeManager");
    const embedText = NM.getCachedInstance().getEmbeddingText("Plot", {
      name,
      description,
      brief: brief ?? "",
    });
    const embedding = embedText ? await this.embedder.embed(embedText) : undefined;
    const now = new Date().toISOString();

    const rows = await this.client.executeWrite(
      `MERGE (p:Plot {name: $name})
       ON CREATE SET
         p._id = $id,
         p.description = $description,
         p.brief = $brief,
         p.status = $status,
         p.flags = $flags,
         p._embedding = $embedding,
         p.trigger_condition = $triggerCondition,
         p._created_at = datetime($now),
         p._updated_at = datetime($now)
       ON MATCH SET
         p._updated_at = datetime($now)
       RETURN p`,
      {
        id,
        name,
        description,
        brief,
        status,
        triggerCondition,
        flags: flags.length > 0 ? JSON.stringify(flags) : null,
        embedding,
        now,
      },
    );

    const node = (rows[0]?.p as Record<string, unknown>) || {};
    return {
      name,
      description: (node.description as string) || description,
      brief: (node.brief as string) || brief || undefined,
      status: (node.status as PlotStatus) || status,
      triggerCondition: (node.trigger_condition as string) || (triggerCondition ?? undefined),
      flags,
      _embedding: embedding,
    };
  }

  async getPlot(name: string): Promise<MemoryPlot | null> {
    const rows = await this.client.executeRead(`MATCH (p:Plot {name: $name}) RETURN p`, { name });
    if (rows.length === 0) return null;
    return this.parsePlot(rows[0].p as Record<string, unknown>);
  }

  async updatePlot(
    name: string,
    options: {
      description?: string;
      brief?: string;
      status?: PlotStatus;
      triggerCondition?: string | null;
    },
  ): Promise<MemoryPlot | null> {
    const existing = await this.getPlot(name);
    if (!existing) return null;

    const newStatus = options.status ?? existing.status;
    const newDescription = options.description ?? existing.description;
    const newBrief = options.brief !== undefined ? options.brief : existing.brief;
    const newTrigger =
      options.triggerCondition !== undefined
        ? options.triggerCondition
        : (existing.triggerCondition ?? null);
    const embedding = options.description
      ? await this.embedder.embed(`${name}: ${options.description}`)
      : existing._embedding;
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `MATCH (p:Plot {name: $name})
       SET p.description = $description, p.brief = $brief, p.status = $status,
           p.trigger_condition = $triggerCondition, p._embedding = $embedding,
           p._updated_at = datetime($now)`,
      {
        name,
        description: newDescription,
        brief: newBrief || null,
        status: newStatus,
        triggerCondition: newTrigger,
        embedding: embedding || null,
        now,
      },
    );

    return {
      ...existing,
      description: newDescription,
      brief: newBrief,
      status: newStatus,
      triggerCondition: newTrigger ?? undefined,
      _embedding: embedding,
    };
  }

  async deletePlot(name: string): Promise<boolean> {
    const result = await this.client.executeWrite(
      `MATCH (p:Plot {name: $name}) DETACH DELETE p RETURN count(p) AS deleted`,
      { name },
    );
    return (result[0]?.deleted as number) > 0;
  }

  // ── Flags ──

  async setFlag(plotName: string, flagId: string, description: string): Promise<MemoryPlot | null> {
    const existing = await this.getPlot(plotName);
    if (!existing) return null;

    const flags = existing.flags.filter((f) => f.flagId !== flagId);
    flags.push({ flagId, description });
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `MATCH (p:Plot {name: $name})
       SET p.flags = $flags, p._updated_at = datetime($now)`,
      { name: plotName, flags: JSON.stringify(flags), now },
    );

    return { ...existing, flags };
  }

  async removeFlag(plotName: string, flagId: string): Promise<MemoryPlot | null> {
    const existing = await this.getPlot(plotName);
    if (!existing) return null;

    const flags = existing.flags.filter((f) => f.flagId !== flagId);
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `MATCH (p:Plot {name: $name})
       SET p.flags = $flags, p._updated_at = datetime($now)`,
      { name: plotName, flags: flags.length > 0 ? JSON.stringify(flags) : null, now },
    );

    return { ...existing, flags };
  }

  async getFlags(plotName: string): Promise<PlotFlag[]> {
    const plot = await this.getPlot(plotName);
    return plot?.flags ?? [];
  }

  // ── Branching ──

  async branchTo(parentPlotName: string, childPlotName: string): Promise<boolean> {
    const rows = await this.client.mergeRelationship(
      "Plot",
      "name",
      parentPlotName,
      "Plot",
      "name",
      childPlotName,
      "BRANCHES_TO",
    );
    return rows.length > 0;
  }

  async unbranch(parentPlotName: string, childPlotName: string): Promise<boolean> {
    await this.client.executeWrite(
      `MATCH (parent:Plot {name: $parent})-[r:BRANCHES_TO]->(child:Plot {name: $child})
       DELETE r`,
      { parent: parentPlotName, child: childPlotName },
    );
    return true;
  }

  async getChildPlots(plotName: string): Promise<MemoryPlot[]> {
    const rows = await this.client.executeRead(
      `MATCH (p:Plot {name: $name})-[:BRANCHES_TO]->(child:Plot) RETURN child`,
      { name: plotName },
    );
    return rows.map((r) => this.parsePlot(r.child as Record<string, unknown>));
  }

  // ── Time Relationships ──

  async markPlotStarted(plotName: string): Promise<void> {
    await this.client.executeWrite(
      `MATCH (a:TimeAnchor {_id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
       MATCH (p:Plot {name: $name})
       MERGE (p)-[r:STARTED_AT]->(tp)
       ON CREATE SET r._created_at = datetime()`,
      { name: plotName },
    );
  }

  async markPlotActive(plotName: string): Promise<void> {
    await this.client.executeWrite(
      `MATCH (a:TimeAnchor {_id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
       MATCH (p:Plot {name: $name})
       MERGE (p)-[r:ACTIVE_AT]->(tp)
       ON CREATE SET r._created_at = datetime()`,
      { name: plotName },
    );
  }

  async markPlotCompleted(plotName: string): Promise<void> {
    await this.client.executeWrite(
      `MATCH (a:TimeAnchor {_id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
       MATCH (p:Plot {name: $name})
       MERGE (p)-[r:COMPLETED_AT]->(tp)
       ON CREATE SET r._created_at = datetime()`,
      { name: plotName },
    );
  }

  // ── Parse ──

  private parsePlot(data: Record<string, unknown>): MemoryPlot {
    let flags: PlotFlag[] = [];
    if (typeof data.flags === "string") {
      try {
        flags = JSON.parse(data.flags) as PlotFlag[];
      } catch {
        flags = [];
      }
    }
    return {
      name: data.name as string,
      description: (data.description as string) || "",
      brief: (data.brief as string) || undefined,
      status: (data.status as PlotStatus) || "PENDING",
      triggerCondition: (data.trigger_condition as string) || undefined,
      flags,
      _embedding: data._embedding as number[] | undefined,
    };
  }
}
