# Known Issues

## 1. LLM Trace Formatting in Debug Panel (`DebugPanel.tsx`)
**Date:** 2026-04-29
**Description:** 
The LLM response object structure returned by the AI provider differed from previous expectations, causing the `GM_Assistant_Communication` section of the LLM trace in the `DebugPanel` to render incorrectly or remain hidden.
**Root Cause:** 
- The raw `log.response` could be a parsed JSON object or string, which caused issues when parsing.
- The Vercel AI SDK/DeepSeek responses structure their steps as an array of `content` items, where each item has a `type` (`text`, `tool-call`, `tool-result`) instead of standard fields.
- Additionally, tool call arguments were located under `call.input` rather than `call.args`, and tool results were under `result.output` rather than `result.result`.
**Resolution:** 
Refactored the parsing logic in `DebugPanel.tsx` to handle both strings and objects directly. Iterated through `step.content` filtering by `type` to properly render `GM (Thinks)`, `GM (ToolName)`, and the corresponding `System`/`Assistant` responses. Used fallbacks (`call.args || call.input` and `result.output ?? result.result`) to accommodate different schema variants.

## 2. Debug Panel UI Clutter
**Date:** 2026-04-29
**Description:**
The "GM_Assistant_Communication" section in the Debug Panel is too cluttered with UI elements like resizable handles and scrollbars for every individual tool call.
**Root Cause:**
Each tool call argument and result is wrapped in a `ResizableContainer`, leading to excessive UI overhead and vertical space consumption.
**Target:**
Simplify the layout by removing resizable handles and optimizing the JSON display for higher information density.
