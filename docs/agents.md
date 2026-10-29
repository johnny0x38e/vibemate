# Agent catalog, discovery and configuration support

This page is the current reference for how vibemate knows about Agents. Third-party
protocol and file contracts live in [`integrations/`](integrations).

## Fixed catalog

The catalog is fixed and typed in Rust; React only renders verified results. The
order never depends on a successful scan.

| Agent       | CLI        | Configuration directory                             | Files vibemate observes (existence only)                  | Write adapter |
| ----------- | ---------- | --------------------------------------------------- | --------------------------------------------------------- | ------------- |
| Claude Code | `claude`   | `~/.claude`, `CLAUDE_CONFIG_DIR`                    | `settings.json`                                           | Yes (first)   |
| Codex       | `codex`    | `~/.codex`, `CODEX_HOME`                            | `config.toml`, `auth.json`                                | Yes (fourth)  |
| OpenCode    | `opencode` | `OPENCODE_CONFIG_DIR`, `XDG_CONFIG_HOME/opencode`   | `opencode.json`, `opencode.jsonc`                         | No            |
| Pi          | `pi`       | `~/.pi/agent`, `PI_CODING_AGENT_DIR`                | `settings.json`, `models.json`, `mcp.json`                | Yes (second)  |
| Grok Build  | `grok`     | `~/.grok`, `GROK_HOME`                              | `config.toml`, `managed_config.toml`, `requirements.toml` | Yes (third)   |
| Hermes      | `hermes`   | `~/.hermes` or `LOCALAPPDATA/hermes`, `HERMES_HOME` | `config.yaml`, `.env`, `auth.json`                        | No            |

Listing an Agent in the catalog never means its Provider, Skill or MCP
configuration is supported. Only the four write adapters have preview, apply
and history; Claude Code, Pi and Grok Build also restore whole retained
versions. A planned adapter must never appear connected or supported in the
interface. The adapter order is Claude Code, then Pi, then Grok Build, then
Codex; the maintainer edits code in Pi daily, so Pi is never the first write
target.

Known entry locations outside `PATH`:

| Agent       | Additional entry candidates                                               |
| ----------- | ------------------------------------------------------------------------- |
| Claude Code | POSIX `~/.local/bin/claude`; on Windows only `PATH` or a manual path      |
| Codex       | `PATH` or a manual path only; no guessed npm/Homebrew prefix              |
| OpenCode    | POSIX `~/.opencode/bin/opencode`; on Windows only `PATH` or a manual path |
| Pi          | `~/.local/bin/pi`, `<agent-dir>/bin/pi`                                   |
| Grok Build  | `~/.local/bin/grok`                                                       |
| Hermes      | POSIX `~/.local/bin/hermes`; Windows `LOCALAPPDATA/hermes/bin`            |

## Discovery boundaries

Automatic discovery checks at most the first 64 absolute directories in the
desktop process `PATH`, the verified fixed user entries above and any path the
user typed for this window. It never scans the whole disk, never treats the
current project or a relative `PATH` entry as a fallback, and never runs a shell,
WSL, container or remote host to find an Agent.

It checks only the type and existence of the fixed files. It does not read
configuration, authentication or secret contents, and an abandoned
configuration does not prove an installation exists. The only content read is
bounded Pi package metadata whose package name and `bin` target match the
resolved entry.

A manual path wins over automatic discovery and stays in window memory only; it
is never written to SQLite. An invalid explicit directory override does not
silently fall back to the default. An empty `CODEX_HOME` uses the default path; a
non-empty override must already exist. OpenCode's directory override is treated
as a candidate because the official versions disagree about its meaning.

Project, profile, managed, CLI and session overrides, Claude's `~/.claude.json`
and Hermes' desktop-package paths are outside this check. The desktop process
environment can differ from a terminal.

Discovery runs on the blocking pool and needs no storage or credential access.
It starts no process, writes no configuration, makes no network request and adds
no capability permission.

## Versions and evidence

Automatic discovery never runs the CLI. A version is reported only from bounded
metadata tied to the resolved entry (for example Pi's matching `package.json`) or
from a verified filename hint (Grok Build's versioned download, still unverified
as a runtime version). Anything else stays unknown. Unknown does not mean
missing or unsupported.

Presence, version provenance, documented capabilities, user configuration roots
and effective settings are separate claims. Discovering an Agent never
establishes that its configuration can be deployed.

### Explicit version probing

The Agent detail page offers **Read version**. It first shows the entry path, the
resolved target, the fixed command and the risks; Rust starts the CLI only after
a second confirmation. The browser preview never executes anything.

The settings toggle **Read Agent versions automatically** is one-time informed
consent: after that, a scan and opening a detail page run the fixed `--version`
without a per-run click. Automatic failures are silent and the manual button
stays available.

| Agent       | Fixed command | Evidence                                                                                                                   | Accepted output                          |
| ----------- | ------------- | -------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------- |
| Claude Code | `--version`   | [CLI reference](https://code.claude.com/docs/en/cli-reference)                                                             | Version, optional `(Claude Code)` suffix |
| Codex       | `--version`   | [official CLI parser](https://github.com/openai/codex/blob/main/codex-rs/cli/src/main.rs)                                  | `codex-cli` / `codex` version line       |
| OpenCode    | `--version`   | [CLI](https://opencode.ai/docs/cli/)                                                                                       | A bare version number                    |
| Pi          | `--version`   | [CLI](https://pi.dev/docs/latest/cli)                                                                                      | A bare version number                    |
| Grok Build  | `--version`   | [official README](https://github.com/xai-org/grok-build)                                                                   | Version, optional commit in parentheses  |
| Hermes      | `--version`   | [official fast-version implementation](https://github.com/NousResearch/hermes-agent/blob/main/hermes_cli/_startup_fast.py) | `Hermes Agent v...` first line           |

Execution boundary:

- IPC accepts only a fixed Agent identity plus the confirmed entry and resolved
  target. The path is rechecked; a changed target requires a new discovery. This
  check does not remove the race between checking and executing a file.
- The absolute resolved target runs directly with the fixed argument. No shell
  or string concatenation is used. On Windows only `.exe` runs; `.cmd`, `.bat` and
  other wrappers are refused.
- stdin is closed; stdout and stderr are limited to 8 KiB each; the run times out
  after 5 seconds and the direct child is terminated and reaped. The whole app
  allows one probe at a time.
- The inherited environment is cleared except the platform, language and
  configuration-path variables the launcher needs. API keys and tokens are not
  passed. A temporary working directory is used instead of the project.
- Only a safe version marker and a fixed evidence category leave Rust. Raw
  output is never displayed, logged or persisted. Probing does not upgrade
  capability evidence or change configuration.
- This is an execution guard, not a sandbox. The program can still read user
  files and credentials, use the network, write files or start descendants. Only
  the direct child is terminated; its effects are not undone.

Frequency: one probe per `(id, entryPath, resolvedPath)` per session. A scan
probes found entries sequentially through the single-probe gate; opening a detail
probes only when no cached result matches. **Read again** re-runs one item, and a
new scan clears the cache. There is no polling.

## Agent pages

The list shows all six entries while loading, in browser preview and after a
failure; entries are never generated from a successful scan. Status and version
share one row, with a decorative dot that supplements accessible text: green for
found, gray for not found, red for a failed check or unusable entry. A failed
refresh keeps the previous observation but labels it as a previous result.
Unresolved Agents open their detail page and accept a manual path.

The Agents list row exposes two icon buttons: **Write** opens a titlebar-safe
dialog with the model configuration preview, and **Detail** opens the detail page.
Clicking elsewhere in the row does nothing. The status rail keeps its width so
dots stack vertically when only Detail is available.

The detail page is keyed by Agent and configuration epoch. Its segments are
**History**, **Skill**, **MCP** and **Install & paths**; History is the default.
A read-only current-configuration pin sits above the segments. The Skill segment
for Claude Code, Pi, Grok Build and Codex lists every library skill with its
injection status and the same Inject/Remove/Resync actions as the Skill detail
page ([`skills.md`](skills.md#deploy-to-agents)); the other Agents state that
skill deployment is not implemented. For Claude Code, Pi, Grok Build and Codex
the MCP segment is read-only: it reports the recorded registrations and links
to MCP management; the other Agents state that MCP deployment is not
implemented. For Codex, the History
segment holds the Codex settings panel (owned Providers, revoke, clear
default, change history and interruption recovery) instead of the shared
version history.

## Configuration adapters

The four write adapters all use the shared, recoverable write path described in
[`configuration.md`](configuration.md). Each owns its file formats and its
native mapping; contracts are recorded in [`integrations`](integrations).

| Agent       | Files written                                                                                    | Verified contract                                                                                                | Accepted limits                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ----------- | ------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Claude Code | `settings.json` (`env` block)                                                                    | Official settings/gateway docs; local 2.1.287                                                                    | DeepSeek and OpenRouter gateways; Command Code refuses until its gateway root is verified. Keys are written in plain text by design.                                                                                                                                                                                                                                                                                                                                                                   |
| Pi          | `models.json`, `settings.json`; reads `auth.json`                                                | Published models/settings schemas of the selected stable Pi 1.x package (1.2.0, 1.3.0 and 1.4.0 verified)        | Model names read as `(vibemate-<Provider>) <model ID>`; UUID entries migrate through preview and backup. Saved per-model thinking defaults only for declared levels Pi accepts; verified DeepSeek `deepseek-flash` extended-level mappings are injected separately from the chosen default. Plans list each model setting's source and write state; existing request parameters are preserved.                                                                                                         |
| Grok Build  | `config.toml`                                                                                    | The shipped `26-config-reference.md` and `.metadata_version` unpacked by the installed version (verified 1.0.67) | `chat_completions` and `responses`; `messages` is refused until xAI documents the key's request header. The TOML `name` reads as `(vibemate-<Provider>) <model ID>`; the table alias stays a stable identity. Declared non-empty effort lists are written to `reasoning_efforts` with `value/label` and an in-list `default`; existing arrays and deprecated effort fields stay untouched. `managed_config.toml`/`requirements.toml` are reported as present only.                                     |
| Codex       | `config.toml` (`[model_providers.vibemate-<slug>]`, optional top-level `model`/`model_provider`) | Codex `rust-v0.165.0` source and official configuration docs; local `codex-cli 0.165.0`                          | Only `wire_api = "responses"`; models without a Responses route are refused. The key is written in plain text to `experimental_bearer_token` (file 0600). Never writes `profile`, `[profiles.*]`, `model_catalog_json`, `env_key` or `auth`. **Set as Codex default** is optional; `model_reasoning_effort` is written only with it, only when the central model declares a default level; otherwise the user's value is left untouched. No whole-version restore. See [Codex](integrations/codex.md). |

Provider/model combinations that are unverified are refused rather than written
with a guess, and the interface states the reason.

Where an Agent file has a display-name field, vibemate writes
`(vibemate-<Provider>) <model ID>`, for example
`(vibemate-Command-Code-GOAT) deepseek/deepseek-v4.1-flash`. The marker identifies
vibemate as the writer and carries the Provider as a hyphenated token; a space,
then the model ID, follows the closing parenthesis. The model ID stays readable
(slashes, capitals and dots are kept); whitespace collapses to one space,
invisible format characters are removed, and the whole name is limited to 160
characters with `…` for an overlong ID. A reasoning level is a separate request
setting and is appended only once an adapter writes one. The model ID remains
the identity — only the display text is combined. Grok Build's TOML `name` and
Pi's model `name` use the shared `shared::agent_model_display_name` helper.

In vibemate's own Agent list, the model each Agent currently names in its live
configuration appears as plain secondary text. The adapters read
`env.ANTHROPIC_MODEL` (then `model`) for Claude Code, the latest Pi session's
`model_change`/assistant `provider`/`model` (falling back to
`defaultModel`/`defaultProvider`) for Pi, `[models].default` for Grok Build,
and top-level `model_provider`/`model` for Codex.
For Pi this reads only the tail of the newest `sessions/**/*.jsonl` file and
extracts those two fields; message content is discarded. A model is shown
as `Provider · Model` only when the adapter's own ownership evidence still
resolves it to a saved Provider/model pair (gateway address and model for Claude
Code, `pi_provider_binding` for Pi, `grok_model_binding` for Grok Build,
`codex_default_binding` plus a still-written `codex_provider_binding` for
Codex). Any
other readable configuration reads **Own configuration** (`自有配置`); an
undiscovered Agent or one whose files cannot be read safely keeps **No
configuration summary**. This summary is independent of history matching: an
external edit must not turn a current vibemate model into a false "own" claim.
Any future write adapter must define both the live active-model field and its
ownership record with the adapter; never infer
ownership from a retained history version or from the central model library
alone.

Writing never implies that a
running session uses the value; each card states whether a new session is needed.

## Verification scope

Six real CLI installations, native execution and a bilingual manual review of
paths, minimum window and keyboard behavior passed on 2026-10-13. The three write
adapters passed native preview/apply/restore checks with temporary configuration
between 2026-10-14 and 2026-10-16. The Codex adapter has Rust and UI tests
only; native Codex checks and real Provider sessions are left to the
maintainer. Windows/Linux native behavior and a dedicated
screen-reader pass are release-time checks, recorded in
[`plans/todo.md`](plans/todo.md).
