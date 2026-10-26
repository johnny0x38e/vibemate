# MCP deployment implementation

Task status remains in [todo.md](todo.md), P32/P33. This document describes the
implementation order, not a second completion checklist.

## First slice: Pi read-only preflight

Use a revision-checked central definition and caller-supplied, bounded native
`mcp.json` bytes. Produce only the new, sanitized server entry, never existing
file content. Reject malformed native containers and exact or normalized
namespace collisions, including disabled or invalid existing entries. Without an
ownership ledger, no existing entry is safe to replace.

Preflight does not read OS credentials, access Agent files, execute processes,
connect servers, or authorize a later write. A missing file differs from an empty
or invalid file. Preserve native relative-cwd and home-expansion semantics in the
proposed metadata and report them rather than silently rewriting paths.

Verify both transports, disabled definitions, revision conflicts, malformed
containers, namespace collisions, output redaction and absence of state changes.

## Second slice: target discovery and review

`preview_pi_registration` resolves the same user-level root as Agent discovery
and uses the shared target reader. Strict IPC accepts only definition identity
and revision. The frontend validates exact response shapes and rejects unmasked
environment/header values. The central list opens a bilingual summary and a
separate new-entry details dialog. Close/hide invalidates pending results;
errors offer retry and list reload. The footer stays visible while long content
scrolls. No capability or CSP expansion is needed for this custom read command.

Runtime notices deliberately do not infer project trust, active CLI flags or
replacement extensions from the user-level file. Browser fixtures exercise the
rendered UI only; they do not prove native IPC or connectivity.

## Delivered Pi workflow

The maintainer requires the whole Pi flow in this implementation, not another
preview-only delivery. Grok remains P33, outside this change.

### Backend and persistence

Add a Pi MCP operation kind to the existing recorded private writer, not another
atomic-file implementation. A new migration records per-root/per-definition
ownership (native name, central revision and exact entry digest) and durable
change intents before any file write. Metadata contains no credential values.
Hold the shared cross-instance configuration lock across approval rebuilding,
intent recording, the shared writer and ownership finalization. Existing writer
entry points keep their own locking; narrowly scoped locked variants avoid
re-entering the OS file lock from orchestration.

Actions are deploy/update, enable, disable, revoke, and restore a retained entry.
Only deploy/update loads current OS credentials. Enable/disable preserve native
values; revoke removes only the owned entry. Refuse foreign or externally changed
owned entries. A revision/digest approval binds the entire current file, central
revision, ownership, the last retained change (including revoked/aborted
states), action and selected history. Do not evaluate dynamic values. Deployment
escapes each literal dollar as `$$` for Pi 1.4.0, preserving `$NAME`, `$$` and `$!`
without triggering environment lookups.

Use the shared writer's private backups, pre-write snapshots and complete result
copies. Retained history restores only the selected managed entry from its own
verified written copy, preserving current unowned entries/top-level fields.
Restoring never silently restores a whole old MCP file or edits other servers.
Warn that historical plaintext credentials can return. Keep root-scoped,
per-definition history paginated and make missing/damaged copies explicit errors.

Pending intents block normal changes to their root and central deletion. Offer
explicit recovery after restart: compare exact observed before/intended digests,
recheck a reviewed approval, repair the shared journal through its existing
resolution mechanism, and finalize or abandon metadata only when the observation
is unambiguous. External divergence stays blocked with instructions; never
silently replay a write. A committed ownership row or pending intent blocks
central deletion in the same deletion transaction. Completed audit history does
not prevent deletion after revoke.

Verify preservation, both transports, secret loading/redaction, permissions,
stale approvals, namespace/root isolation, retained-entry restore, central
revision changes, missing/corrupt backups and each pre/post-write crash window.

### Interface and IPC

Replace the preview-only dialog with Pi management: read deployment status,
choose an action, review its field summary and separate sanitized details, then
confirm. Keep loading/error/retry, pending-mutation dismissal guards, stale-read
protection and focus return. Expose retained history selection and explicit
interruption review/recovery. Add Chinese/English copy with each behavior.

The central list reports only observed written registrations, never inferred
connections. The Pi Agent MCP segment lists managed entries read-only and jumps
to MCP management. Native flags, project overrides and replacement extensions
remain effectiveness notices, not claimed runtime detection.

### Verification and handoff

Run focused behavior tests, full frontend checks, Rust fmt/Clippy/tests and the
locked desktop build. Inspect the actual interface in both languages/themes at
the minimum window size and exercise keyboard/error/long-content behavior. Use
isolated configuration and synthetic credentials for runtime checks, never real
Agent roots or credential exports. A harmless synthetic MCP discovery/disable
check is separate from real Provider or third-party server connectivity.
Record native runtime and cross-platform limitations rather than checking P32
acceptance boxes merely because code builds.

Each slice needs focused behavior tests and applicable repository checks. Native
Windows/Linux and external third-party services remain separate acceptance
evidence. After explicit user approval, local stdio/loopback HTTP discovery,
disable, selected-result restore and revoke passed real Pi CLI checks against
native UI writes; the recorded outcome is in todo.md.
No UI may claim deployment before the write path is delivered.

## Grok read-only preflight and preview (P33)

The first slice provides a pure Rust proposal against caller-supplied, bounded
user-level TOML bytes and a revision-checked central definition. The second slice
connects it to a Rust-selected target and a read-only UI, not a write approval or
deployed registration. Status remains in `todo.md`.

Map stdio `command/args/env/cwd/enabled` and HTTP `url/headers/enabled`
independently from Pi. Return only the new masked entry, never the existing file
or OS credential references. Accept absent/blank TOML as an empty document;
reject malformed UTF-8/TOML, non-table `mcp_servers`, malformed
`disabled_mcp_servers`, and existing exact names, including disabled or malformed
entries. Grok keeps hyphens and underscores in catalog names; do not import Pi's
hyphen/underscore collision rule. Reject digit-leading names, embedded `__` and
trailing underscores because Grok cannot admit their qualified tools.

Report a matching `disabled_mcp_servers` entry without removing it or claiming
that `enabled = true` makes the server active. Reject `${...}` in public stdio
fields instead of silently introducing runtime expansion. Until working-directory
and tilde behavior is verified, refuse relative/tilde cwd and leading `~/` in
commands/arguments; do not drop or silently resolve these inputs. Do not read OS
values, run commands/servers, contact endpoints or mutate central/native data.

Focused regression tests must cover both transports, masking, exact names versus
Pi normalization, malformed containers, stale/deleted definitions, dynamic/path
rejection and personal disable state. Run full Rust checks before handing off.
The delivered read-only preview resolves `GROK_HOME|~/.grok/config.toml` using Agent
discovery and the shared bounded target reader. Its strict IPC accepts only a
central identity/revision and returns target metadata plus the masked proposal.
A separate **Preview for Grok Build** action on central rows leaves Pi management
unchanged. The bilingual read-only dialog shows a field summary, personal-disable
notice and a nested entry-details dialog, with loading, safe errors, retry,
reload, focus return and stale-read protection when closed/hidden. Details are
explicitly a JSON representation of the proposed TOML table, not a full file.

Verify isolated-root/default/override/symlink/size and no-write Rust behavior,
strict response masking/identity/shape checks, and UI retry/close/hide/keyboard
behavior. Inspect both locales/themes and long content at 720×560. This command
needs no credential lock, process permission, capability or CSP expansion because
it only reads the Rust-selected target and central metadata.

The reviewed write/ownership/history/recovery/deletion workflow follows; do not
expose a management or connected state before those behaviors exist.

## Evidence and risks

Pi 1.4.0's shipped `docs/mcp.md`, `dist/extensions/mcp/config.js` and
`dist/core/mcp-servers.js`, together with the official repository documentation
retrieved through Context7, confirm `mcpServers`, optional `type`, `enabled`,
stdio `command/args/env/cwd`, HTTP `url/headers`, trusted-project precedence and
hyphen/underscore namespace collisions. The original preflight ran no server;
a later explicitly approved isolation check ran only fixed local fixtures.

Pi evaluates `$NAME`/`${NAME}` and whole-value `!command` in environment/header values;
vibemate's central validation rejects those syntaxes. Target credentials will be
plaintext in a private Agent file, not OS-store references. Preview must never
serialize them. Pi can expand a leading `~/` in executable, arguments and cwd;
relative cwd uses the session directory. `/reload` is required after external
changes. Third-party `/mcp` replacement extensions and runtime flags may bypass
the native file. OAuth and legacy SSE remain outside phase-one deployment.
