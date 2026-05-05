# Multi-Agent Worktree Procedure

Isolation protocol for agents working in parallel on the same local repo. Each agent operates in its own git worktree,
pushes to remote when done, then the changes are squashed into main under a merge lock.

## 1. Agent Lifecycle

### 1.1 Spawn

Agent is spawned via `EnterWorktree` with a unique name. This:

- Creates a new git worktree under `.claude/worktrees/<name>/`
- Creates a new branch `worktree-<name>` from current HEAD
- Sets the agent's CWD to the worktree path
- **All reads/edits outside the worktree path are rejected** (enforced by the harness)

### 1.2 Work

1. Read CLAUDE.md and DEVELOPER.md from the worktree root
2. Implement the assigned task — all file changes stay within the worktree
3. Commit changes on the worktree branch with meaningful messages and consistent style

### 1.3 Push & Delete Local Worktree

When work is complete:

1. Push the worktree branch to remote: `git push origin worktree-<name>`
2. Delete the local worktree: `git worktree remove .claude/worktrees/<name> --force`
3. Delete the local branch: `git branch -D worktree-<name>`

No local worktree remains after this step. The branch exists only on the remote.

### 1.4 Notify User

Agent MUST explicitly state:

- Task summary
- Remote branch: `origin/worktree-<name>`
- Files changed (list)
- Type-check result
- **"Ready to merge. Awaiting your approval."**

## 2. Merge Protocol

### 2.1 Pre-Merge Gate: Check Main Working Tree

Before ANY merge, the agent MUST check for uncommitted changes:

```bash
git status --porcelain
```

- **Empty** → proceed to 2.2
- **NOT empty** → **ABORT**. Notify user:
  > "Cannot merge: main working tree has uncommitted changes. Please commit or stash them first, then re-approve."

Never stash, commit, or touch the user's working changes. Abort immediately.

### 2.2 Acquire Merge Lock

A lockfile at `.claude/.merge.lock` serializes all merges. No two agents may merge concurrently.

1. Attempt to create `.claude/.merge.lock` with exclusive creation
2. **Success** → write agent name, timestamp, branch name. Hold the lock.
3. **Failure** (lock exists) → read the lockfile:
    - Lock age ≤ 5 min (fresh) → notify user, poll every 10s for up to 2 min, then give up
    - Lock age > 5 min (stale) → warn user, offer to break it

### 2.3 Fetch, Squash, Set HEAD

```bash
# Fetch the remote worktree branch
git fetch origin worktree-<name>

# Squash merge into main
git merge --squash origin/worktree-<name>

# Create squashed commit
git commit -m "<summary of worktree changes>"
```

If merge conflict occurs → abort, release lock, notify user with conflict details.

### 2.4 Release Lock & Notify

```bash
rm .claude/.merge.lock
```

Notify user: "Merged `worktree-<name>` into main. Remote branch deleted. Lock released."

## 3. Lockfile Details

### Format (`.claude/.merge.lock`)

```
agent=<agent-name>
timestamp=<ISO-8601>
branch=worktree-<name>
```

### Staleness

| Lock age | Action                            |
|----------|-----------------------------------|
| ≤ 5 min  | Valid — wait or abort             |
| > 5 min  | Stale — warn user, offer to break |

### Merge ordering

First-come, first-served via lock acquisition. No priority system. If wait exceeds 2 minutes, notify user to intervene.

## 4. Error Scenarios

| Scenario                     | Action                                          |
|------------------------------|-------------------------------------------------|
| Main has uncommitted changes | Abort, notify user                              |
| Lock held (fresh)            | Wait up to 2 min, then notify user              |
| Lock held (stale)            | Ask user, break if approved                     |
| Merge conflict               | Abort, release lock, notify user with conflicts |
| Remote branch not found      | Abort (worktree may have been deleted already)  |
| Type-check fails after merge | Abort, release lock, notify user                |
| Push fails                   | Keep local worktree, notify user                |

## 5. Agent Notification Template

```
## Task Complete

**Summary:** <1-2 sentences>
**Remote branch:** `origin/worktree-<name>`
**Files changed:**
- path/to/file1.ts — <what changed>
- path/to/file2.tsx — <what changed>

**Type-check:** passed

Ready to merge. Awaiting your approval.
```

## 6. User Commands

| Command                  | Effect                                      |
|--------------------------|---------------------------------------------|
| "approved, merge <name>" | Trigger merge protocol for worktree branch  |
| "show branch <name>"     | Show diff of remote worktree branch vs main |
| "abort <name>"           | Delete remote worktree branch (no merge)    |
