export interface Embedder {
  embed(text: string): Promise<number[]>;
  embedBatch(texts: string[]): Promise<number[][]>;
  readonly dimensions: number;
}

// Default model: all-MiniLM-L6-v2, 384 dimensions, ~80MB
const DEFAULT_LOCAL_MODEL = "Xenova/all-MiniLM-L6-v2";
const LOCAL_DIMENSIONS = 384;

let localPipeline: unknown = null;

async function getLocalPipeline() {
  if (!localPipeline) {
    const { pipeline } = await import("@xenova/transformers");
    localPipeline = await pipeline("feature-extraction", DEFAULT_LOCAL_MODEL);
  }
  return localPipeline as {
    (
      text: string,
      options?: { pooling?: string },
    ): Promise<{ data: Float32Array }>;
  };
}

export class LocalEmbedder implements Embedder {
  readonly dimensions = LOCAL_DIMENSIONS;

  async embed(text: string): Promise<number[]> {
    const pipe = await getLocalPipeline();
    const result = await pipe(text, { pooling: "mean" });
    return Array.from(result.data);
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    // Process sequentially to avoid memory pressure from ONNX
    const results: number[][] = [];
    for (const text of texts) {
      results.push(await this.embed(text));
    }
    return results;
  }
}

export class OpenAICompatibleEmbedder implements Embedder {
  readonly dimensions: number;
  private baseURL: string;
  private apiKey: string;
  private model: string;

  constructor(
    baseURL: string,
    apiKey: string,
    model: string = "text-embedding-3-small",
    dimensions: number = 1536,
  ) {
    this.baseURL = baseURL;
    this.apiKey = apiKey;
    this.model = model;
    this.dimensions = dimensions;
  }

  async embed(text: string): Promise<number[]> {
    const res = await fetch(`${this.baseURL}/embeddings`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.apiKey}`,
      },
      body: JSON.stringify({ model: this.model, input: text }),
    });
    const json = (await res.json()) as {
      data: Array<{ embedding: number[] }>;
    };
    return json.data[0].embedding;
  }

  async embedBatch(texts: string[]): Promise<number[][]> {
    const res = await fetch(`${this.baseURL}/embeddings`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${this.apiKey}`,
      },
      body: JSON.stringify({ model: this.model, input: texts }),
    });
    const json = (await res.json()) as {
      data: Array<{ embedding: number[] }>;
    };
    return json.data.map((d) => d.embedding);
  }
}

let embedder: Embedder | null = null;

export function getEmbedder(): Embedder {
  if (embedder) return embedder;

  const apiUrl = process.env.EMBEDDING_API_URL;
  const apiKey = process.env.EMBEDDING_API_KEY;

  if (apiUrl && apiKey) {
    const model = process.env.EMBEDDING_MODEL || "text-embedding-3-small";
    const dims = parseInt(process.env.EMBEDDING_DIMENSIONS || "1536", 10);
    embedder = new OpenAICompatibleEmbedder(apiUrl, apiKey, model, dims);
  } else {
    embedder = new LocalEmbedder();
  }

  return embedder;
}
