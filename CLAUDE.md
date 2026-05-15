IMPORTANT: **Read DEVELOPER.md BEFORE DOING ANYTHING ELSE**. DEVELOPER.md may contain outdated information, so check the exact source files. You should update it after you have modified the codebase, keep concise.

If user's intention is ambiguous, be sure to ask first.

Focus on **MODULARITY** and **EXTENSIBILITY**. Avoid type redefinition and magic strings. Do not let one file grows too big (>700 lines).

Do not start multiple `Explore` sub-agents unless you have read DEVELOPER.md and deem it necessary.

No need to add license header for new files, user will run `npm run add-license-header` manually.
