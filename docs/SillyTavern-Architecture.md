# SillyTavern Architecture & Feature Guide

## Overview

SillyTavern is a local-install frontend for AI chat roleplaying. It provides a rich UI for character creation, world/lorebook management, multi-character group chats, and prompt engineering — while delegating actual text generation to external AI backends (OpenAI, Anthropic, Google, local models via Ollama/KoboldCPP, etc.). It is built as a Node.js Express server with a vanilla JavaScript SPA frontend.

---

## 1. High-Level Architecture

```
┌──────────────────────────────────────────────────┐
│  Browser (SPA)                                   │
│  public/script.js  ─── Main orchestrator         │
│  public/scripts/   ─── Feature modules           │
│    openai.js         Chat completion pipeline    │
│    world-info.js     Lorebook scanning engine    │
│    PromptManager.js  Prompt ordering & templating│
│    chats.js          Message rendering           │
│    group-chats.js    Multi-character groups      │
│    extensions/       Client-side extensions      │
└──────────────────┬───────────────────────────────┘
                   │ HTTP/SSE
┌──────────────────▼───────────────────────────────┐
│  Express Server (Node.js)                        │
│  src/server-main.js     Express app setup        │
│  src/endpoints/         REST API routers         │
│    characters.js        Character CRUD           │
│    worldinfo.js         World CRUD               │
│    chats.js             Chat file I/O            │
│    groups.js            Group CRUD               │
│    backends/            AI API proxies           │
│      chat-completions.js  → OpenAI/Claude/etc.   │
│      text-completions.js  → Ooba/Kobold/Ollama   │
│  src/prompt-converters.js  Message format adapt. │
└──────────────────┬───────────────────────────────┘
                   │
┌──────────────────▼───────────────────────────────┐
│  External AI APIs                                │
│  OpenAI / Anthropic / Google / OpenRouter / ...  │
└──────────────────────────────────────────────────┘
```

**Key architectural patterns:**
- **Client builds the prompt** — token counting, world info scanning, and message assembly happen in the browser; the server only converts formats and proxies to AI APIs.
- **Character-first data model** — characters are the primary organizational unit; chats, settings, worlds, and prompts can all be per-character.
- **PNG as container** — character cards embed JSON metadata inside PNG `tEXt` chunks (spec v2), so the image and card data are a single file.
- **Event-driven extensions** — both server (`serverEvents`) and client (`eventSource`) use EventEmitter with ~100 lifecycle event types.
- **Plugin architecture** — server plugins in `plugins/`, client extensions in `public/scripts/extensions/`.

---

## 2. Character Management

### Storage

Characters are stored as PNG files with embedded JSON metadata under `data/<user>/characters/`. The PNG format follows the **Tavern Card V2** specification.

### Character Card Schema (V2)

```json
{
  "spec": "chara_card_v2",
  "spec_version": "2.0",
  "data": {
    "name": "Character Name",
    "description": "Physical and background description",
    "personality": "Personality traits and behavior",
    "scenario": "Current situation or context",
    "first_mes": "The character's opening message",
    "mes_example": "<START>\n{{char}}: Example dialogue\n{{user}}: Response\n",
    "creator_notes": "Notes from the card creator",
    "system_prompt": "Override for main system prompt",
    "post_history_instructions": "Jailbreak/post-history prompt override",
    "alternate_greetings": ["Alt greeting 1", "Alt greeting 2"],
    "tags": ["tag1", "tag2"],
    "character_book": { "entries": [...], "name": "..." },
    "extensions": {
      "talkativeness": 0.5,
      "fav": true,
      "world": "linked-world-book-name",
      "depth_prompt": { "prompt": "...", "depth": 4 },
      "regex_scripts": [...]
    }
  }
}
```

### Key Character Fields and Their Role in Roleplay

| Field | Role in Prompt |
|-------|---------------|
| `name` | Character's display name, used for name prefixing in messages |
| `description` | Physical appearance + background — injected as system prompt near the top of context |
| `personality` | Behavioral traits — formatted via `personality_format` template (e.g., `[{{char}}'s personality: {{personality}}]`) |
| `scenario` | Current situation — formatted via `scenario_format` template |
| `first_mes` | The character's first message, sets tone and speech pattern |
| `mes_example` | Example dialogues parsed into few-shot examples for the AI |
| `system_prompt` | Overrides the main system prompt for this specific character |
| `post_history_instructions` | Jailbreak — appended after chat history, used for last-minute behavioral guidance |
| `alternate_greetings` | Variant opening messages the user can cycle through |
| `creator_notes` | Metadata, not injected into prompts by default |
| `character_book` | Embedded lorebook entries specific to this character |

### Character Lifecycle

1. **Creation**: POST `/api/characters/create` — writes a PNG file with embedded V2 JSON
2. **Loading**: On character select, `selectCharacterById()` reads the PNG, parses the metadata, and populates the UI
3. **Editing**: The character editor panel allows editing all card fields; the Advanced Definitions panel exposes system prompts, jailbreak, and embedded lorebook
4. **Import**: Supports PNG, JSON (V1/V2), YAML, CharX (ZIP), and BYAF formats
5. **Export**: Exports as PNG or JSON, with private fields (extensions) optionally stripped
6. **Caching**: Server-side memory cache (configurable capacity) + disk cache using `node-persist`

### UI Features

- Character list with sort (A-Z, newest, favorites, most chats, most tokens, random), search, and tag filtering
- Grid/list view toggle, pagination for large libraries
- Right-click context menu: Favorite, Tag, Persona, Duplicate, Delete
- Bulk edit mode
- Drag-and-drop import from external sources

---

## 3. World / Lorebook Management

Worldbooks (also called lorebooks or world info) are the primary mechanism for injecting dynamic context into conversations based on keyword matching.

### Storage

Worldbooks are JSON files in `data/<user>/worlds/`. Each file contains:

```json
{
  "name": "World Name",
  "entries": [
    {
      "id": 1,
      "keys": ["keyword1", "keyword2"],
      "secondary_keys": ["optional", "secondary", "triggers"],
      "content": "This lore entry text is injected when keys match",
      "constant": false,
      "selective": false,
      "selective_logic": 0,
      "enabled": true,
      "position": "before_char",
      "insertion_order": 100,
      "depth": null,
      "probability": 100,
      "group": "",
      "group_score": 0,
      "case_sensitive": false,
      "use_regex": false,
      "exclude_recursion": false,
      "delay_until_recursion": false,
      "extensions": {
        "exclude_char": [],
        "include_char": [],
        "sticky": 0,
        "cooldown": 0,
        "delay": 0
      }
    }
  ]
}
```

### The World Info Scanning Engine

Located in `public/scripts/world-info.js` (265 KB), this is one of the most sophisticated parts of SillyTavern.

**Entry point**: `getWorldInfoPrompt()` → `checkWorldInfo()`

**Scanning process:**

1. **Buffer preparation**: Chat messages are collected into a scannable buffer (optionally including Author's Note content)
2. **Multi-pass scanning**: Entries are sorted by `insertion_order`, then checked in sequence
3. **Recursive scanning**: When an entry activates, its content can trigger additional keyword matches in the next pass (controlled by `world_info_recursive` and `max_recursion_steps`)
4. **Minimum activations**: If not enough entries activate, the engine can scan deeper into chat history
5. **Token budget**: Entries stop being added when a percentage of context size is reached

**For each entry, activation is determined by this checklist (in order):**

| Check | Description |
|-------|-------------|
| Generation type filter | Only activate during normal/continue/impersonate/swipe? |
| Character filter | Include/exclude specific character names or tags |
| Timed effects | Sticky (force-activate for N messages), cooldown (suppress for N after deactivation), delay (suppress for N messages) |
| `@@activate` / `@@dont_activate` | Decorators in entry content override normal behavior |
| External activation | Extensions can force-activate entries |
| `constant` flag | Always active, regardless of keywords |
| Sticky state | Previously activated sticky entries remain active |
| **Keyword matching** | Primary keys matched against buffer text, then secondary keys checked with configured logic |

**Keyword matching strategies:**
- **Regex mode**: Primary keys treated as regular expressions
- **Case-sensitive / whole-word**: Optional constraints
- **Selective logic** (when `selective: true`):
  - `AND_ANY` (0): Primary keys AND any secondary key
  - `NOT_ALL` (1): Primary keys AND NOT all secondary keys
  - `NOT_ANY` (2): Primary keys AND NOT any secondary key
  - `AND_ALL` (3): Primary keys AND all secondary keys
- **Group scoring**: Multiple entries can compete in a group; only the highest-scoring entry wins

**Insertion positions:**
- `before_char`: Content inserted before the character definition in the system prompt
- `after_char`: Content inserted after the character definition
- `@depth N`: Content injected at a specific message depth in the chat history (via depth prompt system)

**Output**: The scan returns:
- `worldInfoBefore`: String of activated entries positioned before character definition
- `worldInfoAfter`: String of activated entries positioned after character definition
- `worldInfoDepth`: Entries injected at specific chat depths
- `outletEntries`: Entries for extension consumption

### Character-Linked Worldbooks

Characters can link to worldbooks in two ways:
1. **Primary world**: A single worldbook file linked via `data.extensions.world`
2. **Embedded character book**: World info entries stored directly inside the character card as `data.character_book`

When a character has a linked world, the system either references the JSON file or converts it into the embedded character book format for portability.

### UI Features

- Multi-book selector — activate multiple worldbooks simultaneously
- Entry editor with keyword management, content editor, position/depth/order controls
- Search, sort, pagination for entries
- Import/export/rename/duplicate/delete books
- Scanning depth slider (0-1000 messages)
- Budget slider (1-100 entries max)
- Budget cap in tokens (0-65536)
- Character strategy: Sorted Evenly, Character First, or Global First

---

## 4. Chat & Conversation Flow

### Storage Format

Chats are stored as **JSONL files** (one JSON object per line):

```
data/<user>/chats/<character_id>/<chat_name>.jsonl    # Per-character chats
data/<user>/group chats/<group_chat_id>.jsonl         # Group chats
```

First line contains metadata:
```json
{"chat_metadata": {...}, "user_name": "...", "character_name": "..."}
```

Each message line:
```json
{
  "name": "Character Name",
  "is_user": true,
  "is_system": false,
  "send_date": 1700000000000,
  "mes": "The actual message text",
  "extra": {
    "bias": "...",
    "token_count": 42,
    "reasoning": "...",
    "media": ["file.png"],
    "tool_invocations": [...],
    "swipe_id": 0,
    "swipes": ["alt response 1", "alt response 2"]
  },
  "force_avatar": null,
  "original_avatar": "group_avatar_url"
}
```

### Generation Pipeline (Full Flow)

The main entry point is `Generate()` in `public/script.js`. Here is the complete sequence:

```
User clicks Send
  │
  ▼
Generate(type, options)
  ├─ Emit GENERATION_STARTED event (extensions can intercept/abort)
  ├─ Process slash commands (/cmd parsing)
  ├─ Group chat routing → generateGroupWrapper() if selected_group set
  ├─ Collect user input from #send_textarea
  ├─ getCharacterCardFields()
  │   ├─ description, personality, scenario
  │   ├─ mes_example (dialogue examples)
  │   ├─ system_prompt (override)
  │   ├─ post_history_instructions (jailbreak)
  │   ├─ persona description
  │   ├─ char_depth_prompt
  │   └─ creator_notes, alternate_greetings
  ├─ Process chat messages: regex, file attachments, reasoning blocks
  ├─ Determine token limit from context size
  ├─ Run extension interceptors (can abort generation)
  ├─ Handle CFG guidance scale
  ├─ Set floating prompt (Author's Note)
  ├─ World Info scan → getWorldInfoPrompt()
  │   └─ checkWorldInfo() scans chat for keyword matches
  ├─ Add persona description extension prompt
  ├─ Build story string → renderStoryString()
  │
  ▼
prepareOpenAIMessages()   [for Chat Completions path]
  ├─ Create ChatCompletion instance with token budget
  ├─ preparePromptsForChatCompletion()
  │   ├─ Apply scenario_format / personality_format templates
  │   ├─ Collect extension prompts (memory, vectors, Author's Note)
  │   ├─ Get user-defined prompt order from PromptManager
  │   └─ Merge everything into PromptCollection
  └─ populateChatCompletion()
      ├─ 1. Reserve budget for assistant prefix
      ├─ 2. Add worldInfoBefore, main, worldInfoAfter
      ├─ 3. Add charDescription, charPersonality, scenario, personaDescription
      ├─ 4. Add system prompts (nsfw, jailbreak)
      ├─ 5. Add user-created relative prompts
      ├─ 6. Add enhanceDefinitions, bias
      ├─ 7. Inject known extension prompts (summary, vectors, smartContext)
      ├─ 8. In-chat injection of absolute-positioned prompts
      ├─ 9. Populate dialogue examples (few-shot)
      ├─ 10. Populate chat history (messages, reverse-iterated, budget-aware)
      └─ 11. Add control prompts (impersonate, quiet, continue prefill)
  │
  ▼
sendOpenAIRequest() → POST /api/backends/chat-completions/generate
  │
  ▼
Server: chat-completions.js
  ├─ Custom prompt post-processing (message merging)
  ├─ Route to provider handler (sendClaudeRequest, sendOpenAIRequest, etc.)
  ├─ prompt-converters.js formats messages for target API
  ├─ Apply prompt caching (Claude, OpenRouter)
  └─ Stream response back via SSE
```

### Chat Features

- **Swipes**: Alternate AI responses stored in `extra.swipes[]`, browsable with arrow keys
- **Message editing/deletion**: Edit any message, delete with confirmation
- **Continue**: Extend the AI's last message
- **Impersonate**: Generate as the user's character
- **Regenerate**: Re-roll the last AI response
- **Quiet mode**: Generate without adding to chat history
- **File attachments**: Images, audio, video, PDF, EPUB, Office docs (inlined as multimodal content for vision-capable models)
- **Reasoning display**: Collapsible thinking/reasoning tokens for Claude and similar models
- **Tool calling**: Function call results displayed inline
- **Chat backups**: Automatic throttled backups to `backups/` directory
- **Import**: Supports JSONL, Ooba, Agnai, CAI Tools, Kobold Lite, RisuAI formats
- **Export**: JSONL or plain text
- **Search**: Full-text search within character or group chats

### Group Chats

Stored as JSON files in `data/<user>/groups/`:

```json
{
  "id": "1700000000000",
  "name": "Group Name",
  "members": ["character_avatar_1.png", "character_avatar_2.png"],
  "allow_self_responses": false,
  "activation_strategy": 0,
  "generation_mode": 0,
  "disabled_members": [],
  "auto_mode_delay": 5
}
```

**Activation strategies** determine which character speaks next:
- `NATURAL` (0): Selects the character most relevant to the last message
- `LIST` (1): Cycles through members in order
- `POOLED` (2): Selects based on talkativeness scores
- `MANUAL` (3): Shuffles and picks randomly

**Group prompt modifications:**
- Character names are prepended to their messages
- Group-specific depth prompts and nudges are injected
- `names_behavior` setting controls whether names appear in messages

---

## 5. Prompt Building Pipeline (In Depth)

### Prompt Manager

`public/scripts/PromptManager.js` provides a drag-and-drop UI for controlling prompt composition order.

**Default prompt insertion order:**
```
main → worldInfoBefore → personaDescription → charDescription →
charPersonality → scenario → enhanceDefinitions → nsfw →
worldInfoAfter → dialogueExamples → chatHistory → jailbreak
```

**Key Prompt properties:**
- `role`: system, user, or assistant
- `injection_position`: relative (to other prompts) or absolute (at a specific chat depth)
- `injection_depth`: for absolute positioning, which message index
- `injection_order`: numeric sort within the same position
- `injection_trigger`: which generation types activate this prompt (normal, continue, impersonate, swipe)
- `content`: the actual prompt text, with macro support
- `identifier`: unique name for extension/override targeting

**Default system prompts** define the standard structure:
- `main`: The primary system prompt with roleplay instructions
- `nsfw`: Auxiliary/jailbreak prompt (historically for NSFW content steering)
- `jailbreak`: Post-history instructions appended after all messages
- `enhanceDefinitions`: Additional character definition context
- Marker prompts for `charDescription`, `charPersonality`, `scenario`, `personaDescription`, `worldInfoBefore`, `worldInfoAfter`, `dialogueExamples`, `chatHistory`

### Character-Specific Overrides

Individual characters can override:
- `main` system prompt via `data.system_prompt`
- `jailbreak` via `data.post_history_instructions`
- Prompt ordering and custom prompts via the Character Settings Overrides menu

### Template Systems

**Scenario & Personality formatting** — configurable templates wrap character fields:
- `{{personality}}` → `[{{char}}'s personality: {{personality}}]`
- `{{scenario}}` → `Scenario: {{scenario}}`

**Chat templates** (`public/scripts/chat-templates.js`): Derives context/instruct presets from model tokenizer configs. Detects known templates (Llama 3, Mistral, Gemma, Command R, ChatML, etc.) by SHA256 hash of the model's Jinja2 template string.

**Presets**: User-configurable JSON files for each API (OpenAI, TextGen, Kobold, NovelAI) containing all sampling parameters, context/instruct templates, and system prompt configurations.

### Author's Note (Floating Prompt)

`public/scripts/authors-note.js` — configurable text injected at intervals:
- **Frequency**: Every N user messages
- **Position**: Before scenario, after scenario, or in-chat at a specific depth
- **Role**: System, user, or assistant
- **Character-specific**: Can replace, prepend to, or append after the global note
- **World Info scanning**: Can optionally let WI scan the Author's Note content for keyword matches
- **Slash commands**: `/note`, `/note-depth`, `/note-frequency`, `/note-position`, `/note-role`

### Macro System

`{{macros}}` are expanded before prompt construction. Built-in macros include:
- `{{user}}`, `{{char}}` — user/character names
- `{{time}}`, `{{date}}` — current time/date
- `{{random:...}}` — random selection from list
- `{{roll:...}}` — dice rolling
- Custom macros can be defined by users

### Server-Side Prompt Conversion

`src/prompt-converters.js` (50K+ lines) converts the internal message format to each provider's API format:

| Provider | Converter Function | Key Adaptations |
|----------|-------------------|-----------------|
| Anthropic Claude | `convertClaudeMessages()` | Extracts system prompts, converts images to base64, merges consecutive same-role messages, prompt caching breakpoints |
| Google Gemini | `convertGooglePrompt()` | Maps roles to user/model, inline data for multimodal |
| Cohere | `convertCohereMessages()` | Chat history with tool calling |
| Mistral AI | `convertMistralMessages()` | Tool call ID sanitization |
| AI21 | `convertAI21Messages()` | Jamba format |
| xAI Grok | `convertXAIMessages()` | Grok format |
| Text Completion | `convertTextCompletionPrompt()` | Flat text with `System:`, `user:`, `assistant:` prefixes |

**Message merging** (`postProcessPrompt()`): Handles models that don't support multiple system messages:
- `MERGE`: Concatenate consecutive same-role messages
- `SEMI`: Keep system messages separate, merge user messages
- `STRICT`: Force alternating user/assistant pattern
- `SINGLE`: Collapse everything into one message

---

## 6. AI Backend Integration

### Supported APIs

| Category | APIs |
|----------|-----|
| **Chat Completions** | OpenAI, Anthropic Claude, Google Gemini, Mistral AI, Cohere, AI21, DeepSeek, xAI Grok, Perplexity, Groq, OpenRouter, Azure OpenAI, and 10+ compatible providers |
| **Text Completions** | Oobabooga, KoboldCPP, Ollama, TextGen WebUI, LlamaCPP, vLLM, Aphrodite, TogetherAI, Infermatic, Tabby, DreamGen, and 10+ more |
| **Other** | NovelAI, KoboldAI, Horde |

### Request Flow

1. Client assembles messages array with `prepareOpenAIMessages()`
2. Client sends POST to `/api/backends/chat-completions/generate` (or text-completions/kobold equivalent)
3. Server dispatches to provider-specific handler based on `chat_completion_source`
4. Server applies prompt conversion for the target API format
5. Server streams response back via Server-Sent Events (SSE)
6. Client renders streaming tokens in real-time

### Common Parameters (All APIs)

- `model`, `temperature`, `max_tokens`, `top_p`, `top_k`, `frequency_penalty`, `presence_penalty`
- `stop` sequences, `seed`, `stream`
- Reasoning effort (Claude, Gemini, DeepSeek)
- JSON Schema / Structured Output
- Tool calling / function calling
- Logprobs

---

## 7. Extension System

### Client-Side Extensions

Extensions live in `public/scripts/extensions/<name>/`. Each must have a `manifest.json`:

```json
{
  "display_name": "Extension Name",
  "loading_order": 10,
  "requires": [],
  "optional": ["embeddings"],
  "js": "index.js",
  "css": "style.css",
  "author": "...",
  "version": "1.0.0"
}
```

**Extension API:**
- `getContext()` — full access to current state (character, chat, group, settings)
- `eventSource` and `event_types` — ~100 lifecycle events to hook into
- `extension_settings` — persistent settings storage
- `renderExtensionTemplate()` — HTML template rendering
- `saveMetadataDebounced()` — persist extension data
- `SlashCommandParser.addCommandObject()` — register custom slash commands

**Bundled extensions:**
| Extension | Purpose |
|-----------|---------|
| Connection Profiles | API connection presets (endpoint + model + settings) |
| Regex | Regex-based text replacement for prompts and display |
| Character Expressions | Dynamic avatar expressions based on message emotion |
| Summarize | Chat summarization for long-term memory management |
| TTS | Text-to-speech (Silero, Edge, Coqui) |
| Image Captioning | Multimodal image understanding |
| Translate | Chat message translation |
| Stable Diffusion | AI image generation |
| Vector Storage | RAG via vector database (chromadb) |

**Key generation lifecycle events extensions can hook:**
- `GENERATION_STARTED` / `GENERATION_STOPPED` / `GENERATION_ENDED`
- `GENERATE_BEFORE_COMBINE_PROMPTS` — inject content before prompt assembly
- `GENERATE_AFTER_DATA` — post-process generation results
- `CHAT_CHANGED` / `MESSAGE_SENT` / `MESSAGE_RECEIVED`
- `CHARACTER_EDITED` / `WORLDINFO_UPDATED`

### Server-Side Plugins

Server plugins in `plugins/` are loaded at startup via `plugin-loader.js`.

### Slash Commands

A full scripting system (`public/scripts/slash-commands.js`, 304 KB) with:
- Parser with closures, variable scoping
- Named/unnamed arguments
- Autocomplete
- Debugging support
- 28 built-in command files

---

## 8. Data Organization

```
data/
├── <user-handle>/
│   ├── settings.json              User settings
│   ├── secrets.json               API keys (encrypted)
│   ├── stats.json                 Usage statistics
│   ├── characters/                Character PNG files (with embedded JSON)
│   ├── chats/<char_id>/           Per-character chat JSONL files
│   ├── group chats/               Group chat JSONL files
│   ├── groups/                    Group JSON definitions
│   ├── worlds/                    World info JSON files
│   ├── backgrounds/               Background images
│   ├── User Avatars/              User avatar images
│   ├── themes/                    Custom CSS themes
│   ├── QuickReplies/              Quick reply presets
│   ├── instruct/                  Instruct mode templates
│   ├── context/                   Context templates
│   ├── sysprompt/                 System prompt templates
│   ├── reasoning/                 Reasoning templates
│   ├── extensions/                Extension-specific data
│   ├── vectors/                   Vector store data
│   ├── backups/                   Automatic chat backups
│   ├── OpenAI Settings/           OpenAI presets
│   ├── TextGen Settings/          TextGen presets
│   ├── NovelAI Settings/          NovelAI presets
│   └── KoboldAI Settings/         KoboldAI presets
├── _cache/                        Server-side caches
├── _storage/                      User storage (node-persist)
└── _uploads/                      Temporary uploads
```

---

## Summary

SillyTavern's architecture is best understood as a **sophisticated prompt engineering and context management layer** sitting between the user and external AI APIs:

1. **Characters** are defined as structured JSON (name, description, personality, scenario, example dialogue) embedded in PNG files
2. **Worldbooks** scan chat context for keyword matches and dynamically inject relevant lore entries into the prompt at configurable positions
3. **The Prompt Manager** lets users control the exact ordering, positioning, and depth of every prompt component
4. **Chat history** is assembled with token budgeting, message merging, and formatting for each specific AI provider
5. **Extensions** hook into ~100 lifecycle events to inject memory summaries, vector search results, translations, and more
6. **Group chats** cycle through multiple characters using configurable activation strategies, each generating in their own voice
7. **Server-side prompt converters** translate the unified internal message format into the native format of 50+ AI backends
