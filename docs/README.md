# Documentation index

vibemate's documentation is split by purpose. Read the reference pages to
understand the delivered behavior, and the plan pages to see what is still
planned.

## Rules for everyone

- [`../AGENTS.md`](../AGENTS.md): project, architecture, readability and
  verification rules.
- [`../src/AGENTS.md`](../src/AGENTS.md): strict React/TypeScript rules.
- [`../CONTRIBUTING.md`](../CONTRIBUTING.md): local checks and contribution
  expectations.

## Reference: how the delivered app works

- [`architecture.md`](architecture.md): Rust module responsibilities, command
  registration, persistence, credentials and extension points.
- [`frontend.md`](frontend.md): React structure, IPC boundaries, i18n, themes,
  UI behavior and automated checks.
- [`agents.md`](agents.md): fixed Agent catalog, read-only discovery, explicit
  version probing, supported write adapters and the Agent detail layout.
- [`configuration.md`](configuration.md): safe configuration writes, backups,
  atomic replacement, operation journal, history, version restore and deletion.
- [`skills.md`](skills.md): central Skill library, local import, source editing
  and deletion.
- [`mcp.md`](mcp.md): central MCP definitions, credential references, deletion
  and reviewed user-level registration for Claude Code, Pi, Grok Build and
  Codex.
- [`models.md`](models.md): provider model discovery, browse/selection, manual
  rows, capability evidence and model parameters.
- [`logging.md`](logging.md): Rust local logging and Settings log access.
- [`releases.md`](releases.md): changelog rules and the draft release workflow.
- [`integrations/model-parameter-mapping.md`](integrations/model-parameter-mapping.md):
  P24.a Agent field evidence, version/protocol restrictions and unresolved checks.
- [`integrations/codex.md`](integrations/codex.md): Codex user-level Provider,
  default model, MCP and Skill writes, ownership and known limits.
- [`integrations/`](integrations): verified third-party contracts for the three
  providers and the four configured Agents (Claude Code, Pi, Grok Build and
  Codex). Recheck them before changing a protocol mapping.

## Plans: what remains

- [`plans/phase-1.md`](plans/phase-1.md): phase 1 scope, delivered baseline and
  acceptance boundaries.
- [`plans/todo.md`](plans/todo.md): the single task-status source for remaining
  work.
- [`plans/model-parameters.md`](plans/model-parameters.md): model metadata and
  how existing Provider model definitions are injected into native Agents.
- [`plans/mcp-deployment.md`](plans/mcp-deployment.md): Pi registration lifecycle,
  entry ownership, private writes and recovery boundaries, plus the Grok Build
  preflight, preview and delivered write workflow (schema v33). Claude Code and
  Codex registration are described in `mcp.md`.
- [`plans/backend-modularity.md`](plans/backend-modularity.md): backend module
  boundaries and the unplanned future integrations.
- [`plans/configuration-versions.md`](plans/configuration-versions.md): independent
  saved-version storage and selection, with remaining acceptance in `todo.md`.

Completed designs and process records are not kept in a separate archive. Use
Git history and [`CHANGELOG.md`](../CHANGELOG.md) for what shipped and why.
`docs/local/` holds personal, Git-ignored learning notes.
