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

import { v4 as uuidv4 } from "uuid";
import { int } from "neo4j-driver";
import { Neo4jClient } from "@/server/memory/neo4j";
import { Embedder, getEmbedder } from "@/server/memory/embedder";
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
      status?: PlotStatus;
      triggerCondition?: string;
      flags?: PlotFlag[];
    },
  ): Promise<MemoryPlot> {
    const id = uuidv4();
    const description = options?.description ?? "";
    const status = options?.status ?? "PENDING";
    const triggerCondition = options?.triggerCondition ?? null;
    const flags = options?.flags ?? [];
    const embedding = await this.embedder.embed(`${name}: ${description}`);
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `CREATE (p:Plot {
         id: $id, name: $name, description: $description,
         status: $status, triggerCondition: $triggerCondition,
         flags: $flags, embedding: $embedding,
         created_at: datetime($now), updated_at: datetime($now)
       })`,
      {
        id,
        name,
        description,
        status,
        triggerCondition,
        flags: flags.length > 0 ? JSON.stringify(flags) : null,
        embedding,
        now,
      },
    );

    return {
      id,
      name,
      description,
      status,
      triggerCondition: triggerCondition ?? undefined,
      flags,
      embedding,
      createdAt: new Date(now),
      updatedAt: new Date(now),
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
      status?: PlotStatus;
      triggerCondition?: string | null;
    },
  ): Promise<MemoryPlot | null> {
    const existing = await this.getPlot(name);
    if (!existing) return null;

    const newStatus = options.status ?? existing.status;
    const newDescription = options.description ?? existing.description;
    const newTrigger =
      options.triggerCondition !== undefined
        ? options.triggerCondition
        : (existing.triggerCondition ?? null);
    const embedding = options.description
      ? await this.embedder.embed(`${name}: ${options.description}`)
      : existing.embedding;
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `MATCH (p:Plot {name: $name})
       SET p.description = $description, p.status = $status,
           p.triggerCondition = $triggerCondition, p.embedding = $embedding,
           p.updated_at = datetime($now)`,
      {
        name,
        description: newDescription,
        status: newStatus,
        triggerCondition: newTrigger,
        embedding: embedding || null,
        now,
      },
    );

    return {
      ...existing,
      description: newDescription,
      status: newStatus,
      triggerCondition: newTrigger ?? undefined,
      embedding,
      updatedAt: new Date(now),
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
       SET p.flags = $flags, p.updated_at = datetime($now)`,
      { name: plotName, flags: JSON.stringify(flags), now },
    );

    return { ...existing, flags, updatedAt: new Date(now) };
  }

  async removeFlag(plotName: string, flagId: string): Promise<MemoryPlot | null> {
    const existing = await this.getPlot(plotName);
    if (!existing) return null;

    const flags = existing.flags.filter((f) => f.flagId !== flagId);
    const now = new Date().toISOString();

    await this.client.executeWrite(
      `MATCH (p:Plot {name: $name})
       SET p.flags = $flags, p.updated_at = datetime($now)`,
      { name: plotName, flags: flags.length > 0 ? JSON.stringify(flags) : null, now },
    );

    return { ...existing, flags, updatedAt: new Date(now) };
  }

  async getFlags(plotName: string): Promise<PlotFlag[]> {
    const plot = await this.getPlot(plotName);
    return plot?.flags ?? [];
  }

  // ── Branching ──

  async branchTo(parentPlotName: string, childPlotName: string): Promise<boolean> {
    const rows = await this.client.executeWrite(
      `MATCH (parent:Plot {name: $parent}), (child:Plot {name: $child})
       MERGE (parent)-[:BRANCHES_TO]->(child)
       RETURN parent.name AS parentName`,
      { parent: parentPlotName, child: childPlotName },
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

  // ── Search ──

  async searchPlots(
    query: string,
    options?: { limit?: number; threshold?: number },
  ): Promise<Array<MemoryPlot & { similarity: number }>> {
    const { limit = 10, threshold = 0.7 } = options || {};
    const queryEmbedding = await this.embedder.embed(query);

    const rows = await this.client.executeRead(
      `CALL db.index.vector.queryNodes('plot_embedding_idx', $limit, $embedding)
       YIELD node AS p, score WHERE score >= $threshold
       RETURN p, score ORDER BY score DESC`,
      { embedding: queryEmbedding, limit: int(limit), threshold },
    );

    return rows.map((r) => ({
      ...this.parsePlot(r.p as Record<string, unknown>),
      similarity: r.score as number,
    }));
  }

  // ── Time Relationships ──

  async markPlotStarted(plotName: string): Promise<void> {
    await this.client.executeWrite(
      `MATCH (a:TimeAnchor {id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
       MATCH (p:Plot {name: $name})
       MERGE (p)-[:STARTED_AT]->(tp)`,
      { name: plotName },
    );
  }

  async markPlotActive(plotName: string): Promise<void> {
    await this.client.executeWrite(
      `MATCH (a:TimeAnchor {id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
       MATCH (p:Plot {name: $name})
       MERGE (p)-[:ACTIVE_AT]->(tp)`,
      { name: plotName },
    );
  }

  async markPlotCompleted(plotName: string): Promise<void> {
    await this.client.executeWrite(
      `MATCH (a:TimeAnchor {id: 'anchor'})-[:CURRENT_TIMEPOINT]->(tp:TimePoint)
       MATCH (p:Plot {name: $name})
       MERGE (p)-[:COMPLETED_AT]->(tp)`,
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
      id: data.id as string,
      name: data.name as string,
      description: (data.description as string) || "",
      status: (data.status as PlotStatus) || "PENDING",
      triggerCondition: (data.triggerCondition as string) || undefined,
      flags,
      embedding: data.embedding as number[] | undefined,
      createdAt: new Date((data.created_at as string | number) || Date.now()),
      updatedAt: new Date((data.updated_at as string | number) || Date.now()),
    };
  }
}
