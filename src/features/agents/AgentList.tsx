import {
    useEffect,
    useImperativeHandle,
    useRef,
    useState,
    type Ref,
    type JSX,
} from "react";
import type { ModuleNavigationHandle } from "../../components/moduleNavigation";
import { Tooltip } from "@base-ui/react/tooltip";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import editForm from "../../components/editForm.module.css";
import detailForm from "../../components/detailForm.module.css";
import { Icon } from "../../components/Icon";
import { PageModuleHeader } from "../../components/PageModuleHeader";
import {
    discoverAgents,
    DiscoveryRequestError,
    AGENT_IDS,
    type DiscoveryInput,
    type AgentId,
    type ConfigurableAgent,
    type AgentObservation,
    type DiscoveryErrorCode,
    type DiscoveryResult,
} from "../../lib/desktop/agents";
import buttons from "../../components/actionButtons.module.css";
import hierarchy from "../../components/textHierarchy.module.css";
import pageStyles from "../providers/ProviderPage.module.css";
import { AgentIcon } from "./AgentIcon";
import { IconButton } from "../../components/IconButton";
import listActions from "../../components/listActions.module.css";
import { AgentPage } from "./AgentPage";
import { AgentVersionPanel } from "./AgentVersionPanel";
import { CurrentConfigPin, RestoreView } from "./RestoreView";
import { WriteConfigDialog } from "./WriteConfigDialog";
import { CodexWriteDialog } from "./CodexWriteDialog";
import { CodexSettingsPanel } from "./CodexSettingsPanel";
import { codexActiveModel } from "./codexSummary";
import { readCodexSettingsState } from "../../lib/desktop/codex";
import { AgentSkillPanel } from "./AgentSkillPanel";
import { AgentMcpPanel } from "./AgentMcpPanel";
import underlineTabs from "../../components/underlineTabs.module.css";
import {
    readConfigurationHistory,
    type ActiveAgentModel,
    type HistoryCurrentConfig,
} from "../../lib/desktop/history";
import {
    probeAgentVersion,
    type AgentVersionResult,
} from "../../lib/desktop/agentVersions";
import { getAutoVersionProbe } from "../../lib/desktop/settings";
import surfaceList from "../../components/surfaceList.module.css";
import styles from "./AgentList.module.css";

type State =
    | { kind: "loading" }
    | DiscoveryResult
    | { kind: "error"; code: DiscoveryErrorCode };

type AgentsPage = { kind: "list" } | { kind: "detail"; id: AgentId };

type AgentSegment = "history" | "skill" | "mcp" | "install";

/** Agents with a model write flow: the shared adapters plus Codex. */
type WriteTarget = ConfigurableAgent | "codex";

function failure(error: unknown): DiscoveryErrorCode {
    return error instanceof DiscoveryRequestError
        ? error.code
        : "operation_failed";
}

/**
 * Whether this Agent's adapter can write, back up and roll back a configuration.
 *
 * Detection is not the same claim: every catalog entry can be discovered, while
 * the model-configuration cards appear only where a verified adapter exists.
 */
/** Codex writes through `codex.ts`, not the shared ConfigurableAgent adapter. */
function hasWriteFlow(id: AgentId): id is WriteTarget {
    return hasConfigAdapter(id) || id === "codex";
}

function hasConfigAdapter(id: AgentId): id is ConfigurableAgent {
    return id === "claudeCode" || id === "pi" || id === "grokBuild";
}

/**
 * Fixed six-Agent catalog: auto-scan on first mount, including absent entries,
 * and a detail view for paths and config file presence. Hidden navigation keeps
 * the last report and manual paths without rescanning.
 */
export function AgentList({
    hidden = false,
    homeRef,
    onOpenSkills,
    onOpenMcp,
}: {
    hidden?: boolean;
    homeRef?: Ref<ModuleNavigationHandle>;
    /** Jump to the Skills module from the Skill detail slot. */
    onOpenSkills?: () => void;
    /** Jump to central MCP registration management from the read-only slot. */
    onOpenMcp?: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    const [state, setState] = useState<State>({ kind: "loading" });
    /** Last successful desktop report; kept across refresh errors and loading. */
    const [snapshot, setSnapshot] = useState<AgentObservation[]>([]);
    const [page, setPage] = useState<AgentsPage>({ kind: "list" });
    const [segment, setSegment] = useState<AgentSegment>("history");
    const [currentConfig, setCurrentConfig] =
        useState<HistoryCurrentConfig | null>(null);
    /** List-row Write dialog target; null when closed. */
    const [writeTarget, setWriteTarget] = useState<WriteTarget | null>(null);
    const writeReturnFocus = useRef<HTMLElement | null>(null);
    /**
     * List-card summary: the model each Agent currently names. `ours` shows
     * the saved Provider/model, `own` says the configuration belongs to the
     * user, and `unknown` stays out of the row until a read succeeds.
     */
    const [listSummaries, setListSummaries] = useState<
        Partial<Record<WriteTarget, ActiveAgentModel>>
    >({});
    const [versions, setVersions] = useState<
        Partial<Record<AgentId, AgentVersionResult>>
    >({});
    const [versionBusy, setVersionBusy] = useState(false);
    const [configEpoch, setConfigEpoch] = useState(0);
    const [autoProbe, setAutoProbe] = useState(false);
    const [autoProbeBusy, setAutoProbeBusy] = useState(false);
    const [confirmationEpoch, setConfirmationEpoch] = useState(0);
    const [paths, setPaths] = useState<Record<AgentId, string>>({
        claudeCode: "",
        codex: "",
        openCode: "",
        pi: "",
        grokBuild: "",
        hermes: "",
    });
    function discoveryInput(): DiscoveryInput {
        return {
            claudePath: paths.claudeCode.trim() || null,
            codexPath: paths.codex.trim() || null,
            opencodePath: paths.openCode.trim() || null,
            piPath: paths.pi.trim() || null,
            grokPath: paths.grokBuild.trim() || null,
            hermesPath: paths.hermes.trim() || null,
        };
    }
    const generation = useRef(0);
    const pending = useRef(false);
    const mounted = useRef(false);
    const focusCard = useRef<AgentId | null>(null);
    useImperativeHandle(homeRef, () => ({
        openHome() {
            if (pending.current) return false;
            focusCard.current = null;
            setPage({ kind: "list" });
            return true;
        },
    }));

    // Read the auto-probe consent once. The stored value decides whether
    // discovery and detail views may launch `--version` without a per-run click.
    useEffect(() => {
        let active = true;
        getAutoVersionProbe().then(
            (result) => {
                if (active && result.kind === "desktop")
                    setAutoProbe(result.preference.autoProbe);
            },
            () => {
                // A failed read leaves auto-probe off; the manual button stays.
            },
        );
        return () => {
            active = false;
        };
    }, []);

    useEffect(() => {
        const current = ++generation.current;
        mounted.current = true;
        pending.current = true;
        void discoverAgents({
            claudePath: null,
            codexPath: null,
            opencodePath: null,
            piPath: null,
            grokPath: null,
            hermesPath: null,
        }).then(
            (result) => {
                if (mounted.current && current === generation.current) {
                    pending.current = false;
                    if (result.kind === "desktop") setSnapshot(result.items);
                    setState(result);
                }
            },
            (error: unknown) => {
                if (mounted.current && current === generation.current) {
                    pending.current = false;
                    setState({ kind: "error", code: failure(error) });
                }
            },
        );
        return () => {
            mounted.current = false;
            pending.current = false;
        };
    }, []);

    useEffect(() => {
        if (hidden || page.kind !== "list" || focusCard.current === null)
            return;
        const id = focusCard.current;
        focusCard.current = null;
        // Return focus to the row, not the Detail icon. Focusing the icon would
        // reopen its tooltip on focus and leave the hint floating over the list.
        const marker = document.querySelector(`[data-agent-card="${id}"]`);
        if (marker instanceof HTMLElement) marker.focus();
    }, [hidden, page]);

    // List rows: the model each Agent's live configuration currently names.
    // Rust classifies it as vibemate's saved pair, the user's own, or
    // unreadable; a failed/pending read stays out of the row.
    useEffect(() => {
        if (hidden || page.kind !== "list") return;
        let current = true;
        const agents: ConfigurableAgent[] = ["claudeCode", "pi", "grokBuild"];
        // Codex keeps its own provider/default state in codex.ts.
        const codexRow = async (): Promise<
            readonly ["codex", ActiveAgentModel]
        > => {
            try {
                return [
                    "codex",
                    codexActiveModel(await readCodexSettingsState()),
                ] as const;
            } catch {
                return ["codex", { kind: "unknown" }] as const;
            }
        };
        void Promise.all([
            codexRow(),
            ...agents.map(async (agentId) => {
                const empty: ActiveAgentModel = { kind: "unknown" };
                try {
                    const result = await readConfigurationHistory(agentId);
                    return [
                        agentId,
                        result.kind === "desktop" ? result.page.active : empty,
                    ] as const;
                } catch {
                    return [agentId, empty] as const;
                }
            }),
        ]).then((rows) => {
            if (!current) return;
            const next: Partial<Record<WriteTarget, ActiveAgentModel>> = {};
            for (const row of rows) {
                next[row[0]] = row[1];
            }
            setListSummaries(next);
        });
        return () => {
            current = false;
        };
    }, [hidden, page.kind, configEpoch, state.kind]);

    async function refresh(): Promise<void> {
        if (pending.current || state.kind === "preview") return;
        pending.current = true;
        const current = ++generation.current;
        // CLI results describe a previously checked target. A new discovery
        // invalidates them even when it later fails or returns the same path.
        probedRef.current.clear();
        setAutoProbeBusy(false);
        setVersions({});
        setConfirmationEpoch((previous) => previous + 1);
        setState({ kind: "loading" });
        try {
            const result = await discoverAgents(discoveryInput());
            if (mounted.current && current === generation.current) {
                if (result.kind === "desktop") setSnapshot(result.items);
                setState(result);
            }
        } catch (error: unknown) {
            if (mounted.current && current === generation.current)
                setState({ kind: "error", code: failure(error) });
        } finally {
            if (mounted.current && current === generation.current)
                pending.current = false;
        }
    }

    const blocked =
        state.kind === "loading" || state.kind === "preview" || versionBusy;

    function cliVersion(row: AgentObservation): AgentVersionResult | undefined {
        const result = versions[row.id];
        return result?.entryPath === row.entryPath &&
            result.resolvedPath === row.resolvedPath
            ? result
            : undefined;
    }

    // Track which (id, entry, resolved) triples have been probed this session.
    // The set is updated inside async callbacks, never during render.
    const probedRef = useRef(new Set<string>());
    // Cancellation flag for the scan auto-probe loop. Reset at effect start and
    // flipped in cleanup so a newer scan stops the previous loop.
    const scanCancelled = useRef(false);
    function probeKey(
        id: AgentId,
        entryPath: string,
        resolvedPath: string,
    ): string {
        return `${id}\0${entryPath}\0${resolvedPath}`;
    }

    /**
     * Probe every found Agent sequentially after a scan. The single-probe gate
     * in Rust accepts one process at a time, so probes never overlap. Each run
     * is tied to an `AbortController`; a new scan or unmount cancels the rest.
     *
     * `setTimeout` defers the busy flag and the async loop so no `setState`
     * runs synchronously inside the effect body.
     */
    useEffect(() => {
        if (state.kind !== "desktop" || !autoProbe) return;
        const items = state.items;
        scanCancelled.current = false;
        const timer = window.setTimeout(() => {
            if (scanCancelled.current) return;
            setAutoProbeBusy(true);
            void (async () => {
                try {
                    for (const row of items) {
                        if (scanCancelled.current) return;
                        const entryPath = row.entryPath;
                        const resolvedPath = row.resolvedPath;
                        if (
                            row.state !== "found" ||
                            typeof entryPath !== "string" ||
                            typeof resolvedPath !== "string"
                        )
                            continue;
                        const key = probeKey(row.id, entryPath, resolvedPath);
                        if (probedRef.current.has(key)) continue;
                        probedRef.current.add(key);
                        try {
                            const result = await probeAgentVersion({
                                id: row.id,
                                entryPath,
                                resolvedPath,
                                confirmed: true,
                            });
                            // The state updater is safe after cancellation:
                            // `refresh()` clears the map and the next scan
                            // overwrites any transient stale entry.
                            setVersions((previous) => ({
                                ...previous,
                                [result.id]: result,
                            }));
                        } catch {
                            // Auto-probe failures stay silent: the manual
                            // button remains and discovery metadata is shown.
                        }
                    }
                } finally {
                    if (!scanCancelled.current) setAutoProbeBusy(false);
                }
            })();
        }, 0);
        return () => {
            // Cancel the in-flight loop when a newer scan starts or the
            // component unmounts. The Rust side still reaps the direct child.
            scanCancelled.current = true;
            window.clearTimeout(timer);
        };
    }, [state, autoProbe]);

    // Opening a detail page probes only when no cached version matches. This
    // covers the case where the setting was enabled after the scan finished.
    useEffect(() => {
        if (page.kind !== "detail" || !autoProbe) return;
        const row = (state.kind === "desktop" ? state.items : snapshot).find(
            (item) => item.id === page.id,
        );
        if (!row) return;
        const entryPath = row.entryPath;
        const resolvedPath = row.resolvedPath;
        if (
            row.state !== "found" ||
            typeof entryPath !== "string" ||
            typeof resolvedPath !== "string"
        )
            return;
        const key = probeKey(row.id, entryPath, resolvedPath);
        if (probedRef.current.has(key)) return;
        probedRef.current.add(key);
        void (async () => {
            try {
                const result = await probeAgentVersion({
                    id: row.id,
                    entryPath,
                    resolvedPath,
                    confirmed: true,
                });
                if (mounted.current)
                    setVersions((previous) => ({
                        ...previous,
                        [result.id]: result,
                    }));
            } catch {
                // Silent; the manual button can still retry.
            }
        })();
    }, [page, autoProbe, state, snapshot]);

    function items(): AgentObservation[] {
        return state.kind === "desktop" ? state.items : snapshot;
    }

    function observation(id: AgentId): AgentObservation | undefined {
        return items().find((item) => item.id === id);
    }

    function discoveryLabel(row: AgentObservation | undefined): string {
        if (state.kind === "loading") return t("agents.list.checking");
        if (state.kind === "preview") return t("agents.list.preview");
        if (state.kind === "error") {
            return row === undefined
                ? t("agents.list.failed")
                : t("agents.list.previousStatus", {
                      status: t(`agents.states.${row.state}`),
                  });
        }
        return row === undefined
            ? t("agents.list.failed")
            : t(`agents.states.${row.state}`);
    }

    function openDetail(id: AgentId): void {
        if (state.kind === "loading" || versionBusy) return;
        setWriteTarget(null);
        setSegment(hasWriteFlow(id) ? "history" : "skill");
        setCurrentConfig(null);
        setPage({ kind: "detail", id });
    }

    function openWrite(id: WriteTarget, returnEl: HTMLElement | null): void {
        if (state.kind === "loading" || versionBusy) return;
        writeReturnFocus.current = returnEl;
        setWriteTarget(id);
    }

    function closeWrite(): void {
        if (pending.current || versionBusy) return;
        setWriteTarget(null);
    }

    function returnToList(from: AgentId): void {
        if (pending.current) return;
        focusCard.current = from;
        setCurrentConfig(null);
        setPage({ kind: "list" });
    }

    function configFileLabel(name: string): string {
        switch (name) {
            case "settings.json":
                return t("agents.files.items.settings.json");
            case "models.json":
                return t("agents.files.items.models.json");
            case "mcp.json":
                return t("agents.files.items.mcp.json");
            case "config.toml":
                return t("agents.files.items.config.toml");
            case "managed_config.toml":
                return t("agents.files.items.managed_config.toml");
            case "requirements.toml":
                return t("agents.files.items.requirements.toml");
            case "auth.json":
                return t("agents.files.items.auth.json");
            case "opencode.json":
                return t("agents.files.items.opencode.json");
            case "opencode.jsonc":
                return t("agents.files.items.opencode.jsonc");
            case "config.yaml":
                return t("agents.files.items.config.yaml");
            case ".env":
                return t("agents.files.items.env");
            default:
                return name;
        }
    }

    function factBadge(value: string, hint: string): JSX.Element {
        // Evidence remains available without turning a version hint into a
        // verified installation claim. Focusable text also exposes it to keyboard users.
        return (
            <Tooltip.Provider delay={300}>
                <Tooltip.Root>
                    <Tooltip.Trigger
                        render={<span tabIndex={0} />}
                        className={styles["badge"]}
                    >
                        {value}
                    </Tooltip.Trigger>
                    <Tooltip.Portal>
                        <Tooltip.Positioner
                            className={styles["badgePositioner"]}
                            sideOffset={6}
                        >
                            <Tooltip.Popup className={styles["badgeHint"]}>
                                {hint}
                            </Tooltip.Popup>
                        </Tooltip.Positioner>
                    </Tooltip.Portal>
                </Tooltip.Root>
            </Tooltip.Provider>
        );
    }

    /**
     * Status and version for the title row. They are the facts a user checks
     * first, and keeping them here leaves the card below for paths and controls.
     */
    function detailMeta(row: AgentObservation): JSX.Element {
        const reported = cliVersion(row);
        return (
            <>
                <span className={styles["metaPair"]}>
                    <span className={hierarchy["label"]}>
                        {t("agents.detail.labels.status")}
                    </span>
                    {factBadge(
                        discoveryLabel(row),
                        t(`agents.sources.${row.source}`),
                    )}
                </span>
                <span className={styles["metaPair"]}>
                    <span className={hierarchy["label"]}>
                        {t("agents.detail.labels.version")}
                    </span>
                    {factBadge(
                        reported?.version ?? row.version ?? t("agents.unknown"),
                        reported === undefined
                            ? t(`agents.evidence.${row.versionEvidence}`)
                            : t("agents.versionProbe.evidence"),
                    )}
                </span>
            </>
        );
    }

    /** Everything the detail card shows: where the entry points and what exists. */
    function details(row: AgentObservation): JSX.Element {
        return (
            <>
                {row.entryPath !== null && (
                    <DetailFact
                        label={t("agents.detail.labels.entry")}
                        value={row.entryPath}
                    />
                )}
                {row.resolvedPath !== null &&
                    row.resolvedPath !== row.entryPath && (
                        <DetailFact
                            label={t("agents.detail.labels.target")}
                            value={row.resolvedPath}
                        />
                    )}
                <DetailFact
                    label={t("agents.detail.labels.configDirectory")}
                    value={`${row.configDirectory ?? t("agents.unknown")} · ${t(`agents.directories.${row.directorySource}`)}`}
                />
                {row.files.map((file) => (
                    <DetailFact
                        key={file.name}
                        label={configFileLabel(file.name)}
                        value={t(`agents.files.${file.state}`)}
                    />
                ))}
            </>
        );
    }

    function refreshButton(): JSX.Element {
        return (
            <button
                type="button"
                className={[buttons["primary"], buttons["withIcon"]]
                    .filter((value): value is string => value !== undefined)
                    .join(" ")}
                aria-disabled={blocked}
                onClick={() => {
                    void refresh();
                }}
            >
                <Icon name="refresh" />
                {t("agents.refresh")}
            </button>
        );
    }

    /**
     * Discovery live region. The completed count is list-only: detail pages
     * describe the open entry and drop the region once a scan succeeds.
     */
    function statusLine(scope: "list" | "detail"): JSX.Element | null {
        // Only `found` rows count; absent or unchecked catalog slots stay
        // visible as cards and do not inflate the total.
        let message: string;
        if (state.kind === "loading") {
            message = t("agents.loading");
        } else if (state.kind === "preview") {
            message = "";
        } else if (state.kind === "error") {
            message = t("agents.list.failed");
        } else if (scope === "list") {
            const discovered = state.items.filter(
                (row) => row.state === "found",
            ).length;
            message = t("agents.count", { count: discovered });
        } else {
            return null;
        }
        // Keep the live region mounted so completion is announced without
        // moving focus away from the path or refresh action.
        return (
            <p
                className={styles["hint"]}
                role="status"
                hidden={state.kind === "preview"}
            >
                {message}
            </p>
        );
    }

    function listView(): JSX.Element {
        // Discovered Agents first; each group reads alphabetically by the name
        // shown in the row. The sort is stable, so equal names keep AGENT_IDS
        // order and discovery updates never reshuffle rows on their own.
        const orderedAgentIds = [...AGENT_IDS].sort((left, right) => {
            const leftFound = observation(left)?.state === "found";
            const rightFound = observation(right)?.state === "found";
            if (leftFound !== rightFound) return leftFound ? -1 : 1;
            return t(`agents.names.${left}`).localeCompare(
                t(`agents.names.${right}`),
            );
        });
        return (
            <>
                <PageModuleHeader
                    icon="agents"
                    title={t("desktop.nav.agents")}
                    trailing={refreshButton()}
                />
                {statusLine("list")}
                {state.kind === "error" && (
                    <p className={styles["error"]} role="alert">
                        {t(`agents.errors.${state.code}`)}
                    </p>
                )}
                <ul
                    className={[surfaceList["list"], styles["list"]].join(" ")}
                    aria-label={t("agents.list.label")}
                >
                    {orderedAgentIds.map((id) => {
                        const row = observation(id);
                        const reported =
                            row === undefined ? undefined : cliVersion(row);
                        const summary = hasWriteFlow(id)
                            ? listSummaries[id]
                            : undefined;
                        const providerModel =
                            summary?.kind === "ours"
                                ? {
                                      provider: summary.providerLabel,
                                      model: summary.modelLabel,
                                  }
                                : null;
                        // One string feeds the summary and its tooltip, so a
                        // truncated name stays readable on hover.
                        const providerModelText =
                            providerModel === null
                                ? null
                                : t(
                                      "agents.summary.providerModel",
                                      providerModel,
                                  );
                        // Not vibemate's current pair: the config is the
                        // user's own, but only when the Agent itself was found.
                        // Configurable Agents need a readable active model;
                        // other Agents have no write adapter to consult.
                        const found =
                            state.kind === "desktop" && row?.state === "found";
                        const ownConfiguration =
                            providerModel === null &&
                            found &&
                            (hasWriteFlow(id) ? summary?.kind === "own" : true);
                        const statusText =
                            state.kind === "desktop" && row?.state === "found"
                                ? (reported?.version ??
                                  row.version ??
                                  t("agents.unknown"))
                                : discoveryLabel(row);
                        const statusTitle =
                            state.kind === "desktop" && row?.state === "found"
                                ? reported === undefined
                                    ? t(
                                          `agents.evidence.${row.versionEvidence}`,
                                      )
                                    : t("agents.versionProbe.evidence")
                                : undefined;
                        const name = t(`agents.names.${id}`);
                        return (
                            <li
                                key={id}
                                className={[
                                    surfaceList["item"],
                                    styles["item"],
                                ].join(" ")}
                            >
                                <div className={styles["details"]}>
                                    <div
                                        className={styles["row"]}
                                        data-agent-card={id}
                                        tabIndex={-1}
                                    >
                                        <AgentIcon id={id} size="row" />
                                        <span className={styles["textColumn"]}>
                                            <span className={styles["rowName"]}>
                                                {name}
                                            </span>
                                            {providerModel !== null ? (
                                                <span
                                                    className={
                                                        styles["rowSummary"]
                                                    }
                                                    title={
                                                        providerModelText ??
                                                        undefined
                                                    }
                                                >
                                                    {providerModelText}
                                                </span>
                                            ) : ownConfiguration ? (
                                                <span
                                                    className={
                                                        styles["rowSummary"]
                                                    }
                                                >
                                                    {t("agents.summary.own")}
                                                </span>
                                            ) : !found ? (
                                                <span
                                                    className={
                                                        styles["rowSummary"]
                                                    }
                                                >
                                                    {t("agents.summary.none")}
                                                </span>
                                            ) : hasWriteFlow(id) &&
                                              summary !== undefined ? (
                                                <span
                                                    className={
                                                        styles["rowSummary"]
                                                    }
                                                >
                                                    {t("agents.summary.none")}
                                                </span>
                                            ) : null}
                                        </span>
                                        <span
                                            className={styles["rowStatus"]}
                                            id={`agent-card-status-${id}`}
                                        >
                                            <span
                                                className={styles["statusDot"]}
                                                aria-hidden="true"
                                                data-state={
                                                    state.kind === "desktop"
                                                        ? (row?.state ??
                                                          "unavailable")
                                                        : state.kind
                                                }
                                            />
                                            <span
                                                className={styles["statusText"]}
                                                title={statusTitle}
                                            >
                                                {statusText}
                                            </span>
                                        </span>
                                    </div>
                                </div>
                                <div
                                    className={[
                                        listActions["group"],
                                        styles["rowActions"],
                                    ].join(" ")}
                                >
                                    {hasWriteFlow(id) ? (
                                        <IconButton
                                            icon="edit"
                                            blocked={
                                                state.kind === "loading" ||
                                                versionBusy
                                            }
                                            label={t("agents.list.writeLabel", {
                                                name,
                                            })}
                                            onClick={() => {
                                                const el =
                                                    document.activeElement;
                                                openWrite(
                                                    id,
                                                    el instanceof HTMLElement
                                                        ? el
                                                        : null,
                                                );
                                            }}
                                        />
                                    ) : null}
                                    <IconButton
                                        icon="info"
                                        blocked={
                                            state.kind === "loading" ||
                                            versionBusy
                                        }
                                        label={t("agents.list.openDetail", {
                                            name,
                                        })}
                                        onClick={() => {
                                            openDetail(id);
                                        }}
                                    />
                                </div>
                            </li>
                        );
                    })}
                </ul>
            </>
        );
    }

    function detailView(id: AgentId): JSX.Element {
        const row = observation(id);
        const pathValue = paths[id];
        const name = t(`agents.names.${id}`);
        const pathMatches =
            pathValue.trim() === ""
                ? row?.source !== "manual"
                : pathValue.trim() === row?.entryPath;
        const configurable = hasConfigAdapter(id);
        const segments: AgentSegment[] = hasWriteFlow(id)
            ? ["history", "skill", "mcp", "install"]
            : ["skill", "mcp", "install"];

        return (
            <AgentPage
                key={id}
                title={
                    <span className={pageStyles["titleBrand"]}>
                        <AgentIcon id={id} />
                        <span className={pageStyles["titleBrandText"]}>
                            {name}
                        </span>
                    </span>
                }
                onBack={() => {
                    returnToList(id);
                }}
                backBlocked={blocked}
                meta={
                    state.kind === "desktop" && row !== undefined
                        ? detailMeta(row)
                        : undefined
                }
            >
                {statusLine("detail")}
                {state.kind === "error" && (
                    <p className={styles["error"]} role="alert">
                        {t(`agents.errors.${state.code}`)}
                    </p>
                )}
                {row?.state === "notFound" && state.kind === "desktop" ? (
                    <p className={styles["notice"]}>
                        {t("agents.detail.notFound")}
                    </p>
                ) : null}
                <section className={styles["detailGroup"]} data-layout="page">
                    {configurable && currentConfig !== null && (
                        <CurrentConfigPin current={currentConfig} agent={id} />
                    )}
                    <div
                        className={[
                            underlineTabs["tabs"],
                            styles["segments"],
                        ].join(" ")}
                        role="tablist"
                        aria-label={t("agents.segments.ariaLabel")}
                    >
                        {segments.map((idSegment) => (
                            <button
                                key={idSegment}
                                type="button"
                                className={underlineTabs["tab"]}
                                role="tab"
                                aria-selected={segment === idSegment}
                                id={`agent-segment-${id}-${idSegment}`}
                                aria-controls={`agent-panel-${id}-${idSegment}`}
                                tabIndex={segment === idSegment ? 0 : -1}
                                onClick={() => {
                                    setSegment(idSegment);
                                }}
                            >
                                {t(`agents.segments.${idSegment}`)}
                            </button>
                        ))}
                    </div>
                    {configurable && (
                        <div
                            role="tabpanel"
                            id={`agent-panel-${id}-history`}
                            aria-labelledby={`agent-segment-${id}-history`}
                            hidden={segment !== "history"}
                        >
                            <RestoreView
                                key={JSON.stringify([id, configEpoch])}
                                blocked={blocked}
                                agent={id}
                                hidePin
                                onCurrentChange={(current) => {
                                    if (!mounted.current) return;
                                    setCurrentConfig(current);
                                }}
                                onBusyChange={(busy) => {
                                    pending.current = busy;
                                    if (mounted.current) setVersionBusy(busy);
                                }}
                            />
                        </div>
                    )}
                    {id === "codex" && (
                        <div
                            role="tabpanel"
                            id={`agent-panel-${id}-history`}
                            aria-labelledby={`agent-segment-${id}-history`}
                            hidden={segment !== "history"}
                        >
                            <CodexSettingsPanel
                                key={configEpoch}
                                mode="manage"
                                blocked={blocked}
                                onBusyChange={(busy) => {
                                    pending.current = busy;
                                    if (mounted.current) setVersionBusy(busy);
                                }}
                            />
                        </div>
                    )}
                    <div
                        role="tabpanel"
                        id={`agent-panel-${id}-skill`}
                        aria-labelledby={`agent-segment-${id}-skill`}
                        hidden={segment !== "skill"}
                    >
                        <AgentSkillPanel
                            agentId={id}
                            hidden={hidden || segment !== "skill"}
                            onOpenSkills={() => {
                                onOpenSkills?.();
                            }}
                            onBusyChange={(busy) => {
                                pending.current = busy;
                                if (mounted.current) setVersionBusy(busy);
                            }}
                        />
                    </div>
                    <div
                        role="tabpanel"
                        id={`agent-panel-${id}-mcp`}
                        aria-labelledby={`agent-segment-${id}-mcp`}
                        hidden={segment !== "mcp"}
                    >
                        <AgentMcpPanel
                            agentId={id}
                            hidden={hidden || segment !== "mcp"}
                            onOpenMcp={() => {
                                onOpenMcp?.();
                            }}
                        />
                    </div>
                    <div
                        role="tabpanel"
                        id={`agent-panel-${id}-install`}
                        aria-labelledby={`agent-segment-${id}-install`}
                        hidden={segment !== "install"}
                    >
                        <DetailPanel label={t("agents.detail.detailsHeading")}>
                            {row !== undefined && details(row)}
                            <form
                                className={detailForm["row"]}
                                aria-label={t("agents.detail.manualPathTitle")}
                                onSubmit={(event) => {
                                    event.preventDefault();
                                    void refresh();
                                }}
                            >
                                <label
                                    className={detailForm["label"]}
                                    htmlFor={`agent-entry-path-${id}`}
                                >
                                    {t("agents.pathLabel", { name })}
                                </label>
                                <span className={styles["pathField"]}>
                                    <input
                                        id={`agent-entry-path-${id}`}
                                        className={[
                                            editForm["input"],
                                            styles["pathInput"],
                                        ].join(" ")}
                                        value={pathValue}
                                        readOnly={blocked}
                                        spellCheck={false}
                                        autoComplete="off"
                                        placeholder={t(
                                            "agents.pathPlaceholder",
                                        )}
                                        onChange={(event) => {
                                            setPaths({
                                                ...paths,
                                                [id]: event.currentTarget.value,
                                            });
                                        }}
                                    />
                                    <IconButton
                                        type="submit"
                                        icon="refresh"
                                        label={t("agents.detail.applyPath")}
                                        blocked={blocked}
                                    />
                                </span>
                            </form>
                            <AgentVersionPanel
                                key={JSON.stringify([
                                    confirmationEpoch,
                                    id,
                                    row?.entryPath,
                                    row?.resolvedPath,
                                ])}
                                row={row}
                                enabled={
                                    !blocked &&
                                    !autoProbeBusy &&
                                    state.kind === "desktop" &&
                                    pathMatches
                                }
                                hidden={hidden}
                                onBusyChange={(busy) => {
                                    pending.current = busy;
                                    if (mounted.current) setVersionBusy(busy);
                                }}
                                onResult={(result) => {
                                    if (mounted.current && !hidden)
                                        setVersions((previous) => ({
                                            ...previous,
                                            [result.id]: result,
                                        }));
                                }}
                            />
                        </DetailPanel>
                    </div>
                </section>
            </AgentPage>
        );
    }

    const codexSummary = listSummaries.codex;
    const codexOurs = codexSummary?.kind === "ours" ? codexSummary : null;
    const writeRow =
        writeTarget === null || writeTarget === "codex"
            ? null
            : (() => {
                  const summary = listSummaries[writeTarget];
                  const ours = summary?.kind === "ours" ? summary : null;
                  return {
                      agent: writeTarget,
                      entryPath: observation(writeTarget)?.entryPath ?? null,
                      providerLabel: ours?.providerLabel ?? null,
                      modelLabel: ours?.modelLabel ?? null,
                  };
              })();

    return (
        <section
            className={styles["view"]}
            hidden={hidden}
            data-secondary-page={page.kind !== "list" ? true : undefined}
        >
            {page.kind === "list" ? listView() : detailView(page.id)}
            {writeTarget === "codex" && (
                <CodexWriteDialog
                    providerLabel={codexOurs?.providerLabel ?? null}
                    modelLabel={codexOurs?.modelLabel ?? null}
                    blocked={blocked}
                    onBusyChange={(busy) => {
                        pending.current = busy;
                        if (mounted.current) setVersionBusy(busy);
                    }}
                    onApplied={() => {
                        // Keep the dialog open: a deploy may be followed by
                        // its set-default review, and the outcome shows here.
                        setConfigEpoch((value) => value + 1);
                    }}
                    onClose={closeWrite}
                    returnFocus={() =>
                        writeReturnFocus.current?.isConnected
                            ? writeReturnFocus.current
                            : null
                    }
                />
            )}
            {writeRow !== null && (
                <WriteConfigDialog
                    agent={writeRow.agent}
                    agentName={t(`agents.names.${writeRow.agent}`)}
                    entryPath={writeRow.entryPath}
                    providerLabel={writeRow.providerLabel}
                    modelLabel={writeRow.modelLabel}
                    blocked={blocked}
                    onBusyChange={(busy) => {
                        pending.current = busy;
                        if (mounted.current) setVersionBusy(busy);
                    }}
                    onApplied={() => {
                        setConfigEpoch((value) => value + 1);
                        setWriteTarget(null);
                    }}
                    onClose={closeWrite}
                    returnFocus={() =>
                        writeReturnFocus.current?.isConnected
                            ? writeReturnFocus.current
                            : null
                    }
                />
            )}
        </section>
    );
}
