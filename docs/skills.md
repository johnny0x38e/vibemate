# Central Skill library

`src/features/skills/` and `src-tauri/src/skills/` implement the central Skill
library. Installing from a confirmed GitHub source, browsing/installing from the
skills.sh marketplace at Git `HEAD`, a lightweight Git revision check via
`ls-remote`, previewing and applying an update from a recorded Git or npx fetch
method, and deploying a library Skill into Claude Code, Pi or Grok Build
user-level `skills/` directories are implemented. See
[`plans/todo.md`](plans/todo.md) and the
[marketplace reference](plans/skills-marketplace-reference.md).

## Scope

- Import existing local Skill folders (a directory holding `SKILL.md`) into the
  central library.
- Install from a public GitHub repository at a pinned Git revision, through the
  reviewed `skills` npx installer, or from the skills.sh marketplace at `HEAD`.
- Store managed copies in the app data directory; original folders are never
  moved, modified or deleted.
- Record a stable ID, content digest, origin path or upstream source, revision,
  fetch method and the reviewed install command.
- Page the list with a bounded cursor; an address with no fetch method is shown
  as not auto-updatable.
- Preview an update for a skill that has a recorded Git or npx fetch method:
  re-acquire into a temporary workspace, compare digests and path lists, and name
  keep/replace strategies without writing the library or Agent targets.
- Deploy a library Skill into Claude Code, Pi or Grok Build as a symlink when
  possible, or as an explicit copy-sync; revoke only vibemate-owned targets.
- Delete a library entry with a bilingual confirmation; deletion refuses while
  any deployment row remains.

## Data model

Migration v10 created `skill`; v11 rebuilt it to widen the content limits; v27
added the fetch method and installer fields; v28–v29 added `skill_deployment`
for per-Agent ownership; v30 added revision-check columns; v31 added Claude Code
as a deployment target; v34 added `skill_deploy_operation` for unfinished
apply/disable recovery. Files live in
`<app-data>/skills/<id>/`. Managed copies move with the database, so a backup of
the app-data folder stays complete.

| Field                                     | Meaning                                                                   |
| ----------------------------------------- | ------------------------------------------------------------------------- |
| `id`                                      | 32 lowercase hex characters; also the managed directory name              |
| `name`                                    | Unique Skill name, 1–64 characters                                        |
| `description`                             | From the frontmatter, 1–1024 characters                                   |
| `origin_path`                             | Normalized absolute local import directory                                |
| `source_url` / `source_revision`          | Upstream source and pinned revision; empty means unknown                  |
| `fetch_method`                            | `git` or `npx`; empty means only an address is known                      |
| `installer_version` / `installer_command` | Reviewed npx version and exact command; empty for Git and unknown sources |
| `remote_revision` / `update_check_status` | Last `ls-remote` tip and outcome; empty until a Git revision check runs   |
| `last_checked_at` / `last_check_error`    | Check bookkeeping; error stores a safe code only                          |
| `content_digest`                          | SHA-256 over the ordered relative paths and contents                      |
| `file_count` / `byte_size`                | Managed copy size, for display and bounds                                 |
| `revision`                                | Metadata revision, incremented on source edits                            |
| `imported_at` / `updated_at`              | Unix milliseconds                                                         |

`origin_path IS NOT NULL OR source_url IS NOT NULL` guarantees that every record
has at least one explainable source. A digest is computed as `"vibemate-skill-v1\0"`
followed, for each file sorted by its `/`-joined relative path, by
`u64_be(path length) + path + u64_be(content length) + content`, so the same
directory hashes the same on every platform.

## Import

**Import locally** opens the system folder picker through Rust
(`tauri-plugin-dialog`, called from the blocking pool). The webview never calls
the plugin's own IPC commands, so no new capability and no `fs` permission is
granted.

1. Validate the path string (absolute, bounded, no control characters); the
   canonical path must be a directory outside the library directory.
2. Discover candidates: the selected folder itself when it holds `SKILL.md`,
   otherwise every direct child directory that does. Deeper levels are not
   searched.
3. Parse each `SKILL.md` YAML frontmatter for `name` and `description`; other keys
   are ignored. The frontmatter block is limited to 64 KiB, while the body may be
   long.
4. Walk the source with bounded traversal and build a file manifest; symbolic
   links and special files are refused.
5. Copy into `skills/.import-<random>/` while computing the digest and size. A
   source change during traversal fails the whole import.
6. Rename atomically to `skills/<id>/` and write the database row in one
   transaction (the name is unique).
7. If the database write fails, delete the managed copy; if that also fails,
   report `outcome_unknown` and require a reload.

One failing candidate does not stop the others: the command reports `imported`
and `skipped` (folder name plus a safe reason code). Cancelling the picker is not
an error.

Bounds: one path segment 1–128 bytes, total relative path ≤ 512 bytes, depth ≤ 16,
one file ≤ 16 MiB, total ≤ 64 MiB, at most 2048 files, at most 64 Skill folders
per selection. `.git` is skipped at every level. Import never executes anything
from a Skill and never reads files outside it.

## Marketplace installation

**Browse marketplace** loads skills.sh leaderboards (`all_time`, `trending`,
`hot`) or search results. Only public GitHub `owner/repo` rows are shown;
site-only hosts are filtered out. Choosing a row reviews the same Git
acquisition path as a hand-entered install, pinned to `HEAD` for the named
skill selector. Confirming runs Git in the private workspace and imports that
skill into the central library with the resolved commit as `source_revision`.
Catalog HTTP failures map to `marketplace_*` codes; a non-GitHub source refused
at confirm time is `marketplace_source_unsupported`. Deploy remains a separate
step on the Skill detail page.

## Remote installation

**Install from source** reviews one of two hand-entered acquisition paths. Both
run in a private temporary workspace under the system temporary directory; the
workspace is removed on every outcome. The reviewed command is shown before any
process starts. Nothing inside a Skill is executed.

### Git

1. Accept a public GitHub repository as `owner/repo` or an HTTPS `github.com`
   URL, plus a tag, branch or commit using ASCII letters, digits, `.`, `_`, `/`
   and `-`. Credentials, other hosts, extra path segments, query strings and
   fragments are refused.
2. Run Git with a cleared environment, a temporary `HOME`, no system or global
   Git configuration, no terminal prompt, no LFS smudge and no submodules.
3. Fetch exactly the requested revision with depth 1, check it out detached and
   resolve `HEAD` to a full commit. The resolved commit, not a moving branch
   name, is stored as `source_revision`.
4. Refuse a checkout that contains `.gitmodules`; a nested repository is a
   second source this install did not review.

### npx

The only allowlisted installer is `skills` at the reviewed version `1.7.4`.
A different exact version is refused rather than executed. The command is:

```text
npx --yes skills@1.7.4 add <github-url> --skill <name-or-*> --copy -g -a claude-code -y
```

It runs with a temporary `HOME`, `USERPROFILE` and npm cache, and with telemetry
disabled. The installed canonical copies are read from `<home>/.agents/skills`,
then `<home>/.claude/skills` as a fallback. The installer's stdout is never shown
or logged; only its exit status, the reviewed command and the imported rows are
reported. Node.js 22.20 or later is required by the installer.

### Shared result

- A missing `git` or `npx` executable reports `installer_unavailable`; a non-zero
  exit reports `installer_failed`; a timeout stops the process.
- Discovery is bounded to 64 candidates and six directory levels. Hidden folders
  and symbolic links below the acquired root are skipped, and a link inside a
  real Skill is refused by the normal manifest inspection.
- Each candidate goes through the same `SKILL.md` frontmatter, path, link, file
  count and size checks as a local import, then the same atomic copy and digest.
- Candidates with a duplicate name or invalid manifest appear in `skipped`; the
  others are imported. A failed acquisition leaves the library unchanged.
- Remote rows have no `origin_path`. They record the canonical source URL, the
  resolved Git commit or no revision for npx, `fetch_method`, the reviewed npx
  version when applicable, and the exact command.
- The source editor can add a fetch method to an existing local import. A method
  requires a GitHub address; Git requires a revision, npx has no Git revision.
  Clearing the address clears the method. A hand-entered address without a method
  remains metadata only and is not shown as auto-updatable.

Private repositories are not supported in this slice: the install path
deliberately forwards no Git credential helper or token.

## Source semantics

- A source with `fetch_method = git` shows its address, pinned revision and
  **Git**. A source with `fetch_method = npx` shows its address and
  **skills@1.7.4**. Both are the baseline P29 will follow.
- A hand-entered address without a fetch method is labelled **Manual source** and
  remains non-updatable. Recording an address alone must never imply an update
  path.
- A local-only import shows **Local import** / 「本地导入」.
- The library requires at least one of the two, so an entirely unknown source
  cannot be stored.

Auto-update is decided by whether an explicit fetch method and its fields are
recorded, not by who installed the Skill. A locally imported Skill can gain the
same Git or npx fields in its source editor; the update preview uses those fields.

## Revision check

**Check remote revision** is Git-only. It runs `git ls-remote <url> HEAD` with
the same cleared environment as a Git install, compares that tip to the recorded
`source_revision`, and stores `remote_revision`, `update_check_status`
(`up_to_date` / `update_available` / `error`), `last_checked_at` and a safe
`last_check_error` code when needed (schema v30). The skill metadata `revision`
is not bumped. npx-recorded skills cannot use this path. List rows show an
update-available hint when the stored status says so. Batch check and monorepo
subdirectory tree-id equality are not in this slice.

## Update preview and apply

**Preview content changes** on the Skill detail page only runs when
`fetch_method` is `git` or `npx` with the required fields. Git update
acquisition always fetches `HEAD` (the recorded pin remains the digest
baseline). npx re-runs the reviewed installer version. The preview acquires into
a private temporary workspace, finds the candidate matching the skill name, and
digests both the live managed copy and the candidate. The workspace is removed
before the result returns. Preview never writes the library or any Agent
`skills/` directory.

| Status             | Meaning                                                        |
| ------------------ | -------------------------------------------------------------- |
| `current`          | Live, recorded and candidate digests match                     |
| `local_modified`   | Managed copy differs; candidate matches the recorded digest    |
| `upstream_changed` | Managed copy matches recorded; candidate differs               |
| `diverged`         | Local edits and candidate both differ from the recorded digest |

When a change is present, the preview offers `keep` and `replace`. **Apply**
re-acquires and refuses when digests, status or the candidate revision no longer
match the reviewed tokens (`update_plan_stale`). `keep` writes nothing.
`replace` swaps the managed copy (staging + park + rename), advances
`source_revision` for Git, marks the revision check `up_to_date`, then refreshes
owned deployments: symlinks get ownership metadata only; owned copies are
rewritten. Per-target refresh failures are reported without rolling back the
library or touching foreign Agent content. Path lists name added, removed and
changed relative paths (bounded), and whether the description text differs.

## Deletion

The list row's delete action opens a bilingual confirmation naming the Skill and
stating that only vibemate's managed copy is removed while the import folder
stays.

1. Validate the ID and the `expected_revision` the user saw; a stale confirmation
   returns `revision_conflict` without touching files.
2. Rename `skills/<id>/` to `skills/.delete-<id>/` atomically (continue when the
   copy is already gone).
3. Delete the row in one transaction by revision; restore the rename on refusal,
   and report `outcome_unknown` if restoring fails.
4. Delete the parked copy; failure does not change the delete result but returns
   `cleanup_pending`, and the interface keeps a retry.

`cleanup_deleted_skill_copies` removes only parked copies with no matching record.
A parked copy whose record still exists comes from an interrupted delete and
stays until that record is deleted. Deleting a Skill that still has a
`skill_deployment` row or an unfinished `skill_deploy_operation` journal row
returns `skill_in_use`. The delete confirmation loads deployment rows and shows
the Agent, mode, revision and sync state; deletion stays disabled until the user
opens the detail page and revokes the deployments (and finishes or dismisses any
journal row). This is the Skill-specific reference view; MCP references remain
with P32/P33.

## Deploy to Claude Code, Pi and Grok Build

Deployment writes only each Agent's user-level skills directory, resolved the
same way as model configuration:

| Agent       | Root resolution                           | Target                        |
| ----------- | ----------------------------------------- | ----------------------------- |
| Claude Code | `CLAUDE_CONFIG_DIR`, else `~/.claude`     | `<root>/skills/<skill-name>/` |
| Pi          | `PI_CODING_AGENT_DIR`, else `~/.pi/agent` | `<root>/skills/<skill-name>/` |
| Grok Build  | `GROK_HOME`, else `~/.grok`               | `<root>/skills/<skill-name>/` |

All three Agents use the same `SKILL.md` directory form. Shared locations such
as `~/.agents/skills/` and project-local discovery paths are not written.
Claude Code's personal skills directory follows its configuration root; the
installed 2.1.308 binary resolves `<CLAUDE_CONFIG_DIR|~/.claude>/skills`, and
the official Skill contract allows a `<name>` entry to be a symbolic link.

1. **Preview** names the Agent, classifies the target as absent, vibemate-owned
   symlink, owned copy, or foreign; probes whether a directory symlink can be
   created under `skills/`; and suggests symlink when available, otherwise copy.
2. **Apply** requires the reviewed Agent, skill revision, config root, mode and
   target state. Symlink mode is refused when the probe fails. Foreign targets
   are never overwritten. After a successful filesystem write, schema v31 records
   ownership (`agent_id` is `pi`, `grokBuild` or `claudeCode`, plus mode, skill
   revision and content digest). A failed record attempt rolls back the
   filesystem write when possible, otherwise `outcome_unknown`. One skill may be
   deployed to each Agent independently; revoking one does not touch the others.
3. **Disable** removes only a still-owned symlink or copy. If the user replaced
   the target, the path is left alone and the ownership row is cleared.
4. **Recovery** journals each apply/disable as `prepared` before the first
   filesystem change and clears the row when ownership and the target agree. A
   write that cannot be rolled back is marked `unknown`. The Skills list shows
   unfinished rows with a live classification and Finish/Dismiss actions; Finish
   repairs ownership only and never replays a filesystem write. Design details
   live in [skill-recovery](plans/skill-recovery.md).

The Skill detail page owns the deploy controls and an Agent selector. The Agent
detail Skill segment for Claude Code, Pi and Grok Build is read-only: it lists
deployments for that agent directory and jumps to the Skills module.

## Interface

- Each list row shows the name, at most two description lines and the source
  state, including **Git**, **skills@1.7.4** or **Manual source**. File count,
  size, digest, paths, timestamps and the recorded install command stay in the
  detail page.
- **Install from source** opens a dialog with the method, repository or source,
  revision, and an optional Skill name. **Review command** validates the fields
  and shows the exact command; only the following **Install** action runs it.
  The result names imported and skipped folders, the resolved commit or installer
  version, and the exit status. Closing after a result returns focus to the
  header button.
- The browser preview invents no command or result; it reports that installation
  needs the desktop app.
- The library total sits above the list and comes from the paged response, so it
  does not depend on how many pages were loaded.
- The detail page shows the facts, the recorded command, and edits the source
  address, revision method and method-specific fields with the usual conflict and
  unknown-outcome handling. Opening **Detail** re-reads the Skill so that edits
  start from the latest revision.
- Digests stay out of the interface: they are a machine comparison value and a
  baseline for the future update comparison.
- The detail page includes a **Deploy to an agent** card: Agent and mode choice,
  target path and status, apply, current deployments and remove. Browser preview
  reports that deploy needs the desktop app.
- The Skills list shows an **Unfinished skill deployments** panel when journal
  rows remain: Finish repairs ownership, Dismiss clears the journal without
  rewriting Agent files, and the panel stays hidden when nothing is pending.

## Verification scope

Rust tests cover path rules, unknown sources, duplicate names, parse failures,
links and limits, the untouched source directory, deterministic digests, folder
discovery and batch reporting, large bodies and headers, the IPC contract, Git
and npx request validation, acquired-tree discovery, partial imports, workspace
cleanup, source-method edits, the v27 fields, and deploy preview/apply/disable
for Claude Code, Pi and Grok Build (symlink, copy, config-root override, foreign
conflict, stale tokens, cross-Agent isolation, delete-while-deployed,
interrupted-journal Finish/Dismiss and delete-while-journaled). Storage tests
cover the skill migrations, preserved deployment rows, the `claudeCode` target
value and the v34 `skill_deploy_operation` table.

The reviewed public source `vercel-labs/agent-skills` was installed through both
real paths: a pinned Git commit (`063bee9…`) resolved to the same commit in the
record, and the reviewed `skills@1.7.4` npx installer produced
`vercel-composition-patterns`. A real Git fetch with a missing commit reported
`installer_failed` and left the library empty. These checks run as ignored
network tests, not in CI.

A real collection check imported all 13 Skills each from
`~/.skills-manager/skills`, `~/.grok/skills` and `~/.agents/skills` with no
skips, including an 11 MiB skill with 315 files and an 87 KiB `SKILL.md`.
Maintainer acceptance on 2026-10-13 confirmed the native picker and the deletion
dialog passed. The P26 install dialog was exercised in the browser preview at
720×560 and 520×640 with method switching, browser-preview refusal, Escape and
focus return; native installer UI, maintainer visual acceptance and Windows/Linux
native behavior remain release-time checks.
