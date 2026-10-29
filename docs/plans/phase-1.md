# Phase 1: shared Agent configuration

## Confirmed scope

- MIT-licensed desktop app for macOS, Windows and Linux: stable Rust, edition
  2024, Tauri 2, React and TypeScript. Follow current stable toolchains and APIs.
- Providers: Command Code, DeepSeek and OpenRouter.
- Fixed Agent catalog and read-only discovery: Claude Code, Codex, OpenCode, Pi,
  Grok Build and Nous Research Hermes Agent. All six detectors are implemented.
  Configuration deployment exists for Claude Code, Pi, Grok Build and Codex
  (Codex: user-level Provider, optional default model, MCP and Skills; awaiting
  review fixes and maintainer acceptance); the maintainer edits code in Pi
  daily, so Pi is never the first write target.
- Shared skills with source/revision tracking and updates; shared MCP definitions
  and explicit per-Agent deployment.
- Simplified Chinese/English UI with system detection and persistent choice.
- Beginner-readable code, English code comments, native CI and draft releases.

Behavior references: [`agents.md`](../agents.md), [`configuration.md`](../configuration.md),
[`skills.md`](../skills.md), [`mcp.md`](../mcp.md), [`models.md`](../models.md),
[`logging.md`](../logging.md) and [`frontend.md`](../frontend.md).

## Delivered baseline

Implemented and accepted: scaffold, stable toolchain, private SQLite storage, OS
credential interface, i18n startup and preferences, desktop shell and overview,
provider/key saving, model discovery/selection/manual entries, aliases,
capability evidence, context window and output limits, thinking-effort
declarations, provider deletion with credential compensation, central MCP
management and deletion, local Skill import and deletion, six-Agent read-only
discovery, explicit and automatic CLI version probing, and the shared safe write
path with history.

Claude Code, Pi and Grok Build have configuration preview, apply, full paginated
history, direct restoration of retained versions, interrupted-write inspection,
and history deletion. Provider/model P10–P12 and Command Code routing P15/C15 have
maintainer acceptance; P15's real credential connection is a maintainer report,
not an agent-executed test. Previews follow the shared two-layer presentation
described in [`configuration.md`](../configuration.md).

Acceptance dates and the remaining verifications are recorded in
[`todo.md`](todo.md), the only task-status source. Completed process records,
old file lists and per-run test counts belong in Git and `CHANGELOG.md`.

## Remaining work

The order below is the intended dependency order. Task cards, acceptance criteria
and current blockers are in [`todo.md`](todo.md).

1. Maintainer desktop sign-off for the History interface additions (deletion UI,
   current-configuration pin, Write/Detail list actions). Windows/Linux native
   behavior and a dedicated screen-reader pass stay in the release-time platform
   check.
2. Complete P24 injection of existing Provider model definitions into Claude
   Code, Pi and Grok Build, then check real Provider/Agent combinations. Do not
   add a second model editor or request-tuning controls. For Grok's Anthropic
   Messages backend, keep the existing refusal until xAI documents its key handling.
3. Skill installation from a confirmed source, update preview and application,
   and per-Agent deployment with symlink preference and a copy-sync fallback.
4. Central MCP deployment to each Agent with a verified native format, using the
   safe write, history and restore path. Unsupported features remain explicit.
5. Deletion constraints for referenced Providers, models, Skills and MCP
   definitions once deployment relationships exist.
6. Interrupted-operation recovery entry for Skill/MCP deployment once operation
   types beyond Agent configuration exist.
7. Frontend usability pass with real pages, cross-platform verification, real
   credentials/integration acceptance, and the first reviewed draft release.

## Acceptance boundaries

A user can configure verified providers, choose a model and supported settings
for each compatible Agent, preview/apply/restore changes and understand rejected
combinations. Skill updates preserve local edits; MCP maps only to verified
features. Every Agent write has preview, backup and rollback.

Keep secrets out of logs, URLs, frontend persistence and exports. Real requests
require explicitly permitted test credentials; record actual versions and results.
Discovery, managed installation, configuration support, written configuration,
effective runtime state and inference success are separate claims.

## Deferred

Subscription/quota monitoring, token heatmaps, automatic routing/failover,
arbitrary executable plugins, cloud sync and release publication. GitHub backup,
source installation, Agent installation and local API forwarding remain separate
proposals in [`backend-modularity.md`](backend-modularity.md) until a concrete
behavior and its boundaries are implemented.
