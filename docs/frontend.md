# Frontend development

The frontend is React + TypeScript, rendered inside Tauri's system WebView.
React describes the interface; Rust handles local system and provider operations.
`src/AGENTS.md` contains the frontend implementation rules.

## Current code and layout

Sidebar activation always requests the destination module home, including clicks
on the currently active module. Providers, MCP and Agents open their lists;
Settings opens General without remounting preference controls. Modules expose only
`openHome(): boolean` through `ModuleNavigationHandle`; React's
`useImperativeHandle` connects that synchronous navigation request to each module's
current guards. A ref here is a handle to an action, not a DOM element. When the
active Providers page has model drafts, cancelling its confirmation returns
`false`, and a busy module also rejects home navigation, so App does not switch
destinations. Provider API writes, MCP pending/uncertain outcomes and Agent scans
retain their guards; model busy/dirty reporting covers visible model panels only,
so hiding them cancels fetches and invalidates reads rather than providing a
global leave-page guard. Leaving for another module hides the old module; returning requests
its home instead of resuming an editor. Unsubmitted Provider API and MCP forms are
discarded on successful home navigation, matching their Back behavior; Agent
manual paths and Settings controls remain in memory, and late MCP edit reads are
invalidated when returning home.

- `src/main.tsx` locates the HTML root and mounts React in StrictMode, with
  `LocaleStartup` withholding the App until its language preference is ready.
- `src/App.tsx` composes the desktop shell: the collapsible sidebar, the
  home overview (`OverviewView` — five configuration-total cards including
  `count_provider_models` (selected-into-library only)), `ProvidersView` (mounted on the first visit, then
  kept mounted while hidden to retain caches and pending work),
  central MCP management, read-only Agent discovery, `SkillLibrary`, and
  `SettingsView` (mounted while hidden). Sidebar width is fixed at 200 px expanded
  and 88 px collapsed; only the collapse button changes layout. `src/App.css`
  holds shared design variables and scaffold styles.
- `src/features/providers/` holds the provider views; see
  [Provider configurations](#provider-configurations). `SettingsView` uses
  General/About tabs, and tab switches or leaving Settings keep preference
  controls mounted so pending saves and input survive.
- `src/components/FieldSelect.tsx` is the shared single-choice dropdown.
  Create/edit rows (MCP transport, provider protocol, create-form provider kind)
  and the Agent detail provider and model pickers use `size="fill"`, which
  stretches the trigger to its column so its right edge lines up with the card's
  other controls; settings rows keep the default narrow width. Settings and
  provider forms use it instead of a native `<select>`; CSS Modules and existing
  theme tokens still own its appearance.
- `src/components/IconButton.tsx` provides auxiliary form actions with an
  accessible name and a Base UI tooltip on hover or keyboard focus. Tooltips are
  portaled to avoid card clipping and support Escape dismissal. Blocked actions
  remain focusable but do not invoke callbacks. Main save, navigation and recovery
  actions keep visible text.
- Forms share two styles: `editForm.module.css` for create/edit cards (label left,
  control column, hints, field errors) and `detailForm.module.css` for read-only
  detail cards through `DetailPanel` / `DetailFact`.
- `src/components/WindowDragRegion.tsx` and `TitlebarChrome.tsx` (with
  `WindowControls` on Windows/Linux builds) implement drag strips and custom
  window chrome. Never place buttons or fields inside a drag region; check this
  by eye in the native window, because jsdom has no layout engine to catch it.
  Window IPC lives in `src/lib/desktop/window.ts`.
- `src/components/BrandLogo.tsx` renders one SVG logo: the transparent V and
  rounded wordmark when expanded, and the V symbol alone when collapsed.
- `src/components/PageModuleHeader.tsx` is the shared module title row. Secondary
  pages reuse its metrics in `ProviderPage.module.css`, where Back (icon-only) and
  Save live; editors publish Save through `SecondaryPageActions`.
- `src/components/Notifications.tsx` is the app-level notification host, mounted
  once in `App`. Features call `useNotify()` with an already translated `message`
  and optional `tone` (`success` | `error`); a plain string defaults to success.
  One notification shows at a time in a polite `role="status"` region that holds
  only the message text, so it is announced without moving focus or reading the
  close button (which sits outside the live region). It dismisses itself after
  `NOTIFICATION_DURATION_MS`, pausing the timer while hovered or focused and
  keeping the time left; the close button or Escape also returns focus to where
  the user came from. The entry fade only runs under
  `prefers-reduced-motion: no-preference`. Outside the provider, `notify` does
  nothing.
- `src/lib/desktop.ts` checks runtime availability, calls Rust, and validates the
  returned data. Components never import Tauri APIs directly.

On macOS the window uses Tauri's overlay title bar (`titleBarStyle: "Overlay"`,
`hiddenTitle`, `trafficLightPosition` in `tauri.conf.json`): the native traffic
lights stay, but there is no separate title-bar row, and the top of the sidebar
and of the content column are drag regions centered on the traffic-light row.
Windows and Linux builds use undecorated windows with in-app minimize, maximize,
and close controls in the content-column title strip (`TitlebarChrome`).
Vite emits every asset as a file (`assetsInlineLimit: 0`) because the CSP's
`img-src 'self'` blocks the `data:` URIs Vite would otherwise create.

As functionality grows, use `src/features/agents/`, `src/features/skills/`, and
`src/features/mcp/` for implemented features. Create these folders when needed,
not as empty placeholders.

A component is a function that returns JSX, React's HTML-like UI notation.
Props are its typed inputs. State is data React keeps between renders; updating
it asks React to render again. An effect coordinates with an external system.
It should not be the default place for calculations or button-click actions.

Effects that wait for IPC, such as the language startup read in `LocaleStartup`,
ignore results after cleanup. Cleanup runs when the component unmounts and during
StrictMode's development lifecycle check. Without that guard, a previous request
could update an obsolete view.

## Changes must pass automated checks

```sh
pnpm run check:frontend
```

This runs Prettier, ESLint with zero allowed warnings, TypeScript for both the
UI and Vite configuration, release-tool tests, the translation-resource check and
its tests, the palette contrast check, UI behavior tests, and a production
frontend build. CI and the release workflow run the same command before continuing.

TypeScript enables strict, exact optional properties, and unchecked-index checks.
ESLint uses type-aware strict rules and React Hooks rules. It rejects unsafe
types, unhandled promises, and misplaced Tauri imports. Oxlint runs a dedicated
set of JSX accessibility checks. This avoids holding ESLint on an unsupported
major version because of a JavaScript accessibility plugin's peer constraints.
Use compatible current stable versions for both tools.

ESLint checks TypeScript frontend/config files and the Node release and translation
scripts. Formatting is handled by Prettier, so style rules must not conflict with it.

## Editing in Zed

Open the repository root, run `pnpm install --frozen-lockfile`, then use
`task: spawn` → a `vibemate:` task for dev, preview, or `check:frontend`.
Zed covers Rust/TypeScript/TSX without extra extensions.

Shared repo config only:

- `.zed/settings.json` — ESLint language servers, `src-tauri/Cargo.toml` for
  rust-analyzer/Clippy on save, and `pnpm-lock.yaml` as Plain Text (pnpm's
  multi-document lockfile is not normal YAML).
- `.zed/tasks.json` — same commands as CI (see tasks in repo).
- `package.json` → `prettier.tabWidth: 4` for formatted JS/TS/JSON/CSS; Rust
  uses `rustfmt` via `rust-toolchain.toml`. Themes, format-on-save, and
  keybindings stay in user settings.

Editor diagnostics do not replace `pnpm run check:frontend`.

## Shared dropdown interaction

`FieldSelect` uses the stable MIT-licensed `@base-ui/react` Select. Base UI is
headless: it supplies behavior without a visual theme or a Tailwind requirement.
Only the Select entry point is imported. Existing buttons, inputs, theme radios,
layout, notifications and business state are not migrated to a new framework.
Future complex controls can use Base UI when an implemented behavior needs them.

The parent still owns `value` and `onChange`. Selecting a language or appearance
requests a save; the widget cannot treat that request as a confirmed saved value.
Base UI handles item focus, arrow keys, Home/End, character-based navigation,
Enter/Space selection and dismissal. The wrapper controls only whether the popup
is open and whether a business operation has locked the field.

`disabled` remains a native disabled button. `blocked` keeps the provider field
focusable with `aria-disabled`, uses Base UI's read-only value protection, and
refuses opening or value callbacks. A conditional state reset during the field's
own render closes an already-open menu when it becomes locked, so unlocking
cannot reopen an old menu. This is a guarded update of the same component, not an
effect that repeatedly derives state.

`Select.Portal` renders the menu outside the card's DOM subtree while retaining
React composition. This avoids card/scroll-container clipping. Tokens on `:root`
still supply the palette, and the named `--layer-popover` token places menus above
ordinary app content. Positioning uses the trigger width and viewport collision
handling; the list scrolls within the available height. A hidden anchor also hides
its portaled popup. `alignItemWithTrigger={false}` prevents the menu from
overlapping the selected value.

Tests use MIT-licensed `@testing-library/user-event` for complete pointer and
keyboard sequences. The shared helpers follow `aria-controls`, not DOM parentage,
and are asynchronous because pointer opening and focus restoration can wait for
animation frames; callers must await them. FieldSelect cases cover selection,
actual focus movement/restoration, Home/End, typeahead, Escape, Tab, outside
clicks, both lock transitions, labels/icons, multiple fields and StrictMode. The
Tab test supplies visible-button rectangles because jsdom has no layout; it does
not establish real viewport placement. Dedicated screen-reader and Windows/Linux
runtime verification are deferred to the release-time platform check.

## Internationalization foundation

i18next and react-i18next (both MIT) provide bundled English and Simplified
Chinese resources in `src/i18n/index.ts`. A resource is a JSON object whose stable
keys identify complete UI messages, grouped by feature. Provider brands, model IDs,
URLs, and user content stay outside translations.

`resolveSystemLocale(systemLanguage)` maps the Chinese language subtag, including
traditional Chinese variants, to `zh-CN`; all other languages resolve to `en`.
`createAppI18n(locale)` returns a promise for a ready, independent translator.
Importing the module neither detects a language nor reads or saves preferences.
Resources are bundled rather than fetched, with English as the fallback.

Rust persistence, validated IPC, the startup gate and the language selector are
connected. Saved English/Chinese choices override the captured system language;
following system and browser preview use `resolveSystemLocale`. Every navigation
label, heading, state, hint, error and accessible name lives on grouped keys in
both languages. Chinese uses one term set everywhere: 服务商, Agent, 技能 and
MCP 服务器. Brand names (`vibemate`, providers and agents), the `MIT` license
name, model IDs and URLs stay untranslated. The window title remains the product
name `vibemate`. The ready tree uses an explicit `I18nextProvider`.

The i18next `CustomTypeOptions` declaration constrains translation keys using the
English JSON shape, and `src/i18n/index.test.ts` asserts at the type level that
the Chinese JSON has the same shape, so `pnpm run typecheck` fails on a missing
or extra key. JSON string values are not literal types, so TypeScript alone does
not check values, interpolation names or plural forms; the resource check below does.

A plural message such as `itemCount_one`/`itemCount_other` with
`{{count, number}}`, called as `t("feature.itemCount", { count: 1200 })`, uses
plural rules and the built-in Intl number formatter. Dates can use
`{{date, datetime}}` with explicit timezone options where needed; standalone
values can use `Intl.DateTimeFormat` and `Intl.NumberFormat` with the resolved
locale. React escapes rendered text, so i18next interpolation escaping is
disabled; never render these strings as raw HTML.

Add both translations, accessible names, and error messages with every feature.
Rust returns safe error codes; React translates messages rather than raw
internal errors. Do not use browser storage for language preferences.

### Translation resource check

```sh
pnpm run check:i18n   # validate src/locales/*.json
pnpm run test:i18n    # the checker's own tests, with passing and failing fixtures
```

`scripts/i18n-resources.mjs` uses only Node built-ins and runs in
`check:frontend` before the UI tests. It compares every locale with English and
prints each problem with its key path, then exits with status 1. It reports:

- keys missing from, or not present in, the English reference;
- empty or whitespace-only values, non-string values and empty groups;
- `{{ }}` markers that are not closed or name no parameter (`{{}}`), and
  interpolation names that differ from English. The unescaped form `{{- name}}`
  passes the same `name` parameter as `{{name}}`. Plural forms of one key are
  compared as one message, so an English singular may omit `{{count}}`;
- plural keys without every form that `Intl.PluralRules` selects for that locale
  (English `one`/`other`, Chinese `other`), or with a form no bundled locale uses;
- ordinal plural keys (`_ordinal_*`), which fail with "ordinal plurals are not
  supported yet" until the check learns ordinal rules.

The suffixes `_zero`, `_one`, `_two`, `_few`, `_many` and `_other` are reserved
for i18next cardinal plurals; do not end an ordinary key with them. `_zero` is
always optional: i18next uses `key_zero` for a count of 0 in every language, even
though `Intl.PluralRules` never selects "zero" for English or Chinese.
Chinese may keep an `_one` form because English needs it and the TypeScript
shape check requires identical keys; Chinese never selects it.

The check does not cover single-brace text such as `{name}` (i18next ignores it,
so it renders literally), conflicts between a dotted key name and a nested group
(`"a.b"` next to `"a": { "b": … }`), or whether a nested `$t(other.key)`
reference points to an existing key. Review those by hand if they are introduced.

There is deliberately no unused-key detection. Components build some keys from
typed values, such as ``t(`desktop.nav.${page}`)`` and
``t(`settings.theme.names.${palette}`)``, so a text search would report those
messages as unused or need a list of exceptions. Remove keys when the UI that used
them is removed.

`LocaleStartup` does not use the translator: it must explain loading and failure
before any translator exists, so it reads `settings.startup` directly from the
bundled JSON for the system language. Those messages therefore have no English
fallback at runtime; the empty-value check is what keeps them from rendering blank.

Official references: [i18next configuration](https://www.i18next.com/overview/configuration-options),
[TypeScript](https://www.i18next.com/overview/typescript),
[formatting](https://www.i18next.com/translation-function/formatting), and
[React instance provider](https://react.i18next.com/latest/i18nextprovider).

## Theme color tokens and the contrast check

`src/themes.css` holds palette values only: every palette declares the same token
names with `-light`/`-dark` suffixes, and `src/App.css` resolves them into the
semantic `--color-*` tokens that components use. Three hue families sit beside
the neutral ramp: chips (`--theme-chip-info-*`, `--theme-chip-count-*`) label
metadata such as a protocol, transport or model count; status colors
(`--theme-status-success-*`, `--theme-status-danger-*`) carry an outcome for
notifications, status dots and destructive actions; and the accent pair
(`--theme-accent-*` with `--theme-on-accent-*`) fills primary buttons.

Status hues follow the platform a palette imitates: Apple's system green and red
for `macos`, Fluent's `colorPaletteGreen*` and `colorPaletteRed*` tokens for
`windows`, and the original pair for the other six palettes. Apple publishes only
solid fill colors, and its light red and green are too pale to read on a light
page, so those two foregrounds use Apple's increased-contrast variants and the
light red is darkened until it reaches 4.5:1.

```sh
pnpm run test:themes   # scripts/theme-contrast.test.mjs
```

The check parses `src/themes.css` and asserts, for all eight palettes in both
brightness modes, that chip labels reach 4.5:1 on their capsule, danger text
reaches 4.5:1 on the page background, and the success and danger foregrounds
reach 3:1 on the page background and on the tinted notification card. A palette
edit that misses a floor fails with the theme, mode, pair and measured ratio.

## Language preference storage and IPC

`src-tauri/src/settings.rs` owns the three validated choices: `system`, `zh-CN`,
and `en`. Schema v2 adds a singleton `locale_preference` table to the existing
app-data SQLite database. A missing row means follow system; reading it does not
write a default. A read error or invalid stored value is reported instead of
silently switching to system. A single UPSERT atomically inserts or replaces the
choice; a failed SQLite write leaves the previous committed choice unchanged.

The thin commands in `src-tauri/src/commands.rs` run database work through Tauri's
blocking worker pool. This keeps SQLite's lock wait off the window thread and
async executor. Errors serialize as fixed codes, not SQL, paths, or raw platform
messages. No new plugin or capability permission is required for these app-owned
commands; the existing local-content CSP and capability configuration are unchanged.

`src/lib/desktop/settings.ts` validates Rust responses at runtime:

- `getLocalePreference()` returns either `{ kind: "desktop", preference }` or
  `{ kind: "preview" }`. Preview does not invent a saved preference.
- `saveLocalePreference(preference)` rejects preview and requires the response to
  match the requested choice before reporting success.
- `SettingsRequestError.code` identifies a safe failure for the UI to translate.
  Unknown runtime errors are discarded. A task/transport failure or malformed
  acknowledgment has an unknown saved outcome: reload the preference before
  claiming rollback or displaying a confirmed saved choice.

The Rust tests use temporary databases and cover reopen, migration preservation,
read/write failures, invalid values, SQL constraints, and poisoned locks. Boundary
tests replace Tauri core APIs and cover preview, all choices, malformed responses,
mismatched acknowledgments, and error sanitization. These tests alone do not prove
real WebView IPC; Windows/Linux acceptance is deferred to the release-time platform check.

## Language selector component

`src/features/settings/LanguageSelector.tsx` receives the validated startup
snapshot and a system language tag. Mount it beneath the initialized
`I18nextProvider`; do not remount the provider or application when language changes,
because the snapshot initializes local selection once and unrelated parent
rerenders must not discard a confirmed choice. The selector does not read startup
preferences or own the document language; those belong to `LocaleStartup`.

It uses the shared `FieldSelect` and a live status message. Preview is disabled
with an explicit explanation, and a pending save also blocks the control; a
synchronous request guard prevents two writes before React renders that state.
Only a confirmed save changes the translator. Explicit English/Chinese choices
win over the system tag; choosing system resolves it through `resolveSystemLocale`.
Sibling forms remain mounted and retain their unsubmitted input.

Known storage/write failures retain the previous choice and allow another attempt.
An unknown saved outcome blocks new writes and offers **Reload saved preference**;
a failed reload stays blocked and can be retried. If persistence succeeded but the
translator failed, the saved choice is retained and the message says that only the
interface change failed, with reloading retrying that application. The component
never displays raw exception text or claims an uncertain rollback.

Effect cleanup cannot cancel an IPC write, but late save/read results cannot
change the shared translator after this component unmounts. Tests exercise this
observable behavior rather than only checking that the unmounted DOM is empty.

## Startup language gate

`LocaleStartup` captures the system language for the window's lifetime. It first
reads the validated preference, then awaits an independent translator, and only
then mounts the selector and App. Startup never saves a default. Until the saved
choice is known, bundled system-language messages explain loading or failure;
reading failure and translator initialization failure have separate messages and
a keyboard-accessible retry. Private exception text is never rendered.
These messages come from the bundled JSON rather than the translator; see
"Translation resource check" above.

The read effect has its own cleanup flag for each attempt. This is important in
StrictMode: a response from its discarded first effect must not replace the ready
view or turn it into an error. A synchronous retry guard prevents duplicate reads.
Language changes update the existing translator, not the startup state, so the
provider and child inputs are not remounted.

A short layout effect synchronizes the document's `lang` before paint, then
subscribes to i18next's `languageChanged` event. Its cleanup removes that exact
listener. The document element is outside React's managed root; changing this
attribute is an external synchronization step, not direct manipulation of JSX.
The brand-only window title needs no localized title command or extra permission.

Startup tests cover waiting, saved-choice priority, preview, safe failures/retry,
translator initialization failure, obsolete StrictMode responses, late unmounted
reads, and input/HTML-language preservation while a save is pending. The i18next
factory wrapper retains real initialization except for one explicit external-service
failure; the startup module itself is not mocked. Native platform acceptance is
distinct from DOM input-preservation tests.

## Manual review still matters

Automated checks do not enforce every architecture decision or prove a usable
interface. Review component responsibility, state transitions, runtime data
validation, comments, credentials, styling, and platform assumptions explicitly.
For UI changes, inspect loading/error/empty states, keyboard focus, long labels,
and resizing. For desktop behavior, also run the real Tauri app.

**Appearance is verified by eye, not by tests.** Do not write styling or layout
tests: no assertions on class names, CSS-module selectors, computed or inline
styles, color or token values, element sizes and positions, or which decorative
asset renders. jsdom has no layout engine, so a passing test cannot show how the
window looks, and it breaks whenever the stylesheet changes. Spacing, color and
visual hierarchy are checked in the browser preview and the native app. Tests
cover only what a user or a screen reader acts on — roles, accessible names,
focus, keyboard behavior, state text and the data those states carry. A purely
decorative image needs no test; a control is identified by its accessible name,
not its styling.

## UI behavior tests

Run `pnpm run test:ui` once, or `pnpm run test:ui src/App.test.tsx` for the
desktop-shell and bilingual behavior. Vitest shares the Vite configuration and uses
jsdom to supply a DOM inside Node.js. React Testing Library renders the real
component. DOM Testing Library is an explicit peer dependency; the test packages,
including user-event for dropdown interactions, are MIT.

Tests import Vitest APIs explicitly, so no test globals or relaxed lint rules are
needed. Both existing TypeScript configurations and strict ESLint remain in force.
Only `src/**/*.test.{ts,tsx}` runs in Vitest; release-tool tests retain Node's runner.
Cleanup is registered explicitly because Vitest globals are disabled.

`src/App.test.tsx` replaces only the desktop boundary modules (`src/lib/desktop`
and its settings/appearance wrappers) and composes `LocaleStartup`, the real
selector and App as `main.tsx` does. Its cases cover navigation and collapse, and
language where language is the behavior: a saved Chinese choice over an English
system, following a Chinese system, switching through the real selector while
keeping the page, settings tab, collapsed sidebar, unsaved input and loaded About
metadata, English fallback for an untranslated Chinese entry, and startup-read,
About-metadata and language-save failures with retry. Scenarios whose locale only
decides which strings the test queries run once, in English; see the i18n rules
in `src/AGENTS.md`. A helper checks visible text, `aria-label` and `title` for
unresolved keys and private diagnostics; temporarily rendering a raw key or
allowing empty translations makes the matching cases fail.

Component-level metadata states (loading, preview, obsolete StrictMode responses)
are covered in `AboutPanel.test.tsx`; IPC validation is covered by
`src/lib/desktop/*.test.ts`.

These DOM tests do not verify Rust, the Tauri WebView, or provider connectivity.
Add behavior tests for new forms and mutations at their desktop boundary as they
are implemented; keep synthetic credentials out of production fixtures.

## Safe IPC example

`getAppInfo()` returns typed metadata or `null` for a browser-only preview.
Settings → About reads it on the first visit. It uses `invoke<unknown>()` and
checks the actual fields before returning an `AppInfo`. A TypeScript generic alone
cannot check data received at runtime.

Failures from both `getAppInfo()` and `openProjectRepository()` reject with
`MetadataRequestError`, whose `code` is one of:

| Code               | Meaning                                                      |
| ------------------ | ------------------------------------------------------------ |
| `invalid_response` | The command answered with data of the wrong shape.           |
| `operation_failed` | IPC itself failed; `get_app_info` cannot fail in Rust.       |
| `open_failed`      | Rust's repository command could not open the system browser. |

The original runtime error is discarded rather than wrapped, so paths and OS
messages cannot reach the UI. About shows one translated message per action.

Each desktop boundary module defines its own `XRequestError` class and code union
type: `SettingsRequestError` with `SettingsErrorCode` in `settings.ts`,
`AppearanceRequestError` in `appearance.ts`, and `MetadataRequestError` with
`MetadataErrorCode` in `desktop.ts`. Appearance reuses `SettingsErrorCode`
because Rust returns the same `SettingsError` for both preference commands.
Keeping one class per module lets UI code check `instanceof` for the request it
made and keeps each code list limited to what that request can actually return.
More complex commands will need structured success/error results and documented
payload schemas shared with Rust. Do not expose credentials in error messages.

## Configuration history IPC boundary

`src/lib/desktop/history.ts` wraps the read-only `configuration_history_page`
command. The request contains only a supported Agent identity, seek cursor and
page size; Rust resolves the configuration root. Browser preview returns
`{ kind: "preview" }` without invoking IPC or inventing records.

The nested history response uses [Zod](https://zod.dev/api) strict schemas rather
than another hand-written validation framework. A schema describes the runtime
shape, and `z.infer` derives its TypeScript type from that same definition.
`z.strictObject` rejects extra fields instead of silently stripping them. Zod
4.6.7 is a stable MIT dependency, locked in `pnpm-lock.yaml`; it was already in the
lint dependency tree and is now an explicit application dependency. Existing
boundary modules are not migrated in this slice.

Schemas check bounded arrays, paths, identities, nullable digests, journal states,
source-step ordinals and reconciliation relationships. Page checks enforce seek
ordering, unique identities, requested size and a next cursor anchored to the last
entry. The page also requires a separately loaded `currentEntry`: matched
markers must agree with its apply identity, complete file set and written
digests. Other marker states require `null`; restore/compensation rows are
refused in the configuration-version list. A malformed page is rejected as a
whole, not partially displayed as complete history. `HistoryRequestError` contains only an allowlisted safe code;
Zod issues and raw transport diagnostics are discarded, including unknown field
names. Do not log the original response or validation error.

The data deliberately separates source-operation state from each mutation's result.
`backupRecorded` means only that the journal has a reference; it does not prove a
backup exists, a version is current, or a restore is permitted. Reconciliation is
an observation, not evidence of who changed a file. `RestoreView` uses the
root-scoped paginated wrapper; version restoration is a separate preview and
confirmation, not permission inferred from the history row.

## Shared two-layer configuration preview

Model configuration and retained-version restore share `VersionSummary` for
field paths, add/remove/replace kinds and sanitized old/new values. Expanded
history rows show this summary, necessary errors, one concise key/session warning
and **Use this configuration**, without repeated backup, owner or timestamp
metadata. Model previews additionally expose `ConfigurationTextDialog` for complete
text. The detail dialog requests one complete
native JSON/TOML file only when opened or switched; React never reconstructs a
configuration from displayed fields. Missing, empty and unavailable sides are
separate states. Both original and sanitized text are limited to 1 MiB; the line
diff has its own computation limit and says when comparison is unavailable.

`src/lib/desktop/configurationPreview.ts` wraps the read-only
`read_model_configuration_preview` command. It sends the existing native model
plan back for exact comparison, plus an optional file index. Rust rebuilds that
plan with the same adapter used by apply, verifies file digests again and masks
credential-like keys throughout the document before returning data. Summary and
text reuse strict schemas from `versionRestore.ts`; response checks also bind
file order, paths, format and the requested text identity to the model plan.
The read accepts no arbitrary target path, reads no OS key and writes nothing.
The existing apply commands still recheck their original tokens.

`ModelPlanPresentation` owns summary loading/retry and blocks apply until that
read succeeds. A new reviewed plan remounts the presentation, while cleanup
ignores late replies. A memoized reader callback keeps the shared dialog's
request tied to that plan without reloading on unrelated renders. The dialog
focuses its title initially so a small viewport opens at the start of long
content rather than scrolling to the bottom Close button. Base UI traps focus,
handles Escape and returns focus to the invoking detail button.

Browser preview returns an explicit preview state, never synthetic history or
configuration. Acceptance fixtures are separate temporary QA pages, not a
production fallback. Isolated macOS Tauri verification used synthetic credentials
and three separate configuration roots; no real Agent configuration or Provider
request was involved. Windows/Linux native behavior remains a release-time check.

## Model IPC boundary

`src/lib/desktop/models.ts` wraps the model commands registered in Rust. It does
not fetch provider URLs or read credentials. Rust remains responsible for model
validation, fuzzy search, persistence, cancellation and network limits.

Each response is received as `unknown`. The wrapper checks exact field sets,
nullable metadata, enum values, non-negative safe integers, provider identity,
duplicate model IDs and requested page limits before returning typed data.
Selection acknowledgments must contain exactly the requested unique IDs and
selection state. A manual-add acknowledgment must identify the normalized ID
as a selected manual row; deletion requires a null acknowledgment. Ranked search
results have a match count and no next-page cursor. A fetch summary reports list
and merge counts, not proof of valid credentials or working inference.

`ModelRequestError` keeps only Rust's model error-code allowlist, plus
`invalid_response` and `desktop_required` generated here. Unknown strings,
objects and exceptions become `operation_failed`; the original payload is
not retained. After a mutation returns `invalid_response` or `operation_failed`,
reload persisted data before retrying: IPC failure does not establish rollback.
Browser reads return `{ kind: "preview" }`, mutations/fetch reject with
`desktop_required`, and cancellation returns `{ wasRunning: false }` without IPC.

The boundary tests in `src/lib/desktop/models.test.ts` replace only Tauri's
runtime check and invocation. They cover request shapes, missing/extra fields,
malformed nested metadata, mismatched acknowledgments, safe errors and preview.
They do not verify native WebView IPC, OS credential access or real providers.

## Settings logs

`LogSettings` is a separate section below the language/appearance card on the
General tab (its own heading and grouped-card row, same pattern as future General
sections). It shows the actual Rust-owned log file path in the same grouped-card
row layout as language and appearance (label left, value right-aligned), with
wrapping and text selection. Two small icon buttons after the path request a local
text tool (View logs / 「查看日志」) or the system file manager (Open log folder /
「打开日志文件夹」). Paths are read once when the group mounts.

`src/lib/desktop/logs.ts` validates both paths and the startup active flag,
rejects malformed responses, and keeps only safe error codes. Browser preview
shows no invented path or OS actions. An inactive startup file logger gets a
warning; an older log can still be opened. Raw errors are never rendered.

The component keeps read and open states separate. Each read has a generation
number, so StrictMode cleanup and unmount invalidate stale results.
A synchronous ref blocks rapid duplicate or competing open requests; rendered
buttons use `aria-disabled` rather than dropping focus with `disabled`.
The group remains mounted across settings/sidebar navigation, preserving paths
and pending actions. It never launches a tool just because it was displayed.

Rust accepts no frontend path or executable. It checks the fixed paths and uses
the existing opener's Rust functions, with TextEdit on macOS, Notepad on Windows
and the default file association on Linux. No opener/plugin capability or CSP
change is needed. Dispatch acknowledgment is not proof that a viewer displayed
the file, so the UI makes no success claim. See
[logging access and safety rules](logging.md).

Automated tests do not launch external tools. Native editor/file-manager opening
and visual acceptance remain manual checks.

## Settings About

About mounts on its first visit and stays mounted across tab and sidebar
navigation. It uses General's full content width, with the expanded `BrandLogo`
and description above version, license and repository rows. The GitHub action is
a bare icon button with no tile background or border, a translated name, visible
focus, disabled opening state and retryable error feedback. Its Octicons MIT
notice is bundled in `assets/licenses/octicons-MIT.txt`.

`getAppInfo()` reads Rust build metadata. Loading, ready, browser preview and
error/retry states remain distinct; preview never invents a desktop version.
Effect cleanup rejects obsolete StrictMode responses. Language changes translate
feedback without rereading. Tabs use one Tab stop; arrows, Home and End select
and focus native buttons.

`openProjectRepository()` calls a Rust command with no URL or executable input.
Rust uses the [opener crate's API](https://docs.rs/tauri-plugin-opener/2.7.0/tauri_plugin_opener/fn.open_url.html)
for the fixed `https://github.com/johnny0x38e/vibemate` address. The opener plugin
is not registered and no generic opener IPC permissions are granted. Browser
preview opens the same URL with `noopener,noreferrer`; private OS errors become
bundled feedback. The icon is from [GitHub Octicons](https://github.com/primer/octicons/blob/main/icons/mark-github-16.svg).

## Provider configurations

A saved configuration holds its settings and, when configured, its API key. A new
configuration takes its key in the create form, sent with the other fields, and a
saved one shows only whether a key is set and lets the user replace it (see
"API keys" below).

**Rust.** `src-tauri/src/providers.rs` validates and stores instances in the
`provider_instance` table added by schema v5. An instance has a stable random ID
(32 lowercase hex characters from SQLite `randomblob(16)`), a fixed kind
(`command-code`, `deepseek`, `openrouter`), a display name of 1–64 characters, a
normalized HTTPS base URL, a protocol, a `revision`, and creation and update
times. Names may repeat; only the ID identifies an instance. Base URLs are parsed
with the `url` crate, which rejects user names/passwords, queries, fragments and
non-HTTPS schemes; the scheme and host are lowercased, international domains
become punycode, and one trailing `/` is removed (no path is added). Phase 1
templates allow all three evidence-backed protocols (`chat_completions`,
`responses`, `anthropic_messages`). Every extension allowlist is empty, so any
extension key is rejected (`extension_field_not_supported`); new kinds and keys
are added only after official evidence.

**IPC.** Provider commands are `list_provider_templates`, `list_providers`,
`get_provider`, `create_provider` (whose request also carries the required
`secret`) and `update_provider`; two key commands are wrapped by
`src/lib/desktop/providerSecrets.ts`: `get_provider_secret_status({ providerId })`
and `replace_provider_secret({ request: { providerId, secret } })`. There is no
delete or clear command.
`src/lib/desktop/providers.ts` validates every response with a strict key set
(an extra field, such as an echoed key, is `invalid_response`) and keeps only the
safe error codes below; `providerSecrets.ts` reuses its error class and
allowlist. Lists use an opaque cursor: pass `nextCursor` back as `after`;
`limit` must be 1–100, and rows are ordered by creation time, then ID. Unlike an
offset, a cursor cannot skip or repeat rows when another window inserts one
between pages. An edit sends `id` and `expectedRevision`; a stale revision is
refused with `revision_conflict` instead of overwriting a newer save. The kind is
never part of an edit request.

**UI.** The Providers page switches views inside `ProvidersView` (state
`list | create | edit`, no router library). The list view shows loading, empty,
error-with-retry, saved rows and "Load more" while `nextCursor` is set. Each row
shows the brand icon, official brand name, a protocol pill and a selected-model
count pill, with the saved display name aligned under the brand text; Base URL is
omitted. Rows are keyed by stable ID. List data stays in `ProvidersView` while a
secondary view is open; going back does not re-read the whole list, but returning
from edit refreshes that row from storage so model counts stay current.

**Brand icons.** `ProviderIcon` maps a provider kind (never a display name) to an
official brand file in `assets/providers/`, imported through Vite like
`assets/brand/` (emitted as files, so the `img-src 'self'` CSP allows them).
The icon is decorative (`alt=""`), and unknown kinds render nothing. The files are
kept byte-for-byte as published: no recolouring or stretching, and Prettier has no
SVG parser, so checks never rewrite them. OpenRouter has a separate dark file;
both are rendered and `ProviderIcon.module.css` shows one with the same conditions
App.css uses for dark tokens (`data-appearance` and `prefers-color-scheme`), so no
script repeats the theme decision.

| Kind           | File(s)                                 | Source                                                                                     | Brand guidelines                                                                  |
| -------------- | --------------------------------------- | ------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------- |
| `command-code` | `command-code.svg` (both themes)        | `symbol.svg` from https://commandcode.ai/brand                                             | https://commandcode.ai/brand                                                      |
| `deepseek`     | `deepseek.svg` (both themes)            | https://api-docs.deepseek.com/img/favicon.svg                                              | No public brand kit; the maintainer reviewed the terms and chose to use the icon. |
| `openrouter`   | `openrouter.svg`, `openrouter-volt.svg` | `glyph-grape` (light theme) and `glyph-volt` (dark theme) from https://openrouter.ai/brand | https://openrouter.ai/brand                                                       |

Trademark note: the README states that "Provider names and logos are
trademarks of their respective owners and are used only to identify the
providers." (「服务商名称和标志的商标归各自所有者，仅用于标识服务商」). The icons
above are used on that basis only.

Create and edit use `ProviderPage` and `ProviderTabbedView`. The secondary header
row is **Back** (icon-only, with a destination-specific accessible label), a
left-aligned title (brand icon plus name via `titleBrand`), and trailing **Save**
published by the editor through `SecondaryPageActions`. Agent detail and MCP
editors reuse this shell; shared CSS variables keep the Back / title / Save
centerline aligned with the sidebar product logo.

Tab layout (Settings-style tabs: **API** / 「API 配置」 and **Models** with
optional selected count). Edit seeds the Models tab label from
`record.selectedModelCount` so the count is visible while the API tab is still
active, and `ProviderModels` refreshes it even when its panel is hidden. Panels
stay mounted while switching tabs; create shows a Models gate until the instance
is saved. The protocol control lists **OpenAI Chat Completions**, **OpenAI
Responses** and **Anthropic Messages** (same labels in zh-CN) for every Phase 1
vendor. The value is the **preferred** protocol when a model declares several
routes, or the instance wire protocol otherwise. Templates include per-protocol
suggested base URLs; switching protocol updates the Base URL only while it still
matches a documented default (DeepSeek Anthropic uses `/anthropic`). Different
bases are separate configurations. Wire values remain `chat_completions`,
`responses`, and `anthropic_messages`, and the resolved protocol appears as a
badge on model rows. Save and recovery actions ("Save again", "Refresh list",
"Reload latest settings") live in the header trailing slot; form-level feedback
stays in the scrollable form. There is no form Cancel; **Back** leaves the page.
One Save submits settings and an optional replacement key together.

The Models tab follows [models.md](models.md). In brief: the
default **Added** / 「已添加」 view lists persisted models, with a fixed list title
(**Added** / **All**) above the scroll region. **Fetch models** opens **upstream**
browse with lazy-loaded result pages via `browse_upstream_models_page` (optional
fuzzy `query`; session catalog in Rust). **Refresh** / 「刷新」, beside it,
downloads the catalog and updates only rows already on the added list
(`refresh_saved_provider_models`): aliases, sources and checkboxes stay, nothing
is inserted or deleted, and only a complete catalog can mark a saved row as
unavailable upstream. It is disabled while checkbox drafts or other model work is
pending. Checkboxes are draft until **Save**;
unchecking on the added list and saving removes rows from SQLite
(`save_provider_model_selections`). Search applies to whichever view is active.
Dirty drafts block leaving with an unsaved prompt (API tab switch, **Back**, or
re-clicking the active Providers menu). Internal `selected` fields and translation
keys remain unchanged: "added" names saved list membership, while checkbox actions
still use selection terminology.

Both lists offer **Add a model manually** as a plus icon on the right of the models toolbar. It explicitly saves
and selects one manual row through the existing Rust command (alias and
capability declarations included); it does not stage an
upstream checkbox draft, and pending checkbox drafts block creation until saved or
discarded. The manual row is removed by unchecking it and using the normal
selection **Save**. Route support still comes from the provider's own route
declaration; an alias is only a display label. Capability icons show a capability
only when it is effectively supported, from either the vendor catalog's evidence
or the user's own declaration (see
[models.md](models.md)); a missing icon never
claims that the model lacks the ability. Every icon names its source (catalog or
user declaration) in its accessible label.

The Providers list row has a destructive **Delete** action
(`ProviderDeleteAction`) that opens a confirmation dialog naming the provider and
its added-model count. Deletion removes the configuration, its saved models and
its stored key; a stale confirmation fails without touching the credential store,
a definite database failure restores the key, and an unknown outcome reloads the
list. Agent configuration files are never read or written by this action.

Each added row has an **Edit model** action that expands an inline editor under
the row; **Add a model manually** expands the same `ModelEntryForm` in create
mode. In both modes the first row holds Model ID (read-only when editing; model
identities are never renamed) and the alias field, with the actions aligned on
the right; the second row holds the three capability checkboxes (thinking, image
input, tool calls) side by side, using the shared `ModelCheckbox` style.
Creation saves a manual row through `add_manual_provider_model` and can declare
capabilities immediately; editing updates alias and declarations through
`update_provider_model_edit`, behind a Rust content digest with `model_changed`
on stale content. Every checkbox interaction is a user declaration ("supported"
or "not supported") that overrides the catalog evidence in both directions;
untouched fields keep the catalog result or unknown, and **Restore default**
clears all three declarations and disables itself when nothing is declared.
Declarations live in their own stored field, so a catalog refresh replaces
catalog evidence without touching them. An unknown write outcome is never
reported as saved: creation verifies by exact model ID (**Check saved models**),
editing requires an explicit reload that replaces the local draft. Escape
collapses an open inline editor from its text fields, and focus returns to the row
action that opened it; hiding the section closes the editor. The open editor
blocks competing model operations through the page's busy state and never
changes checkboxes or saved membership. The form carries no per-capability hint
text or source/state labels; the row icons name the source.

Rust validates submitted values: field codes appear next to their field and other
codes at form level. Successful creation clears the form and extends the saved
baseline, row list and tab count without a provider network request. Pending
manual writes block fetching, checkbox saves and the detail-page Back action.
Hiding the view unmounts the form; late replies are ignored, and the saved-list
read reconciles persisted state on return. An unknown write acknowledgment blocks
a second submission until **Check saved models** reads cursor pages for the exact
ID; similar IDs never confirm a write, and a failed read leaves the check
retryable. Cursor reads also support valid IDs longer than the fuzzy-search limit.
After a successful save the page returns to the list, the row shows the values
Rust stored, and the list is re-read, with an app-level notification
(`Saved name.` / 「已保存 name。」, see `Notifications`) surviving the jump back.

- Hierarchy: each view has one main primary action ("New" / 「新建」 on Providers
  and MCP lists, "Discover" / 「发现」 on Agents, or "Save" in the secondary-page
  header): solid `--color-accent` with `--color-on-accent` text. Recovery actions
  and "Load more" are secondary; destructive row actions (for example MCP
  "Remove") use `dangerSecondary`; row "Edit" is a text button. List "New",
  Agents "Discover" and row "Edit" carry a leading `Icon`. Blocked buttons use
  `aria-disabled` styling rather than `disabled`, so focus is never dropped.
- Focus: opening a secondary view focuses its title; Back returns focus to "New"
  or that row's "Edit"; a successful save focuses the saved row's "Edit" (found
  by ID), or the page heading when a new row is not on the loaded pages. Controls
  that can hold focus are never `disabled` while work is pending: buttons and
  selects use `aria-disabled`, text fields `readOnly`, and handlers check the
  state. Back is blocked while API save or model fetch/list work is pending, and
  "Saving…" is announced through a `role="status"` message.

- `operation_failed` or `invalid_response` after a save means the outcome is
  unknown. The form re-reads the list first and offers "Save again" only after a
  successful refresh, so a second click cannot silently create a duplicate.
- `revision_conflict` blocks saving until "Reload latest settings" loads the
  newer record for review.
- `create_outcome_unknown` (Rust could not confirm a create, and an unused key
  entry may remain in the credential store) follows the same refresh-first flow.
  The key field was cleared, so the text after the refresh says that "Save
  again" needs the key typed again; the key field stays editable in this state
  while the other fields are read-only.
- Browser preview shows that nothing can be read or saved, hides "New", and
  never calls IPC.

### API keys

New and edit forms both use an "API key" / 「API 密钥」 password field with
`autoComplete="new-password"`, spellchecking and automatic capitalization off.
The edit form reads `getProviderSecretStatus(id)` on mount (StrictMode may repeat
the read), which reads only SQLite metadata — not the key or the OS credential
store. A configured key appears as a fixed `••••••••` placeholder with an
accessible status text; the input value stays empty, and neither the mask nor any
stored key is fetched or submitted. Typing a replacement clears that status.

A blank edit field preserves the existing key; creation and legacy instances
without a key require input, and empty input produces a field error that moves
focus there. The frontend sends the value exactly as typed, and Rust trims and
validates it. Loading, failed status reads and browser preview block saves without
pretending a key exists. A failed read offers a retry, moving focus to the input
before the retry button disappears. Obsolete reads cannot replace a newer state or
update an unmounted form.

**One save.** `updateProvider` sends non-secret `request` fields and a separate
`secret` argument (`null` when omitted) to `update_provider`. Rust uses
`update_provider_with_secret`:

- With no replacement, update settings only; never access the credential store.
- With a replacement, validate settings, revision and key before OS access.
  Keep the old key temporarily in memory, write the new key, then update settings
  and the credential reference in one SQLite transaction. Recheck the revision
  after the OS call. No database lock spans a system credential-store prompt.
- A definite transaction failure restores the old key. A failed restoration or
  uncertain commit returns `operation_failed`, not success or a claimed rollback.
  The process-wide credential-write lock serializes writes within this app;
  separate app processes are not serialized.

A successful acknowledgment returns to the list and emits the existing saved
notification. Pending saves preserve focus with `readOnly` and
`aria-disabled`; duplicate submissions cannot start another write. Definite
errors retain draft settings but clear the typed key. Invalid keys appear beside
the field; credential-store errors appear at form level. Unknown outcomes
refresh before an explicit retry. If that attempt included a replacement key,
retry requires it to be typed again; reading metadata cannot confirm which key
the credential store holds. Revision conflicts require reloading the latest
settings for review.

Typed keys are cleared before every submit, when leaving the page, on unmount
and before a hidden frame renders. They are never passed to parent callbacks,
translations, notifications, logs or persistent frontend storage. IPC and OS
credential access necessarily receive the new value; SQLite and responses hold
only references and non-secret settings. The legacy standalone replacement
command and its safe desktop adapter remain available, but the provider interface
does not call them.

`ProviderForm.edit.test.tsx` and `ProviderForm.test.tsx` cover key replacement,
legacy instances, status retry, focus, uncertain outcomes, duplicate submits,
obsolete StrictMode reads and blank-field preservation; boundary tests verify the
single IPC payload, strict responses and sanitized errors. Rust
`providers::tests::unified_save` uses temporary databases and an injected
credential store to cover the commit, rollback, concurrent-edit and key-exclusion
paths described above.

Remaining credential-store, protocol/model inference and Windows/Linux runtime
acceptance is tracked in [`plans/todo.md`](plans/todo.md), not inferred from passing
automated tests.

## Product branding

`BrandLogo` renders the expanded V/wordmark in the sidebar and About, and only
the V in the collapsed rail. The brand-home button has no hover decoration and
retains keyboard focus. Its accessible name belongs to the button; the SVG is
decorative.

The V uses forest `#324e40` and sage `#a8b8a7`, with the reference's shallow lower
turn. The wordmark uses reference letter shapes through an SVG mask: a color
matrix removes the pale background and `currentColor` supplies text color.
The original i dot is cleared before drawing the raised orange `#db915b` circle.
The lettering is raster-derived; the V is vector artwork.

The expanded/collapsed exports are in `assets/brand/`. The app icon uses an
off-white tile. The macOS master has separate transparent padding, continuous
corners and a slight downward V offset, leaving other platform icons and sidebar
geometry unchanged. [Brand asset instructions](../assets/brand/README.md) record
the masters, dimensions and locked Tauri generation commands.

## Built-in color themes

General shows all eight themes as native radio circles, in this order:

| English  | 简体中文 | Persisted ID | Palette                       |
| -------- | -------- | ------------ | ----------------------------- |
| macOS    | macOS    | `macos`      | System blue, grouped surfaces |
| Windows  | Windows  | `windows`    | Fluent neutrals, accent blue  |
| Forest   | 森林     | `forest`     | Calm green                    |
| Ink      | 纸墨     | `notion`     | White, warm gray and charcoal |
| Graphite | 石墨     | `graphite`   | Neutral monochrome            |
| Linen    | 亚麻     | `linen`      | Warm paper and clay           |
| Iris     | 鸢尾     | `iris`       | Restrained violet             |
| Ocean    | 海湾     | `ocean`      | Cool blue                     |

Each theme has light/dark values in `src/themes.css`; `App.css` maps them to
shared semantic colors. Brightness is independent of theme. System brightness
uses `prefers-color-scheme` in CSS. Circles reuse palette accents, with translated
names, selected rings and checks. Native radios provide arrow-key selection;
forced-colors mode can show the native input. Brand V colors remain fixed.
`macos` and `windows` approximate platform system palettes (Apple HIG blue /
grouped grays; Fluent `#0078D4` / `#60CDFF`); they are not OS skins and do not
change control chrome beyond vibemate tokens.

Rust validates and atomically saves brightness/theme pairs through thin Tauri
commands that run SQLite work in a blocking worker, so database waits do not stall
the async executor. A missing row means system/forest without writing defaults.
Schema migrations that extend the theme CHECK constraint rebuild that owned table
in a transaction, and are tested to preserve prior themes, language and unrelated
rows while rejecting unknown themes and rolling back an invalid prior row without
data loss.

The persisted `notion` ID remains unchanged after renaming the display label to
Ink / 纸墨. Its neutral colors refer to Notion's [official public stylesheet](https://www.notion.so/_assets/77281-c4b3c46f690b55b2.css)
linked from the [login page](https://www.notion.so/login), observed 2026-10-10.
Hover, selection and focus/accent values are adapted to vibemate. This is a color
reference, not an integration with Notion.

`AppearanceControl` applies root attributes only after an exact saved-pair
acknowledgment. Saves disable controls without showing temporary saving or
success text. Known failed writes keep the previous choice and permit retry;
uncertain saves block writes until reload. Read/loading, preview, error and
reload feedback remains. Browser preview applies local colors without claiming
they were saved. Cleanup restores root attributes; navigation retains mounted
controls, pending operations and edited input.

## Appearance acceptance and verification

The maintainer accepted the expressive-palette shell/branding iteration, the
Select migration and the macOS/Windows system-style palettes (2026-10-13).
Windows/Linux icon and runtime behavior, system-browser opening, native restart
behavior where unrecorded and comprehensive accessibility acceptance remain
release-time checks.

Historical test counts, token contrast measurements and packaging results are in
Git/CI. They are scoped observations, not current full-platform or accessibility
claims. Visual checking follows the maintainer's manual-review policy; formatting,
lint, type, behavior and applicable build checks remain required.

## Central MCP management

The central list also offers a separate read-only Grok Build preview through
`McpGrokPreview` and `desktop/grokMcpPreview.ts`. Rust selects the user TOML target;
the client validates an exact Zod response, matching identity/revision and
placeholder-only env/header values. A field summary and nested JSON view of the
proposed TOML table distinguish the personal disable list from the proposed native
flag. Retry/list reload, per-attempt cleanup, hidden-dialog invalidation and
focus return mirror existing read-only review behavior without sharing Pi's
runtime semantics or exposing an apply action. Language switches preserve the
open review and do not trigger another file read. At narrow desktop widths MCP
rows put the action group below the identity so the additional preview control
does not squeeze long names. No capability/CSP changes were required.

`src/features/mcp/` implements central MCP metadata. The sidebar opens a metadata
list lazily on first visit. New/edit pages collect display name (the server
identifier is derived on first save and never displayed in either form; edits
preserve it), transport, stdio executable/ordered argument rows/optional cwd, or
Streamable HTTP URL. Environment variables and request headers use named password
rows. An empty existing value preserves its OS reference; a replacement writes a
fresh reference; removing a row removes that field. Each argument row is one
literal value without shell quoting. Saving only stores a definition, and
enable/disable describes central state — never an applied Agent config or
connection.

MCP list rows report recorded Pi registrations in the active Pi directory,
not discovered Agents or live connections. `usePiMcpRegistrations` finishes all
bounded root pages before showing a complete summary; unreadable/partial results
cannot become a false unregistered state. Visibility/epoch changes invalidate
old requests. The Pi Agent MCP slot reuses this read-only observation and links
to central management. Other Agents explicitly remain unimplemented.

`McpPiManager` owns deployment/update, native enable/disable, revoke, history and
interruption review. Each file mutation first obtains a sanitized plan, then
requires a separate confirmation; cancelling never applies. `PiMcpReview` reuses
the existing field-summary renderer and shows separate masked entry details.
Close stays outside the scrollable body and nested Escape restores focus.
Closing/hiding/retrying ignores late reads. Confirmed writes cannot be dismissed
or double-submitted: the parent keeps its busy guard even if hiding unmounts the
dialog, and settlement requests a fresh observation. Uncertain writes discard
their approval and read recovery state instead of blindly retrying.

History pages retain the selected source identity. Recovery has its own
before/intended observation review and explicit metadata-only confirmation;
divergence has no confirm action. The delete dialog's `McpDeleteReferences`
reads references across Pi directories before enabling delete; Rust still makes
the authoritative transactional check.

`src/lib/desktop/piMcp.ts` validates strict state/plan/history/result shapes,
identity/revision/action, cursors/order, placeholder-only env/headers and protected
summary values. Raw errors, native bytes and credential values are discarded.
Browser mode refuses native operations rather than simulating successful writes.
Rust selects native paths and rechecks approvals; React sends no paths or native
documents. No capabilities or CSP permissions were added.

`McpForm` clears typed values on every submit, cancel, transport change and hide,
and its hidden transition clears during rendering so a hidden panel cannot retain
values through a rapid navigation change. Non-secret drafts stay mounted while
hidden, but successful sidebar home navigation closes the editor. Unknown write
outcomes block another mutation until a successful metadata reload. The list
supports bounded cursor pages, safe retry, and obsolete credential cleanup.
Deleting a definition uses a Base UI confirmation dialog with safe initial focus
and focus restoration. Pending deletion blocks dismissal, navigation and duplicate
submissions. Known failures keep the row; conflicts and unknown outcomes require
a successful list reload. Deleted credential cleanup remains visible and retryable
after restart even when no definitions remain.
Unmount guards ignore late responses; page transitions focus the heading.
`src/lib/desktop/mcp.ts` validates exact metadata shapes and cursor ordering,
rejects unexpected secret-bearing response fields, discards raw errors, and
blocks mutations in browser preview. Rust is authoritative for field validation.
All env/header values use OS credential storage; frontend persistent storage is
never used. Commands run blocking storage work off the async executor, reuse the
credential mutation lock and add no capabilities or dependencies.

MCP new and edit reuse the left-aligned `ProviderPage` header, the shared
edit-form rows, and `FieldSelect`. Save feedback stays in the form; reload and
Save actions publish to the header. Pending saves change the button label without
replacing the button or losing focus. The main-column scroll override applies
only to visible secondary pages (Provider create/edit, MCP create/edit and Agent
details). Lists, Settings, Overview and Skills keep their previous scrolling.

Recorded isolated Linux Tauri checks covered no-secret stdio save/edit/restart
without executing its test command. Earlier central-definition credential/native/
visual acceptance passed maintainer review; that does not cover Pi deployment.
A new isolated macOS Tauri WebView run exercised nine real-IPC lifecycle checks:
deploy, enable, disable, revoke, retained-entry restore, reference protection,
Agent summary and final central deletion, with temporary HOME/Pi/app data and no
credential fields or server execution. After explicit approval, another nine
installed Pi CLI probes verified actual local stdio/loopback HTTP discovery,
disable, selected-result restore and revoke following native UI writes, without
Provider credentials or external requests. Browser mocks separately checked both
languages/themes, 720×560, long data and nested keyboard/recovery behavior.
OS-credential deployment and Windows/Linux remain separate acceptance checks;
builds and behavior tests do not prove visual accessibility.

## Read-only Agent discovery

`AgentList` mounts on the first Agents visit, then remains mounted while hidden.
The fixed catalog lists Claude Code, Codex, OpenCode, Pi, Grok Build and Hermes
in that order, including absent entries, loading, errors and browser preview.
Discovery is automatic on first visit and explicit through **Discover** / 「发现」
or detail rediscovery; it never launches an Agent.

Cards align status and version to opposite ends of one row. A decorative dot
supplements accessible status text: found entries use the success token, absent
entries the muted summary token, failed checks and unavailable entries the danger
token. Failed refreshes cannot retain a green success dot. Each card opens
entry/status/version, configuration-presence and window-local manual-path details,
and the six independent paths survive navigation and language changes. Fields
become read-only during a scan; `aria-disabled`, immediate pending guards,
generations and mount checks prevent duplicate work and stale replies. Failed
refreshes label retained observations as previous results; initial failures and
browser preview do not pretend entries are absent. Returning restores card focus.

Version provenance and documented capabilities remain separate from authenticity,
connection and deployment support. Rust owns bounded filesystem observations;
`src/lib/desktop/agents.ts` validates all six identities, file lists and evidence.
Configuration presence and precedence are not effective runtime settings.

All six entries have locally bundled identification marks; official sources and
usage boundaries are in [assets/agents/README.md](../assets/agents/README.md).
No marks are recolored or fetched from third-party servers at runtime, and
trademark ownership is not covered by vibemate's MIT license.

Browser checks cover the six-card display, loaded light/dark assets and
keyboard-opened detail at a narrow window width. Simulated IPC samples establish
neither native IPC nor real-installation acceptance; these are scoped checks, not
full accessibility or maintainer acceptance. Current filesystem rules, sources
and native verification boundaries are in
[agents.md](agents.md); real installations, native
cross-platform behavior and maintainer visual acceptance remain separate.

### Confirmed CLI version probing

`AgentVersionPanel` provides optional **Read version** / 「读取版本」. Showing the
fixed command and risk does not execute it; a separate confirmation calls
`src/lib/desktop/agentVersions.ts`, which validates the response and discards raw
errors. Only a current found entry with an unchanged path is eligible; browser
preview, unavailable entries and edited paths are blocked. The panel ignores
hidden/unmounted replies and prevents duplicate requests. Execution blocks page
rediscovery and Back; after launch there is no UI cancellation API, and Rust
independently bounds the direct child.

CLI results have separate `cliOutput` provenance, remain window-local and clear
on rediscovery. They do not upgrade configuration capabilities. The selected
program may still read credentials, access the network, write files or launch
other processes; the protection is not a sandbox. Native execution limits,
official command sources and the verification scope are in
[agents.md](agents.md#explicit-version-probing).

### Automatic version probing

`AgentVersionSettings` on the Settings General tab holds a single toggle
(**自动读取 Agent 版本** / **Read Agent versions automatically**). The persisted
boolean is one-time informed consent: when enabled, `AgentList` launches the
fixed `--version` probe without a per-run click. After each discovery scan,
found Agents are probed sequentially; opening a detail page probes only when
no cached result matches the current path fingerprint. Each (entry, resolved)
pair is probed at most once per session. `refresh()` clears the cache and
`probedRef` set so the next scan re-probes. Automatic failures are silent;
the manual button stays available. Results live in the same window-local
`versions` map and are never written to disk.

## Agent model configuration

`AgentList` shows the saved Provider and model for each write-capable Agent as
plain secondary text reading `Provider · Model` (for example
`Command Code GOAT · deepseek/deepseek-v4.1-flash`), truncated with an ellipsis
because the row gives it one line. There is no chip or badge around it. This is
vibemate's own presentation: the `name` field written into an Agent file reads
`(vibemate-<Provider>) <model ID>`, so the two formats are deliberately
independent. The row reads the model each Agent currently names in its live
configuration, independent of history matching: Claude Code's
`env.ANTHROPIC_MODEL` (then `model`), Pi's `defaultModel`/`defaultProvider`, and
Grok Build's `[models].default` alias. When that model resolves to a still-saved
vibemate Provider/model pair the row shows `Provider · Model`; any other readable
configuration reads **Own configuration** (`自有配置`); an Agent that was not
discovered, or whose files could not be read safely, keeps **No configuration
summary**. Rows have no hover state: only the Write and Detail buttons are
interactive.

`ConfigPreview` serves Claude Code, Pi and Grok Build through their typed desktop
adapters and opens from the list's **Write** icon button in a compact dialog. Pi
previews both `models.json` and `settings.json` and binds the read-only
`auth.json` precedence token. Grok Build changes one TOML document, so its card
lists only the planned `[model.<alias>]` fields and the higher-precedence files
that were located but never read. It deliberately leaves `[models].default`
alone: Grok's own picker writes that user preference, and vibemate shows its last
recorded alias write instead of reading the default. Each adapter keeps its own
restart and outcome sentences, because "written" means a different set of files in
each one. Applying consumes the reviewed preview, reports per-file backups and keeps
navigation and other configuration actions blocked while the native operation is
pending. After a preview is generated, `ModelPlanPresentation` renders the field
summary and detail entry inline in the same dialog; it follows the shared
two-layer rules in [configuration.md](configuration.md). A successful apply
notifies, closes the dialog and refreshes the list's current-configuration pin.

Claude Code's effort field uses `useClaudeEffortPreference` to load a reusable
starting choice for the selected Provider/model. The model and preference come
from one native SQLite snapshot; model capability defaults never select a value.
`ClaudeEffortPreferencePanel` separates **Remember choice** from Preview and Apply.
Remembering sends both the model digest and preference revision, clears this
window's displayed plan and changes no Agent file. Native apply/restore does not
silently save a starting preference. The original global env scope warning stays
visible: remembering separately in vibemate does not isolate native sessions.

Pi's thinking field (`PiThinkingLevelField`) has no remembered copy. Its choice is
the native `modelThinkingLevels` entry: a draft that any provider or model change clears,
and that is written only through preview and apply. `availablePiThinkingLevels` offers
declared levels that Pi accepts by default, plus the verified DeepSeek
`deepseek-flash` Chat `xhigh/max` mappings for the official endpoint. Existing
user map entries are never overwritten. A draft left over from before a metadata
refresh that no longer offers it is shown as unavailable and blocks the preview.

Pi plans render `PiModelFacts` in the plan card, above the file changes. Rust sends one
`modelFacts` row per setting (`contextWindow`, `maxTokens`, `reasoning`, `input` and the
chosen thinking level), each with its source and what the plan does with it. The
component translates the known tokens, formats token counts for the active locale and
adds three fixed notes. It never recomputes a value, and `pi.ts` rejects a list that
differs from Rust's fixed order or disagrees with its own state.

Agent model configuration consumes Provider-model definitions; it is not a
second model editor. No temperature/top_p or request-output fields are offered.
Thinking selectors choose an injection default from declared model levels;
Pi's verified mappings are injected independently so choosing one default does
not prevent native switching to another mapped level. Existing remembered Claude
choices are starting preferences only. Grok's `GrokModelFacts` shares
`ModelFacts.module.css` with Pi and reports typed context source/write state,
not arbitrary preserved TOML; Grok effort-array injection is still pending.

The hook keeps the draft on save failure. Read errors require reload rather than
pretending inheritance; conflicts and uncertain acknowledgments also offer
explicit reload. An obsolete remembered effort stays visible as unmapped and
blocks preview until the user changes it. Render-time identity checks hide a
previous binding's state. A guarded state reset during render also discards the
old draft when returning to a binding before an intervening read finishes; a fresh
read must finish before preview becomes available. Effect generations invalidate
late reads and saves on switching, retry or unmount. A generation is a counter identifying the current
request; a completion with an older counter cannot replace current state. Save
shares the configuration panel's single-flight and navigation guard, while a
pending read still permits switching bindings. `src/lib/desktop/claudePreference.ts`
validates the response and reuses the model snapshot validator, without storing
anything in the browser. Pi and Grok do not call this Claude-only API.

The Agent detail page shows History, Skill, MCP and Install & paths segments. The
shared `RestoreView` lists only the selected Agent's recorded writes from the
root-scoped paginated history, choosing its commands from one per-Agent table; the
backend independently checks operation ownership before previewing or restoring any
target. It pins the separately loaded matched version once above chronological
apply-only rows, even outside the current seek page. Restore/compensation batches
remain internal audit records. Expanded rows preview the selected apply's written
result from its independent snapshots, not its pre-write backups. The view loads
older versions by cursor and offers selection, resolution review and confirmed
version-only deletion. The
pin's **Detailed configuration** dialog reads and sanitizes the live file directly,
so it stays enabled after an external edit no longer matches a retained version;
the taller dialog shows only the current text, without a current-configuration
heading or original-size row.
Interrupted mutations set `requiresResolution` on a history row, withholding
configuration selection until the interrupted operation is reviewed. `src/lib/desktop/recovery.ts` validates metadata-only reviews
and sends only the fixed Agent identity, operation ID and complete backend plan's
approval digest. `ResolutionReviewCard` displays the observations and backend
eligibility; confirmation records observations only, discards approval and refreshes
history. It never chains a configuration restore. Stale/blocked observations require
fresh inspection; all actions share the existing navigation and single-flight guard.
The Agent detail keys this view by Agent/configuration epoch, so changing its scope
unmounts it and late replies cannot update the new view.
Provider/model selections stay local, and browser preview never invents a plan or a
completed write.
