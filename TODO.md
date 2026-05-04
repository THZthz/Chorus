None of your business, LLM coding agent (your name's Claude?). This file is ONLY used by **HUMAN**.

The TODO is more like prompts.

Order does not matter.

- [x] Refresh story-telling style.
- [x] Remove HISTORY/DIALOGUE_BUFFER page in DebugPanel, I don't think it is useful.
- [x] Improve UI of PARSED_EXCHANGE in LLM_TRACE page of DebugPanel. Specifically, do not simply display JSON for tool call input.
- [x] Tool name of PARSED_EXCHANGE should separate by space between words.
- [x] Sync PLOT_TREE with DIALOGUE_TREE's replay state, reflecting current plot status, modifying it will also modify the plot snapshot at this moment.
- [x] Make PLOT_TREE also have active/inactive dim effect based on current replay status (or normal status).
- [x] Verify in all GM tool's input/output, only plain ASCII exists (only English text, no emoji, no other languages). When this verification fails, tell GM in the tool call result.
- [x] Fix the "cannot open" error of console log, dialogue buffer when it is updating. The page switch button's UI flickers. [[LIMITATION]]
- [x] In DIALOGUE_TREE and PLOT_TREE in DebugPanel, the node graph starts small at top left corner and flicker to center on the graph when the page is opened.
- [x] Make GM's system prompt runtime configurable, add a page in DebugPanel. Use templating word like {{active_plots}}, {{entities_brief}}, etc. to represent dynamic content.
- [x] I can see that plot node card is split into 4 parts vertically and dialogue node card is split into 3 parts. Now make their last 2 parts the same in height.
- [x] Make queryEntity tool of GM can bulk query.
- [x] Make getPlot tool of GM can bulk get plots. Also, make PARSED_EXCHANGE section in LLM_TRACE page reflect this.
- [x] I found ASCII restriction on tools' input is too harsh for GM. Let GM can use chars like '—' and other. But emoji and chars from other languages is prohibited.
- [x] Do not use Date.now for entity ID, use shorter version: /[A-Za-z0-9]{4}/, 4 characters.
- [x] JsonNode for JSON output of tool calls in PARSED_EXCHANGE section in LLM_TRACE page of DebugPanel.
- [x] Review skill checks.
- [x] Remove the dice rolling UI, just delay 1 sec and show the result.
- [x] Should tell GM more info on {{entities_brief}} and {{active_plots}}.
- [x] Display more info on skill check result, current one (like `[CLOCKWORK - Challenging 10] FAILURE (8 vs 10)`) is too simple, the option text is missing.
- [x] When generateDialogueStep retry with same messages, the messages are typed again (redundant animation).
- [x] Separator '---' in Markdown editor of "GM System Prompt" is not working. [[LIMITATION]]
- [ ] Review system prompts' sections about tool using.
- [ ] Simplify system prompt, inject into user's prompts.
- [ ] Further refine system prompt to make GM generate well polished and consistent styled text.
- [ ] Significant checks should be recorded.
- [ ] A system to analyze user's input.
- [ ] Analyze dialogue steps and see if there are any points to improve.
- [x] Analyze the structure of following INCOMING_RESPONSE example, Remove OUTGOING_REQUEST, improve PARSED_EXCHANGE UI to cover most information shown in INCOMING_RESPONSE (focus on "steps" field). For system prompt, only show content that are different (dynamic content, plots and entities info).                                                                                                  
- [x] Check the usage of "Regenerate All Leaf Steps". Should we remove it? (Removed)
- [x] Dialogue options should have two separate display: one to display on options text, another to display as a message in dialogue history after player chosen the option. Keep skill check result display option text. Update system prompt for this. Make sure this is also persisted in database. No need to consider data migration or backward compatibility.
- [x] `textarea` in ENTITY_MANIFEST, DIALOGUE_TREE and PLOT_TREE should have thinner scrollbar like the scrollbar used in JsonExplorer/JsonNode.


