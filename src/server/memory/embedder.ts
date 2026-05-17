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

export interface Embedder {
  embed(text: string): Promise<number[]>;
  embedBatch(texts: string[]): Promise<number[][]>;
  readonly dimensions: number;
}

class LlamaEmbedder implements Embedder {
  readonly dimensions: number;
  private url: string;

  constructor(url: string, dimensions: number) {
    this.url = url;
    this.dimensions = dimensions;
  }

  private async post(body: unknown): Promise<number[][]> {
    const res = await fetch(this.url, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      const err = await res.text();
      throw new Error(`Embedder returned ${res.status}: ${err.slice(0, 300)}`);
    }
    const json = (await res.json()) as Record<string, unknown>;
    if (json.data && Array.isArray(json.data)) {
      return (json.data as Array<{ embedding?: number[] }>).map((d) => d.embedding || []);
    }
    throw new Error(`Embedder unexpected response: ${JSON.stringify(json).slice(0, 300)}`);
  }

  async embed(text: string): Promise<number[]> {
    const embeddings = await this.post({ model: "embedding", input: text });
    return embeddings[0];
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    return this.post({ model: "embedding", input: texts });
  }
}

let embedder: Embedder | null = null;

export function getEmbedder(): Embedder {
  if (embedder) return embedder;

  const url = process.env.LLAMA_EMBED_URL || "http://localhost:8080/v1/embeddings";
  const dims = parseInt(process.env.EMBEDDING_DIMENSIONS || "1024", 10);

  embedder = new LlamaEmbedder(url, dims);
  console.log(`[embedder] ${dims}d at ${url}`);
  return embedder;
}
