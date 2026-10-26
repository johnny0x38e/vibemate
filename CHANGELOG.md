# Changelog

User-visible changes are recorded here in English. Keep work in `Unreleased`
until a version is ready. Release headings use `## [X.Y.Z] - YYYY-MM-DD`.
GitHub release notes are extracted from the matching version section only.

## [Unreleased]

### Added

- Overview RecoveryHub: aggregates unfinished Agent configuration history
  (`requiresResolution`), Skill deploy journal rows, and Pi/Grok MCP
  interruptions into one list that only navigates to the owning module. Browser
  preview stays honest; a failed domain read does not hide the others. No write
  is replayed from Overview. Maintainer desktop acceptance remains separate.

- Skill deploy/disable interruption recovery (schema v34): apply and disable
  journal a `prepared` row before the first filesystem change, clear it when
  ownership and the target agree, and mark `unknown` only when rollback after a
  write cannot be proven. The Skills page lists unfinished rows with live
  classification (completed, not started, needs ownership record/clear, or
  conflict), offers Finish/Dismiss without replaying writes, and refuses
  central skill deletion while a journal row still names the skill.

- Grok Build MCP registration management from the central list: review and
  confirm deployment/update, native enable/disable, owned-entry revoke and
  retained-entry restore against user-level `config.toml`. Rust selects
  `GROK_HOME|~/.grok`, rechecks central/file digests, resolves OS values only for
  deploy/update (refusing `${…}` expansion), mutates only `mcp_servers.<name>`
  with `toml_edit`, and leaves model tables plus `disabled_mcp_servers`
  untouched. Exact-name ownership (no Pi hyphen/underscore collision) refuses
  foreign or externally modified entries; history restores only the selected
  entry copy. Durable interruptions require explicit observed-state
  reconciliation without replaying writes. Central deletion and Agent details
  include Grok references alongside Pi. Nine focused Rust lifecycle tests cover
  both transports, credential masking, name isolation, stale plans, restore
  preservation, recovery abandon and dynamic-secret refusal. Desktop Tauri
  preview IPC, native Grok connection and Windows/Linux acceptance remain
  separate; no server is started by vibemate.

- Reference-aware deletion for Providers, saved models and Skills. Provider and
  model dialogs list Pi provider bindings and Grok model aliases, classify them
  against the live Agent file, and refuse deletion while an entry is active or
  unreadable. A stale binding can be cleared explicitly; Claude Code remembered
  choices are reported and removed by the existing cascade. The Models tab
  lists the same references before saving removals. The Skill delete
  confirmation lists deployed Agents and disables deletion until those
  deployments are revoked. Agent files are only read, never rewritten.
- Complete Pi MCP configuration management from the central list: review and
  confirm deployment/update, native enable/disable, owned-entry revoke and
  retained-entry restore. Rust selects the user-level path, rechecks central and
  file revisions, resolves OS values only for deployment/update and uses the
  existing private backup/atomic-write journal. Per-root entry ownership refuses
  foreign/modified entries; history restores only the selected operation's own
  entry copy, preserving unrelated current content and warning about old
  credentials. Durable interruptions require explicit observed-state
  reconciliation without replaying file writes. Central deletion checks all Pi
  references, and Agent details show read-only disk registrations with a management
  link. Bilingual masked review/details, stale-read and duplicate-submit guards
  distinguish written/enabled from effective/connected. Nine isolated macOS
  Tauri WebView/IPC checks passed with temporary app/Pi data and no credential
  fields or server execution. After explicit approval, nine additional installed
  Pi 1.4.0 CLI checks verified real local stdio/loopback HTTP tool discovery,
  disable without reconnect, selected-result restore and revoke from native UI
  writes. No real configuration, Provider credentials, inference session or
  external service was used. OS-credential deployment and Windows/Linux
  acceptance remain separate.

- Apply a reviewed Skill update after digest preview. Apply re-checks digests
  and refuses a stale plan; `keep` writes nothing; `replace` swaps the managed
  copy, advances the Git source revision, marks the revision check up to date,
  and refreshes owned Agent deployments (symlink metadata or copy rewrite) with
  per-target outcomes. Partial deploy refresh failures leave the library change
  in place and do not touch foreign Agent content. Git update preview/apply
  fetches `HEAD`; the recorded pin remains the digest baseline.

- Lightweight Git revision check for Skills with a recorded fetch method. The
  check runs `git ls-remote` for remote `HEAD`, compares it to the recorded
  revision, and stores `up_to_date` / `update_available` / `error` without
  downloading the tree or bumping the skill metadata revision (schema v30).
  List rows hint when an update is available; npx sources stay on the digest
  preview path.

- Browse and install Skills from the public skills.sh marketplace. Leaderboards
  and search return GitHub-installable rows only; non-GitHub hosts are refused.
  Review shows the Git `HEAD` command for the selected skill before Install
  clones into an isolated workspace and copies the skill into the central
  library with the resolved commit recorded. Catalog failures use dedicated
  `marketplace_*` codes. Agent deploy remains a separate step.

- Preview a Skill update from a recorded Git or skills-installer source. The
  check re-acquires into a temporary workspace, compares the live library digest
  with the candidate, reports current / local-modified / upstream-changed /
  diverged status, path-level diffs and keep/replace strategies, then removes the
  workspace. Preview never writes the library or Agent targets. Skills without a
  fetch method stay non-updatable.

- Deploy a central Skill into Claude Code, Pi or Grok Build user-level `skills/`
  directories (`CLAUDE_CONFIG_DIR|~/.claude`, `PI_CODING_AGENT_DIR|~/.pi/agent`
  and `GROK_HOME|~/.grok`). Preview names the Agent, target path and whether it
  is empty, vibemate-owned or foreign; symlink is preferred when the platform
  can create one, otherwise the user must choose copy. Apply records ownership
  (schema v31, `claudeCode`, `pi` or `grokBuild`) with the skill revision and
  content digest; one skill may land on each Agent independently. Disable
  removes only a still-managed target and leaves user content alone. Deleting a
  Skill that is still deployed returns `skill_in_use`. The Skill detail page
  owns the deploy controls and Agent selector; Claude Code, Pi and Grok Agent
  Skill segments list deployments read-only and jump to the Skills module.
  Claude Code's personal skills directory follows its configuration root; the
  installed 2.1.308 binary resolves `<CLAUDE_CONFIG_DIR|~/.claude>/skills`.

- Grok Build writes a declared, non-empty reasoning-effort list and its model default to the native `reasoning_efforts` array when that field is absent. Each entry carries `value`, a generated English `label`, and `default = true` on the central default when that level is in the list. Existing arrays, deprecated effort fields, `[models]` globals, unrelated aliases and request parameters stay unchanged; open (`any`), empty and unsupported statements are not expanded. Isolated macOS Grok 1.0.67 checks confirm the injected menu, declared default and running-session selection for Chat Completions and Responses; six isolated local-stub samples confirm Chat's `reasoning_effort` and Responses' `reasoning.effort`, including the declared default. Both backends also send the changed effort on the next isolated TUI request. Grok persists its own selection globally and reuses it in new sessions; vibemate does not write that global setting. Real Provider acceptance remains unverified. Independent maximum-output and tool mappings remain unwritten.

- Pi injects every declared, verified extended thinking level independently of the saved default, so native switching does not require first selecting each level in vibemate. For official DeepSeek Chat bindings with the exact `deepseek-flash` or `deepseek-v4-pro` ID, `xhigh` maps to `high` and `max` to `max`. Official OpenRouter Chat bindings inject explicitly declared `xhigh` and `max` unchanged; the gateway receives them as nested `reasoning.effort`. Custom bases, other APIs and open (`any`) declarations receive no guessed extended mappings. Existing user mappings and null exclusions are preserved; other bindings receive no guessed mappings. Isolated macOS Pi 1.4.0 checks confirm the extended menu, runtime selection, saved per-model defaults and same-ID Provider isolation. Returning to a model reloads its saved thinking level rather than retaining a temporary selection; four installed-SDK payload samples confirm the new Chat mappings before network access, while real Provider acceptance remains unverified. No request-tuning editor or new Provider model fields are added. Grok previews identify context-capacity source/write state and map declared image capability to the native `vision` flag without displaying unknown native shapes; Grok effort-array injection follows the preservation rules above.

- Pi plans now list their model settings. For the context window, maximum output, reasoning, image input and the chosen thinking level, each row names where the central value came from (the model library, the provider catalog, your declaration, your choice in the form, or not stated) and what the plan does with it: write it, keep the value already in the file, or leave Pi's default. Rust builds each row beside the write it describes, the reviewed token and detailed preview compare the rows, and the interface only translates them. Pi model definitions have no tool-call field, so tool-call support is not written. Existing user request parameters are preserved without request-tuning controls.

- Saved Pi thinking levels per provider and model. The Pi model configuration offers declared base levels `off` through `high`, with a catalog `none` shown as `off`; the verified DeepSeek extension is described above. Preview, detailed text and apply carry the same reviewed choice and write one `modelThinkingLevels` entry in `settings.json`; other models, the global default and "Do not overwrite" leave their existing values untouched. Rust rechecks the choice against the merged model definition, so unmapped extended levels and any level excluded by a user's own map are refused with `thinking_level_unavailable` before a plan exists. All verified declared DeepSeek extended mappings may be added when missing, independently of the default choice. Deleting a model or provider does not yet remove its saved key.

- Reviewed Skill installation from public GitHub sources. A Git source is fetched at a pinned commit; the allowlisted `skills@1.7.4` npx installer runs in an isolated temporary home. The dialog shows the exact command before **Install** may run, and a result reports imported and skipped folders, the resolved commit or installer version, and the exit status. Acquired trees are bounded, hidden folders and links are skipped, and every imported Skill goes through the same manifest, path, size and digest checks as a local import. A failed acquisition leaves the library unchanged. Installed rows record the source URL, fetch method, revision and command; the source page can add a Git or npx method to a local import, while an address without a method stays marked as non-updatable. Schema v27 adds the new fields. Private repositories are not supported and no Git credential helper or token is forwarded.

- Remembered Claude Code effort starting choices per Provider instance and exact model ID in private SQLite. An explicit remember action is separate from preview/apply and never writes Agent files or accesses credentials. Reads include current model metadata; saves compare model and preference tokens, retain a new revision on clear, and cascade only the deleted binding's preference. The bilingual editor handles read/retry, save conflicts, obsolete choices and late responses. Native environment settings still have their existing cross-model scope; applying or restoring configuration does not silently change the remembered choice.

- Explicit Claude Code reasoning-effort selection for saved DeepSeek `deepseek-flash` models with declared reasoning support and an explicit effort list. The menu intersects that list with the native environment channel, including `max`, without selecting the model default or expanding `any`. “Do not overwrite” preserves native values; changing provider/model clears the draft. Plans, full-detail IPC, approval, application and version restore retain the chosen effort. The bilingual interface warns that the environment setting may affect other models and that a planned value does not prove a running session uses it. Independent per-binding preference storage is not included in this slice.

- Claude Code context-capacity mapping for saved DeepSeek `deepseek-flash` models without `[1m]`. Plans, detailed previews and application use the same stored model capacity and change token; other model IDs preserve the existing window and explain the missing mapping in English and Simplified Chinese. Compaction settings and model IDs remain untouched, and configuration versions retain the mapped capacity.

- Desktop foundation: a Tauri 2 shell with a private SQLite database in the app-data folder, OS credential storage for API keys and MCP values, English and Simplified Chinese resources with a startup language gate and a persisted preference, local Rust logging with Settings access to the log file and folder, an About panel with version, MIT license and a fixed repository action, and native CI checks plus a tag-triggered draft release workflow that extracts notes from this file.

- Agent model configuration for Claude Code, Pi and Grok Build. Each Agent detail page selects a saved Provider configuration and one of its added models, then asks Rust for a sanitized plan. Claude Code writes the user `settings.json` `env` block with the gateway base URL, the key placeholder, the session model and the four category keys that Claude Code resolves for its aliases, subagents and background work; only the vendor-documented DeepSeek and OpenRouter gateways are planned. Pi writes `models.json` and `settings.json`, binds the read-only `auth.json` precedence check, keeps readable per-instance Provider names and migrates recorded UUID entries through the reviewed backup path. Grok Build writes one `config.toml` entry, records alias ownership per configuration root, refuses an alias the user wrote by hand, leaves the user's `[models].default` untouched, and reports the higher-precedence files it located without reading them.

    Every plan names the target file and its change token, the Provider revision and the model content digest, and any unresolved protocol or parameter. Applying rebuilds the plan and compares every reviewed token, so a change made after the preview is refused with `plan_stale`. The key is read from OS credential storage only after those checks and lands as plain text in the Agent's own configuration, which the interface states. Unowned fields survive byte for byte, and a restore puts the original file back exactly.

- Saved configuration versions for Claude Code, Pi and Grok Build, with root-scoped seek pagination. Only original apply records appear; restore and compensation batches remain in the recovery journal. The actual current version is matched against complete written file digests and pinned first, even outside the requested page, with its Provider and live model metadata. External edits, unreadable files and partial matches are reported honestly. History IPC carries no configuration bytes, backup paths or recovery approval.

- Selecting a saved configuration restores the bytes its original apply wrote, not its pre-write backup. Schema v25 gives every version its own complete private copies, including unchanged Pi files; recording failure prevents Agent writes. Legacy result bytes are copied once into independent storage, interrupted conversion resumes after restart, and unavailable bytes are never guessed or borrowed during later selection. Interrupted writes retain a separate backend-approved inspection/resolution flow that records observations without changing Agent files or credentials.

- Two-layer previews for model configuration and history restore. The first layer lists each file's field paths with added, removed or replaced values, distinguishing absent from empty and masking credential-like keys. **View detailed configuration** opens a dialog with the complete sanitized JSON or TOML, read-only highlighting, file switching and a bounded line diff; TOML keeps comments and layout, an unparsable document is reported as unavailable rather than shown raw, and both original and sanitized text are limited to 1 MiB. Rust builds both layers from the same native plan that apply uses. The dialog focuses its title, supports keyboard dismissal and returns focus.

- Confirmed deletion of an independent configuration version and its own snapshots. Earliest, intermediate and previously selected versions are deletable without breaking other versions; completed hidden restore batches no longer block deletion. A durable journal keeps failed removal visible and retryable, rechecking active/unresolved work before every retry. Agent files, credentials, alias ownership and audit/recovery logs with their backups are retained, so deletion is not secure erasure. The bilingual dialog explains the scope and prevents duplicate submission.

- Private snapshots of every rewrite. Schema v20 records an independent per-file revision, including the absent state, before every apply, restore and compensation; a backup or pre-write journal failure prevents the mutation, and an interrupted result keeps its snapshot and blocks legacy replay until it is inspected and resolved. Unchanged content creates no revision, and retained copies cannot be overwritten by a reused ID.

- Private backup copies left without a journal reference after a pre-write failure can be inspected and explicitly cleaned up under the shared configuration lock and a SQLite write transaction. Cleanup refuses changed approvals and never recursively deletes unknown layouts or follows symlinks, failures stay discoverable after restart, and the operation has no interface entry and never runs automatically.

- Optional automatic Agent CLI version reading. A settings toggle grants one-time informed consent to run the fixed `--version` command without a per-run click; found Agents are probed sequentially after each scan and when opening a detail page without a cached result, at most once per path fingerprint per session. Results stay in window memory, never persist, and the manual **Read version** button remains available.

- Overview home shows five configuration totals: providers, models added to the library, discovered Agents, central skills and central MCP definitions. Each card has a loading, failure-with-retry and browser-preview state, zero is a valid number, and clicking opens the matching module. Skills and MCP copy names the central library only and does not claim Agent injection.

- Central MCP definitions and the Skill library. MCP stores stdio and Streamable HTTP metadata with every environment variable and header value in OS credential storage, retries obsolete credential cleanup after a restart, and deletes a definition with a revision check, queued credentials and a retryable cleanup. Saving never starts or contacts a server and never writes Agent configuration. Skills import existing local folders through the system picker, keep the source untouched, record a content digest and an optional upstream address, page the list and delete a library entry with a revision check and a retryable copy cleanup. No Skill content is executed.

- Provider models, capability evidence and parameters. Models are fetched into an in-memory browse session, saved as an explicit added list, refreshed without inserting or deleting rows, and edited inline with a SHA-256 content digest. Thinking, image input and tool calls each record catalog evidence or a user declaration with its source, an icon is drawn only for explicit support, and the model dialog stores the context window, maximum output and declared thinking efforts. Providers can be deleted with a credential compensation path that restores the key on a known failure.

### Changed

- Documented the first P24.a model-parameter evidence matrix for installed Claude Code 2.1.304, Pi 1.3.0 and Grok Build 1.0.62, distinguishing existing metadata writes, native request channels, deprecated Grok effort fields and unresolved protocol checks. Parameter editing is not yet delivered.

- Agent history details now open in a dialog instead of expanding the list. Multi-file versions use the same compact file switcher as detailed configuration; selecting a file changes only the preview, while restoration still applies the whole version.

- Shortened English and Simplified Chinese global notifications for Agent writes, model refreshes, provider conflicts and pending MCP/Skill cleanup. Notifications retain uncertain outcomes and recovery actions; detailed Agent write results remain on the page.

- Successful configuration-version restores show a concise app notification instead of a result card. Partial or uncertain results retain per-file details with the shared follow-on card spacing.

- Agent history previews use consistent plain-text hints and nest the selected file's differences and notices beneath its path. Field comparisons show old → new values without redundant added/replaced/removed labels in history and model previews.

- The Agent detail current-configuration path label now uses plain secondary text instead of an outlined badge; history rows retain their current-version badges.

- Configuration-history detail dialogs focus on field differences and **Use this configuration**, with one concise API-key/session warning. Repeated backup descriptions, timestamps, internal Provider IDs and restoration explanations are removed. Interrupted-write errors and inspection remain available.

- Grok Build's `[models].default` is now user- and Grok-owned. A model write adds or updates only its `[model.<alias>]` entry, so the applied model becomes selectable without silently changing which model new sessions use. The Agent list reads the model each Agent currently names from its live configuration: one that resolves to a still-saved vibemate Provider/model shows `Provider · Model`, any other readable configuration shows **Own configuration**, and an undiscovered or unreadable Agent keeps **No configuration summary**. Pi's `/model` switch is session-scoped, so its row reads the newest session JSONL tail and extracts only the provider/model metadata (message content is discarded), falling back to `settings.json` when no session exists. Grok's current `[models].default` is used only for this read, never written.

- Agent list rows now label a discovered Agent with no vibemate-written Provider/model pair as **Own configuration**; an undiscovered Agent keeps **No configuration summary**. The **Detailed configuration** dialog reads the live file through a new Agent-scoped, sanitized read command, so an external edit no longer disables it; the taller dialog shows only the current text, without the current-configuration heading or original-size row, and its file switcher uses compact buttons.

- Agent history details use the shared bordered icon button in the right-hand action group, before Delete; the left-hand disclosure and duplicate legacy preview action are removed.

- Documentation is now split into current reference pages and remaining plans. `docs/README.md` indexes `agents.md`, `architecture.md`, `configuration.md`, `frontend.md`, `logging.md`, `mcp.md`, `models.md`, `releases.md` and `skills.md` as behavior references, while `docs/plans/` keeps only `phase-1.md`, `todo.md`, `model-parameters.md` and `backend-modularity.md`. Delivered design notes and their process records were removed; `docs/plans/todo.md` remains the only task-status source and Git history holds the earlier records.

- The superseded latest-ten history listing commands (`claude_code_operations`, `pi_operations`, `grok_operations`), their domain functions and types, and the TypeScript `listAgentOperations` wrapper were removed. The root-scoped paginated history is the only history read. No user-visible behavior changed.

- Agent module interface: list rows expose **Write** and **Detail** icon buttons, and clicking elsewhere in the row no longer opens the detail page. **Write** opens a titlebar-safe dialog that reuses the model configuration preview and renders the field summary and detail entry inline after generating; a successful apply notifies, closes the dialog and refreshes the list's current-configuration pin. The status dot and version form a fixed left-aligned rail. The detail page shows the current configuration above History, Skill, MCP and Install & paths segments, with History as the default and explicit empty states for the Skill and MCP slots; list cards reserve summary slots with dash empty states instead of inventing zeros.

- Agent discovery covers the fixed catalog of Claude Code, Codex, OpenCode, Pi, Grok Build and Hermes with bounded automatic lookup, independent window-local manual paths and strict IPC. Status and version share one row with a decorative dot that supplements accessible text, a failed refresh labels retained observations as previous results, and every entry stays visible while loading, in browser preview and after a failure. Officially sourced marks replaced guessed artwork, and the Codex entry uses OpenAI's official Blossom rather than a separately verified Codex logo.

- Reading an Agent CLI version runs the fixed `--version` command on the first click after the risk disclosure, with a single-probe gate, a restricted environment, bounded output and a five-second direct-child deadline; Windows accepts native `.exe` entries only.

- Settings, themes and navigation. The desktop shell keeps a fixed 200 px expanded and 88 px collapsed sidebar, an overlay title bar on macOS with native traffic lights, in-app window controls on Windows and Linux, and eight light/dark themes with independent brightness. Sidebar activation always returns to the module home while preserving mounted state, and secondary pages share one left-aligned header with an icon-only Back control and trailing Save.

### Fixed

- Masked existing credential values in Claude Code plan changes before they cross IPC, including the old direct API key that OpenRouter clears. Repeated previews no longer carry the old key alongside the new credential placeholder.

- Configuration selection now moves the current badge to the original saved version without adding a visible restore row. Matching uses complete successful written results rather than pre-write snapshots, and Provider/model attribution no longer depends on the selected page.

- Two history regressions now compare Windows paths by components and build test paths with the platform-aware helper instead of assuming POSIX separators. Native Windows verification remains a CI check.

- Icon-button tooltips no longer stay on screen after their action replaces the page, such as opening an Agent detail page and returning. A press closes the hint explicitly, and its portal is mounted only while open, so a detached tooltip cannot outlive its trigger.

- Agent model display names now follow the status line the Agent prints for a model. Grok Build's TOML `name` and Pi's model `name` read as `(vibemate-<Provider>) <model ID>` (for example `(vibemate-Command-Code-GOAT) deepseek/deepseek-v4.1-flash`): the parentheses mark vibemate and the Provider as a hyphenated token, a space follows, and the model ID stays unchanged with no user alias or upstream label. Slashes, capitals and dots are kept; whitespace collapses to one space, invisible format characters (including bidi overrides) are removed, and the whole name is limited to 160 characters with `…` for an overlong ID. A reasoning level is a separate request setting and will be appended only when an adapter writes one. The model ID remains the identity, and Grok's table alias keeps its stable `vibemate-…` form. Re-applying an owned entry updates its name to this format. The Agent list in vibemate itself shows the current active model as plain secondary text reading `Provider · Model` when it resolves to a saved pair, otherwise **Own configuration**; rows have no hover state because only the Write and Detail buttons are interactive.

- Grok Build owned entries always set TOML `name` to `vibemate-<slug(model_id)>`, and a plan whose API model id exactly matches an existing `[model.<alias>]` table name is refused as a target conflict, so a handwritten alias is never taken over or collided with.

- Version-restore previews keep the target digest when a present file already matches its retained version, so the strict boundary accepts the skipped file. Two absent file states no longer invent a root-level change, and an external scalar JSON root edit uses an explicit `$` field path.

- A partly rolled back configuration write can be finished: the journal records what a rollback did to each applied step and whether the file already existed before, so a retry skips restored steps and never deletes a settings file that was already there. A row written before these columns existed is reported as blocked rather than guessed at.

- Pi preview accepts a compatible installation after upgrading from 1.2.0 to 1.3.0 by checking the selected package's published schemas, and schema v18 records current and historical alias ownership so renames, duplicate labels and restores cannot adopt another account's entry.
