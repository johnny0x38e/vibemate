# Configuration writes, history and restore

This page describes the shared mechanism every Agent write uses: Claude Code,
Pi, Grok Build and Codex today, and any later adapter. Protocol details per Agent live
in [`integrations/`](integrations); the Agent catalog is in
[`agents.md`](agents.md).

## Safety rules

A write may only touch a file when it can prove the change is still the one the
user reviewed.

- A preview binds the target file digests, the Provider revision, the model
  content digest and any adapter-specific token such as a Grok alias. Any change
  makes the plan stale; the interface has to preview again.
- The last path component is never followed when it is a symbolic link, so the
  atomic replacement cannot land somewhere else.
- File permissions are preserved. A pre-existing target keeps its mode; a new
  target is created as 0600 because it may hold a credential.
- A read-only target is refused even when the directory would allow `rename`.
- Errors and logs carry stable codes and paths, never file contents or values.
- Every private copy lives in a 0700 directory with 0600 files. The interface
  receives a locatable path and a digest, never bytes.

The flow is: discover the installation, resolve precedence, read current state,
validate the supported options, build a sanitized plan, recheck the file state,
write a private backup, write a temporary file in the same directory and rename
it, verify, and allow restore.

## Preview presentation

Model configuration previews expose two layers:

1. **Field summary.** Per file, the field paths with added, removed, replaced or
   container changes, and sanitized old/new values. Absent, unset, empty string
   and `null` are distinct. A file that is created or deleted is stated as such.
2. **Complete text.** On request, a dialog shows the full native JSON or TOML for
   each file, read-only and highlighted, with a bounded line diff and file
   switching. The model preview compares the current file with the planned
   result. The history panel uses the same field-summary data, comparing current
   bytes with the selected configuration's written result.

History rows open a detail dialog with field differences, necessary failure
messages, one key/session warning and **Use this configuration**. For a
multi-file version, compact file buttons select one file's differences at a
time; confirmation still restores the entire version, not only the selected
file. The dialog supports Escape and returns focus to its trigger; an active
restore blocks dismissal. It does not repeat backup details, internal Provider
references or the row's timestamp. The live
configuration's separate detail action still reads sanitized current text.

Rust builds previews from the same native plan that apply will use, so the
frontend never assembles a configuration by itself. Text is masked for
credential-like keys across the whole document, not only the keys vibemate owns.
An unparsable document is reported as unavailable instead of being shown raw.
Original and sanitized text are each limited to 1 MiB; JSON indentation growth
that would exceed the limit is refused rather than truncated. TOML keeps the
user's comments and layout through `toml_edit`.

The detail dialog focuses its title first, traps focus, closes on Escape and
returns focus to the button that opened it. Loading, failure and retry are
explicit.

Applying stays blocked until the field summary has loaded. A stale detail read or
a stale restore is refused with `plan_stale` and writes nothing.

## Codex writes

Codex model settings (`OperationKind::CodexSettings`) and Codex MCP entries
(`OperationKind::CodexMcp`) both write the user-level
`$CODEX_HOME/config.toml` through the same private writer: approval digest,
`plan_stale` on any change, `.config-lock`, private backup, journal, atomic
rename. Unlike the mode-preserving rule above, these private writes set an
existing `config.toml` to 0600 as well, because it can hold plain-text keys.
Each keeps its own durable change table, and a
pending change in either one blocks writes from the other to the same root.
Codex is not part of `configuration_history_page` or whole-version restore:
the Codex settings panel shows a read-only, newest-first list of completed
changes, MCP history restores only the selected entry, and interrupted changes
are resolved by an explicit review that never replays a write. Field details
are in [`integrations/codex.md`](integrations/codex.md).

## Write journal

`schema v16–v25` record recoverable writes and independent configuration versions.

- Before any target is replaced, an operation row and its per-file steps are
  written in a transaction. A multi-file write cannot be a filesystem
  transaction, so a failure compensates the steps already applied and records
  `compensated`, `partial` or `unknown`.
- Schema v20 adds `config_file_revision`: an independent snapshot of each file's
  bytes or absence, written before every actual rewrite, including restores and
  compensation. Snapshots for successful writes are never overwritten or pruned
  automatically.
- Schema v21 records an explicit resolution of an interrupted write with
  `config_revision_resolution`. A resolution records an observation; it never
  restores a file.
- Schema v22 marks direct version restores (`direct = 1`). They do not edit the
  source operation's legacy rollback state and are not mistaken for a missing
  step result.
- Schema v23 adds a durable delete journal (`prepared` / `failed` / `completed`)
  and exclusive backup-target rows, so a delete can be retried without hiding a
  history entry while copies that may hold secrets still exist.
- Schema v25 adds `config_version` and `config_version_file`. Each apply owns a
  complete result copy for every planned file, including unchanged files. These
  copies must persist before the first target write and become available only
  after a successful apply. They are separate from pre-write recovery backups.

Apply, manual restore, compensation and explicit resolution take a permanent
exclusive `.config-lock` file next to the database before the SQLite lock, using
the standard library `File::lock` (flock on macOS/Linux, `LockFileEx` on Windows).
This stops a second instance from treating an in-progress `prepared` record as an
interrupted one. It does not lock an external Agent or editor and does not
replace the digest checks.

## History

Each supported Agent's detail page reads a root-scoped, paginated journal through
`configuration_history_page`. The request contains only the Agent identity, a
seek cursor and a page size; Rust resolves the configuration root from the same
environment precedence as discovery, so history stays readable after an
installation or directory is removed.

- Chronological entries contain original apply records only. Restore and
  compensation batches remain in the full internal audit journal, not as extra
  saved configurations in the interface.
- Every page returns the live `current` configuration and the separately loaded
  `currentEntry`. A successful original apply is matched by every scoped file's
  written digest, never by its pre-write backup. The matching version is pinned
  once before the chronological rows, even outside the requested seek page.
- Current Provider/model metadata comes from that matched apply and live model
  fields, not from whichever page was requested. External edits, unreadable
  files and partial multi-file matches are reported as `unrecorded`,
  `unreadable` or `partial` rather than mislabeled as the newest operation.
- Backup metadata is retained for recovery checks; a backup reference alone
  proves neither the selected written version's availability nor permission to
  restore it. Expanded rows use the version preview instead of backup prose.
- No history response contains configuration bytes, backup paths or recovery
  approval. Interrupted writes retain `requiresResolution` and an inspection
  action; they cannot be selected until the interruption is resolved.

The frontend boundary (`src/lib/desktop/history.ts`) validates the nested
response with strict Zod schemas: unknown fields, contradictory journal or
reconciliation metadata, invalid ordering, duplicate identities and mismatched
cursors reject the whole page with a safe error code. Validation diagnostics and
raw responses are discarded.

### Inspecting an interrupted write

`review_configuration_resolution` accepts only a fixed Agent identity and an
operation ID. It compares the journal with the files and private copies as they
are now, per file: matching the pre-write state, matching the intended state, both,
externally changed, or unreadable, plus backup integrity.

`resolve_configuration_operation` accepts the Agent, the operation ID and the
approval digest of the complete reviewed plan. It rechecks the approved set inside
one transaction with the shared lock and records the observation; ambiguous,
changed or unreadable observations stay blocked. Recording never restores a file,
never reads credentials and never changes configuration.

### Restoring a retained version

A row selects the bytes that its original apply wrote. After A → B → C,
selecting A restores A itself, not the state before A and not B's recovery
backup. Each version owns full independent copies, including Pi's unchanged
second file. Selecting one never consumes its copies or borrows another row's.

- The scope is exact: the Agent's root and owned files. Symlinks, unreadable or
  oversized targets, read-only targets, damaged copies and unresolved writes
  block selection. A missing live file can be recreated from its saved copy.
- Confirmation rebuilds the preview under the shared mutation lock and refuses
  changed live digests, copies or journal state with `plan_stale`.
- Before a restore writes, it saves the current bytes in a separate recovery
  batch. That batch remains in the audit journal and does not create a new
  visible configuration version. The current badge moves to the original row.
- Restoring can put an old plain-text API key back into an Agent file. The
  interface warns before confirmation; OS credentials are unchanged. A new
  Agent session may be needed for the saved files to take effect.

For pre-v25 data, a one-time conversion copies exact, digest-verified result
bytes from live files or old journal snapshots into version-owned storage.
Interrupted copying resumes after restart. Missing bytes make a version
explicitly unavailable; no configuration is guessed from a model name or a
chain of differences. The old pre-entry APIs remain internal recovery tools.

### Deleting a saved configuration

Deletion removes that version's own full copies, then marks it deleted. It does
not remove the original audit operation or its recovery backups. The bilingual
confirmation explains that scope, the retained data and irreversibility.

- Earliest, intermediate and previously restored versions are independently
  deletable. Completed hidden restore batches do not create permanent deletion
  dependencies between versions.
- Unfinished writes (`history_active`) and interrupted revisions
  (`history_unresolved`) remain protected. A failed delete rechecks them before
  touching files when retried.
- File deletion and SQLite cannot be one transaction. The durable journal and
  idempotent retry keep incomplete deletion visible until its copies are gone.
  Tombstoned versions do not reappear after migration or restart.
- Deletion never writes Agent configuration, changes credentials or drops alias
  ownership. Deleting the current version leaves its bytes in place; the live
  marker becomes unrecorded unless another successful retained version matches.
- Deletion is not secure erasure: audit/recovery logs and their backups remain.
  Internal audit-record deletion retains its separate reference protections.

## Module layout

| Path                                  | Responsibility                                                              |
| ------------------------------------- | --------------------------------------------------------------------------- |
| `src-tauri/src/config_plan.rs`        | Target file identity and digest, plan changes, notices and refusal reasons. |
| `src-tauri/src/config_apply.rs`       | Facade for the write facade and its submodules.                             |
| `config_apply/operations.rs`          | Journal rows, step states, listing and unfinished operations.               |
| `config_apply/service.rs`             | The write sequence: checks, backup, atomic replacement and compensation.    |
| `config_apply/versions.rs`            | Complete written-version copies and legacy conversion.                      |
| `config_apply/revisions.rs`           | Independent pre-write snapshots.                                            |
| `config_apply/history.rs`             | Root-scoped paginated reads and the current-configuration pin.              |
| `config_apply/version_restore.rs`     | Written-version selection and internal pre-entry recovery.                  |
| `config_apply/version_text.rs`        | Field summaries, sanitized complete text and credential masking.            |
| `config_apply/history_delete.rs`      | Durable deletion state and cleanup.                                         |
| `config_apply/cleanup.rs`             | Explicit cleanup of unreferenced private copies (backend only).             |
| `config_apply/inspection.rs`          | Read-only inspection and explicit resolution of interrupted writes.         |
| `agents/history.rs`                   | Adapter-scoped history IPC, Provider labels and the Agent file lists.       |
| `src/features/agents/RestoreView.tsx` | History UI, version restore, resolution review and deletion.                |

## Verification scope

The mechanism is covered by fault-injection tests for concurrent changes,
symbolic links, read-only targets and directories, failed replacement, partial
multi-file writes, backup and journal failures, restart, stale approvals and
deletion refusals, using temporary directories and real SQLite.

The latest isolated macOS Tauri run passed 23 assertions for independent
configuration selection and deletion: A/B/C switching, an off-page current pin,
deleting intermediate and previously selected versions, and deleting the current
version without changing its Agent file bytes. It used a separate app identifier,
temporary Claude Code roots and a synthetic OS credential that was removed after
the run. All three Agents are covered by the independent-version Rust tests.

An earlier isolated macOS Tauri run with synthetic OS credentials verified all three
Agents' A→B→C→A→C restore chain, stale reads and writes, independent snapshots
and exact-byte restoration, including Pi's absent-file state, plus a browser
review of both languages, keyboard and focus, retry, JSON/TOML and a 720×560
content viewport. This is agent-run verification. Maintainer sign-off for the
History interface's later additions and Windows/Linux native behavior remain
tracked in [`plans/todo.md`](plans/todo.md).
