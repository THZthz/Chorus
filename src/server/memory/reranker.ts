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

export interface Reranker {
  rerank(
    query: string,
    documents: string[],
    topN?: number,
  ): Promise<Array<{ index: number; score: number }>>;
}

class HttpReranker implements Reranker {
  constructor(private url: string) {}

  async rerank(
    query: string,
    documents: string[],
    topN?: number,
  ): Promise<Array<{ index: number; score: number }>> {
    const body: Record<string, unknown> = { model: "reranking", query, documents };
    if (topN !== undefined) {
      body["top_n"] = topN;
    }

    const res = await fetch(this.url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });

    if (!res.ok) {
      const err = await res.text();
      throw new Error(`Reranker API returned ${res.status}: ${err.slice(0, 200)}`);
    }

    const json = (await res.json()) as {
      results: Array<{ index: number; relevance_score: number }>;
    };
    return json.results.map((r) => ({ index: r.index, score: r.relevance_score }));
  }
}

let reranker: Reranker | null | undefined;

export function getReranker(): Reranker | null {
  if (reranker !== undefined) return reranker;

  const url = process.env.LLAMA_RERANK_URL;
  if (!url) {
    reranker = null;
    return null;
  }

  reranker = new HttpReranker(url);
  console.log(`[reranker] ${url}`);
  return reranker;
}

// ── Shared post-processing helper ──

import { NodeManager } from "@/server/nodeManager";

export function extractSearchTexts<T>(items: T[], kind: string): Array<T & { text: string }> {
  const nodeManager = NodeManager.getCachedInstance();
  return items.map((item) => {
    const obj = item as Record<string, unknown>;
    const text = nodeManager.getEmbeddingText(kind, obj);
    if (!text) {
      const fallback =
        (obj.content as string) || (obj.description as string) || (obj.name as string) || "";
      return { ...item, text: fallback };
    }
    return { ...item, text };
  });
}

export async function applyRerank<T>(
  query: string,
  items: Array<T & { text: string }>,
  topN?: number,
): Promise<Array<T & { relevance: number }>> {
  const r = getReranker();
  if (!r || items.length === 0) {
    return items.map((item) => {
      const s = (item as Record<string, unknown>).similarity as number;
      const { text: _, ...clean } = item as unknown as Record<string, unknown>;
      return { ...clean, relevance: s || 0 } as T & { relevance: number };
    });
  }

  const docs = items.map((i) => i.text);
  const results = await r.rerank(query, docs, topN ?? items.length);

  return results.map((result) => {
    const { text: _, ...clean } = items[result.index] as unknown as Record<string, unknown>;
    return { ...clean, relevance: result.score } as T & { relevance: number };
  });
}
