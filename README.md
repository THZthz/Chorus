# Chorus

> WARNING: Very early in development stage. The story-telling experience is poor right now!

> NOTE: Current project status - focused on refining tools and prompts of GM.

> NOTE: This README might be outdated. Check DEVELOPER for updated details.

Cinematic RPG-style dialogue engine with a fantasy-steampunk setting. The AI Game Master generates branching narrative through tool-calling, streamed to a console client in real-time via SSE. Player choices are guided by twelve inner voices — each a distinct personality mapped to a character stat — with skill checks resolved through 2D6 dice rolls.

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
#   LLAMA_FORMATTER_URL=http://localhost:8082/v1/chat/completions
#   EMBEDDING_DIMENSIONS=1024

make install

# Terminal 1 — Embedding server
make embedding-server

# Terminal 2 — Reranker server (optional; improves search precision)
make rerank-server

# Terminal 3 — Neo4j container and Express server
make neo4j-start neo4j-wait
make server

# Terminal 4 — Play
make console
```

On first run, Neo4j is seeded with a default world, see `src/server/seed-stories/`.

## How It Works

1. **Player chooses an action** — dialogue option, skill check, or custom input
2. **AI Game Master responds** — the LLM uses Neo4j-backed tools to query world state (entities, relationships, facts, conversation history), then generates narrative
3. **Streaming to console** — typed SSE events deliver progressive messages in real-time
4. **Inner voices chime in** — twelve skills (Logic, Rhetoric, Empathy, Perception, Volition, Endurance, Sorcery, Suggestion, Instinct, Might, Clockwork, Alchemy) each have distinct personalities that comment on the situation

### World Memory

All game state lives in Neo4j: characters, locations, objects, factions, plots, relationships, facts, and conversation history. The GM accesses it through 16 locally-defined AI SDK tools in `src/server/memory/tools.ts` — semantic search, graph traversal, entity CRUD, relationship management, and Cypher queries. Embeddings and reranking are served via llama-server (Qwen3-Embedding + Qwen3-Reranker).

### Skill Checks

White checks (repeatable) and red checks (one-time, high-stakes). Formula: `2d6 + Stat >= Difficulty`. Probability is displayed before rolling. Natural 2 and 12 have distinct outcomes.

## Developer Documentation

See [DEVELOPER.md](DEVELOPER.md) for architecture details, API endpoints, game time system, event types, seed story system, and development workflow guides.

## License

[AGPL v3](./LICENSE)
