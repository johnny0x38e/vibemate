import { Dialog } from "@base-ui/react/dialog";
import {
    useCallback,
    useEffect,
    useImperativeHandle,
    useRef,
    useState,
    type Ref,
    type JSX,
} from "react";
import type { ModuleNavigationHandle } from "../../components/moduleNavigation";
import { useTranslation } from "react-i18next";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import listActions from "../../components/listActions.module.css";
import dialog from "./McpDeleteDialog.module.css";
import { PageModuleHeader } from "../../components/PageModuleHeader";
import { useNotify } from "../../components/Notifications";
import {
    cleanupMcpCredentials,
    cleanupDeletedMcpCredentials,
    deleteMcpDefinition,
    getMcpDefinition,
    listMcpDefinitions,
    McpRequestError,
    saveMcpDefinition,
    type McpErrorCode,
    type McpRecord,
    type SaveMcpInput,
} from "../../lib/desktop/mcp";
import { McpForm } from "./McpForm";
import { McpPiManager } from "./McpPiManager";
import { McpGrokManager } from "./McpGrokManager";
import { McpDeleteReferences } from "./McpDeleteReferences";
import { usePiMcpRegistrations } from "./usePiMcpRegistrations";
import { useGrokMcpRegistrations } from "./useGrokMcpRegistrations";
import buttons from "../../components/actionButtons.module.css";
import chip from "../../components/infoChip.module.css";
import surfaceList from "../../components/surfaceList.module.css";
import styles from "./Mcp.module.css";

type ListState =
    | { kind: "loading" | "preview" }
    | { kind: "error"; code: McpErrorCode }
    | {
          kind: "ready";
          items: McpRecord[];
          nextCursor: string | null;
          /** Library total, not the number of loaded rows. */
          total: number;
          deletedCleanupPending: boolean;
          more: "idle" | "loading";
      };
type PageState =
    | { kind: "list" }
    | { kind: "opening"; id: string }
    | { kind: "form"; record: McpRecord | null };
type ActionState =
    | { kind: "idle" | "running" }
    | { kind: "error"; code: McpErrorCode; uncertain: boolean };
/** The sidebar page stays mounted while hidden; password fields clear on hide. */
export interface McpViewProps {
    hidden?: boolean;
    homeRef?: Ref<ModuleNavigationHandle>;
}
function errorCode(error: unknown): McpErrorCode {
    return error instanceof McpRequestError ? error.code : "operation_failed";
}
function uncertain(code: McpErrorCode): boolean {
    return [
        "operation_failed",
        "invalid_response",
        "outcome_unknown",
        "revision_conflict",
    ].includes(code);
}
function preservedRequest(record: McpRecord): SaveMcpInput {
    const connection = record.connection;
    return {
        id: record.id,
        expectedRevision: record.revision,
        displayName: record.displayName,
        serverName: record.serverName,
        enabled: !record.enabled,
        connection:
            connection.type === "stdio"
                ? {
                      ...connection,
                      env: connection.env.map((name) => ({
                          name,
                          value: null,
                      })),
                  }
                : {
                      ...connection,
                      headers: connection.headers.map((name) => ({
                          name,
                          value: null,
                      })),
                  },
    };
}
/** Central definitions and reviewed Pi registration; never executes a server. */
export function McpView({
    hidden = false,
    homeRef,
}: McpViewProps): JSX.Element {
    const { t } = useTranslation();
    const notify = useNotify();
    const [list, setList] = useState<ListState>({ kind: "loading" });
    const [page, setPage] = useState<PageState>({ kind: "list" });
    const [action, setAction] = useState<ActionState>({ kind: "idle" });
    const [formBlocked, setFormBlocked] = useState(false);
    const [deploymentEpoch, setDeploymentEpoch] = useState(0);
    const [deploymentBusy, setDeploymentBusy] = useState(false);
    const registrations = usePiMcpRegistrations(hidden, deploymentEpoch);
    const grokRegistrations = useGrokMcpRegistrations(hidden, deploymentEpoch);
    const [deleteTarget, setDeleteTarget] = useState<McpRecord | null>(null);
    const [deleteAllowed, setDeleteAllowed] = useState(false);
    const [previewTarget, setPreviewTarget] = useState<{
        agent: "pi" | "grok";
        record: McpRecord;
        trigger: HTMLElement | null;
    } | null>(null);
    const cancelDelete = useRef<HTMLButtonElement>(null);
    const deleteTrigger = useRef<HTMLElement | null>(null);
    const active = useRef(true);
    const readGeneration = useRef(0);
    const pageGeneration = useRef(0);
    const mutationPending = useRef(false);
    const root = useRef<HTMLElement>(null);
    const focusRequested = useRef(false);
    const blocked =
        deploymentBusy ||
        previewTarget !== null ||
        deleteTarget !== null ||
        action.kind === "running" ||
        (action.kind === "error" && action.uncertain) ||
        (list.kind === "ready" && list.more === "loading");
    // Clear on the hidden transition before a portal can appear over another
    // module; unmounting the dialog also invalidates its in-flight read.
    const [wasHidden, setWasHidden] = useState(hidden);
    if (hidden !== wasHidden) {
        setWasHidden(hidden);
        if (hidden) setPreviewTarget(null);
    }
    useImperativeHandle(homeRef, () => ({
        openHome() {
            if (blocked || formBlocked || mutationPending.current) return false;
            if (page.kind !== "list") returnToList();
            return true;
        },
    }));
    useEffect(() => {
        active.current = true;
        return () => {
            active.current = false;
        };
    }, []);
    useEffect(() => {
        if (!hidden && focusRequested.current) {
            root.current
                ?.querySelector<HTMLElement>('[data-focus="heading"]')
                ?.focus();
            focusRequested.current = false;
        }
    });
    const reload = useCallback(async (): Promise<void> => {
        const generation = ++readGeneration.current;
        setList({ kind: "loading" });
        setAction({ kind: "idle" });
        try {
            const result = await listMcpDefinitions(null);
            if (!active.current || generation !== readGeneration.current)
                return;
            setList(
                result.kind === "preview"
                    ? { kind: "preview" }
                    : { kind: "ready", ...result.page, more: "idle" },
            );
        } catch (error: unknown) {
            if (active.current && generation === readGeneration.current)
                setList({ kind: "error", code: errorCode(error) });
        }
    }, []);
    useEffect(() => {
        // One initial read, including StrictMode cleanup protection. Revealing a
        // hidden form keeps its non-secret input instead of replacing it.
        const timer = window.setTimeout(() => {
            void reload();
        }, 0);
        return () => {
            clearTimeout(timer);
        };
    }, [reload]);
    async function open(id: string): Promise<void> {
        if (blocked || mutationPending.current) return;
        const generation = ++pageGeneration.current;
        setPage({ kind: "opening", id });
        try {
            const record = await getMcpDefinition(id);
            if (active.current && generation === pageGeneration.current) {
                setPage({ kind: "form", record });
                focusRequested.current = true;
            }
        } catch (error: unknown) {
            if (active.current && generation === pageGeneration.current) {
                setPage({ kind: "list" });
                // A failed reconciliation must not expose stale editable rows.
                setList({ kind: "error", code: errorCode(error) });
                setAction({ kind: "idle" });
            }
        }
    }
    function returnToList(): void {
        pageGeneration.current++;
        focusRequested.current = true;
        setPage({ kind: "list" });
        void reload();
    }
    async function more(): Promise<void> {
        if (
            list.kind !== "ready" ||
            list.nextCursor === null ||
            list.more === "loading" ||
            blocked
        )
            return;
        const generation = readGeneration.current;
        const snapshot = list;
        setList({ ...list, more: "loading" });
        try {
            const result = await listMcpDefinitions(snapshot.nextCursor);
            if (!active.current || generation !== readGeneration.current)
                return;
            if (result.kind === "preview") {
                setList({ kind: "preview" });
                return;
            }
            const seen = new Set(snapshot.items.map((row) => row.id));
            setList({
                kind: "ready",
                items: [
                    ...snapshot.items,
                    ...result.page.items.filter((row) => !seen.has(row.id)),
                ],
                nextCursor: result.page.nextCursor,
                // Later pages repeat the same library total.
                total: result.page.total,
                deletedCleanupPending: result.page.deletedCleanupPending,
                more: "idle",
            });
            // The last page removes Load more; focus the heading if its button was focused.
            if (
                result.page.nextCursor === null &&
                document.activeElement?.getAttribute("data-more") === "true"
            )
                focusRequested.current = true;
        } catch (error: unknown) {
            if (active.current && generation === readGeneration.current) {
                setList({ ...snapshot, more: "idle" });
                setAction({
                    kind: "error",
                    code: errorCode(error),
                    uncertain: false,
                });
            }
        }
    }
    async function mutate(record: McpRecord, cleanup: boolean): Promise<void> {
        if (blocked || mutationPending.current) return;
        mutationPending.current = true;
        setAction({ kind: "running" });
        try {
            const saved = cleanup
                ? await cleanupMcpCredentials(record.id)
                : await saveMcpDefinition(preservedRequest(record));
            if (!active.current) return;
            setList((state) =>
                state.kind === "ready"
                    ? {
                          ...state,
                          items: state.items.map((row) =>
                              row.id === saved.id ? saved : row,
                          ),
                      }
                    : state,
            );
            setAction({ kind: "idle" });
            notify({
                tone: "success",
                message: t(
                    saved.cleanupPending ? "mcp.cleanupPending" : "mcp.saved",
                ),
            });
        } catch (error: unknown) {
            if (active.current) {
                const code = errorCode(error);
                setAction({ kind: "error", code, uncertain: uncertain(code) });
            }
        } finally {
            mutationPending.current = false;
        }
    }
    async function remove(): Promise<void> {
        if (deleteTarget === null || !deleteAllowed || mutationPending.current)
            return;
        const target = deleteTarget;
        mutationPending.current = true;
        setAction({ kind: "running" });
        try {
            const deleted = await deleteMcpDefinition({
                id: target.id,
                expectedRevision: target.revision,
            });
            if (!active.current) return;
            setDeleteTarget(null);
            focusRequested.current = true;
            notify({
                tone: deleted.cleanupPending ? "error" : "success",
                message: t(
                    deleted.cleanupPending
                        ? "mcp.delete.cleanupPending"
                        : "mcp.delete.done",
                    { name: target.displayName },
                ),
            });
            // Re-read the first page rather than leaving holes in a loaded cursor page.
            await reload();
        } catch (error: unknown) {
            if (!active.current) return;
            setDeleteTarget(null);
            const code = errorCode(error);
            setAction({
                kind: "error",
                code,
                uncertain: uncertain(code) || code === "not_found",
            });
        } finally {
            mutationPending.current = false;
        }
    }
    async function cleanupDeleted(): Promise<void> {
        if (blocked || mutationPending.current) return;
        mutationPending.current = true;
        setAction({ kind: "running" });
        try {
            await cleanupDeletedMcpCredentials();
            if (active.current) await reload();
        } catch (error: unknown) {
            if (active.current)
                setAction({
                    kind: "error",
                    code: errorCode(error),
                    uncertain: false,
                });
        } finally {
            mutationPending.current = false;
        }
    }
    function saved(record: McpRecord): void {
        setPage({ kind: "list" });
        focusRequested.current = true;
        notify({
            tone: "success",
            message: t(
                record.cleanupPending ? "mcp.cleanupPending" : "mcp.saved",
            ),
        });
        void reload();
    }
    return (
        <section
            ref={root}
            className={styles["view"]}
            hidden={hidden}
            data-mcp-editor={page.kind === "form" ? true : undefined}
        >
            {page.kind !== "form" && (
                <PageModuleHeader
                    icon="mcp"
                    title={t("mcp.title")}
                    tabIndex={-1}
                    dataFocus="heading"
                    trailing={
                        page.kind === "list" ? (
                            <button
                                type="button"
                                className={[
                                    buttons["primary"],
                                    buttons["withIcon"],
                                ]
                                    .filter(
                                        (value): value is string =>
                                            value !== undefined,
                                    )
                                    .join(" ")}
                                aria-disabled={blocked || list.kind !== "ready"}
                                onClick={() => {
                                    if (!blocked && list.kind === "ready") {
                                        setPage({ kind: "form", record: null });
                                        focusRequested.current = true;
                                    }
                                }}
                            >
                                <Icon name="plus" />
                                {t("mcp.new")}
                            </button>
                        ) : undefined
                    }
                />
            )}
            {page.kind === "form" ? (
                <McpForm
                    key={`${page.record?.id ?? "new"}-${String(page.record?.revision ?? 0)}`}
                    record={page.record}
                    hidden={hidden}
                    onSaved={saved}
                    onNavigationBlockedChange={setFormBlocked}
                    onCancel={returnToList}
                    onReload={() => {
                        const id = page.record?.id;
                        if (id === undefined) returnToList();
                        else void open(id);
                    }}
                />
            ) : page.kind === "opening" ? (
                <p role="status">{t("mcp.loading")}</p>
            ) : (
                <>
                    {list.kind === "loading" && (
                        <p role="status">{t("mcp.loading")}</p>
                    )}
                    {list.kind === "preview" && (
                        <p role="status">{t("mcp.preview")}</p>
                    )}
                    {list.kind === "error" && (
                        <>
                            <p className={styles["error"]} role="alert">
                                {t(`mcp.errors.${list.code}`)}
                            </p>
                            <button
                                className={buttons["secondary"]}
                                type="button"
                                onClick={() => {
                                    void reload();
                                }}
                            >
                                {t("mcp.retry")}
                            </button>
                        </>
                    )}
                    {action.kind === "error" && (
                        <>
                            <p className={styles["error"]} role="alert">
                                {t(`mcp.errors.${action.code}`)}
                            </p>
                            {action.uncertain && (
                                <>
                                    <p className={styles["hint"]}>
                                        {t("mcp.reloadUnknown")}
                                    </p>
                                    <button
                                        className={buttons["secondary"]}
                                        type="button"
                                        onClick={() => {
                                            void reload();
                                        }}
                                    >
                                        {t("mcp.reload")}
                                    </button>
                                </>
                            )}
                        </>
                    )}
                    {action.kind === "running" && (
                        <p role="status">{t("mcp.saving")}</p>
                    )}
                    {list.kind === "ready" && (
                        <>
                            {list.deletedCleanupPending && (
                                <div>
                                    <p role="status">
                                        {t("mcp.delete.cleanupPending")}
                                    </p>
                                    <button
                                        type="button"
                                        className={buttons["secondary"]}
                                        aria-disabled={blocked}
                                        onClick={() => {
                                            void cleanupDeleted();
                                        }}
                                    >
                                        {t("mcp.delete.cleanup")}
                                    </button>
                                </div>
                            )}
                            {list.total > 0 && (
                                <p className={styles["count"]}>
                                    {t("mcp.count", { count: list.total })}
                                </p>
                            )}
                            {list.items.length === 0 ? (
                                <p role="status">{t("mcp.empty")}</p>
                            ) : (
                                <ul
                                    className={[
                                        surfaceList["list"],
                                        styles["list"],
                                    ].join(" ")}
                                    aria-label={t("mcp.title")}
                                >
                                    {list.items.map((record) => (
                                        <li
                                            className={[
                                                surfaceList["item"],
                                                styles["listItem"],
                                            ].join(" ")}
                                            key={record.id}
                                        >
                                            <div className={styles["details"]}>
                                                <h2 className={styles["name"]}>
                                                    {record.displayName}
                                                </h2>
                                                <p
                                                    className={
                                                        styles["subline"]
                                                    }
                                                >
                                                    {[
                                                        registrations.kind ===
                                                        "ready"
                                                            ? t(
                                                                  registrations.value.deployments.some(
                                                                      (item) =>
                                                                          item.definitionId ===
                                                                          record.id,
                                                                  )
                                                                      ? "mcp.pi.manage.registeredSummary"
                                                                      : "mcp.pi.states.unregistered",
                                                              )
                                                            : t(
                                                                  "mcp.pi.manage.summaryUnknown",
                                                              ),
                                                        grokRegistrations.kind ===
                                                        "ready"
                                                            ? t(
                                                                  grokRegistrations.value.deployments.some(
                                                                      (item) =>
                                                                          item.definitionId ===
                                                                          record.id,
                                                                  )
                                                                      ? "mcp.grok.manage.registeredSummary"
                                                                      : "mcp.grok.states.unregistered",
                                                              )
                                                            : t(
                                                                  "mcp.grok.manage.summaryUnknown",
                                                              ),
                                                    ].join(" · ")}
                                                </p>
                                                {record.cleanupPending && (
                                                    <p
                                                        className={
                                                            styles["hint"]
                                                        }
                                                    >
                                                        {t(
                                                            "mcp.cleanupPending",
                                                        )}
                                                    </p>
                                                )}
                                            </div>
                                            <div className={styles["trailing"]}>
                                                <span className={chip["chip"]}>
                                                    {t(
                                                        record.connection
                                                            .type === "stdio"
                                                            ? "mcp.stdio"
                                                            : "mcp.http",
                                                    )}
                                                </span>
                                                <div
                                                    className={
                                                        listActions["group"]
                                                    }
                                                >
                                                    <IconButton
                                                        icon="info"
                                                        blocked={blocked}
                                                        label={t(
                                                            "mcp.pi.manage.open",
                                                            {
                                                                name: record.displayName,
                                                            },
                                                        )}
                                                        onClick={() => {
                                                            if (
                                                                blocked ||
                                                                mutationPending.current
                                                            )
                                                                return;
                                                            setPreviewTarget({
                                                                agent: "pi",
                                                                record,
                                                                trigger:
                                                                    document.activeElement instanceof
                                                                    HTMLElement
                                                                        ? document.activeElement
                                                                        : null,
                                                            });
                                                        }}
                                                    />
                                                    <IconButton
                                                        icon="mcp"
                                                        blocked={blocked}
                                                        label={t(
                                                            "mcp.grok.manage.open",
                                                            {
                                                                name: record.displayName,
                                                            },
                                                        )}
                                                        onClick={() => {
                                                            if (
                                                                blocked ||
                                                                mutationPending.current
                                                            )
                                                                return;
                                                            setPreviewTarget({
                                                                agent: "grok",
                                                                record,
                                                                trigger:
                                                                    document.activeElement instanceof
                                                                    HTMLElement
                                                                        ? document.activeElement
                                                                        : null,
                                                            });
                                                        }}
                                                    />
                                                    <IconButton
                                                        icon="edit"
                                                        blocked={blocked}
                                                        label={t(
                                                            "mcp.editLabel",
                                                            {
                                                                name: record.displayName,
                                                            },
                                                        )}
                                                        onClick={() => {
                                                            void open(
                                                                record.id,
                                                            );
                                                        }}
                                                    />
                                                    <IconButton
                                                        icon={
                                                            record.enabled
                                                                ? "disable"
                                                                : "enable"
                                                        }
                                                        blocked={blocked}
                                                        label={t(
                                                            record.enabled
                                                                ? "mcp.toggleDisable"
                                                                : "mcp.toggleEnable",
                                                            {
                                                                name: record.displayName,
                                                            },
                                                        )}
                                                        onClick={() => {
                                                            void mutate(
                                                                record,
                                                                false,
                                                            );
                                                        }}
                                                    />
                                                    <IconButton
                                                        icon="trash"
                                                        destructive
                                                        label={t(
                                                            "mcp.delete.label",
                                                            {
                                                                name: record.displayName,
                                                            },
                                                        )}
                                                        blocked={blocked}
                                                        onClick={() => {
                                                            if (
                                                                blocked ||
                                                                mutationPending.current
                                                            )
                                                                return;
                                                            deleteTrigger.current =
                                                                document.activeElement instanceof
                                                                HTMLElement
                                                                    ? document.activeElement
                                                                    : null;
                                                            setDeleteAllowed(
                                                                false,
                                                            );
                                                            setDeleteTarget(
                                                                record,
                                                            );
                                                        }}
                                                    />
                                                    {record.cleanupPending && (
                                                        <IconButton
                                                            icon="refresh"
                                                            blocked={blocked}
                                                            label={t(
                                                                "mcp.cleanup",
                                                            )}
                                                            onClick={() => {
                                                                void mutate(
                                                                    record,
                                                                    true,
                                                                );
                                                            }}
                                                        />
                                                    )}
                                                </div>
                                            </div>
                                        </li>
                                    ))}
                                </ul>
                            )}
                            {list.nextCursor !== null && (
                                <button
                                    className={buttons["secondary"]}
                                    type="button"
                                    data-more="true"
                                    aria-disabled={
                                        blocked || list.more === "loading"
                                    }
                                    onClick={() => {
                                        void more();
                                    }}
                                >
                                    {t("mcp.loadMore")}
                                </button>
                            )}
                        </>
                    )}
                </>
            )}
            {!hidden && previewTarget?.agent === "grok" && (
                <McpGrokManager
                    key={`${previewTarget.record.id}-${String(previewTarget.record.revision)}`}
                    record={previewTarget.record}
                    returnFocus={previewTarget.trigger}
                    onClose={() => {
                        setPreviewTarget(null);
                    }}
                    onReload={() => {
                        setPreviewTarget(null);
                        void reload();
                    }}
                    onBusyChange={(busy) => {
                        mutationPending.current = busy;
                        if (active.current) setDeploymentBusy(busy);
                    }}
                    onChanged={() => {
                        if (active.current)
                            setDeploymentEpoch((value) => value + 1);
                    }}
                />
            )}
            {!hidden && previewTarget?.agent === "pi" && (
                <McpPiManager
                    key={`${previewTarget.record.id}-${String(previewTarget.record.revision)}`}
                    record={previewTarget.record}
                    returnFocus={previewTarget.trigger}
                    onClose={() => {
                        setPreviewTarget(null);
                    }}
                    onReload={() => {
                        setPreviewTarget(null);
                        void reload();
                    }}
                    onBusyChange={(busy) => {
                        mutationPending.current = busy;
                        if (active.current) setDeploymentBusy(busy);
                    }}
                    onChanged={() => {
                        if (active.current)
                            setDeploymentEpoch((value) => value + 1);
                    }}
                />
            )}
            <Dialog.Root
                open={deleteTarget !== null}
                onOpenChange={(open) => {
                    if (!open && !mutationPending.current)
                        setDeleteTarget(null);
                }}
            >
                <Dialog.Portal>
                    <Dialog.Backdrop className={dialog["backdrop"]} />
                    <Dialog.Popup
                        className={dialog["popup"]}
                        initialFocus={cancelDelete}
                        // A successful delete removes its trigger; return focus
                        // to the stable heading when that row no longer exists.
                        finalFocus={() =>
                            deleteTrigger.current?.isConnected
                                ? deleteTrigger.current
                                : (root.current?.querySelector<HTMLElement>(
                                      '[data-focus="heading"]',
                                  ) ?? null)
                        }
                    >
                        <Dialog.Title className={dialog["title"]}>
                            {t("mcp.delete.title")}
                        </Dialog.Title>
                        <Dialog.Description className={dialog["body"]}>
                            {t("mcp.delete.body", {
                                name: deleteTarget?.displayName ?? "",
                            })}
                        </Dialog.Description>
                        {deleteTarget !== null && (
                            <McpDeleteReferences
                                key={deleteTarget.id}
                                definitionId={deleteTarget.id}
                                onChecked={setDeleteAllowed}
                                onManage={(agent) => {
                                    setPreviewTarget({
                                        agent,
                                        record: deleteTarget,
                                        trigger: deleteTrigger.current,
                                    });
                                    setDeleteTarget(null);
                                }}
                            />
                        )}
                        <div className={dialog["actions"]}>
                            <button
                                ref={cancelDelete}
                                type="button"
                                className={buttons["secondary"]}
                                disabled={action.kind === "running"}
                                onClick={() => {
                                    if (!mutationPending.current)
                                        setDeleteTarget(null);
                                }}
                            >
                                {t("mcp.delete.cancel")}
                            </button>
                            <button
                                type="button"
                                className={buttons["dangerSecondary"]}
                                aria-disabled={
                                    action.kind === "running" || !deleteAllowed
                                }
                                onClick={() => {
                                    void remove();
                                }}
                            >
                                {t(
                                    action.kind === "running"
                                        ? "mcp.delete.deleting"
                                        : "mcp.delete.confirm",
                                )}
                            </button>
                        </div>
                    </Dialog.Popup>
                </Dialog.Portal>
            </Dialog.Root>
        </section>
    );
}
