# Developer Documentation: Elysian Dialogue

This document provides a comprehensive overview of the architecture, core systems, and data structures of the **Elysian Dialogue** application.

---

## 1. Project Overview

**Elysian Dialogue** is a cinematic RPG-style dialogue engine designed for immersive narrative experiences. It features a vertical-scrolling "thought stream" aesthetic, branching dialogue paths, and probabilistic skill checks influenced by character attributes.

- **Stack:** React 19, TypeScript, Vite.
- **Backend:** Express with SQLite (via `better-sqlite3`).
- **Intelligence:** Single-LLM Game Master (Gemini/DeepSeek) with streaming SSE.
- **Visuals:** Tailwind CSS (v4), `motion` for fluid animations, and Lucide icons.

---

## 2. Project Structure

```text
.
├── .env.example             # Template for environment variables (GEMINI_API_KEY)
├── .gitignore
├── CHANGELOG.md             # Detailed history of codebase modifications
├── DEVELOPER.md             # Technical documentation (this file)
├── ISSUES.md                # Known issues and bugs
├── game.db                  # SQLite database (World state, Narrative history, Dialogue tree)
├── index.html               # Frontend entry point
├── package.json             # Manifest with scripts and dependencies
├── tsconfig.json            # TypeScript build configuration
├── vite.config.ts           # Vite development and build configuration
└── src/
    ├── client/              # Frontend (React)
    │   ├── main.tsx         # App entry point
    │   ├── App.tsx          # Main orchestrator with SSE streaming consumer
    │   └── index.css        # Global styles (Tailwind + Custom filters)
    ├── components/          # UI Components
    │   ├── CharacterPanel.tsx   # Sidebar for stats and inventory
    │   ├── DebugPanel.tsx       # Developer toolbox (Logs, Editors)
    │   ├── DialogueMessage.tsx  # Message rendering (supports streaming)
    │   ├── DialogueOptions.tsx  # Player interaction buttons
    │   ├── DiceRoller.tsx       # Skill check animation and logic
    │   ├── ObjectLink.tsx       # Hoverable text references
    │   ├── ObjectTooltip.tsx    # Detailed entity lore popups
    │   └── TypingIndicator.tsx  # NPC activity feedback
    ├── context/             # React Contexts
    │   └── CharacterContext.tsx # Global character attribute state
    ├── data/                # Static Data
    │   └── sampleDialogue.ts    # Initial scenario/tutorial steps
    ├── server/              # Backend (Node.js/Express)
    │   ├── main.ts          # Server entry and Vite middleware setup
    │   ├── api.ts           # REST API + SSE streaming endpoints
    │   ├── db.ts            # SQLite connection and schema migrations
    │   ├── LlmServiceBackend.ts # Single-LLM streaming GM logic
    │   ├── LlmDebugIntegration.ts # LLM request/response logging
    │   ├── sseEvents.ts     # SSE event emitter and text parser
    │   └── models/          # Database Abstractions
    │       ├── debug.ts     # Logging queries
    │       ├── dialogue.ts  # Dialogue tree CRUD (steps, alternatives, branches)
    │       ├── history.ts   # Narrative flow persistence
    │       ├── plot.ts      # Objective tracking
    │       └── world.ts     # Entity state management
    ├── services/            # Shared Logic
    │   ├── ConsoleLogger.ts # System-wide log interception
    │   ├── LlmService.ts    # Frontend AI client (legacy, non-streaming)
    │   ├── SseClient.ts     # Browser SSE streaming consumer
    │   ├── WorldManager.ts  # World state synchronization client
    │   └── tools/           # LLM Tool Implementations
    │       ├── updateWorldState.ts      # Commit entity changes directly
    │       ├── updatePlotStatus.ts      # Commit plot status changes
    │       └── createPlot.ts           # Create new plot/quest
    └── types/               # TypeScript Definitions
        ├── dialogue.ts      # Narrative system interfaces
        └── entities.ts      # Character and actor models
```

---

## 3. Architecture Overview

### 3.1 Event-Driven Streaming (NEW)

The architecture is **event-driven and streaming-first**:

```
Browser (React)
  │ POST /api/chat/stream
  ▼
Express Server
  │ Sets up SSE (text/event-stream)
  │ Calls streamText with GM model + tools
  │
  ├─ event: token          ← text-delta chunks streamed in real-time
  ├─ event: world_update   ← tool commits entity change
  ├─ event: plot_update    ← tool commits plot status
  ├─ event: options        ← final options ready
  ├─ event: parsed         ← structured messages + options
  └─ event: done           ← stream complete
```

### 3.2 Single LLM (Assistant Removed)

Previously: GM drafts → Assistant reviews → GM revises (multi-step loop, minutes of latency).

Now: **GM generates narrative text directly**. World/plot mutations happen via tools that commit to DB immediately. No verification loop. A tight system prompt and Zod schemas ensure quality.

### 3.3 Dialogue Tree in SQLite

All dialogue steps are persisted as a tree in the `dialogue_steps` table:
- `parent_step_id` / `parent_option_id` — links steps to their origin
- `is_active` — marks the current path (0 = dead branch)
- Dead branches get a synthetic `[Replay]` option to rejoin
- `dialogue_alternatives` stores regenerated/swipe responses

### 3.4 Pre-Generation

After generating a step, the backend asynchronously pre-generates the next step for each `isAiTrigger` option. These are stored as child nodes. When a user clicks an option, the frontend first checks for a pre-generated child — if found, it loads instantly without an API call.

### 3.5 Regenerate

User can regenerate any AI response. The previous response is saved as an alternative (swipe). The new response streams in via the same SSE pipeline. Alternatives are browsable.

---

## 4. API Endpoints

| Method | Path | Purpose |
|--------|------|---------|
| POST | `/api/chat/stream` | SSE streaming AI response |
| POST | `/api/chat` | Non-streaming fallback |
| GET | `/api/dialogue/:id` | Fetch step + children + alternatives |
| GET | `/api/dialogue/:id/path` | Branch path from root |
| GET | `/api/dialogue/:id/children` | Child steps only |
| GET | `/api/dialogue/:id/alternatives` | Regenerated alternatives |
| POST | `/api/dialogue/:id/alternatives/:altId/select` | Switch to alternative |
| POST | `/api/regenerate` | Regenerate a step via SSE |
| POST | `/api/branches/activate` | Mark branch as active path |
| GET | `/api/world` | Get world state |
| GET | `/api/plots` | Get all plots |
| GET/POST | `/api/history` | Get/set message history |
| POST | `/api/reset` | Reset all state |
| GET/POST | `/api/debug/*` | Debug log access |

---

## 5. AI Integration (Single Game Master)

The app uses a single LLM (Gemini or DeepSeek) as the Game Master.

### 5.1 System Context
- **Narrative History**: Recent messages for tone consistency
- **World State**: Current attributes and locations of all entities
- **Plots**: Active quests and objectives

### 5.2 Tools (Direct Commit)
- `updateWorldState`: Mutates entity attributes, descriptions, opinions — commits to DB
- `updatePlotStatus`: Changes plot status (PENDING/IN_PROGRESS/RESOLVED)
- `createPlot`: Creates a new quest/plot

### 5.3 Output Format
The GM generates narrative text in a structured format:
```
[SPEAKER_NAME|TYPE]
Message content with Markdown. Reference entities with [Name](#entity_id).

---

[SPEAKER_NAME|TYPE]
Next message content.

---

<OPTIONS>
[{"id":"opt_1","text":"Choice text","isAiTrigger":true}, ...]
</OPTIONS>
```

Speaker types: `YOU`, `INNER_VOICE`, `CHARACTER`, `SYSTEM`, `ROLL`, `NOTIFICATION`.

---

## 6. Development & Debugging

### 6.1 Debug Panel
- **Logs**: LLM trace payloads with interactive JSON viewing
- **Console**: Intercepted browser logs with filtering
- **State Editors**: CodeMirror instances for live editing World State or History

---

## 7. How to Extend

### 7.1 Adding New Tools
- Create tool in `src/services/tools/` using the `tool()` factory from `ai`
- Pass `TurnEventEmitter` for SSE event emission
- Register in `LlmServiceBackend.ts` in the `streamText` tools object

### 7.2 Adding New Content
- **Initial Seeds**: Modify `src/data/sampleDialogue.ts`
- **Characters/Objects**: Update initial state in `src/server/models/world.ts`
- **Skills**: Add to `CharacterStats` in `src/types/entities.ts`
