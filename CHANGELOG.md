# Changelog: Elysian Dialogue

All notable changes to this project will be documented in this file. Should be extremely concise for each entry. Only main change can be added here.

### 2026-04-28
- **Refactoring**: Modularized agent tools. Moved inline GM drafting and Assistant verification tools from `LlmServiceBackend.ts` to dedicated files in `src/services/tools/` using factory patterns for dependency injection (e.g., drafts, callbacks).
- **Debug Panel**: Added "WRAP" button to Console logs, consistent with LLM trace functionality.
- **AI Engine**: Added a double-LLM verification loop in `LlmServiceBackend.ts`. The primary "GM" can only draft changes to the world state, plots, and dialogue using new draft-based tools. A secondary "Assistant" LLM reviews the GM's drafts and forces revisions or commits them if they are sound, ensuring stricter adherence to rules.
- **Bug Fix**: Corrected `Error` object serialization in `ConsoleLogger.ts`. Previously, errors were logged as empty objects `{}` due to non-enumerable properties; improved with custom Zod-friendly serialization and stack trace capture.
- **Logging**: LLM interaction logs are now initiated at the start of the request, ensuring visibility even during catastrophic API failures.

### 2026-04-27
- **Docs**: Restructured `DEVELOPER.md` for better organization and updated the project structure tree.
- **Docs**: Summarized AI capabilities in `DEVELOPER.md` including context, logic loop, and tools for another LLM instructions.
- **Debug Panel**: Added comprehensive filtering for Console logs (Keywords/Regex, Level toggles, Date Range).
- **Debug Panel**: Implemented SQLite persistence for console logs. Captures browser logs in real-time and hydrates the view on refresh.
- **Debug Panel**: Added date to timestamps in LLM trace and Console logs for better troubleshooting.
- **Debug Panel**: Added text wrap toggle for LLM logs and fixed related horizontal layout issues.
- **Debug Panel**: Enhanced `ConsoleViewer` and LLM trace with interactive JSON inspection and visual fixes.
- **Project**: Reorganized codebase into `client/` and `server/` and updated documentation.
- **UI**: Polished debug panel interaction and simplified changelog format.

### 2026-04-25
- **Advanced Debugging**: Major upgrade to the `DebugPanel`.
- **Design & UI**: Unified toggle button aesthetics and introduced "One Dark" syntax highlighting.
- **System**: Refined LLM request logging and corrected model configurations.

### 2026-04-24
- **Layout**: Resolved critical scroll container and viewport height issues.

### 2026-04-23
- **AI Engine**: Migrated to **Vercel AI SDK** with **Zod** schema validation.
- **Narrative**: Switched core model to **DeepSeek-V3** and hardened system prompts.
- **Polish**: Applied global cinematic styling and refined message flow animations.
