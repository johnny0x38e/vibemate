# MCP definitions and Pi registration

`src/features/mcp/` and `src-tauri/src/mcp/` manage central MCP definitions and
reviewed user-level Pi registrations. Saving or enabling a **central definition**
never changes an Agent file. The separate **Manage Pi** action previews and
confirms deployment, update, native enable/disable, revoke and retained-entry
restore. None of these actions starts, installs or contacts a server.

Grok Build deployment remains unimplemented. **Preview for Grok Build** reads
`GROK_HOME|~/.grok/config.toml` through Rust discovery and the shared bounded target
reader. It checks central revisions and TOML containers, proposes masked
stdio/HTTP entries, rejects exact existing names and unusable Grok tool prefixes,
and reports the independent personal disable list. The strict IPC request accepts
only identity/revision; the client rejects unknown fields, mismatched identities,
invalid targets and unmasked env/header values. The bilingual read-only summary
opens separate details explicitly described as a JSON view of the proposed TOML
table, not a complete file or write approval. Errors offer retry or list reload;
closing/hiding invalidates late reads, and nested dialogs return focus. Relative
cwd and leading home expansion are explicitly refused until their Grok semantics
are verified. No OS credentials are loaded, files changed or servers started.
Task status is recorded only in
[`plans/todo.md`](plans/todo.md); the implementation decisions are in
[`plans/mcp-deployment.md`](plans/mcp-deployment.md).

## Evidence and supported fields

Field evidence was checked against [Pi MCP](https://pi.dev/docs/latest/mcp),
[Grok Build MCP](https://docs.x.ai/build/features/mcp-servers) and the
[MCP transports specification](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports).
Pi's installed 1.4.0 documentation, configuration parser and value resolver were
also inspected and exercised without starting a session or server.

- Stdio: one executable, ordered arguments, optional working directory and
  environment variables.
- Streamable HTTP: URL and request headers.
- OAuth, old SSE transport, dynamic secret commands, imports from other tools
  and protocol bridges are not supported.

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

## Pi review and safe application

Rust selects `PI_CODING_AGENT_DIR|~/.pi/agent` using Agent discovery's rules. IPC
accepts identity, central revision, action, optional retained-result identity and
approval; it never accepts a write path or native document from React. The shared
target reader bounds input. Invalid containers, symbolic-link targets, read-only
or unavailable targets and namespace collisions block mutation. A missing file
is not treated as an empty/invalid existing file.

The review shows the target, field summary and a separate before/after entry
view. All environment/header values are placeholders. Unrelated existing native
content is never returned. Preview does read native bytes that can contain
existing values, but does not read OS credentials or authorize a different plan.

Confirmation holds the cross-instance configuration lock while rebuilding the
approval, recording a durable intent, invoking the shared private writer and
committing ownership. Approval binds the complete target token, central revision,
action, selected history and entry generation. Revoked/aborted generations also
invalidate earlier approvals, even if the native file returns to identical bytes.
The existing writer provides concurrent-change checks, private pre-write backups,
atomic replacement, operation journals and independent complete result copies.

Only deploy/update loads current OS values in Rust. It rejects dynamic command
values and escapes every literal `$` as `$$`, because Pi 1.4.0 otherwise resolves
`$NAME`/`${NAME}` and interprets `$!`. Enable/disable preserves the owned native
values; revoke removes only the owned entry. Pi requires plaintext in its private
file, so backups and retained copies may also contain plaintext. These bytes
never cross IPC or enter application logs.

## Ownership, history and interruptions

Schema 31 adds root/definition entry ownership and durable change intents.
Ownership records the native name, central revision, complete entry digest,
enabled flag and last change identity. It is not whole-file ownership: other
servers and top-level fields remain user-owned and are preserved.

An existing foreign entry is never adopted. An externally modified owned entry
is never overwritten or removed. An owned entry that was removed externally can
be explicitly revoked; revoking a missing file releases metadata without
recreating it. Renaming a native identifier requires revoking its old registration
before deploying the new name.

History is paginated per definition and active Pi directory. Selecting a result
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
prevent central deletion after revoke.

## Deletion and registration states

The delete dialog reads Pi references across recorded directories before enabling
delete. It shows paths/states and links back to Pi management. Deletion repeats
its revision/reference check in an immediate transaction; any owned registration
(including disabled/missing/modified entries) or pending change returns
`mcp_in_use`. References in another Pi directory must be revoked in that directory.
There is no silent cascade into Agent files.

After all references are released, central deletion queues OS references in the
same transaction. Only a confirmed commit starts OS deletion. Uncertain commits
leave OS entries intact; cleanup remains visible/retryable after restart, even
with an empty library. One cleanup request handles at most 100 removed identities
and excludes active references. Failures preserve recoverable metadata.

The MCP list describes recorded Pi registrations, not connections. The Pi Agent
MCP slot is read-only, reports disk matches/external changes/missing/unreadable
entries and links to central management. Other Agents explicitly report that
native MCP deployment is unimplemented. Reads finish all root pages before a
partial result can imply that another definition is unregistered.

After writing, run `/reload` in Pi. Trusted project entries can override the
user-level registration; runtime flags or replacement `/mcp` extensions can bypass
it. Relative cwd is resolved against the Pi session directory, and leading `~/`
expansion follows Pi's runtime home. Written, enabled, effective and connected
are different claims; this UI reports only the first two and observed disk state.

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

Grok preview adds isolated default/override target, malformed/oversized file,
symlink, stale-revision and no-write tests, strict client response/masking tests,
and UI retry/reload, close/hide, language-preservation and focus behavior tests.
Chrome DevTools inspected its actual components with synthetic IPC data in both
locales/themes, 720×560 long-name/path cases, a 1080×760 list, nested Escape/focus
return and keyboard retry. This caught and fixed action groups squeezing the row
identity at the minimum size. These mocks do not prove actual Grok WebView IPC,
native deployment or connectivity; no Grok write path is exposed yet.

Browser fixtures separately inspected English/Chinese, light/dark, 720×560,
long names/paths, masked nested details, keyboard focus/Escape and interruption
review. Browser mocks do not establish native behavior; the isolated Tauri run
does not establish native Windows/Linux behavior. Local fixture discovery/disable
is verified; real third-party services, OS-credential deployment acceptance and
native Windows/Linux checks remain separate acceptance evidence.

Earlier central-definition checks include isolated Linux no-secret stdio
save/edit/restart and maintainer credential/native/appearance review; they must
not be mistaken for acceptance of the newly added Pi deployment path.
