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

export interface Reranker {
  rerank(
    query: string,
    documents: string[],
    topN?: number,
  ): Promise<Array<{ index: number; score: number }>>;
}

class HttpReranker implements Reranker {
  private url: string;
  private model: string;
  private apiKey?: string;

  constructor(url: string, model: string, apiKey?: string) {
    this.url = url;
    this.model = model;
    this.apiKey = apiKey;
  }

  async rerank(
    query: string,
    documents: string[],
    topN?: number,
  ): Promise<Array<{ index: number; score: number }>> {
    const headers: Record<string, string> = { "Content-Type": "application/json" };
    if (this.apiKey) {
      headers["Authorization"] = `Bearer ${this.apiKey}`;
    }

    const body: Record<string, unknown> = {
      model: this.model,
      query,
      documents,
    };
    if (topN !== undefined) {
      body["top_n"] = topN;
    }

    const res = await fetch(this.url, {
      method: "POST",
      headers,
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

  const url = process.env.RERANK_API_URL;
  if (!url) {
    reranker = null;
    return null;
  }

  const model = process.env.RERANK_MODEL || "qwen3-reranker-0.6b";
  const apiKey = process.env.RERANK_API_KEY || undefined;
  reranker = new HttpReranker(url, model, apiKey);
  console.log(`[reranker] using ${model} at ${url}`);
  return reranker;
}

// ── Shared post-processing helper ──

export function extractSearchTexts<T>(
  items: T[],
  kind: "message" | "entity" | "note" | "plot",
): Array<T & { text: string }> {
  return items.map((item) => {
    const obj = item as Record<string, unknown>;
    let text = "";
    switch (kind) {
      case "message":
        text = (obj.content as string) || "";
        break;
      case "entity": {
        const name = (obj.name as string) || "";
        const type = (obj.type as string) || "";
        const desc = (obj.description as string) || (obj.brief as string) || "";
        text = `${name} (${type}): ${desc}`;
        break;
      }
      case "note":
        text = `${(obj.name as string) || ""}: ${(obj.content as string) || ""}`;
        break;
      case "plot":
        text = `${(obj.name as string) || ""}: ${(obj.description as string) || ""}`;
        break;
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
      return { ...item, relevance: s || 0 };
    });
  }

  const docs = items.map((i) => i.text);
  const results = await r.rerank(query, docs, topN ?? items.length);

  return results.map((result) => {
    const item = items[result.index];
    return { ...item, relevance: result.score, _rerankScore: result.score } as T & { relevance: number };
  });
}
