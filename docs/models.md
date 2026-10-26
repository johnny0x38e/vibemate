# Provider models

This page describes how vibemate discovers, stores and edits the models of a
provider instance. Provider identities, endpoints and protocols live in
[`integrations/providers.md`](integrations/providers.md); native Agent injection
of these definitions is planned in [`plans/model-parameters.md`](plans/model-parameters.md).

## Responsibility split

| Location        | Responsibility                                                                    |
| --------------- | --------------------------------------------------------------------------------- |
| Provider API    | Base URL, credential reference and preferred protocol                             |
| Provider models | Fetch, add, alias, capability evidence, context window and output limit           |
| Agent config    | Inject the selected model definition and verified native effort defaults/mappings |
| Rust            | Check the combination against the provider, model, protocol and Agent             |

The saved Provider model is the only model definition. Agent adapters consume its
capacities, image/tool/reasoning capabilities and effort list/default where their
native format supports them. A default-only channel and a native switchable
mapping are different adaptations. Injection may select a default from declared
levels; it does not add another capability editor or temperature/top_p controls.
Unmapped fields are reported without changing the Provider definition or another
Agent.

## Terminology and persistence

Only checked models are persisted in `provider_model`, including manual rows.
Unchecking a model on the added list and saving removes the row; browsing never
persists the fetched catalog. Upstream lazy paging applies to browsing only; the
added list loads every persisted row internally in cursor pages.

The UI calls the saved membership **Added** / 「已添加」. Checkbox actions still use
selection wording. Internal `selected` fields, command parameters and translation
keys stay unchanged.

## Two model views

| View         | Opened by                   | Source                                                            | Saving                                              |
| ------------ | --------------------------- | ----------------------------------------------------------------- | --------------------------------------------------- |
| **Added**    | Default when opening Models | `list_provider_models` with `filter: "selected"`, optional search | Deselections delete rows atomically                 |
| **Upstream** | **Fetch models**            | `browse_upstream_models_page` through provider catalog adapters   | Checked models are persisted, then the list reloads |

The toolbar holds the search field, **Fetch models** (added view), **Cancel** and
**View added models** (upstream), and **Refresh**. The primary checkbox **Save**
sits in the shared secondary-page header beside Back. There is no All/Selected
toggle. The toolbar and list header stay fixed while the list scrolls.

Checkbox changes are drafts until Save. In the added view, Save submits
deselections through `save_provider_model_selections`. In the upstream view, Save
submits new or changed selections and then reloads the added list. An upstream
model that is already persisted starts checked. Unchecking a manual model and
saving deletes it through the same batch.

Dirty checkboxes require a bilingual discard confirmation when switching to API,
using Back, re-clicking the active Providers menu or changing views. Pending
writes separately block Back.

## Search

- Added: fuzzy search over persisted rows with `query`; the summary uses
  `totalMatches` when Rust supplies it.
- Upstream: fuzzy search over model ID and upstream name.
- Rust keeps one in-memory catalog per browse session in `ModelFetchRegistry`.
  OpenRouter grows page by page; single-page vendors load once. UI offsets refer
  to result or ranked rows, never raw API offsets. Debouncing and the 200-character
  query limit apply to both views.

## Fetch and refresh

- **Fetch models** downloads the catalog into the in-memory browse session and
  never merges it into SQLite. Only the user's Save persists rows.
- **Refresh** (`refresh_saved_provider_models`) downloads the catalog and updates
  rows already on the added list. Aliases, sources and checkboxes stay; nothing is
  inserted or deleted. Only a complete catalog can mark a saved row as no longer
  listed upstream; an incomplete one updates metadata only.
- `fetch_provider_models` keeps the original full merge path for tests and
  internal callers; the Models tab uses browse and save.
- Fetching is a Bearer-token GET with bounded bodies, cancellation and system proxy
  support. A successful list only proves that the endpoint answered; it does not
  prove the key, the account, the plan or inference.
- A model that the account may not use can still be listed, and a complete catalog
  is the only signal for absence.

## Manual models

Both views offer **Add a model manually** as a plus icon in the models toolbar. It
expands the same `ModelEntryForm` in create mode and **Add and save** immediately
persists one selected manual row through `add_manual_provider_model`,
independently of checkbox Save. Addition is blocked while checkbox drafts or
other model writes are pending. Route support still comes from the provider's own
route declaration; an alias is only a display label.

An unknown write outcome is never reported as saved. **Check saved models** reads
saved cursor pages and compares the exact ID, including IDs longer than the search
limit; similar IDs do not confirm a save. A failed check stays retryable.

## Capability evidence

Three capabilities are stored per field as state plus source: thinking, image
input and tool calls. Only an explicit **supported** result draws an icon;
unknown and unsupported draw nothing, and a missing icon never claims the model
lacks the ability. Each icon names its source in its accessible label.

| Capability  | DeepSeek                            | OpenRouter                      | Command Code |
| ----------- | ----------------------------------- | ------------------------------- | ------------ |
| Thinking    | `effort.supported_levels`           | `reasoning` object              | No field     |
| Image input | `input_modalities` contains `image` | `architecture.input_modalities` | No field     |
| Tool calls  | Official per-model capability table | `supported_parameters`          | No field     |

Missing, `null`, empty and malformed values are interpreted per protocol and
never become "supported". Field-level parsing rules are in
[`integrations/providers.md`](integrations/providers.md).

Users can declare a capability either way. Every checkbox interaction records a
user declaration (checked means supported, unchecked means unsupported) that
overrides catalog evidence in both directions; untouched fields keep the catalog
result or unknown. **Restore default** clears all three declarations and disables
itself when nothing is declared. Declarations live in their own column, so a
catalog refresh replaces catalog evidence without touching them.

## Model parameters

The model dialog stores the context window, the maximum output and the declared
thinking efforts (`any` for OpenRouter's `supported_efforts: null`, or an explicit
list with an optional default). The vocabulary is `max`, `xhigh`, `high`,
`medium`, `low`, `minimal` and `none`; unknown values are not written. Context
window and maximum output share the 64K/128K/200K/500K/1M slider stops (binary
units; a catalog value between stops snaps to the nearest).

Catalog values override saved values when the field is present; a missing field
keeps the saved value; a later manual edit becomes the new state. These three are
metadata, not capability declarations, so a refresh may replace them. The effort
chosen for a request belongs to the Agent configuration, not to this dialog.

Model editing is protected by a SHA-256 content digest: Rust reads the model,
returns the digest and only writes alias, declarations and parameters when the
digest is unchanged; otherwise it returns `model_changed` and the editor reloads.
Escape collapses an open inline editor from its text fields, and focus returns to
the row.

## Provider deletion

The Providers list row has a destructive **Delete** action that opens a
confirmation naming the provider and its added-model count. Before deleting, the
dialog reads Pi/Grok ownership rows and classifies each one against the live
Agent file:

- an active or unreadable binding blocks deletion;
- a stale binding can be cleared explicitly after the Agent file no longer
  contains the named entry;
- Claude Code remembered starting choices are counted and removed by the same
  foreign-key cascade.

Deletion removes the configuration, its saved models and its stored key only when
no binding row remains. A stale confirmation fails without touching the
credential store; a definite database failure restores the key; an unknown
outcome reloads the list. Agent configuration files are never read or written by
the delete itself; the reference check only reads the files to prove whether an
entry is still present. Pi's saved `modelThinkingLevels` key for a removed model
is a preference rather than a live binding, so it stays in `settings.json` and
becomes inert; it can be cleared in Pi's settings file or by restoring an earlier
configuration version. MCP definitions have their own cross-directory reference checks during
deletion.

Model removal follows the same rule: a Grok alias or a live Pi model entry blocks
removal, and a stale Grok alias can be cleared from the Models tab before saving.
The Models tab lists those references instead of silently dropping the row. See
[`skills.md`](skills.md) for the Skill deployment rule.

## Verification scope

Boundary tests replace only Tauri's runtime check and invocation, covering request
shapes, missing and extra fields, malformed nested metadata, mismatched
acknowledgments, safe errors and preview. Rust tests cover catalog parsing,
capability unknown/malformed values, migration retention, refresh that preserves
aliases, declarations and effort, manual save and clear, and concurrency. Provider
and model acceptance, including a real restart, passed maintainer review on
2026-10-12 and 2026-10-14; real provider requests and Windows/Linux behavior
remain tracked separately.
