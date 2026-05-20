# Chorus

> WARNING: Early in development stage. Development only happens on branch `v3`!

Cinematic dialogue engine. The AI Game Master generates branching narrative through tool-calling, streamed to a console client in real-time via SSE. Player choices are guided by twelve inner voices — each a distinct personality mapped to a character stat — with skill checks resolved through 2D6 dice rolls.

## Tech Stack

| Layer     | Technology                                       |
|-----------|--------------------------------------------------|
| Console   | TypeScript, Node.js, chalk (`@inquirer/prompts`) |
| Backend   | Express, Neo4j                                   |
| AI        | Gemini / DeepSeek via Vercel AI SDK v6           |
| Streaming | Server-Sent Events                               |
| Memory    | Neo4j via local memory module                    |

## Getting Started

### Prerequisites

- Node.js 20+
- Docker (for Neo4j)
- [llama-server](https://github.com/ggml-org/llama.cpp) (for embeddings and reranking)
- A Gemini or DeepSeek API key

### Model Setup

Download the GGUF models into `data/models/`:

```bash
# Qwen3-Embedding (1024-dim bi-encoder)
wget -P data/models/ https://huggingface.co/Qwen/Qwen3-Embedding-0.6B-GGUF/resolve/main/Qwen3-Embedding-0.6B-Q8_0.gguf

# Qwen3-Reranker (cross-encoder)
wget -P data/models/ https://huggingface.co/Qwen/Qwen3-Reranker-0.6B-GGUF/resolve/main/Qwen3-Reranker-0.6B-Q8_0.gguf
```

### Setup

```bash
cp .env.example .env
# Add your keys to .env:
#
#   GEMINI_API_KEY=your_key_here
#   DEEPSEEK_API_KEY=your_key_here
#
#   # Llama-server endpoints (defaults work with default ports):
#   LLAMA_EMBED_URL=http://localhost:8080/v1/embeddings
#   LLAMA_RERANK_URL=http://localhost:8081/v1/rerank
#
#   EMBEDDING_DIMENSIONS=1024

npm install

# Terminal 1 — Embedding server
llama-server -m data/models/Qwen3-Embedding-0.6B-Q8_0.gguf --port 8080 -c 32768 -ngl 99 --embeddings

# Terminal 2 — Reranker server (optional; improves search precision)
llama-server -m data/models/Qwen3-Reranker-0.6B-Q8_0.gguf --port 8081 -c 32768 -ngl 99 --reranking

# Terminal 3 — Neo4j container and Express server
docker compose -f docker-compose.yml up -d # Will take some time to start
npm run server # Or: npm run server:dev

# Terminal 4 — Play
npm run console
```

On first run, Neo4j is seeded with a default world, see `src/server/seed-stories/`.

Other operations:

```bash
# Check all tests passed
npm run test

# Stop Neo4j container
docker compose -f docker-compose.yml down # Or with `-v` parameter

# Notify server to clear data
curl -X POST http://localhost:3000/api/reset
```

## How It Works

1. **Player chooses an action** — dialogue option, skill check, or custom input
2. **AI Game Master responds** — the LLM uses Neo4j-backed tools to query world state (entities, relationships, facts, conversation history), then generates narrative
3. **Streaming to console** — typed SSE events deliver progressive messages in real-time
4. **Inner voices chime in** — twelve skills (Logic, Rhetoric, Empathy, Perception, Volition, Endurance, Sorcery, Suggestion, Instinct, Might, Clockwork, Alchemy) each have distinct personalities that comment on the situation

### World Memory

All game states live in Neo4j: characters, locations, objects, factions, plots, relationships, facts, and conversation history. The GM accesses it through locally-defined AI SDK tools — semantic search, graph traversal, entity CRUD, relationship management, and Cypher queries. Embeddings and reranking are served via llama-server.

### Skill Checks

(outdated, under rework).

## Developer Documentation

See [DEVELOPER.md](DEVELOPER.md) for architecture details, API endpoints, game time system, event types, seed story system, and development workflow guides.

## License

[AGPL v3](./LICENSE)
