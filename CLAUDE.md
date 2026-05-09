# CLAUDE.md

Repository for developing the ISUNARABE Joint Exercise 2026 problem set (webapp +
benchmarker + AMI/CFn/bench.sh + problem.json). The overall event plan is in
[docs/project.md](docs/project.md), and the problem concept is in
[docs/idea.md](docs/idea.md).

## Design principles

### webapp (the target application)

1. API first. The frontend exists only so contestants can understand and operate the application; polished design is not required.
2. Express the external specification correctly. The distributed version of the code may be slow and ugly (improving it is the whole point of the contest), but it must satisfy the external specification.

### bench

1. The benchmarker's correctness is the foundation of the contest. False positives in inconsistency detection are especially stressful. It is better to miss an inconsistency than to flag one unfairly.
2. Performance matters. If the benchmarker is too slow, scores saturate and rankings stop separating contestants.

### Everything else (build pipeline, etc.)

1. Keep it simple. Do not introduce unnecessarily complex mechanisms.
2. Keep it fast. Authoring depends on the number of trial-and-error cycles, so slow mechanisms are unwelcome.

## Development Workflow

- Before starting implementation, use the smart-friend skill to get a plan review. When in plan mode, get the smart-friend review **before** presenting the plan to the user (i.e., before calling ExitPlanMode).
- After implementation, use the smart-friend skill to get a code review before committing.
- When a smart-friend review raises issues, address them immediately if feasible. Issues deferred for later must be appended to [docs/todo.md](docs/todo.md) with the review context (what was reviewed, what was pointed out, and why it was deferred).
- Follow TDD (Test-Driven Development). Write tests before implementing features.
  - Write webapp tests so they are easy to separate out. Putting them at the end of the source file makes them hard to strip when preparing the distributed version.
  - The benchmarker is effectively part of the webapp's test suite alongside `cargo test`. Grow the two in tandem.

## 用語集 Terminology

ISUCON には独特な用語がいくつかある。

- 初期実装・参考実装: 競技者が最初に与えられるコード。具体的には webapp を指す。アプリケーションコードだけでなく、インフラ構成全体を指すこともある。
- FAIL: ベンチマーカーが0点で完了すること。レギュレーション違反による失格のことではない。
- 失格: レギュレーション違反による失格と、追試で条件を満たさず0点扱いになることの両方を指すことがあり曖昧。
- 参加者・競技者・選手: 競技に参加する人。
- 運営: 本家 ISUCON では競技を運営する人。問題作成者も含む。ISUNARABE では「主催者」とも呼ぶ。

## Reading order (at the start of a new session)

1. [docs/README.md](docs/README.md) — entry point for the whole `docs/` tree
2. [docs/project.md](docs/project.md), [docs/idea.md](docs/idea.md) — event plan and problem concept
3. [docs/authoring/README.md](docs/authoring/README.md) → [docs/authoring/platform.md](docs/authoring/platform.md) — required background for authoring decisions
4. Refer to `../isunarabe2/docs` via [docs/isunarabe-references.md](docs/isunarabe-references.md) as needed

## Handling of the ISUNARABE2 specification

The ISUNARABE2 portal specification (architecture / contest / problem-descriptor /
benchwarmer, etc.) lives in a separate repository at `../isunarabe2/docs/`, which is
the primary source. This repository does not duplicate it; refer to it via
[docs/isunarabe-references.md](docs/isunarabe-references.md).
