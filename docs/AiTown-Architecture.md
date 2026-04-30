# AI Town — Architecture

This document provides a comprehensive overview of AI Town's architecture, covering all layers from the client-side game UI to the server-side simulation engine, agent AI, and deployment infrastructure.

---

## Table of Contents

1. [High-Level Overview](#high-level-overview)
2. [Technology Stack](#technology-stack)
3. [Project Structure](#project-structure)
4. [Server-Side: Convex Backend](#server-side-convex-backend)
   - [Game Engine (`convex/engine/`)](#game-engine-convexengine)
   - [AI Town Game Logic (`convex/aiTown/`)](#ai-town-game-logic-convexaitown)
   - [Agent System (`convex/agent/`)](#agent-system-convexagent)
   - [World Management (`convex/world.ts`)](#world-management-convexworldts)
   - [Messages (`convex/messages.ts`)](#messages-convexmessagests)
   - [Cron Jobs (`convex/crons.ts`)](#cron-jobs-convexcronds)
   - [Utilities (`convex/util/`)](#utilities-convexutil)
5. [Database Schema](#database-schema)
6. [Client-Side: React + PixiJS (`src/`)](#client-side-react--pixijs-src)
7. [Data Flow](#data-flow)
8. [Deployment](#deployment)
9. [Design Goals & Limitations](#design-goals--limitations)

---

## High-Level Overview

AI Town is a virtual town simulator where AI agents and human players coexist, move around a 2D map, and have LLM-powered conversations with each other. The system is split into four layers:

| Layer | Location | Responsibility |
|-------|----------|---------------|
| **Game Engine** | `convex/engine/` | Generic, reusable engine for tick-based simulation, input handling, and state persistence |
| **AI Town Game Logic** | `convex/aiTown/` | Specific game rules: players, agents, conversations, movement, pathfinding |
| **Agent System** | `convex/agent/` | LLM-powered agent behavior: conversation generation, memory/vector search, embeddings |
| **Client UI** | `src/` | React + PixiJS renderer displaying the game state and accepting human input |

The engine is **single-threaded per world** — no two engine steps ever overlap. All game state mutations flow through the engine via an **input queue**. External systems (client UI, agent actions) submit inputs; the engine processes them sequentially during simulation steps.

---

## Technology Stack

| Component | Technology |
|-----------|-----------|
| **Runtime / Backend** | [Convex](https://convex.dev) (reactive backend-as-a-service) |
| **Frontend Framework** | React 18 + TypeScript |
| **2D Rendering** | PixiJS 7 via `@pixi/react` |
| **Styling** | Tailwind CSS 3 |
| **Build Tool** | Vite 4 |
| **LLM Provider** | OpenAI, Together.ai, Ollama (local), or custom API — configured via `convex/util/llm.ts` |
| **Vector Database** | Convex built-in vector index |
| **Embedding Model** | Configurable (default: `mxbai-embed-large` via Ollama, 1024-dim) |
| **Authentication** | Clerk (commented out, currently anonymous) |
| **Containerization** | Docker + docker-compose |
| **Deployment** | Fly.io (see `fly/` and `Dockerfile`) |
| **Testing** | Jest + ts-jest |

---

## Project Structure

```
ai-town/
├── convex/                    # Convex backend (serverless functions + schema)
│   ├── _generated/            # Auto-generated: api.d.ts, dataModel.d.ts, server.d.ts
│   ├── engine/                # Reusable game engine framework
│   │   ├── abstractGame.ts    # AbstractGame class: step loop, input processing, state save/load
│   │   ├── schema.ts          # Engine tables: `engines`, `inputs`
│   │   └── historicalObject.ts # Compressed time-series history for smooth client replay
│   ├── aiTown/                # AI Town-specific game rules
│   │   ├── main.ts            # Engine lifecycle: create, start, stop, kick; runStep action
│   │   ├── game.ts            # Game subclass of AbstractGame: diff computation, load/save world
│   │   ├── world.ts           # World model: container for players, agents, conversations
│   │   ├── player.ts          # Player model: position, pathfinding, join/leave
│   │   ├── agent.ts           # Agent model (in-engine): tick behavior, operation scheduling
│   │   ├── agentInputs.ts     # Input handlers for agent lifecycle (createAgent, finishDoSomething, etc.)
│   │   ├── agentOperations.ts # Async agent operations (LLM calls, memory, decision-making)
│   │   ├── agentDescription.ts# Agent identity/plan data model
│   │   ├── conversation.ts    # Conversation model: lifecycle, typing, invite/accept/reject
│   │   ├── conversationMembership.ts # Per-player conversation state
│   │   ├── playerDescription.ts # Player display data (name, character, description)
│   │   ├── movement.ts        # Pathfinding (A*-like), collision detection, move/stop
│   │   ├── worldMap.ts        # Tile map data model
│   │   ├── location.ts        # Player location type and HistoricalObject field config
│   │   ├── ids.ts             # GameId type system and ID allocation
│   │   ├── inputs.ts          # Combined input map (player + conversation + agent inputs)
│   │   ├── inputHandler.ts    # Input handler factory with type-safe args validation
│   │   ├── insertInput.ts     # Helper to insert input via engineId lookup
│   │   └── schema.ts          # AI Town tables: worlds, players, agents, conversations, maps, etc.
│   ├── agent/                 # LLM agent logic (runs as Convex actions)
│   │   ├── conversation.ts    # Prompt engineering for start/continue/leave conversation messages
│   │   ├── memory.ts          # Memory creation, vector search, ranking (relevance+recency+importance)
│   │   ├── embeddingsCache.ts # LRU-ish embeddings cache to avoid redundant API calls
│   │   └── schema.ts          # Agent tables: memories, memoryEmbeddings, embeddingsCache
│   ├── schema.ts              # Root schema combining all tables
│   ├── world.ts               # Public-facing world queries & mutations
│   ├── messages.ts            # Chat message queries & mutations (outside engine)
│   ├── music.ts               # Background music (Replicate webhook)
│   ├── crons.ts               # Scheduled jobs: stop idle worlds, restart dead engines, vacuum old data
│   ├── http.ts                # HTTP router (Replicate webhook endpoint)
│   ├── init.ts                # One-time world initialization mutation
│   ├── testing.ts             # Test helpers (resume/pause engine)
│   ├── constants.ts           # Gameplay tuning constants
│   └── util/                  # Shared utilities
│       ├── llm.ts             # LLM provider abstraction (OpenAI/Together/Ollama/custom)
│       ├── geometry.ts        # Distance, path position, normalization, vector math
│       ├── types.ts           # Point, Vector, Path type definitions
│       ├── minheap.ts         # Binary min-heap for A* pathfinding
│       ├── compression.ts     # Quantization, delta encoding, run-length encoding
│       ├── FastIntegerCompression.ts # Variable-length integer encoding
│       ├── xxhash.ts          # xxHash32 for config hash verification
│       ├── object.ts          # Map serialization/deserialization helpers
│       ├── asyncMap.ts        # Concurrent async map
│       ├── assertNever.ts     # Exhaustive type checking
│       ├── sleep.ts           # Promise-based sleep
│       └── isSimpleObject.ts  # Plain object detection
├── src/                       # React + PixiJS frontend
│   ├── main.tsx               # Entry point: React root + ConvexClientProvider
│   ├── App.tsx                # Main layout: title, help modal, Game component, footer
│   ├── index.css              # Tailwind imports + global styles
│   ├── components/
│   │   ├── Game.tsx           # Top-level game component: PixiJS Stage, historical time, player selection
│   │   ├── PixiGame.tsx       # PixiJS game world: viewport, map, players, click-to-move
│   │   ├── PixiStaticMap.tsx  # Renders tile layers + animated sprites
│   │   ├── PixiViewport.tsx   # Pan/zoom viewport wrapper (pixi-viewport)
│   │   ├── Player.tsx         # Renders a single player sprite with historical position interpolation
│   │   ├── PlayerDetails.tsx  # Right sidebar: selected player info, messages, conversation start
│   │   ├── Messages.tsx       # Chat message display with typing indicator
│   │   ├── MessageInput.tsx   # Chat input box
│   │   ├── Character.tsx      # Character sprite loading and animation
│   │   ├── DebugPath.tsx      # Visualizes current pathfinding path
│   │   ├── DebugTimeManager.tsx # Debug UI for controlling historical time
│   │   ├── FreezeButton.tsx   # Toggle game freeze
│   │   ├── PositionIndicator.tsx # Click-to-move destination indicator
│   │   ├── ConvexClientProvider.tsx # Convex client setup
│   │   ├── PoweredByConvex.tsx # Convex branding
│   │   └── buttons/
│   │       ├── Button.tsx     # Generic button component
│   │       ├── InteractButton.tsx # Join/leave world toggle
│   │       ├── LoginButton.tsx # Clerk login
│   │       └── MusicButton.tsx # Music toggle
│   ├── hooks/
│   │   ├── serverGame.ts      # useServerGame: parses Convex query results into in-memory game models
│   │   ├── sendInput.ts       # useSendInput: submits input + waits for engine to process it
│   │   ├── useHistoricalTime.ts # Manages synchronized historical replay clock
│   │   ├── useHistoricalValue.ts # Unpacks and replays compressed HistoricalObject buffers
│   │   ├── useWorldHeartbeat.ts # Periodic heartbeat to keep world alive
│   │   └── toasts.ts          # Toast notification helpers
│   ├── editor/                # Standalone level editor (separate Vite build)
│   └── toasts.ts              # Toast notification helpers
├── data/
│   ├── characters.ts          # Agent character definitions (name, description, identity, plan)
│   ├── gentle.js              # Pre-built map tile data
│   └── convertMap.js          # Map conversion utility
├── public/                    # Static assets (favicon, sprites, tilesets)
├── assets/                    # UI images (logos, icons)
├── fly/                       # Fly.io deployment guide
├── docker-compose.yml         # Local dev: frontend + Convex backend + dashboard
├── Dockerfile                 # Production container (Ubuntu + Node 18 + Vite)
├── vite.config.ts             # Vite config with base path `/ai-town`
├── tailwind.config.js         # Tailwind configuration
├── tsconfig.json              # TypeScript config with `@/*` path alias → `src/*`
├── jest.config.ts             # Jest test configuration
└── package.json               # Dependencies and scripts
```

---

## Server-Side: Convex Backend

### Game Engine (`convex/engine/`)

The game engine is a **reusable framework** for tick-based simulations. It is decoupled from AI Town's specific game rules.

#### `abstractGame.ts` — AbstractGame

The `AbstractGame` class defines the core simulation loop:

```
runStep(ctx, now):
  1. Load unprocessed inputs from 'inputs' table
  2. Determine simulation interval (lastStepTs → now)
  3. For each tick in the interval:
     a. Feed inputs whose `received` timestamp ≤ current tick time into `handleInput()`
     b. Call `tick(currentTs)` to advance simulation
  4. Bump `generationNumber`, update `currentTime`
  5. Call `saveStep()` to persist diff to database
```

Key parameters (configurable by subclass):
- **`tickDuration`**: 16ms (targets ~60 ticks/sec for smooth movement)
- **`stepDuration`**: 1000ms (how often the engine wakes to run)
- **`maxTicksPerStep`**: 600 (upper bound per Convex action invocation)
- **`maxInputsPerStep`**: 32 (input processing budget per step)

**Generation number**: A monotonically increasing counter that prevents overlapping engine runs. If the engine is kicked (generation bumped), any in-flight `runStep` call will fail on commit because the generation doesn't match — guaranteeing single-threaded execution per world.

**Input lifecycle**:
1. External code calls `engineInsertInput()` → inserts into `inputs` table with sequential `number`
2. `runStep` loads unprocessed inputs via `loadInputs` query
3. Inputs are dispatched to `handleInput(name, args)` at the correct simulation tick
4. Results (ok/error) are written back to the input row via `applyEngineUpdate()`
5. Clients poll `inputStatus` query to get results

#### `historicalObject.ts` — HistoricalObject

Solves the problem of smooth client-side animation. The engine runs steps at 1 Hz, but ticks at 60 Hz. Without history, positions would only update once per second, looking choppy.

**How it works**:
- At each tick, player location (x, y, dx, dy, speed) is recorded into a `HistoricalObject`
- At step end, all histories are compressed into binary `ArrayBuffer`s
- Compression pipeline: Quantization → Delta encoding → Run-length encoding (optional) → Varint encoding
- The client unpacks the buffer and replays the time-series to interpolate positions smoothly
- An xxHash32 checksum of the field config prevents client/server field mismatches

**Limitations**: Only numeric fields, max 16 fields per object, no nested objects.

#### `schema.ts` — Engine Tables

| Table | Purpose |
|-------|---------|
| `engines` | Per-world engine state: `currentTime`, `generationNumber`, `running`, `processedInputNumber` |
| `inputs` | Input queue: `engineId`, `number` (sequential), `name`, `args`, `returnValue`, `received` |

---

### AI Town Game Logic (`convex/aiTown/`)

#### `game.ts` — The Game Class

`Game` extends `AbstractGame` and is the central orchestrator:

```typescript
class Game extends AbstractGame {
  world: World;                          // In-memory world container
  worldMap: WorldMap;                    // Tile map data
  playerDescriptions: Map<GameId, PlayerDescription>;
  agentDescriptions: Map<GameId, AgentDescription>;
  historicalLocations: Map<GameId, HistoricalObject<Location>>;
  pendingOperations: Array<{name, args}>; // Agent operations to kick off
  numPathfinds: number;                  // Pathfinding budget tracker
}
```

**`tick(now)`** advances the simulation in this order:
1. `player.tick()` — idle timeout for human players
2. `player.tickPathfinding()` — A* path computation (budgeted: max 16 per step)
3. `player.tickPosition()` — move along computed path, collision detection
4. `conversation.tick()` — participant state transitions, facing orientation, typing timeout
5. `agent.tick()` — agent decision-making, operation scheduling

**`takeDiff()`** computes a minimal diff of changed state since the step began:
- Always: serialized `world` (players + agents + conversations + historical location buffers)
- Conditionally: `playerDescriptions`, `agentDescriptions`, `worldMap` (only if `descriptionsModified`)
- Always: `agentOperations` (pending async operations to launch)

**`saveStep()`** calls the `saveWorld` mutation which:
1. Applies the engine update (time, generation, completed inputs)
2. Archives deleted players/conversations/agents
3. Updates the `participatedTogether` graph for ended conversations
4. Replaces the world document
5. Upserts description tables and map if changed
6. Schedules pending agent operations via `ctx.scheduler`

#### `world.ts` — World Model

The `World` is an in-memory container holding:
- `players: Map<GameId, Player>` — all active characters
- `agents: Map<GameId, Agent>` — AI agents (linked to players)
- `conversations: Map<GameId, Conversation>` — active conversations
- `nextId: number` — monotonic ID allocator

The entire world state is serialized into a **single document** in the `worlds` table. This keeps game state small and load/save fast.

#### `player.ts` — Player Model

Each player has:
- **Identity**: `id`, `human` (token identifier if human), `character` (sprite name)
- **Movement state**: `position` (Point), `facing` (Vector), `speed`, optional `pathfinding`
- **Activity**: optional `activity` with description, emoji, and expiration
- **`pathfinding` state machine**: `needsPath` → `waiting` (backoff) → `moving` (following path)

**Pathfinding** (`movement.ts`): A* search on a grid with collision avoidance against object tiles and other players. Movement speed is character-dependent. Paths are compressed to key waypoints.

**Join/Leave**: Players are placed at random unblocked positions. Humans time out after 5 minutes of idle (`HUMAN_IDLE_TOO_LONG`). Max 8 human players per world.

#### `agent.ts` — Agent Model (In-Engine)

The `Agent` class runs in the simulation tick and makes decisions:

**State machine in `tick()`**:
1. If an operation is in progress, wait for it (with timeout)
2. If not in conversation and idle → `startOperation('agentDoSomething')` — decides to wander, do an activity, or invite someone
3. If `toRemember` is set → `startOperation('agentRememberConversation')` — summarize and store conversation memory
4. If in conversation:
   - **invited**: Accept with 80% probability (100% for humans) or reject
   - **walkingOver**: Walk towards the other player (midpoint or direct)
   - **participating**: Manage conversation flow — initiate, wait for response, continue, or leave based on duration/message limits

Agents maintain `lastConversation` (cooldown between conversations) and `lastInviteAttempt` (cooldown between invite attempts).

#### `agentOperations.ts` — Async Agent Actions

These run as **Convex actions** (can call external APIs):

| Operation | What it does |
|-----------|-------------|
| `agentDoSomething` | Decide next activity: wander, do an activity (reading/gardening/daydreaming), or invite another player |
| `agentGenerateMessage` | Call LLM to generate start/continue/leave conversation text, then insert message via mutation |
| `agentRememberConversation` | Summarize finished conversation via LLM, compute embedding, store in vector DB, trigger reflection |

#### `agentInputs.ts` — Agent Input Handlers

These inputs are processed by the engine to commit agent operation results back into game state:

| Input | Purpose |
|-------|---------|
| `finishRememberConversation` | Clear agent's `inProgressOperation` and `toRemember` |
| `finishDoSomething` | Apply agent decision: start conversation, wander, or set activity |
| `agentFinishSendingMessage` | Commit sent message (typing state, leave if needed) |
| `createAgent` | Create a new player + agent from character definition |

#### `conversation.ts` — Conversation Model

Conversations are strictly **two-player**. State transitions:

```
                     Player A starts conversation
                              │
              ┌───────────────┴───────────────┐
              ▼                               ▼
     Player A: walkingOver            Player B: invited
              │                               │
              │                    ┌──────────┴──────────┐
              │                    ▼                     ▼
              │              acceptInvite           rejectInvite
              │                    │                → stop (both)
              │                    ▼
              │           Player B: walkingOver
              │                    │
              └────────────────────┘
                       │
                  both close enough
                  (< CONVERSATION_DISTANCE)
                       │
                       ▼
              both: participating
                       │
              ┌────────┴────────┐
              ▼                 ▼
         leaveConversation   timeout / max messages
              │                 │
              └────────┬────────┘
                       ▼
                   stop() → agents get toRemember
```

**Typing indicator**: A single-player lock with `messageUuid` and timeout. Enforces turn-taking.

**Auto-facing**: Participating players auto-face each other when stationary.

---

### Agent System (`convex/agent/`)

#### `conversation.ts` — LLM Prompt Engineering

Three message types:

1. **`startConversationMessage`**: System prompt with agent identity + goals, other agent info, previous conversation context, related memories. Generates greeting.
2. **`continueConversationMessage`**: Full chat history + system prompt. Generates brief (<200 char) response.
3. **`leaveConversationMessage`**: System prompt + chat history. Generates polite goodbye.

Prompts are enriched with:
- Agent `identity` and `plan` (goals for the conversation)
- Other agent's identity
- Previous conversation date (from `participatedTogether` graph)
- Top 3 related memories from vector search

#### `memory.ts` — Memory System

**Memory creation** (`rememberConversation`):
1. Load full conversation messages
2. Ask LLM to summarize from the agent's first-person perspective (with like/dislike sentiment)
3. Calculate importance score (LLM rates 0-9)
4. Compute embedding vector
5. Store in `memories` table + `memoryEmbeddings` vector index

**Memory search** (`searchMemories`):
1. Compute query embedding (e.g., "What do you think about X?")
2. Vector search in `memoryEmbeddings` (over-fetch 10x)
3. Rank by combined score: **relevance** (vector similarity) + **importance** (0-9) + **recency** (exponential decay, half-life ~hours)
4. Return top N

**Reflection** (`reflectOnMemories`):
- Triggers when accumulated importance of new memories > 500
- Asks LLM to infer 3 high-level insights from recent memories
- Stores reflections as new `reflection`-type memories

#### `embeddingsCache.ts` — Embeddings Cache

Deduplicates embedding API calls by hashing text. Stores `(textHash, embedding)` pairs in the `embeddingsCache` table.

#### `schema.ts` — Agent Tables

| Table | Purpose | Indexes |
|-------|---------|---------|
| `memories` | Agent memories with metadata | `embeddingId`, `playerId_type`, `playerId` |
| `memoryEmbeddings` | Vector embeddings for memories | Vector index on `embedding`, filtered by `playerId` |
| `embeddingsCache` | Deduplicated embedding cache | `text` (by hash) |

Memory types (discriminated union on `data.type`):
- `relationship` — opinion about another player
- `conversation` — summary of a specific conversation
- `reflection` — high-level insight derived from multiple memories

---

### World Management (`convex/world.ts`)

Public API for world lifecycle:

| Function | Type | Purpose |
|----------|------|---------|
| `defaultWorldStatus` | query | Get the default world's status |
| `heartbeatWorld` | mutation | Update `lastViewed` to keep world alive; restart inactive worlds |
| `stopInactiveWorlds` | internalMutation | Cron job: stop worlds idle > 5 min |
| `restartDeadWorlds` | internalMutation | Cron job: kick engines stuck > 60s |
| `userStatus` | query | Get current user identity (hardcoded to "Me") |
| `joinWorld` | mutation | Submit `join` input for human player |
| `leaveWorld` | mutation | Submit `leave` input for human player |
| `sendWorldInput` | mutation | Low-level: insert raw input into engine |
| `worldState` | query | Get world + engine state |
| `gameDescriptions` | query | Get player descriptions + agent descriptions + map |
| `previousConversation` | query | Find last non-empty conversation for a player |

### Messages (`convex/messages.ts`)

Messages live **outside the game engine** for two reasons:
1. The simulation doesn't need message content to function
2. Messages update frequently (streaming LLM output) and benefit from lower latency

| Function | Type | Purpose |
|----------|------|---------|
| `listMessages` | query | Get all messages in a conversation with author names |
| `writeMessage` | mutation | Write a message + trigger `finishSendingMessage` input |

Messages are keyed by `conversationId` + `messageUuid` (deduplication). The `messages` table is indexed on `(worldId, conversationId)`.

### Cron Jobs (`convex/crons.ts`)

| Job | Schedule | Action |
|-----|----------|--------|
| `stop inactive worlds` | Every 5 min | Pause worlds with no heartbeat for >5 min |
| `restart dead worlds` | Every 60s | Kick engines that haven't run in >60s |
| `vacuum old entries` | Daily at 4:20 UTC | Delete inputs, memories, and embeddings older than 2 weeks |

### Utilities (`convex/util/`)

| Module | Purpose |
|--------|---------|
| `llm.ts` | LLM provider abstraction: OpenAI, Together.ai, Ollama, or custom API. Chat completion (streaming + non-streaming), embedding generation, moderation. Auto-pulls Ollama models. Exponential backoff with jitter. |
| `geometry.ts` | 2D vector math: distance (Euclidean), manhattan distance, normalization, path position interpolation, path compression, points equality |
| `types.ts` | Core types: `Point {x, y}`, `Vector {dx, dy}`, `Path` (array of timed waypoints) |
| `minheap.ts` | Binary min-heap for A* priority queue |
| `compression.ts` | Quantization, delta encoding/decoding, run-length encoding/decoding for HistoricalObject buffers |
| `FastIntegerCompression.ts` | Variable-length integer encoding (smaller numbers → fewer bytes) |
| `xxhash.ts` | xxHash32 for field config checksum verification |
| `object.ts` | `parseMap` / `serializeMap` for converting between arrays and Maps |
| `asyncMap.ts` | Concurrent async map with concurrency limit |
| `sleep.ts` | Promise-based sleep |
| `assertNever.ts` | Exhaustive switch checking |

---

## Database Schema

All tables are defined in Convex's schema system. Here's the complete table map:

### Engine Tables (`convex/engine/schema.ts`)
- **`engines`**: `currentTime`, `lastStepTs`, `processedInputNumber`, `running`, `generationNumber`
- **`inputs`**: `engineId`, `number`, `name`, `args`, `returnValue` (ok/error), `received`

### AI Town Tables (`convex/aiTown/schema.ts`)
- **`worlds`**: Single document containing all players, agents, conversations, and historical location buffers
- **`worldStatus`**: `worldId`, `engineId`, `isDefault`, `lastViewed`, `status` (running/stoppedByDeveloper/inactive)
- **`maps`**: `worldId`, tile layer data (`bgTiles`, `objectTiles`), `animatedSprites`, tileset config
- **`playerDescriptions`**: `worldId`, `playerId`, `name`, `character`, `description`
- **`agentDescriptions`**: `worldId`, `agentId`, `identity`, `plan`
- **`archivedPlayers`**: Snapshot of deleted players
- **`archivedConversations`**: Ended conversation metadata + participant list
- **`archivedAgents`**: Snapshot of deleted agents
- **`participatedTogether`**: Edge list: `(worldId, player1, player2, conversationId, ended)` — enables "last talked to X at time Y" queries

### Agent Tables (`convex/agent/schema.ts`)
- **`memories`**: `playerId`, `description`, `embeddingId`, `importance` (0-9), `lastAccess`, `data` (discriminated union: relationship/conversation/reflection)
- **`memoryEmbeddings`**: `playerId`, `embedding` (float64 array) — vector-indexed
- **`embeddingsCache`**: `textHash` (bytes), `embedding` (float64 array)

### Other Tables (`convex/schema.ts`)
- **`messages`**: `conversationId`, `messageUuid`, `author`, `text`, `worldId`
- **`music`**: `storageId`, `type` (background/player)

---

## Client-Side: React + PixiJS (`src/`)

### Rendering Architecture

```
App.tsx
└── Game.tsx
    ├── PixiJS <Stage> (canvas)
    │   └── PixiGame.tsx
    │       ├── PixiViewport (pan/zoom container)
    │       │   ├── PixiStaticMap (tile layers + animated sprites)
    │       │   ├── DebugPath (pathfinding visualization)
    │       │   ├── PositionIndicator (click destination marker)
    │       │   └── Player (character sprites with interpolation)
    │       └── ... (per-player)
    └── PlayerDetails.tsx (right sidebar)
        ├── Messages.tsx (chat history)
        └── MessageInput.tsx (chat input)
```

### Key Patterns

**State flow**: Convex `useQuery` hooks → `useServerGame` (parses into typed in-memory models) → React components → PixiJS rendering.

**Smooth motion** with historical replay:
1. `useHistoricalTime` at the top level computes a `historicalTime` offset (slightly behind real time)
2. `useHistoricalValue` in `Player.tsx` unpacks the compressed `ArrayBuffer` from the world document
3. Values (position, facing, speed) are interpolated between the latest sample and current time
4. The `HistoryManager` class maintains a sliding window of samples, discarding old ones

**Input submission**: `useSendInput` hook:
1. Calls `sendWorldInput` mutation → gets `inputId`
2. Subscribes to `inputStatus` query via `convex.watchQuery`
3. Awaits the result (ok → return value, error → throw)

**World lifecycle**:
- `useWorldHeartbeat` sends periodic heartbeats (`WORLD_HEARTBEAT_INTERVAL` = 60s) to prevent inactivity timeout
- `InteractButton` calls `joinWorld` / `leaveWorld` mutations

### Level Editor (`src/editor/`)

A standalone tile-map editor built with vanilla JS. Launched via `npm run level-editor`. Edits JSON map files with tile layers, animated sprites, and sprite sheets.

---

## Data Flow

### Human Player Joining

```
1. User clicks "Interact"
2. joinWorld mutation → insertInput(worldId, 'join', {name, character, description})
3. Input queued in 'inputs' table
4. Engine's runStep picks up input → handleInput('join', ...) → Player.join()
5. Game state saved → world document updated with new player
6. useQuery(worldState) re-renders → new player appears on map
```

### Human Player Moving

```
1. User clicks on map
2. PixiGame.onMapPointerUp → converts screen coords to tile coords
3. useSendInput('moveTo', {playerId, destination}) → mutation → inputId
4. waitForInput polls inputStatus until engine processes it
5. Engine: moveTo handler → movePlayer() → sets pathfinding state
6. Subsequent ticks: tickPathfinding → findRoute (A*) → tickPosition → update position
7. Positions recorded in HistoricalObject each tick
8. At step end: pack history buffers → save to world document
9. Client: useHistoricalValue unpacks buffer → Player sprite interpolates smoothly
```

### AI Agent Conversation Flow

```
1. Agent.tick() detects idle state → startOperation('agentDoSomething')
2. agentDoSomething action runs: checks cooldowns, finds nearest free player
3. Sends 'finishDoSomething' input with invitee → Conversation.start()
4. Invited agent's tick: accepts invite → Conversation.acceptInvite()
5. Both agents tick 'walkingOver' → findRoute + move towards each other
6. Conversation.tick: when distance < CONVERSATION_DISTANCE → transition to 'participating'
7. Initiator agent.tick: startOperation('agentGenerateMessage', {type:'start'})
8. agentGenerateMessage action: prompt engineering + chatCompletion() → LLM call
9. agentSendMessage mutation: inserts message + sends 'agentFinishSendingMessage' input
10. Engine processes input: clears typing, increments numMessages
11. Other agent observes new lastMessage → after cooldown → generates 'continue' response
12. Loop continues until duration limit or message limit → 'leave' message → stop()
13. stop() sets agent.toRemember → next tick: 'agentRememberConversation'
14. rememberConversation: summarize via LLM → embedding → vector DB + reflection
```

### Engine RunStep Lifecycle

```
┌─────────────────────────────────────────────┐
│  runStep action (convex/aiTown/main.ts)      │
│                                               │
│  1. Load world state (query)                  │
│  2. Create Game instance (parse → memory)     │
│  3. Loop until deadline:                      │
│     ├── runStep(now):                         │
│     │   ├── beginStep → init history          │
│     │   ├── Load inputs                       │
│     │   ├── For each tick:                    │
│     │   │   ├── handleInput for this tick     │
│     │   │   ├── tick(now) → simulate          │
│     │   │   └── Record history                │
│     │   ├── takeDiff → compute changes        │
│     │   ├── saveStep → mutation               │
│     │   │   ├── applyEngineUpdate             │
│     │   │   ├── saveDiff (archive + replace)  │
│     │   │   └── schedule agent operations     │
│     │   └── bump generation number            │
│     └── sleep(stepDuration)                   │
│  4. Schedule next runStep                     │
└─────────────────────────────────────────────┘
```

---

## Deployment

### Local Development

```bash
npm run dev           # Starts Convex backend + Vite frontend concurrently
```

Or with Docker:
```bash
docker-compose up     # Frontend (Vite) + Convex backend + Dashboard
```

### Production (Fly.io)

- **Dockerfile**: Ubuntu 22.04 + Node 18 + Vite dev server (`npx vite --host`)
- **Convex backend**: Hosted on Convex cloud
- **`fly/` directory**: Contains deployment instructions
- **`vercel.json`**: Alternative deployment via Vercel (static site)

### LLM Provider Configuration

Set environment variables for your provider:

| Provider | Variables |
|----------|-----------|
| OpenAI | `OPENAI_API_KEY`, `OPENAI_CHAT_MODEL`, `OPENAI_EMBEDDING_MODEL` |
| Together.ai | `TOGETHER_API_KEY`, `TOGETHER_CHAT_MODEL`, `TOGETHER_EMBEDDING_MODEL` |
| Ollama (local) | `OLLAMA_HOST`, `OLLAMA_MODEL`, `OLLAMA_EMBEDDING_MODEL` |
| Custom | `LLM_API_URL`, `LLM_API_KEY`, `LLM_MODEL`, `LLM_EMBEDDING_MODEL` |

The embedding dimension constant in `convex/util/llm.ts` must match the provider.

---

## Design Goals & Limitations

### Design Goals
- **Close to regular Convex**: Game state in regular tables, standard `useQuery` hooks, no custom websocket protocol
- **Familiar engine model**: Tick-based simulation similar to common game engines
- **Decoupled agents**: Humans and AI agents use the same input system; agent logic runs in separate async actions
- **Single-threaded engine**: No race conditions or concurrency concerns within a world

### Inherent Limitations
- **In-memory state**: All active game state must fit in memory and be loadable/saveable per step (~few dozen KB)
- **Input throughput**: All inputs flow through the database `inputs` table
- **Input latency**: ~1.5 seconds (1 RTT + half step size + historical buffer delay). Not suitable for competitive games.
- **Single-threaded**: CPU-bound simulations may hit limits; JavaScript over plain objects is fast but not infinitely scalable
- **Two-player conversations only**: No group chat support in current conversation model
- **Max 8 human players**: Hard limit per world
