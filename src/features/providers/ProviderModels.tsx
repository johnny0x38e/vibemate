import {
    useCallback,
    useEffect,
    useMemo,
    useRef,
    useState,
    type JSX,
    type ChangeEvent,
} from "react";
import { useTranslation } from "react-i18next";
import type { TFunction } from "i18next";
import { useNotify } from "../../components/Notifications";
import {
    browseUpstreamModelsPage,
    cancelProviderModelFetch,
    clearModelReference,
    getProviderModelFetchStatus,
    listProviderModels,
    ModelRequestError,
    modelDeletionImpact,
    refreshSavedProviderModels,
    saveProviderModelSelections,
    type ModelDeletionImpact,
    type ModelErrorCode,
    type ProviderModel,
    type UpstreamBrowseModel,
} from "../../lib/desktop/models";
import type { AgentReference } from "../../lib/desktop/providers";
import { getProviderSecretStatus } from "../../lib/desktop/providerSecrets";
import { IconButton } from "../../components/IconButton";
import { ModelEntryForm } from "./ModelEntryForm";
import { ModelCapabilityIcons } from "./ModelCapabilityIcons";
import { ModelLimitMarks } from "./ModelLimitMarks";
import { NO_CAPABILITY_DECLARATIONS } from "./modelCapabilities";
import { ModelCheckbox } from "../../components/ModelCheckbox";
import { CodexReferenceRows } from "./CodexReferenceRows";
import buttons from "../../components/actionButtons.module.css";
import editForm from "../../components/editForm.module.css";
import styles from "./ProviderModels.module.css";
import { SecondaryPageActions } from "../../components/SecondaryPageActions";

/** Search results cap; fuzzy search returns at most this many rows per query. */
export const MODEL_PAGE_SIZE = 50;

/** Matches Rust `MAX_MODEL_PAGE_SIZE`; internal chunks when reading all selected rows. */
const SAVED_CATALOG_CHUNK = 200;

/** Selected rows for tab labels and quick filters. */
export interface SelectedModelCount {
    count: number;
    hasMore: boolean;
}

const SEARCH_DEBOUNCE_MS = 300;

type ViewMode = "selected" | "upstream";

type ListState =
    | { kind: "loading" }
    | { kind: "preview" }
    | { kind: "error"; code: ModelErrorCode }
    | {
          kind: "ready";
          items: ProviderModel[];
          totalMatches: number | null;
      };

type UpstreamListState =
    | { kind: "idle" }
    | { kind: "loading" }
    | { kind: "error"; code: ModelErrorCode }
    | { kind: "ready"; items: UpstreamBrowseModel[] };

type KeyState =
    | { kind: "loading" }
    | { kind: "preview" }
    | { kind: "error"; code: ModelErrorCode }
    | {
          kind: "ready";
          configured: boolean;
      };

type SaveState =
    | { kind: "idle" }
    | { kind: "running" }
    | { kind: "error"; code: ModelErrorCode };

type RefreshState =
    | { kind: "idle" }
    | { kind: "running" }
    | { kind: "error"; code: ModelErrorCode };

/** Inputs for {@link ProviderModels}. */
export interface ProviderModelsProps {
    providerId: string;
    hidden?: boolean;
    layout?: "section" | "tabPanel";
    onBusyChange?: (busy: boolean) => void;
    onDirtyChange?: (dirty: boolean) => void;
    onSelectedCountChange?: (count: SelectedModelCount | null) => void;
}

/** Any Agent or Codex row keeps the model from being removed. */
function blocksRemoval(impact: ModelDeletionImpact): boolean {
    return (
        impact.references.length > 0 ||
        (impact.codexReferences?.length ?? 0) > 0
    );
}

function errorCode(error: unknown): ModelErrorCode {
    if (error instanceof ModelRequestError) return error.code;
    return "operation_failed";
}

function mergeModels(
    existing: ProviderModel[],
    incoming: ProviderModel[],
): ProviderModel[] {
    const seen = new Set(existing.map((model) => model.modelId));
    const appended = incoming.filter((model) => !seen.has(model.modelId));
    return [...existing, ...appended];
}

function mergeUpstream(
    existing: UpstreamBrowseModel[],
    incoming: UpstreamBrowseModel[],
): UpstreamBrowseModel[] {
    const seen = new Set(existing.map((model) => model.modelId));
    const appended = incoming.filter((model) => !seen.has(model.modelId));
    return [...existing, ...appended];
}

function setsEqual(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
    if (a.size !== b.size) return false;
    for (const id of a) {
        if (!b.has(id)) return false;
    }
    return true;
}

type SelectedCatalogLoad =
    | { kind: "preview" }
    | { kind: "desktop"; items: ProviderModel[]; ids: Set<string> };

/** Read every persisted selected row (paged in Rust, one list in the UI). */
async function listAllSelected(
    providerId: string,
): Promise<SelectedCatalogLoad> {
    let after: string | null = null;
    let items: ProviderModel[] = [];
    for (;;) {
        const chunk = await listProviderModels({
            providerId,
            after,
            limit: SAVED_CATALOG_CHUNK,
            filter: "selected",
        });
        if (chunk.kind === "preview") {
            return { kind: "preview" };
        }
        items = mergeModels(items, chunk.page.items);
        after = chunk.page.nextCursor;
        if (after === null) break;
    }
    return {
        kind: "desktop",
        items,
        ids: new Set(items.map((model) => model.modelId)),
    };
}

function selectionBlocked(
    routeSupport: ProviderModel["routeSupport"],
    source?: ProviderModel["source"],
): boolean {
    if (source === "manual") return false;
    return routeSupport === "unsupported";
}

function displayTitle(model: {
    modelId: string;
    alias?: string | null;
    upstreamName?: string | null;
}): string {
    const trimmed = model.alias?.trim();
    if (trimmed) return trimmed;
    if (model.upstreamName?.trim()) return model.upstreamName.trim();
    return model.modelId;
}

function formatMissingDate(ms: number, locale: string): string {
    return new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(
        new Date(ms),
    );
}

/** One centered footer line under the list (selected end, upstream scroll/load/end). */
function resolveEndOfListMessage(
    t: TFunction,
    viewMode: ViewMode,
    list: ListState,
    debouncedQuery: string,
    upstream: UpstreamListState,
    upstreamMorePages: boolean,
    browseActive: boolean,
): string | null {
    if (viewMode === "selected") {
        if (
            list.kind !== "ready" ||
            debouncedQuery !== "" ||
            list.items.length === 0
        ) {
            return null;
        }
        return t("providers.models.allSelectedLoaded");
    }
    if (upstream.kind !== "ready" || upstream.items.length === 0) {
        return null;
    }
    if (upstreamMorePages) {
        if (browseActive) {
            return t("providers.models.upstreamLoadingMore");
        }
        return t("providers.models.upstreamScrollForMore");
    }
    return t("providers.models.upstreamAllLoaded");
}

/**
 * Models tab (P12.c.5): selected list by default, upstream browse via Fetch models,
 * draft checkboxes until Save.
 */
export function ProviderModels({
    providerId,
    hidden = false,
    layout = "section",
    onBusyChange,
    onDirtyChange,
    onSelectedCountChange,
}: ProviderModelsProps): JSX.Element {
    const { t, i18n } = useTranslation();
    const notify = useNotify();
    const [viewMode, setViewMode] = useState<ViewMode>("selected");
    const [searchInput, setSearchInput] = useState("");
    const [debouncedQuery, setDebouncedQuery] = useState("");
    const [list, setList] = useState<ListState>({ kind: "loading" });
    const [upstream, setUpstream] = useState<UpstreamListState>({
        kind: "idle",
    });
    const [upstreamNextOffset, setUpstreamNextOffset] = useState<number | null>(
        null,
    );
    const [upstreamMorePages, setUpstreamMorePages] = useState(false);
    const [baselineSelected, setBaselineSelected] = useState<
        ReadonlySet<string>
    >(() => new Set());
    const [draftSelected, setDraftSelected] = useState<ReadonlySet<string>>(
        () => new Set(),
    );
    const [keyState, setKeyState] = useState<KeyState>({ kind: "loading" });
    const [browseRunning, setBrowseRunning] = useState(false);
    const [serverBrowseRunning, setServerBrowseRunning] = useState(false);
    const [manualBusy, setManualBusy] = useState(false);
    const [editBusy, setEditBusy] = useState(false);
    // At most one inline model editor is open, on one saved row.
    const [editor, setEditor] = useState<
        { kind: "create" } | { kind: "edit"; modelId: string } | null
    >(null);
    const [refreshState, setRefreshState] = useState<RefreshState>({
        kind: "idle",
    });
    const refreshGeneration = useRef(0);
    const searchField = useRef<HTMLInputElement>(null);
    const [saveState, setSaveState] = useState<SaveState>({ kind: "idle" });
    /** Removals stopped by Agent ownership rows, keyed by model ID. */
    const [blockedRemovals, setBlockedRemovals] = useState<
        ModelDeletionImpact[]
    >([]);
    const [clearingReference, setClearingReference] = useState<string | null>(
        null,
    );
    const [referenceError, setReferenceError] = useState<ModelErrorCode | null>(
        null,
    );

    const loadGeneration = useRef(0);
    const browseGeneration = useRef(0);
    const listScrollRef = useRef<HTMLDivElement>(null);
    const loadMoreSentinelRef = useRef<HTMLDivElement>(null);
    const upstreamLoadingMore = useRef(false);
    /** Last upstream search query we already started a full reload for. */
    const upstreamSearchQueryRef = useRef(debouncedQuery);

    const dirty = useMemo(
        () => !setsEqual(baselineSelected, draftSelected),
        [baselineSelected, draftSelected],
    );
    const dirtyRef = useRef(dirty);
    useEffect(() => {
        dirtyRef.current = dirty;
    }, [dirty]);

    // Tab badge count is useful while the Models panel is still hidden (API tab).
    const refreshSelectedCount = useCallback(async (): Promise<void> => {
        try {
            const loaded = await listAllSelected(providerId);
            if (loaded.kind === "preview") {
                onSelectedCountChange?.(null);
                return;
            }
            const next: SelectedModelCount = {
                count: loaded.ids.size,
                hasMore: false,
            };
            onSelectedCountChange?.(next);
        } catch {
            onSelectedCountChange?.(null);
        }
    }, [providerId, onSelectedCountChange]);

    useEffect(() => {
        const handle = window.setTimeout(() => {
            setDebouncedQuery(searchInput.trim());
        }, SEARCH_DEBOUNCE_MS);
        return () => {
            clearTimeout(handle);
        };
    }, [searchInput]);

    useEffect(() => {
        onDirtyChange?.(!hidden && dirty);
    }, [onDirtyChange, dirty, hidden]);

    const loadKeyStatus = useCallback(async (): Promise<void> => {
        try {
            const result = await getProviderSecretStatus(providerId);
            if (hidden) return;
            if (result.kind === "preview") {
                setKeyState({ kind: "preview" });
                return;
            }
            setKeyState({
                kind: "ready",
                configured: result.status.state === "set",
            });
        } catch {
            if (hidden) return;
            setKeyState({ kind: "ready", configured: false });
        }
    }, [providerId, hidden]);

    const syncBaselineFromLoaded = useCallback((ids: ReadonlySet<string>) => {
        setBaselineSelected(ids);
        setDraftSelected(new Set(ids));
    }, []);

    const loadSelectedList = useCallback(async (): Promise<void> => {
        const generation = ++loadGeneration.current;
        setList({ kind: "loading" });
        try {
            if (debouncedQuery) {
                const result = await listProviderModels({
                    providerId,
                    after: null,
                    limit: MODEL_PAGE_SIZE,
                    filter: "selected",
                    query: debouncedQuery,
                });
                if (hidden || generation !== loadGeneration.current) return;
                if (result.kind === "preview") {
                    setList({ kind: "preview" });
                    return;
                }
                setList({
                    kind: "ready",
                    items: result.page.items,
                    totalMatches: result.page.totalMatches,
                });
                return;
            }
            const loaded = await listAllSelected(providerId);
            if (hidden || generation !== loadGeneration.current) return;
            if (loaded.kind === "preview") {
                setList({ kind: "preview" });
                return;
            }
            if (!dirtyRef.current) {
                syncBaselineFromLoaded(loaded.ids);
            }
            setList({
                kind: "ready",
                items: loaded.items,
                totalMatches: null,
            });
            void refreshSelectedCount();
        } catch (error: unknown) {
            if (hidden || generation !== loadGeneration.current) return;
            setList({ kind: "error", code: errorCode(error) });
        }
    }, [
        providerId,
        debouncedQuery,
        hidden,
        refreshSelectedCount,
        syncBaselineFromLoaded,
    ]);

    const refreshBrowseRunning = useCallback(async (): Promise<void> => {
        try {
            const result = await getProviderModelFetchStatus(providerId);
            if (hidden) return;
            if (result.kind === "desktop") {
                setServerBrowseRunning(result.status.running);
            }
        } catch {
            if (hidden) return;
            setServerBrowseRunning(false);
        }
    }, [providerId, hidden]);

    const loadUpstreamPage = useCallback(
        async (offset: number | null, append: boolean): Promise<void> => {
            const generation = ++browseGeneration.current;
            const query =
                debouncedQuery.length > 0 ? debouncedQuery : undefined;
            if (!append) {
                setUpstream({ kind: "loading" });
            }
            setBrowseRunning(true);
            try {
                const page = await browseUpstreamModelsPage({
                    providerId,
                    offset,
                    ...(query !== undefined ? { query } : {}),
                });
                if (hidden || generation !== browseGeneration.current) return;
                setUpstream((current) => {
                    const previous =
                        append && current.kind === "ready" ? current.items : [];
                    return {
                        kind: "ready",
                        items: mergeUpstream(previous, page.items),
                    };
                });
                setUpstreamNextOffset(page.nextOffset);
                setUpstreamMorePages(page.morePages);
            } catch (error: unknown) {
                if (hidden || generation !== browseGeneration.current) return;
                if (!append) {
                    setUpstream({ kind: "error", code: errorCode(error) });
                } else {
                    notify({
                        tone: "error",
                        message: t(
                            `providers.models.errors.${errorCode(error)}`,
                        ),
                    });
                }
            } finally {
                if (generation === browseGeneration.current) {
                    setBrowseRunning(false);
                    void refreshBrowseRunning();
                }
            }
        },
        [providerId, hidden, notify, t, refreshBrowseRunning, debouncedQuery],
    );

    useEffect(() => {
        if (hidden || viewMode !== "upstream") return;
        if (upstream.kind === "idle") {
            upstreamSearchQueryRef.current = debouncedQuery;
            return;
        }
        if (upstreamSearchQueryRef.current === debouncedQuery) return;
        upstreamSearchQueryRef.current = debouncedQuery;
        setUpstreamNextOffset(null);
        setUpstreamMorePages(false);
        const handle = window.setTimeout(() => {
            void loadUpstreamPage(null, false);
        }, 0);
        return () => {
            clearTimeout(handle);
        };
    }, [hidden, viewMode, debouncedQuery, upstream.kind, loadUpstreamPage]);

    // Tab badge: always keep the selected-model count fresh for the inactive API tab.
    useEffect(() => {
        const handle = window.setTimeout(() => {
            void refreshSelectedCount();
        }, 0);
        return () => {
            clearTimeout(handle);
        };
    }, [providerId, refreshSelectedCount]);

    useEffect(() => {
        if (hidden) {
            loadGeneration.current += 1;
            browseGeneration.current += 1;
            void cancelProviderModelFetch(providerId);
            return;
        }
        const handle = window.setTimeout(() => {
            void loadKeyStatus();
            void refreshBrowseRunning();
        }, 0);
        return () => {
            clearTimeout(handle);
        };
    }, [hidden, providerId, loadKeyStatus, refreshBrowseRunning]);

    useEffect(() => {
        if (hidden || viewMode !== "selected") return;
        // Defer so the effect body does not synchronously enter list loading state.
        const handle = window.setTimeout(() => {
            void loadSelectedList();
        }, 0);
        return () => {
            clearTimeout(handle);
        };
    }, [hidden, viewMode, loadSelectedList]);

    useEffect(() => {
        if (
            hidden ||
            viewMode !== "upstream" ||
            upstream.kind !== "ready" ||
            !upstreamMorePages ||
            upstreamNextOffset === null
        ) {
            return;
        }
        const root = listScrollRef.current;
        const sentinel = loadMoreSentinelRef.current;
        if (root === null || sentinel === null) return;

        const observer = new IntersectionObserver(
            (entries) => {
                const visible = entries.some((entry) => entry.isIntersecting);
                if (
                    !visible ||
                    upstreamLoadingMore.current ||
                    browseRunning ||
                    serverBrowseRunning
                ) {
                    return;
                }
                upstreamLoadingMore.current = true;
                void loadUpstreamPage(upstreamNextOffset, true).finally(() => {
                    upstreamLoadingMore.current = false;
                });
            },
            { root, rootMargin: "120px" },
        );
        observer.observe(sentinel);
        return () => {
            observer.disconnect();
        };
    }, [
        hidden,
        viewMode,
        upstream,
        upstreamMorePages,
        upstreamNextOffset,
        browseRunning,
        serverBrowseRunning,
        loadUpstreamPage,
    ]);

    async function startUpstreamBrowse(): Promise<void> {
        if (browseRunning || serverBrowseRunning) return;
        const resetDraft = new Set(baselineSelected);
        setDraftSelected(resetDraft);
        setViewMode("upstream");
        setSearchInput("");
        setDebouncedQuery("");
        setUpstreamNextOffset(null);
        setUpstreamMorePages(false);
        upstreamSearchQueryRef.current = "";
        await loadUpstreamPage(null, false);
    }

    async function runCancelBrowse(): Promise<void> {
        try {
            await cancelProviderModelFetch(providerId);
        } catch {
            // Best-effort; status refresh shows the truth.
        }
        setBrowseRunning(false);
        await refreshBrowseRunning();
    }

    function toggleDraft(modelId: string, nextSelected: boolean): void {
        setBlockedRemovals([]);
        setReferenceError(null);
        setDraftSelected((current) => {
            const next = new Set(current);
            if (nextSelected) next.add(modelId);
            else next.delete(modelId);
            return next;
        });
    }

    function referenceKey(modelId: string, reference: AgentReference): string {
        return `${modelId}:${reference.agentId}:${reference.configRoot}:${reference.label}`;
    }

    async function runSave(): Promise<void> {
        if (!dirty || saveState.kind === "running") return;
        setSaveState({ kind: "running" });
        setBlockedRemovals([]);
        setReferenceError(null);
        const upstreamById = new Map<string, UpstreamBrowseModel>();
        if (upstream.kind === "ready") {
            for (const row of upstream.items) {
                upstreamById.set(row.modelId, row);
            }
        }
        const removeModelIds: string[] = [];
        for (const id of baselineSelected) {
            if (!draftSelected.has(id)) removeModelIds.push(id);
        }
        const add = [];
        for (const id of draftSelected) {
            if (baselineSelected.has(id)) continue;
            const row = upstreamById.get(id);
            add.push({
                modelId: id,
                upstreamName: row?.upstreamName ?? null,
                contextWindow: row?.contextWindow ?? null,
                maxOutputTokens: row?.maxOutputTokens ?? null,
                inputModalities: row?.inputModalities ?? null,
                outputModalities: row?.outputModalities ?? null,
                supportedEndpoints: row?.supportedEndpoints ?? null,
                capabilities: row?.capabilities ?? {
                    thinking: { state: "unknown", source: null },
                    imageInput: { state: "unknown", source: null },
                    toolCalls: { state: "unknown", source: null },
                },
                thinkingEffort: row?.thinkingEffort ?? null,
            });
        }
        try {
            const impacts = await Promise.all(
                removeModelIds.map((modelId) =>
                    modelDeletionImpact(providerId, modelId),
                ),
            );
            const blocking = impacts.filter(blocksRemoval);
            if (blocking.length > 0) {
                setBlockedRemovals(blocking);
                setSaveState({ kind: "idle" });
                return;
            }
        } catch (error: unknown) {
            setSaveState({ kind: "error", code: errorCode(error) });
            return;
        }
        try {
            await saveProviderModelSelections({
                providerId,
                removeModelIds,
                add,
            });
            setSaveState({ kind: "idle" });
            const nextBaseline = new Set(draftSelected);
            setBaselineSelected(nextBaseline);
            notify({
                tone: "success",
                message: t("providers.models.saveNotice"),
            });
            if (viewMode === "upstream") {
                setViewMode("selected");
                setUpstream({ kind: "idle" });
            }
            await loadSelectedList();
            await refreshSelectedCount();
        } catch (error: unknown) {
            setSaveState({ kind: "error", code: errorCode(error) });
        }
    }

    async function clearReference(
        impact: ModelDeletionImpact,
        reference: AgentReference,
    ): Promise<void> {
        if (
            clearingReference !== null ||
            !reference.clearable ||
            reference.agentId !== "grokBuild"
        ) {
            return;
        }
        const key = referenceKey(impact.modelId, reference);
        setClearingReference(key);
        setReferenceError(null);
        try {
            await clearModelReference({
                providerId,
                modelId: impact.modelId,
                agentId: "grokBuild",
                configRoot: reference.configRoot,
                label: reference.label,
            });
            const next = await modelDeletionImpact(providerId, impact.modelId);
            setBlockedRemovals((current) =>
                current
                    .map((row) => (row.modelId === impact.modelId ? next : row))
                    .filter(blocksRemoval),
            );
        } catch (error: unknown) {
            setReferenceError(errorCode(error));
        } finally {
            setClearingReference(null);
        }
    }

    /** Refresh saved rows only; never changes membership, so drafts block it. */
    async function runRefreshSaved(): Promise<void> {
        if (refreshState.kind === "running" || dirty) return;
        const generation = ++refreshGeneration.current;
        setRefreshState({ kind: "running" });
        try {
            const summary = await refreshSavedProviderModels(providerId);
            if (hidden || generation !== refreshGeneration.current) return;
            setRefreshState({ kind: "idle" });
            await loadSelectedList();
            notify({
                tone: "success",
                message: t(
                    summary.complete
                        ? "providers.models.refreshNotice"
                        : "providers.models.refreshPartial",
                    { count: summary.refreshed },
                ),
            });
        } catch (error: unknown) {
            if (hidden || generation !== refreshGeneration.current) return;
            const code = errorCode(error);
            setRefreshState({ kind: "error", code });
            notify({
                tone: "error",
                message: t(`providers.models.errors.${code}`),
            });
        }
    }

    function onSearchChange(event: ChangeEvent<HTMLInputElement>): void {
        setSearchInput(event.currentTarget.value);
    }

    function confirmDiscardDraft(): boolean {
        if (!dirty) return true;
        return window.confirm(t("providers.models.unsavedLeave"));
    }

    const browseActive = browseRunning || serverBrowseRunning;
    const keyMissing = keyState.kind === "ready" && !keyState.configured;

    const upstreamItems = upstream.kind === "ready" ? upstream.items : [];

    const searchSummary =
        viewMode === "selected" &&
        list.kind === "ready" &&
        debouncedQuery !== "" &&
        list.totalMatches !== null
            ? t("providers.models.searchSummary", {
                  total: list.totalMatches,
                  shown: list.items.length,
              })
            : viewMode === "upstream" &&
                debouncedQuery !== "" &&
                upstream.kind === "ready"
              ? t("providers.models.searchSummaryUpstream", {
                    shown: upstreamItems.length,
                })
              : null;

    // Busy for every competing model operation. The open inline editor itself
    // is excluded from `sharedBusy` so it never locks itself out, while
    // `sectionBusy` still blocks everything else while the editor is open.
    const sharedBusy =
        manualBusy ||
        refreshState.kind === "running" ||
        browseActive ||
        saveState.kind === "running" ||
        (viewMode === "selected" && list.kind === "loading") ||
        (viewMode === "upstream" && upstream.kind === "loading");
    const sectionBusy = editBusy || editor !== null || sharedBusy;

    useEffect(() => {
        onBusyChange?.(!hidden && sectionBusy);
    }, [onBusyChange, sectionBusy, hidden]);

    const emptyMessage = ((): string => {
        if (keyMissing) return t("providers.models.emptyNeedKey");
        if (viewMode === "selected") {
            if (debouncedQuery !== "")
                return t("providers.models.emptyFiltered");
            return t("providers.models.emptyNeedFetch");
        }
        if (upstream.kind === "error")
            return t("providers.models.emptyUpstream");
        return t("providers.models.emptyUpstream");
    })();

    const listHeadingId = "provider-models-list-heading";
    const listTitle =
        viewMode === "selected"
            ? t("providers.models.filter.selected")
            : t("providers.models.filter.all");
    const showListShell = list.kind !== "preview";

    const endOfListMessage = resolveEndOfListMessage(
        t,
        viewMode,
        list,
        debouncedQuery,
        upstream,
        upstreamMorePages,
        browseActive,
    );

    function manualSaved(model: ProviderModel): void {
        // Addition is a separate, immediate save. It is blocked while checkbox
        // drafts exist, so extending both sets cannot discard unsaved choices.
        const next = new Set(baselineSelected);
        next.add(model.modelId);
        syncBaselineFromLoaded(next);
        onSelectedCountChange?.({ count: next.size, hasMore: false });
        if (list.kind === "ready") {
            setList({ ...list, items: mergeModels(list.items, [model]) });
        }
        setSearchInput("");
        setDebouncedQuery("");
        notify({
            tone: "success",
            message: t("providers.models.manual.saved"),
        });
        setEditor(null);
    }

    // The model dialog remembers the control that opened it so focus can return
    // there after saving or cancelling.
    const editTrigger = useRef<HTMLElement | null>(null);

    function rememberTrigger(): void {
        editTrigger.current =
            document.activeElement instanceof HTMLElement
                ? document.activeElement
                : null;
    }

    function openEditor(modelId: string): void {
        rememberTrigger();
        setEditor({ kind: "edit", modelId });
    }

    function closeEditor(): void {
        setEditor(null);
    }

    // Hiding the section closes the model dialog; its unmounting also releases
    // the busy guard the form reported. Adjusting state during render keeps the
    // open editor a derivation of `hidden` instead of an effect round-trip.
    if (hidden && editor !== null) {
        setEditor(null);
    }

    const listBody = (
        <>
            {!hidden && list.kind === "ready" && dirty && (
                <p
                    className={[editForm["hint"], styles["manualDraft"]].join(
                        " ",
                    )}
                    role="status"
                >
                    {t("providers.models.manual.draft")}
                </p>
            )}
            {viewMode === "selected" &&
                list.kind === "loading" &&
                !browseActive && (
                    <p className={styles["message"]} role="status">
                        {t("providers.models.loading")}
                    </p>
                )}

            {viewMode === "selected" && list.kind === "preview" && (
                <p className={styles["message"]} role="status">
                    {t("providers.models.preview")}
                </p>
            )}

            {viewMode === "selected" && list.kind === "error" && (
                <>
                    <p
                        className={[editForm["error"], styles["error"]].join(
                            " ",
                        )}
                        role="alert"
                    >
                        {t(`providers.models.errors.${list.code}`)}
                    </p>
                    <button
                        type="button"
                        className={buttons["secondary"]}
                        onClick={() => {
                            void loadSelectedList();
                        }}
                    >
                        {t("providers.models.retry")}
                    </button>
                </>
            )}

            {viewMode === "upstream" && upstream.kind === "loading" && (
                <p className={styles["message"]} role="status">
                    {t("providers.models.upstreamLoading")}
                </p>
            )}

            {viewMode === "upstream" && upstream.kind === "error" && (
                <>
                    <p
                        className={[editForm["error"], styles["error"]].join(
                            " ",
                        )}
                        role="alert"
                    >
                        {t(`providers.models.errors.${upstream.code}`)}
                    </p>
                    <button
                        type="button"
                        className={buttons["secondary"]}
                        onClick={() => {
                            void loadUpstreamPage(null, false);
                        }}
                    >
                        {t("providers.models.retry")}
                    </button>
                </>
            )}

            {((viewMode === "selected" &&
                list.kind === "ready" &&
                list.items.length === 0) ||
                (viewMode === "upstream" &&
                    upstream.kind === "ready" &&
                    upstreamItems.length === 0 &&
                    !browseActive)) && (
                <p className={styles["empty"]} role="status">
                    {emptyMessage}
                </p>
            )}

            {viewMode === "selected" &&
                list.kind === "ready" &&
                list.items.length > 0 && (
                    <ul
                        className={styles["list"]}
                        aria-labelledby={listHeadingId}
                    >
                        {list.items.map((model) => {
                            const checked = draftSelected.has(model.modelId);
                            const blocked = selectionBlocked(
                                model.routeSupport,
                                model.source,
                            );
                            return (
                                <li
                                    key={model.modelId}
                                    className={styles["item"]}
                                >
                                    <ModelCheckbox
                                        checked={checked}
                                        disabled={sectionBusy || blocked}
                                        ariaLabel={
                                            checked
                                                ? t(
                                                      "providers.models.deselectModel",
                                                      {
                                                          modelId:
                                                              model.modelId,
                                                      },
                                                  )
                                                : t(
                                                      "providers.models.selectModel",
                                                      {
                                                          modelId:
                                                              model.modelId,
                                                      },
                                                  )
                                        }
                                        onCheckedChange={(nextSelected) => {
                                            if (blocked) return;
                                            toggleDraft(
                                                model.modelId,
                                                nextSelected,
                                            );
                                        }}
                                    />
                                    <div className={styles["body"]}>
                                        <p className={styles["line"]}>
                                            <span
                                                className={styles["lineText"]}
                                            >
                                                <span
                                                    className={styles["name"]}
                                                >
                                                    {displayTitle(model)}
                                                </span>
                                                {displayTitle(model) !==
                                                    model.modelId && (
                                                    <span
                                                        className={
                                                            styles["modelId"]
                                                        }
                                                    >
                                                        {" · "}
                                                        {model.modelId}
                                                    </span>
                                                )}
                                                {model.source === "manual" && (
                                                    <span
                                                        className={
                                                            styles["manualTag"]
                                                        }
                                                    >
                                                        {" · "}
                                                        {t(
                                                            "providers.models.manualSource",
                                                        )}
                                                    </span>
                                                )}
                                            </span>
                                            <ModelLimitMarks
                                                contextWindow={
                                                    model.contextWindow
                                                }
                                                maxOutputTokens={
                                                    model.maxOutputTokens
                                                }
                                            />
                                            <ModelCapabilityIcons
                                                capabilities={
                                                    model.capabilities
                                                }
                                                capabilityOverrides={
                                                    model.capabilityOverrides
                                                }
                                            />
                                            {model.resolvedProtocol !== null &&
                                                model.routeSupport ===
                                                    "supported" && (
                                                    <span
                                                        className={
                                                            styles["badge"]
                                                        }
                                                    >
                                                        {t(
                                                            "providers.models.badges.resolvedProtocol",
                                                            {
                                                                protocol: t(
                                                                    `providers.protocols.${model.resolvedProtocol}`,
                                                                ),
                                                            },
                                                        )}
                                                    </span>
                                                )}
                                            {model.routeSupport ===
                                                "unsupported" && (
                                                <span
                                                    className={styles["badge"]}
                                                >
                                                    {t(
                                                        "providers.models.badges.routeUnsupported",
                                                    )}
                                                </span>
                                            )}
                                            {model.routeSupport === "unknown" &&
                                                model.source !== "manual" && (
                                                    <span
                                                        className={
                                                            styles["badge"]
                                                        }
                                                    >
                                                        {t(
                                                            "providers.models.badges.routeUnknown",
                                                        )}
                                                    </span>
                                                )}
                                        </p>
                                        {model.upstreamState === "missing" &&
                                            model.missingSinceMs !== null && (
                                                <div
                                                    className={styles["badges"]}
                                                >
                                                    <span
                                                        className={
                                                            styles["badge"]
                                                        }
                                                    >
                                                        {t(
                                                            "providers.models.badges.upstreamMissing",
                                                            {
                                                                date: formatMissingDate(
                                                                    model.missingSinceMs,
                                                                    i18n.language,
                                                                ),
                                                            },
                                                        )}
                                                    </span>
                                                </div>
                                            )}
                                    </div>
                                    {!hidden && (
                                        <IconButton
                                            icon="edit"
                                            label={t(
                                                "providers.models.editor.title",
                                                { modelId: model.modelId },
                                            )}
                                            blocked={
                                                sectionBusy ||
                                                dirty ||
                                                searchInput.trim() !==
                                                    debouncedQuery
                                            }
                                            onClick={() => {
                                                openEditor(model.modelId);
                                            }}
                                        />
                                    )}
                                </li>
                            );
                        })}
                    </ul>
                )}

            {viewMode === "upstream" &&
                upstream.kind === "ready" &&
                upstreamItems.length > 0 && (
                    <ul
                        className={styles["list"]}
                        aria-labelledby={listHeadingId}
                    >
                        {upstreamItems.map((model) => {
                            const checked = draftSelected.has(model.modelId);
                            const blocked = selectionBlocked(
                                model.routeSupport,
                            );
                            return (
                                <li
                                    key={model.modelId}
                                    className={styles["item"]}
                                >
                                    <ModelCheckbox
                                        checked={checked}
                                        disabled={sectionBusy || blocked}
                                        ariaLabel={
                                            checked
                                                ? t(
                                                      "providers.models.deselectModel",
                                                      {
                                                          modelId:
                                                              model.modelId,
                                                      },
                                                  )
                                                : t(
                                                      "providers.models.selectModel",
                                                      {
                                                          modelId:
                                                              model.modelId,
                                                      },
                                                  )
                                        }
                                        onCheckedChange={(nextSelected) => {
                                            if (blocked) return;
                                            toggleDraft(
                                                model.modelId,
                                                nextSelected,
                                            );
                                        }}
                                    />
                                    <div className={styles["body"]}>
                                        <p className={styles["line"]}>
                                            <span
                                                className={styles["lineText"]}
                                            >
                                                <span
                                                    className={styles["name"]}
                                                >
                                                    {displayTitle(model)}
                                                </span>
                                                {displayTitle(model) !==
                                                    model.modelId && (
                                                    <span
                                                        className={
                                                            styles["modelId"]
                                                        }
                                                    >
                                                        {" · "}
                                                        {model.modelId}
                                                    </span>
                                                )}
                                            </span>
                                            {/* Upstream browse rows are
                                            catalog-only: a model that is not
                                            saved yet cannot carry user
                                            declarations. */}
                                            <ModelLimitMarks
                                                contextWindow={
                                                    model.contextWindow
                                                }
                                                maxOutputTokens={
                                                    model.maxOutputTokens
                                                }
                                            />
                                            <ModelCapabilityIcons
                                                capabilities={
                                                    model.capabilities
                                                }
                                                capabilityOverrides={
                                                    NO_CAPABILITY_DECLARATIONS
                                                }
                                            />
                                            {model.resolvedProtocol !== null &&
                                                model.routeSupport ===
                                                    "supported" && (
                                                    <span
                                                        className={
                                                            styles["badge"]
                                                        }
                                                    >
                                                        {t(
                                                            "providers.models.badges.resolvedProtocol",
                                                            {
                                                                protocol: t(
                                                                    `providers.protocols.${model.resolvedProtocol}`,
                                                                ),
                                                            },
                                                        )}
                                                    </span>
                                                )}
                                            {model.routeSupport ===
                                                "unsupported" && (
                                                <span
                                                    className={styles["badge"]}
                                                >
                                                    {t(
                                                        "providers.models.badges.routeUnsupported",
                                                    )}
                                                </span>
                                            )}
                                            {model.routeSupport ===
                                                "unknown" && (
                                                <span
                                                    className={styles["badge"]}
                                                >
                                                    {t(
                                                        "providers.models.badges.routeUnknown",
                                                    )}
                                                </span>
                                            )}
                                        </p>
                                    </div>
                                </li>
                            );
                        })}
                    </ul>
                )}

            {viewMode === "upstream" && upstreamMorePages && (
                <div
                    ref={loadMoreSentinelRef}
                    className={styles["loadSentinel"]}
                    aria-hidden
                />
            )}

            {endOfListMessage !== null && (
                <p
                    className={styles["listFooter"]}
                    role="status"
                    aria-live="polite"
                >
                    {endOfListMessage}
                </p>
            )}
        </>
    );

    const sectionLabel =
        layout === "section" ? "provider-models-heading" : undefined;

    const saveButton = (
        <button
            type="button"
            className={[buttons["primary"], styles["toolbarButton"]].join(" ")}
            aria-disabled={!dirty || sectionBusy}
            onClick={() => {
                if (!dirty || sectionBusy) return;
                void runSave();
            }}
        >
            {saveState.kind === "running"
                ? t("providers.models.saving")
                : t("providers.models.save")}
        </button>
    );

    return (
        <section
            className={styles["section"]}
            data-layout={layout}
            aria-labelledby={sectionLabel}
            aria-label={
                layout === "tabPanel" ? t("providers.tabs.models") : undefined
            }
            aria-busy={sectionBusy}
        >
            {layout === "section" && (
                <h2 className={styles["title"]} id="provider-models-heading">
                    {t("providers.groups.models")}
                </h2>
            )}

            {saveState.kind === "error" && (
                <p
                    className={[editForm["error"], styles["error"]].join(" ")}
                    role="alert"
                >
                    {t(`providers.models.errors.${saveState.code}`)}
                </p>
            )}

            {refreshState.kind === "error" && (
                <p
                    className={[editForm["error"], styles["error"]].join(" ")}
                    role="alert"
                >
                    {t(`providers.models.errors.${refreshState.code}`)}
                </p>
            )}

            {blockedRemovals.length > 0 && (
                <section
                    className={styles["referencePanel"]}
                    aria-label={t("providers.models.referencesLabel")}
                >
                    <p className={styles["message"]} role="alert">
                        {t("providers.models.referencesBlocked")}
                    </p>
                    <ul className={styles["referenceList"]}>
                        {blockedRemovals.flatMap((impact) =>
                            impact.references.map((reference) => {
                                const key = referenceKey(
                                    impact.modelId,
                                    reference,
                                );
                                return (
                                    <li
                                        className={styles["referenceRow"]}
                                        key={key}
                                    >
                                        <div>
                                            <p
                                                className={
                                                    styles["referenceName"]
                                                }
                                            >
                                                {t(
                                                    `providers.models.referenceNames.${reference.agentId}`,
                                                    {
                                                        model: impact.modelId,
                                                        label: reference.label,
                                                    },
                                                )}
                                            </p>
                                            <p
                                                className={
                                                    styles["referenceMeta"]
                                                }
                                            >
                                                {reference.configRoot}
                                                {" · "}
                                                {t(
                                                    `providers.models.referenceStates.${reference.state}`,
                                                )}
                                            </p>
                                        </div>
                                        {reference.clearable &&
                                        reference.agentId === "grokBuild" ? (
                                            <button
                                                type="button"
                                                className={buttons["secondary"]}
                                                disabled={
                                                    clearingReference !== null
                                                }
                                                onClick={() => {
                                                    void clearReference(
                                                        impact,
                                                        reference,
                                                    );
                                                }}
                                            >
                                                {clearingReference === key
                                                    ? t(
                                                          "providers.models.clearingReference",
                                                      )
                                                    : t(
                                                          "providers.models.clearReference",
                                                      )}
                                            </button>
                                        ) : (
                                            <p
                                                className={
                                                    styles["referenceHint"]
                                                }
                                            >
                                                {t(
                                                    `providers.models.referenceHints.${reference.state}`,
                                                )}
                                            </p>
                                        )}
                                    </li>
                                );
                            }),
                        )}
                        {blockedRemovals.map((impact) => (
                            <CodexReferenceRows
                                key={`codex:${impact.modelId}`}
                                references={impact.codexReferences ?? []}
                                classes={{
                                    row: styles["referenceRow"],
                                    name: styles["referenceName"],
                                    meta: styles["referenceMeta"],
                                    hint: styles["referenceHint"],
                                }}
                            />
                        ))}
                    </ul>
                    <p className={styles["referenceHint"]}>
                        {t("providers.models.referencesHelp")}
                    </p>
                    {referenceError !== null && (
                        <p className={styles["error"]} role="alert">
                            {t(`providers.models.errors.${referenceError}`)}
                        </p>
                    )}
                </section>
            )}

            {keyMissing && (
                <p className={styles["message"]} role="status">
                    {t("providers.models.needKey")}
                </p>
            )}

            <div className={styles["toolbarRow"]}>
                <div className={styles["searchField"]}>
                    <input
                        className={[
                            editForm["input"],
                            styles["searchInput"],
                        ].join(" ")}
                        id="provider-models-search"
                        ref={searchField}
                        type="search"
                        value={searchInput}
                        autoComplete="off"
                        spellCheck={false}
                        placeholder={t("providers.models.searchPlaceholder")}
                        aria-label={t("providers.models.searchPlaceholder")}
                        disabled={
                            editBusy ||
                            editor !== null ||
                            manualBusy ||
                            list.kind === "preview" ||
                            keyMissing ||
                            keyState.kind === "loading" ||
                            (viewMode === "selected" &&
                                list.kind !== "ready" &&
                                list.kind !== "loading") ||
                            (viewMode === "upstream" &&
                                upstream.kind === "idle")
                        }
                        onChange={onSearchChange}
                    />
                </div>
                <div className={styles["toolbarActions"]}>
                    {viewMode === "selected" ? (
                        <>
                            <button
                                type="button"
                                className={[
                                    buttons["secondary"],
                                    styles["toolbarButton"],
                                ].join(" ")}
                                aria-disabled={
                                    sectionBusy ||
                                    list.kind !== "ready" ||
                                    dirty ||
                                    keyMissing ||
                                    keyState.kind === "loading"
                                }
                                onClick={() => {
                                    if (
                                        sectionBusy ||
                                        list.kind !== "ready" ||
                                        dirty ||
                                        keyMissing ||
                                        keyState.kind === "loading"
                                    ) {
                                        return;
                                    }
                                    void runRefreshSaved();
                                }}
                            >
                                {refreshState.kind === "running"
                                    ? t("providers.models.refreshing")
                                    : t("providers.models.refreshSaved")}
                            </button>
                            <button
                                type="button"
                                className={[
                                    buttons["secondary"],
                                    styles["toolbarButton"],
                                ].join(" ")}
                                aria-disabled={
                                    sectionBusy ||
                                    list.kind === "preview" ||
                                    keyMissing ||
                                    keyState.kind === "loading"
                                }
                                onClick={() => {
                                    if (
                                        sectionBusy ||
                                        list.kind === "preview" ||
                                        keyMissing ||
                                        keyState.kind === "loading"
                                    ) {
                                        return;
                                    }
                                    if (dirty && !confirmDiscardDraft()) return;
                                    void startUpstreamBrowse();
                                }}
                            >
                                {t("providers.models.fetch")}
                            </button>
                        </>
                    ) : (
                        <>
                            {browseActive && (
                                <button
                                    type="button"
                                    className={[
                                        buttons["secondary"],
                                        styles["toolbarButton"],
                                    ].join(" ")}
                                    onClick={() => {
                                        void runCancelBrowse();
                                    }}
                                >
                                    {t("providers.models.cancelFetch")}
                                </button>
                            )}
                            <button
                                type="button"
                                className={[
                                    buttons["secondary"],
                                    styles["toolbarButton"],
                                ].join(" ")}
                                aria-disabled={
                                    browseActive ||
                                    keyMissing ||
                                    keyState.kind === "loading"
                                }
                                onClick={() => {
                                    if (
                                        browseActive ||
                                        keyMissing ||
                                        keyState.kind === "loading"
                                    ) {
                                        return;
                                    }
                                    if (dirty && !confirmDiscardDraft()) return;
                                    setDraftSelected(new Set(baselineSelected));
                                    setViewMode("selected");
                                    setUpstream({ kind: "idle" });
                                    void loadSelectedList();
                                }}
                            >
                                {t("providers.models.backToSelected")}
                            </button>
                        </>
                    )}
                    {!hidden && list.kind === "ready" && (
                        <IconButton
                            icon="plus"
                            label={t("providers.models.manual.title")}
                            blocked={sectionBusy || dirty}
                            onClick={() => {
                                rememberTrigger();
                                setEditor({ kind: "create" });
                            }}
                        />
                    )}
                    {layout !== "tabPanel" && saveButton}
                </div>
            </div>

            {searchSummary !== null && (
                <p className={styles["summary"]} role="status">
                    {searchSummary}
                </p>
            )}

            {layout === "tabPanel" ? (
                <div className={styles["listRegion"]}>
                    {showListShell && (
                        <h3 className={styles["listHeader"]} id={listHeadingId}>
                            {listTitle}
                        </h3>
                    )}
                    <div className={styles["listScroll"]} ref={listScrollRef}>
                        {showListShell ? (
                            <div className={styles["listShell"]}>
                                {listBody}
                            </div>
                        ) : (
                            listBody
                        )}
                    </div>
                </div>
            ) : showListShell ? (
                <>
                    <h3 className={styles["listHeader"]} id={listHeadingId}>
                        {listTitle}
                    </h3>
                    <div className={styles["listShell"]}>{listBody}</div>
                </>
            ) : (
                listBody
            )}
            {layout === "tabPanel" && (
                <SecondaryPageActions active={!hidden}>
                    {saveButton}
                </SecondaryPageActions>
            )}
            {editor?.kind === "create" && (
                <ModelEntryForm
                    key={`${providerId}-create`}
                    mode="create"
                    providerId={providerId}
                    blocked={sharedBusy || dirty}
                    onSaved={manualSaved}
                    onDismiss={closeEditor}
                    onBusyChange={setManualBusy}
                    returnFocus={editTrigger}
                />
            )}
            {editor?.kind === "edit" && (
                <ModelEntryForm
                    key={editor.modelId}
                    mode="edit"
                    providerId={providerId}
                    modelId={editor.modelId}
                    blocked={
                        sharedBusy ||
                        dirty ||
                        searchInput.trim() !== debouncedQuery
                    }
                    onBusyChange={setEditBusy}
                    onDismiss={closeEditor}
                    returnFocus={editTrigger}
                    onSaved={(updated) => {
                        setList((current) =>
                            current.kind === "ready"
                                ? {
                                      ...current,
                                      items: current.items.map((row) =>
                                          row.modelId === updated.modelId
                                              ? updated
                                              : row,
                                      ),
                                  }
                                : current,
                        );
                        closeEditor();
                        notify({
                            tone: "success",
                            message: t("providers.models.editor.saved"),
                        });
                    }}
                />
            )}
        </section>
    );
}
