**IMPORTANT: Read DEVELOPER.md before anything else, update it after you have modified the codebase, keep concise.**

If user's intention is ambiguous, be sure to ask first.

Focus on modularity and extensibility. Avoid type redefinition and magic strings. Do not let one file grows too big (>700 lines).

Do not start multiple `Explore` sub-agents unless you have read DEVELOPER.md and deem it necessary.

## Other notice

- No need to add license header for new files, user will run `npm run add-license-header` manually.
- When you want to get the overall files structure, run exactly this command: `wsl -e tree -I 'node_modules|.git|dist|build|target'`.
