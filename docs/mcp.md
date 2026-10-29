# MCP definitions and Agent registration

`src/features/mcp/` and `src-tauri/src/mcp/` manage central MCP definitions and
reviewed user-level registrations for Claude Code, Pi, Grok Build and Codex.
Saving or enabling a **central definition** never changes an Agent file. The MCP
detail page has one row per Agent; its **Manage Pi**, **Manage Grok Build**,
**Manage Claude Code** and **Manage Codex** actions preview and confirm
deployment/update, revoke and retained-entry restore. Pi, Grok Build and Codex
also offer native enable/disable; Claude Code has no user-level per-server
enable flag. None of these actions starts, installs or contacts a server. All
MCP registration is user-level only; project, local-scope and managed/policy
files are never written. Other catalog Agents report that native MCP deployment
is not implemented.

| Agent       | Target (Rust-selected)                          | Native entry                  | Schema |
| ----------- | ----------------------------------------------- | ----------------------------- | ------ |
| Pi          | `PI_CODING_AGENT_DIR\|~/.pi/agent` → `mcp.json` | `mcpServers.<name>`           | v31    |
| Grok Build  | `GROK_HOME\|~/.grok` → `config.toml`            | `[mcp_servers.<name>]`        | v33    |
| Claude Code | `CLAUDE_CONFIG_DIR\|home` → `.claude.json`      | top-level `mcpServers.<name>` | v35    |
| Codex       | `CODEX_HOME\|~/.codex` → `config.toml`          | `[mcp_servers.<name>]`        | v36    |

Task status is recorded only in [`plans/todo.md`](plans/todo.md) (P32, P33,
P33.b, P41); the Pi and Grok Build implementation decisions are in
[`plans/mcp-deployment.md`](plans/mcp-deployment.md).

## Evidence and supported fields

Field evidence was checked against [Pi MCP](https://pi.dev/docs/latest/mcp),
[Grok Build MCP](https://docs.x.ai/build/features/mcp-servers),
[Claude Code MCP](https://code.claude.com/docs/en/mcp), the Codex sources and
documentation recorded in [`integrations/codex.md`](integrations/codex.md) and
the
[MCP transports specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports).
Pi's installed 1.4.0 documentation, configuration parser and value resolver were
also inspected and exercised without starting a session or server. Per-Agent
contract notes are in [`integrations/`](integrations).

Central definitions support:

- Stdio: one executable, ordered arguments, optional working directory and
  environment variables.
- Streamable HTTP: URL and request headers.
- OAuth, old SSE transport, dynamic secret commands, imports from other tools
  and protocol bridges are not supported.

Each adapter maps these fields independently; the per-Agent sections below list
the written keys and the inputs each Agent refuses rather than drops.

## Central storage and credentials

Definitions have random stable IDs and revisions. Lists use ID cursor pages,
report the stored total and reject concurrent updates. The server identifier is
separate from the display name: ASCII letters, digits, underscores and hyphens,
with hyphen/underscore namespace collisions rejected. Forms derive it on first
save and preserve it on edit; they do not expose a second identifier editor.

HTTP requires HTTPS or loopback HTTP without credentials, query or fragment.
Header names are case-insensitive; environment names are portable identifiers.
Arguments, fields and values have size/count limits. Each argument is one literal
value without shell quoting. Dynamic substitutions are not supported.

Every environment/header value uses OS credential storage, including harmless
values. SQLite stores field names and opaque references, not those values.
Unchanged fields keep their references; replacements get fresh references. Known
failed saves compensate by deleting new unused references; uncertain outcomes
retain them and require reconciliation. Obsolete references are queued in the
metadata transaction and retried without deleting active references.

`McpForm` clears typed values on submit, cancel, transport change and hide, before
a hidden panel can retain them through rapid navigation. No frontend persistent
storage is used. Commands run blocking work off the async executor and reuse the
credential mutation lock. No new dependency, capability or CSP permission was
needed for Pi registration.

## Shared review and safe application

Rust selects each target root using Agent discovery's rules. IPC accepts
identity, central revision, action, optional retained-result identity and
approval; it never accepts a write path or native document from React. The
shared target reader bounds input. Invalid containers, symbolic-link targets,
read-only or unavailable targets and name collisions block mutation.

The review shows the target, field summary and a separate before/after entry
view. All environment/header values are placeholders. For all four Agents the
review and the history summaries and full texts also mask credentials inside
otherwise public fields: credential-like URL query parameters (`api_key`,
`token`, `access_token`, `key`, `sig`, `signature`, `X-Amz-Credential`) and the
password in `user:password@`, plus `args` such as `--api-key`/`--token` values,
`Bearer <value>`, `Authorization: …`-style header arguments and credential-like
`NAME=value` assignments. The masking is for display, approval digests and
history only; the written file keeps the original values. Unrelated existing native
content is never returned. Preview does read native bytes that can contain
existing values, but does not read OS credentials or authorize a different plan.
The entry digest used for ownership stays in Rust and is not sent over IPC.

Confirmation holds the cross-instance configuration lock while rebuilding the
approval, recording a durable intent, invoking the shared private writer and
committing ownership. Approval binds the complete target token, central revision,
action, selected history and entry generation. Revoked/aborted generations also
invalidate earlier approvals, even if the native file returns to identical bytes.
The existing writer provides concurrent-change checks, private pre-write backups,
atomic replacement, operation journals and independent complete result copies,
and writes the target as owner-only (0600 on Unix).

Only deploy/update loads current OS values in Rust. Values that are empty,
contain control characters or contain `${` are refused
(`credential_unavailable`). Enable/disable preserves the owned native values;
revoke removes only the owned entry. Every Agent receives plaintext in its file,
so backups and retained copies may also contain plaintext. These bytes never
cross IPC or enter application logs.

## Ownership, history and interruptions

Each adapter has its own root/definition ownership and durable change intents
(schemas above). Ownership records the native name, central revision, entry
digest, last change identity and, where the Agent has one, the enabled flag. It
is not whole-file ownership: other servers and top-level fields remain
user-owned and are preserved.

An existing foreign entry is never adopted (`name_conflict`). An externally
modified owned entry is never overwritten or removed (`ownership_conflict`); what
counts as a modification differs per Agent (see below). An owned entry that was
removed externally can be explicitly revoked; revoking a missing file releases
metadata without recreating it. Renaming a native identifier requires revoking
its old registration before deploying the new name.

History is paginated per definition and active root. Selecting a result
restores **that entry** from that operation's own verified complete result copy,
not its pre-write backup or a neighboring operation's copy. Other current entries
and top-level fields are preserved. A revoked historical result restores absence
only when it does not take over a foreign entry. Missing/corrupt/out-of-scope
copies fail explicitly. Restoring may bring back old plaintext credentials; the
review warns before confirmation and never reads current OS values for restore.

A prepared intent blocks further MCP writes to that root and central deletion.
After restart, management observes the exact before/intended file digests and
provides a separate explicit interruption review. Confirmation repairs the shared
journal and finalizes or abandons ownership **without rewriting the native file**.
External divergence or unreadable state stays blocked with actionable feedback;
there is no automatic replay. A missing/corrupt required copy also keeps recovery
blocked rather than claiming success. Completed audit history alone does not
prevent central deletion after revoke. Pending changes for all four Agents are
also listed in Overview's RecoveryHub.

## Agent registrations

### Pi registration

- **File and keys:** `mcpServers.<name>` in `PI_CODING_AGENT_DIR|~/.pi/agent/mcp.json`.
  Stdio writes `command`, `args`, `env`, optional `cwd` and `enabled`; HTTP
  writes `url`, `headers` and `enabled`. A missing file is not treated as an
  empty/invalid existing file.
- **Values:** Pi 1.4.0 resolves `$NAME`/`${NAME}` and interprets `$!`, so
  deploy/update escapes every literal `$` from OS storage as `$$` and also
  refuses values starting with `!`.
- **Enable/disable:** the native `enabled` flag. A first deploy writes the
  central definition's enabled state.
- **Names:** hyphens and underscores share one namespace, so `my-tools` and
  `my_tools` collide (`name_conflict`).
- **Hand edits:** the digest covers the complete JSON entry; any change,
  including a manual `enabled` flip, marks it modified and locks it.
- **Runtime:** run `/reload` in Pi. Trusted project entries can override the
  user-level registration; runtime flags or replacement `/mcp` extensions can
  bypass it. Relative cwd is resolved against the Pi session directory, and
  leading `~/` expansion follows Pi's runtime home.

### Grok Build registration

- **File and keys:** `[mcp_servers.<name>]` in `GROK_HOME|~/.grok/config.toml`,
  edited with `toml_edit`; model tables and `disabled_mcp_servers` are
  preserved. Stdio writes `command`, `args`, `env`, optional `cwd` and
  `enabled`; HTTP writes `url`, `headers` and `enabled` with no `type` key.
  A non-table `mcp_servers` or malformed `disabled_mcp_servers` is
  `invalid_config`. Revoking the last entry removes the empty `mcp_servers`
  table.
- **Refused inputs:** `${` in command, arguments, working directory or OS values
  (`dynamic_value_unsupported` / `credential_unavailable`); a relative or `~/`
  working directory and leading `~/` in command/arguments
  (`path_semantics_unverified`); names that start with a digit or hyphen,
  contain `__` or end with `_` (`unsupported_server_name`), because Grok cannot
  admit their qualified tools.
- **Enable/disable:** the native `enabled` flag. A first deploy writes the
  central definition's enabled state. The review reports when the name is also
  in `disabled_mcp_servers`; vibemate never edits that personal list, so an
  enabled entry can still be inactive.
- **Names:** exact names only; hyphens and underscores stay distinct.
- **Hand edits:** the digest covers the managed keys (`command`, `args`, `env`,
  `cwd`, `enabled` or `url`, `headers`, `enabled`). Changing one of them,
  including a manual `enabled` flip, marks the entry modified and locks it.
  A key outside that set (for example a timeout or tool filter added by hand)
  is listed as `unmanagedKeys` and also makes the entry modified, as does a
  non-boolean `enabled`. vibemate refuses every action on a modified entry
  (`ownership_conflict`, or `invalid_config` when `enabled` cannot be read), so
  hand-added keys are never dropped; the manager shows a notice naming the
  unmanaged keys and the row stays locked until the user reverts the edit.
- **Layout:** replacing an existing table keeps its leading comments and its
  position in the file. A whole inline `mcp_servers = { ... }` value is refused
  as `invalid_config` at preview for deploy, enable, disable and restore; a
  revoke still removes the entry from an inline parent, so it never reports
  success while the entry stays on disk.
- **Runtime:** refresh `/mcps` or start a new Grok session. Project, policy and
  compatibility sources may take precedence.
- **Earlier preview:** the read-only **Preview for Grok Build**
  (`preview_grok_mcp_registration`, `McpGrokPreview`) is still in the code but
  no screen opens it; **Manage Grok Build** replaced it.

### Claude Code registration

- **File and keys:** only the top-level `mcpServers.<name>` member of
  `<CLAUDE_CONFIG_DIR|home>/.claude.json` (beside, not inside, `~/.claude/`).
  An invalid `CLAUDE_CONFIG_DIR` is unavailable and never falls back to the
  home file. vibemate never writes `projects.<path>.mcpServers` (local scope),
  `.mcp.json` (project scope) or `managed-mcp.json`. Edits splice only the owned
  member and copy every other byte, because Claude Code keeps account, session
  and project state in the same file; an absent file is treated as `{}`, and
  revoking the last server leaves `"mcpServers": {}`. A non-object document,
  a non-object `mcpServers` or a duplicated relevant key is `invalid_config`.
- **Written fields:** stdio writes `type = "stdio"`, `command`, `args` and `env`
  (when present); HTTP writes `type = "http"`, `url` and `headers` (when
  present). Legacy SSE is never inferred.
- **Refused inputs:** a stdio working directory (`unsupported_field`, because
  Claude Code's user scope would drop it); `${` in command, arguments, URL or
  OS values; leading `~/` in command/arguments; the reserved names
  `workspace`, `claude-in-chrome` and `computer-use`
  (`unsupported_server_name`).
- **Enable/disable:** not supported (`toggle_unsupported`); a centrally disabled
  definition cannot be deployed. Disable per project with `/mcp` in Claude Code,
  or revoke.
- **Names:** exact top-level names; local-scope entries under `projects` are
  neither read as conflicts nor written.
- **Hand edits:** the digest covers the complete owned entry; any change marks it
  modified and locks it. Because Claude Code rewrites unrelated keys at any time,
  the post-write check compares the owned entry rather than the whole file.
- **Runtime:** start a new Claude Code session or reconnect with `/mcp`. A
  same-named server in a project's local scope takes precedence.

### Codex registration

- **File and keys:** `[mcp_servers.<name>]` in the user-level
  `$CODEX_HOME/config.toml` through `toml_edit`, preserving every other key, comment and the
  model tables written by the Codex settings adapter. Project
  `.codex/config.toml` files are never written. The root is resolved by the same
  function as discovery and the Codex settings adapter: a non-empty
  `CODEX_HOME` must already exist as a directory and is used as its
  canonical path; when it is empty or unset, `~/.codex` is used. vibemate never
  creates `$CODEX_HOME`; a missing directory returns `target_unavailable` and
  nothing is written. Stdio writes `command`, `args`,
  `env`, `cwd` and `enabled`; Streamable HTTP writes `url`, `http_headers` and
  `enabled`. A central `Authorization` header is written as a plain
  `http_headers.Authorization` value, because Codex refuses an inline
  `bearer_token`. vibemate never writes `bearer_token`, `bearer_token_env_var`,
  `env_vars`, `env_http_headers`, `http_headers_helper`, `auth`/`oauth` or a
  `type` key. Replacing an existing table keeps its leading comments and its
  position. A whole inline `mcp_servers = { ... }` value is refused as
  `invalid_config` at preview for deploy, enable, disable and restore, before
  any approval exists; a revoke still removes the entry from an inline parent.
- **Refused inputs:** `${` in command, arguments, working directory, URL or OS
  values (`dynamic_value_unsupported` / `credential_unavailable`); a relative or
  `~/` working directory and leading `~/` in command/arguments
  (`path_semantics_unverified`); names outside `[A-Za-z0-9_\-:@/.]+` or longer
  than 64 characters (`unsupported_server_name`). Names are not normalized.
- **Enable/disable:** the native `enabled` flag. A first deploy writes the
  central definition's enabled state.
- **Hand edits:** ownership (`codex_mcp_deployment`, `codex_mcp_change`)
  compares a semantic entry digest: inline versus dotted tables and an omitted
  `enabled = true` count as unchanged. Any other change, including a manual
  `enabled = false`, a non-boolean `enabled` or an unmanaged key such as
  `enabled_tools` (`unmanagedKeys`), is an external modification: every action returns
  `ownership_conflict` (`invalid_config` when `enabled` cannot be read) and the row is locked until the user reverts it. A manual
  disable is shown as **Manually disabled in Codex**.
- **Interlocks:** a pending Codex MCP change also blocks Codex settings writes,
  and a pending settings change blocks Codex MCP writes.
- **Runtime:** start a new Codex session. A profile or project config may
  override these entries. Contract details are in
  [`integrations/codex.md`](integrations/codex.md#mcp-配置).

## Deletion and registration states

The delete dialog reads Pi, Grok Build, Claude Code and Codex references before
enabling delete. It shows paths/states and links back to each Agent's
management. Deletion repeats its revision/reference check in an immediate
transaction; any owned registration in any of the four Agents (including
disabled/missing/modified entries) or pending change returns `mcp_in_use`.
References recorded under another root (for example a different
`PI_CODING_AGENT_DIR` or `GROK_HOME`) must be revoked with that root selected.
There is no silent cascade into Agent files.

After all references are released, central deletion queues OS references in the
same transaction. Only a confirmed commit starts OS deletion. Uncertain commits
leave OS entries intact; cleanup remains visible/retryable after restart, even
with an empty library. One cleanup request handles at most 100 removed identities
and excludes active references. Failures preserve recoverable metadata.

The MCP list describes recorded registrations, not connections. Each supported
Agent's detail MCP segment is read-only: it reports `written`, `modified`,
`missing` or `unreadable` entries (Pi, Grok Build and Codex also show the
enabled flag) and links to central management. Agents without MCP registration
explicitly report that native MCP deployment is unimplemented. Reads finish all
root pages before a partial result can imply that another definition is
unregistered.

Written, enabled, effective and connected are different claims; this UI reports
only the first two and observed disk state.

## Verification scope

Rust lifecycle tests use real isolated SQLite/private files and synthetic
credential stores. They cover both transports, masking/literal-dollar encoding,
private permissions, preservation, roots/namespaces, missing/modified ownership,
central deletion protection, selected-result restore, corrupt/borrowed copies,
pagination, stale/repeated/ABA approvals, cross-instance locking and failures
before writing, after native rename and before ownership finalization. Restart
recovery never replays a write. IPC/UI tests cover exact masked boundaries,
review/confirm/cancel, duplicate/dismiss guards, uncertain outcomes, history and
recovery review, reference checks, paging and late hidden/unmounted responses.

An isolated macOS **real Tauri WebView and IPC** run used a unique application
identifier, temporary HOME/application data and temporary Pi root, with no OS
credential fields. Nine checks exercised UI deployment, enable, disable, revoke,
selected-result restore, native deletion protection, Agent summary and final
central deletion. Six generated result copies passed the installed Pi 1.4.0
parser; 14 pure parser/value assertions also checked HTTP shape and literal
`$`/`$$`/`$!` handling. This first run started no server or Agent session, made no
Provider/external connection and did not modify real Agent files.

After explicit user approval, a second real Tauri run deployed harmless local
stdio and loopback Streamable HTTP fixtures through the actual management UI.
Nine `pi mcp list --json` probes using installed Pi 1.4.0 confirmed fixed-tool
discovery when enabled, no additional stdio starts/HTTP initializations after
disable, reconnection after selected-result restore and absence after revoke.
The CLI had a clean environment, isolated HOME/cwd, `PI_OFFLINE=1`, version checks
disabled and no Provider credentials. Both central definitions were deleted only
after revoke; no intent remained pending, unrelated native content was preserved
and all temporary servers exited. No Agent inference session or external service
was involved.

The earlier Grok preview added isolated default/override target, malformed/oversized file,
symlink, stale-revision and no-write tests, strict client response/masking tests,
and UI retry/reload, close/hide, language-preservation and focus behavior tests.
Chrome DevTools inspected its actual components with synthetic IPC data in both
locales/themes, 720×560 long-name/path cases, a 1080×760 list, nested Escape/focus
return and keyboard retry. This caught and fixed action groups squeezing the row
identity at the minimum size. These mocks cover only the preview components;
they do not prove actual Grok WebView IPC, native deployment or connectivity.

Grok Build, Claude Code and Codex each have Rust lifecycle tests in
`src-tauri/src/mcp/{grok,claude,codex}_deployment/tests.rs` using isolated roots,
SQLite and synthetic credential stores. They cover both transports, masking,
preservation of unowned content, foreign and externally modified entries,
stale targets and revisions, deletion protection, selected-entry restore,
interrupted changes without replay and refused dynamic values; the Claude Code
tests also cover the unsupported toggles and `cwd`, and the Codex tests the
semantic digest and `unmanagedKeys`. Frontend tests cover the Claude Code
manager, the Codex review and the per-Agent MCP rows. No real Grok Build,
Claude Code or Codex session, server or desktop WebView run is recorded for
these three adapters; that acceptance remains open in
[`plans/todo.md`](plans/todo.md).

Browser fixtures separately inspected English/Chinese, light/dark, 720×560,
long names/paths, masked nested details, keyboard focus/Escape and interruption
review. Browser mocks do not establish native behavior; the isolated Tauri run
does not establish native Windows/Linux behavior. Local fixture discovery/disable
is verified; real third-party services, OS-credential deployment acceptance and
native Windows/Linux checks remain separate acceptance evidence.

Earlier central-definition checks include isolated Linux no-secret stdio
save/edit/restart and maintainer credential/native/appearance review; they must
not be mistaken for acceptance of the newly added Pi deployment path.
