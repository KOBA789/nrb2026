---
name: smart-friend
description: |
  Use Codex CLI (OpenAI) to discuss and review code or wording. Codex is your smart friend.
  Triggers: "smart-friend"
  Use cases: (1) code review, (2) design consultation, (3) bug investigation, (4) investigation of hard-to-resolve issues
---

# Smart friend

A skill for running code review and analysis with Codex CLI. Codex is your smart friend.

## Command

codex exec --full-auto --sandbox read-only --cd <project_directory> "<request>" </dev/null

## Timeout

Set the Bash tool timeout to **600000** (10 minutes) when running codex commands.

## Prompt Rules

**Important**: The request you send to codex must always include the following instruction:

> "No confirmations or questions are needed. Proactively provide concrete proposals, fixes, and code examples."

## Parameters

| Parameter | Description |
|-----------|-------------|
| `--full-auto` | Runs in fully automatic mode |
| `--sandbox read-only` | Read-only sandbox (for safe analysis) |
| `--cd <dir>` | Target project directory |
| `"<request>"` | Request content (Japanese is also allowed) |

## Examples

**Note**: Each example includes the instruction at the end: "No confirmation needed; provide concrete proposals."

### Code Review
codex exec --full-auto --sandbox read-only --cd /path/to/project "Please review the code in this project and point out improvements. No confirmations or questions are needed. Proactively provide concrete fix proposals and code examples." </dev/null

### Bug Investigation
codex exec --full-auto --sandbox read-only --cd /path/to/project "Please investigate the cause of an error in the authentication flow. No confirmations or questions are needed. Proactively identify the root cause and provide concrete fix proposals." </dev/null

### Architecture Analysis
codex exec --full-auto --sandbox read-only --cd /path/to/project "Please analyze and explain the architecture of this project. No confirmations or questions are needed. Proactively include improvement proposals." </dev/null

### Refactoring Proposal
codex exec --full-auto --sandbox read-only --cd /path/to/project "Please identify technical debt and propose a refactoring plan. No confirmations or questions are needed. Proactively include concrete code examples." </dev/null

## Execution Steps

1. Receive the user's request.
2. Identify the target project directory (current working directory or user-specified directory).
3. **When building the prompt, always append: "No confirmations or questions are needed. Proactively provide concrete proposals."**
4. Run Codex in the command format above.
5. Report the results back to the user.
