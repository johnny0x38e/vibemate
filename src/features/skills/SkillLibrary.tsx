import {
    useCallback,
    useEffect,
    useImperativeHandle,
    useRef,
    useState,
    type JSX,
    type Ref,
} from "react";
import { Dialog } from "@base-ui/react/dialog";
import type { ModuleNavigationHandle } from "../../components/moduleNavigation";
import { useTranslation } from "react-i18next";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { PageModuleHeader } from "../../components/PageModuleHeader";
import { useNotify } from "../../components/Notifications";
import {
    cleanupDeletedSkillCopies,
    deleteSkill,
    getSkill,
    importSkillFolder,
    listSkillDeployments,
    listSkills,
    pickSkillFolder,
    SkillRequestError,
    type ImportReport,
    type SkillDeploymentRecord,
    type SkillErrorCode,
    type SkillInstallReport,
    type SkillRecord,
} from "../../lib/desktop/skills";
import { SkillDetail } from "./SkillDetail";
import { SkillInstallDialog } from "./SkillInstallDialog";
import { SkillMarketplacePage } from "./SkillMarketplacePage";
import { SkillRecoveryPanel } from "./SkillRecoveryPanel";
import { skillSource } from "./skillSource";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./SkillDeleteDialog.module.css";
import surfaceList from "../../components/surfaceList.module.css";
import styles from "./SkillLibrary.module.css";

type ListState =
    | { kind: "loading" | "preview" }
    | { kind: "error"; code: SkillErrorCode }
    | {
          kind: "ready";
          items: SkillRecord[];
          nextCursor: string | null;
          /** Library total, not the number of loaded rows. */
          total: number;
          more: "idle" | "loading";
      };
type PageState =
    | { kind: "list" }
    | { kind: "detail"; record: SkillRecord }
    | { kind: "marketplace" };
type DeleteReferences =
    | { kind: "idle" | "loading" | "preview" }
    | { kind: "ready"; items: SkillDeploymentRecord[] }
    | { kind: "error"; code: SkillErrorCode };
type ActionState =
    | { kind: "idle" | "picking" | "importing" | "deleting" }
    | { kind: "error"; code: SkillErrorCode; uncertain: boolean };

/**
 * The source line of one list row: the recorded address with its revision, the
 * local import, or an honest unknown value.
 */
function rowSourceText(
    record: SkillRecord,
    t: (key: string, options?: Record<string, unknown>) => string,
): string {
    const source = skillSource(record);
    if (source.kind !== "address") {
        return t(
            source.kind === "local"
                ? "skills.sourceLocalRow"
                : "skills.sourceUnknownValue",
        );
    }
    const address = t("skills.sourceAddress", { url: source.url });
    const revision =
        source.revision === null
            ? ""
            : ` · ${t("skills.sourceRevision", { revision: source.revision })}`;
    const method =
        source.fetchMethod === "git"
            ? t("skills.sourceGit")
            : source.fetchMethod === "npx"
              ? t("skills.sourceNpx", {
                    version: source.installerVersion ?? "",
                })
              : t("skills.sourceManual");
    const update =
        record.updateCheckStatus === "update_available"
            ? ` · ${t("skills.update.revisionStatuses.update_available")}`
            : "";
    return `${address}${revision} · ${method}${update}`;
}

/** The sidebar page stays mounted while hidden, so drafts and pages survive. */
export interface SkillLibraryProps {
    hidden?: boolean;
    homeRef?: Ref<ModuleNavigationHandle>;
}

function errorCode(error: unknown): SkillErrorCode {
    return error instanceof SkillRequestError ? error.code : "operation_failed";
}

/** Failures that may have changed something beyond this window's knowledge. */
function uncertain(code: SkillErrorCode): boolean {
    return [
        "operation_failed",
        "invalid_response",
        "outcome_unknown",
        "write_failed",
        "storage_unavailable",
        "revision_conflict",
    ].includes(code);
}

/**
 * Central skill library: import local skill folders, then list what the library
 * holds. The list stays compact; one skill's files, size, digest and source live
 * on its detail page. Importing copies files into the private library, leaves the
 * chosen folders untouched, and never executes a skill command.
 */
export function SkillLibrary({
    hidden,
    homeRef,
}: SkillLibraryProps): JSX.Element {
    const { t } = useTranslation();
    const notify = useNotify();
    const [list, setList] = useState<ListState>({ kind: "loading" });
    const [page, setPage] = useState<PageState>({ kind: "list" });
    const [action, setAction] = useState<ActionState>({ kind: "idle" });
    const [report, setReport] = useState<ImportReport | null>(null);
    const [formBlocked, setFormBlocked] = useState(false);
    const [installOpen, setInstallOpen] = useState(false);
    const [deleteTarget, setDeleteTarget] = useState<SkillRecord | null>(null);
    const [deleteReferences, setDeleteReferences] = useState<{
        targetId: string | null;
        state: DeleteReferences;
    }>({ targetId: null, state: { kind: "idle" } });
    // A deletion whose files could not be removed keeps offering the retry.
    const [cleanupPending, setCleanupPending] = useState(false);
    const active = useRef(true);
    const readGeneration = useRef(0);
    const pageGeneration = useRef(0);
    const mutationPending = useRef(false);
    const root = useRef<HTMLElement>(null);
    const focusRequested = useRef(false);
    const cancelDelete = useRef<HTMLButtonElement>(null);
    const deleteTrigger = useRef<HTMLElement | null>(null);
    const installTrigger = useRef<HTMLButtonElement>(null);
    const busy = action.kind === "picking" || action.kind === "importing";
    // A running deletion shows its progress on the dialog button, so it blocks
    // the list without joining the import progress area.
    const blocked =
        busy ||
        installOpen ||
        action.kind === "deleting" ||
        (action.kind === "error" && action.uncertain) ||
        (list.kind === "ready" && list.more === "loading");
    useImperativeHandle(homeRef, () => ({
        openHome() {
            if (blocked || formBlocked || mutationPending.current) return false;
            if (page.kind !== "list") returnToList();
            return true;
        },
    }));
    useEffect(() => {
        // Async replies after unmount must not touch state.
        active.current = true;
        return () => {
            active.current = false;
        };
    }, []);
    // Read through a function so TypeScript cannot narrow the ref value: the
    // picker and the import are two separate awaits in one handler.
    const stillMounted = useCallback((): boolean => active.current, []);
    useEffect(() => {
        // The heading is focused after an async recovery, once it can be seen.
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
            const result = await listSkills(null);
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
        // One initial read, including StrictMode cleanup protection.
        const timer = window.setTimeout(() => {
            void reload();
        }, 0);
        return () => {
            clearTimeout(timer);
        };
    }, [reload]);
    // Reset in render when a different row opens its confirmation, so the effect
    // only starts the read and never calls setState synchronously for loading.
    if (
        deleteTarget !== null &&
        deleteReferences.targetId !== deleteTarget.id
    ) {
        setDeleteReferences({
            targetId: deleteTarget.id,
            state: { kind: "loading" },
        });
    }
    const referencesState: DeleteReferences =
        deleteTarget === null
            ? { kind: "idle" }
            : deleteReferences.targetId === deleteTarget.id
              ? deleteReferences.state
              : { kind: "loading" };
    const canDeleteSkill =
        action.kind !== "deleting" &&
        referencesState.kind === "ready" &&
        referencesState.items.length === 0;

    useEffect(() => {
        if (deleteTarget === null) return;
        let active = true;
        void listSkillDeployments(deleteTarget.id).then(
            (result) => {
                if (!active) return;
                setDeleteReferences({
                    targetId: deleteTarget.id,
                    state:
                        result.kind === "preview"
                            ? { kind: "preview" }
                            : { kind: "ready", items: result.items },
                });
            },
            (error: unknown) => {
                if (active) {
                    setDeleteReferences({
                        targetId: deleteTarget.id,
                        state: {
                            kind: "error",
                            code: errorCode(error),
                        },
                    });
                }
            },
        );
        return () => {
            active = false;
        };
    }, [deleteTarget]);

    function returnToList(): void {
        pageGeneration.current++;
        focusRequested.current = true;
        setPage({ kind: "list" });
    }

    function closeDeleteDialog(): void {
        setDeleteTarget(null);
        setDeleteReferences({ targetId: null, state: { kind: "idle" } });
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
            const result = await listSkills(snapshot.nextCursor);
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
                more: "idle",
            });
            // The last page removes Load more; keep focus on the heading instead.
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
    /**
     * Ask the desktop app for a folder, then import what it holds: one skill or
     * every skill folder inside it. Closing the picker changes nothing, and a
     * refused folder is reported instead of failing the whole import.
     */
    async function chooseFolder(): Promise<void> {
        if (blocked || mutationPending.current) return;
        mutationPending.current = true;
        setReport(null);
        setAction({ kind: "picking" });
        try {
            const picked = await pickSkillFolder(t("skills.pickTitle"));
            if (!stillMounted()) return;
            if (picked.kind === "cancelled") {
                setAction({ kind: "idle" });
                return;
            }
            setAction({ kind: "importing" });
            const result = await importSkillFolder(picked.path);
            if (!stillMounted()) return;
            setReport(result);
            setAction({ kind: "idle" });
            // A clean import is announced once; a partial one is explained by the
            // report panel, which already states the counts.
            if (result.imported.length > 0 && result.skipped.length === 0) {
                notify({
                    tone: "success",
                    message: t("skills.reportImported", {
                        count: result.imported.length,
                    }),
                });
            }
            // Imported rows can belong on a later page, so the first page is re-read.
            await reload();
        } catch (error: unknown) {
            if (active.current) {
                const code = errorCode(error);
                setAction({ kind: "error", code, uncertain: uncertain(code) });
            }
        } finally {
            mutationPending.current = false;
        }
    }
    async function openDetail(record: SkillRecord): Promise<void> {
        if (blocked || mutationPending.current) return;
        const generation = ++pageGeneration.current;
        setPage({ kind: "detail", record });
        try {
            const current = await getSkill(record.id);
            // Re-read the revision so an edited source cannot overwrite a newer one.
            if (active.current && generation === pageGeneration.current)
                setPage({ kind: "detail", record: current });
        } catch (error: unknown) {
            if (active.current && generation === pageGeneration.current) {
                setPage({ kind: "list" });
                setList({ kind: "error", code: errorCode(error) });
                setAction({ kind: "idle" });
            }
        }
    }
    /** A remote install may add rows on a later page, so re-read the first page. */
    function installed(report: SkillInstallReport): void {
        if (report.imported.length > 0) {
            notify({
                tone: "success",
                message: t("skills.reportImported", {
                    count: report.imported.length,
                }),
            });
        }
        void reload();
    }

    function sourceSaved(record: SkillRecord): void {
        // The saved record is authoritative, so the detail page and the list row
        // both show it without another read.
        setPage({ kind: "detail", record });
        notify({ tone: "success", message: t("skills.sourceSaved") });
        setList((state) =>
            state.kind === "ready"
                ? {
                      ...state,
                      items: state.items.map((row) =>
                          row.id === record.id ? record : row,
                      ),
                  }
                : state,
        );
    }
    /**
     * Delete the reviewed skill record and vibemate's managed copy. The
     * confirmation carries the revision the user saw, so a record that changed
     * meanwhile is refused and the list is re-read instead of overwritten. The
     * folder the skill was imported from is never touched.
     */
    async function remove(): Promise<void> {
        if (
            deleteTarget === null ||
            mutationPending.current ||
            referencesState.kind !== "ready" ||
            referencesState.items.length > 0
        )
            return;
        const target = deleteTarget;
        mutationPending.current = true;
        setAction({ kind: "deleting" });
        try {
            const deleted = await deleteSkill({
                id: target.id,
                expectedRevision: target.revision,
            });
            if (!stillMounted()) return;
            closeDeleteDialog();
            focusRequested.current = true;
            // Files that could not be removed are a real leftover, so the
            // deletion is reported with the retry instead of as fully clean.
            setCleanupPending(deleted.cleanupPending);
            notify({
                tone: deleted.cleanupPending ? "error" : "success",
                message: t(
                    deleted.cleanupPending
                        ? "skills.delete.cleanupPending"
                        : "skills.delete.done",
                    { name: target.name },
                ),
            });
            // Re-read the first page rather than leaving holes in a loaded page.
            await reload();
        } catch (error: unknown) {
            if (!stillMounted()) return;
            closeDeleteDialog();
            const code = errorCode(error);
            setAction({
                kind: "error",
                code,
                // A missing skill is also a stale view that needs a reload.
                uncertain: uncertain(code) || code === "not_found",
            });
        } finally {
            mutationPending.current = false;
        }
    }
    function targetAgentLabel(
        agentId: SkillDeploymentRecord["agentId"],
    ): string {
        if (agentId === "pi") return t("skills.deploy.agentPi");
        if (agentId === "grokBuild") return t("skills.deploy.agentGrokBuild");
        return t("skills.deploy.agentClaudeCode");
    }

    /** Retry removing parked copies after a deletion could not clean up. */
    async function cleanupDeleted(): Promise<void> {
        if (blocked || mutationPending.current) return;
        mutationPending.current = true;
        setAction({ kind: "deleting" });
        try {
            const complete = await cleanupDeletedSkillCopies();
            if (!stillMounted()) return;
            setCleanupPending(!complete);
            setAction({ kind: "idle" });
            notify({
                tone: complete ? "success" : "error",
                message: t(
                    complete
                        ? "skills.delete.cleanupDone"
                        : "skills.delete.cleanupPending",
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
    return (
        <section
            ref={root}
            className={styles["view"]}
            hidden={hidden}
            data-secondary-page={
                page.kind === "detail" || page.kind === "marketplace"
                    ? true
                    : undefined
            }
        >
            {page.kind === "detail" ? (
                <SkillDetail
                    key={`${page.record.id}-${String(page.record.revision)}`}
                    record={page.record}
                    onSaved={sourceSaved}
                    onCancel={returnToList}
                    onReload={() => {
                        void openDetail(page.record);
                    }}
                    onNavigationBlockedChange={setFormBlocked}
                />
            ) : page.kind === "marketplace" ? (
                <SkillMarketplacePage
                    onCancel={returnToList}
                    onInstalled={installed}
                    onNavigationBlockedChange={setFormBlocked}
                />
            ) : (
                <>
                    <PageModuleHeader
                        icon="skills"
                        title={t("skills.title")}
                        tabIndex={-1}
                        dataFocus="heading"
                        trailing={
                            <div className={styles["headerActions"]}>
                                {/* Marketplace browse, hand-entered install and
                                local import are separate acquisition paths. */}
                                <button
                                    className={[
                                        buttons["secondary"],
                                        buttons["withIcon"],
                                    ].join(" ")}
                                    type="button"
                                    aria-disabled={blocked}
                                    onClick={() => {
                                        if (blocked || mutationPending.current)
                                            return;
                                        setPage({ kind: "marketplace" });
                                    }}
                                >
                                    <Icon name="skills" />
                                    {t("skills.marketplace.open")}
                                </button>
                                <button
                                    ref={installTrigger}
                                    className={[
                                        buttons["secondary"],
                                        buttons["withIcon"],
                                    ].join(" ")}
                                    type="button"
                                    aria-disabled={blocked}
                                    onClick={() => {
                                        setInstallOpen(true);
                                    }}
                                >
                                    <Icon name="plus" />
                                    {t("skills.install.open")}
                                </button>
                                <button
                                    className={[
                                        buttons["primary"],
                                        buttons["withIcon"],
                                    ].join(" ")}
                                    type="button"
                                    aria-disabled={blocked}
                                    onClick={() => {
                                        void chooseFolder();
                                    }}
                                >
                                    <Icon name="folder" />
                                    {t("skills.localImport")}
                                </button>
                            </div>
                        }
                    />
                    {/* Import and cleanup feedback sits under the header, next
                    to the list. */}
                    {(busy ||
                        action.kind === "error" ||
                        cleanupPending ||
                        (report !== null && report.skipped.length > 0)) && (
                        <div className={styles["importFeedback"]}>
                            {busy && (
                                <p className={styles["hint"]} role="status">
                                    {t(
                                        action.kind === "picking"
                                            ? "skills.picking"
                                            : "skills.importing",
                                    )}
                                </p>
                            )}
                            {report !== null && report.skipped.length > 0 && (
                                <div className={styles["report"]}>
                                    <p
                                        className={
                                            report.imported.length === 0
                                                ? styles["error"]
                                                : styles["hint"]
                                        }
                                        role="status"
                                    >
                                        {report.imported.length === 0
                                            ? t("skills.reportNone")
                                            : t("skills.reportImported", {
                                                  count: report.imported.length,
                                              })}
                                    </p>
                                    <p className={styles["hint"]}>
                                        {t("skills.reportSkipped", {
                                            count: report.skipped.length,
                                        })}
                                    </p>
                                    <ul className={styles["reportList"]}>
                                        {report.skipped.map((row) => (
                                            <li key={row.folder}>
                                                {t("skills.reportItem", {
                                                    folder: row.folder,
                                                    reason: t(
                                                        `skills.errors.${row.code}`,
                                                    ),
                                                })}
                                            </li>
                                        ))}
                                    </ul>
                                </div>
                            )}
                            {action.kind === "error" && (
                                <>
                                    <p className={styles["error"]} role="alert">
                                        {t(`skills.errors.${action.code}`)}
                                    </p>
                                    {action.uncertain && (
                                        <button
                                            className={buttons["secondary"]}
                                            type="button"
                                            onClick={() => {
                                                void reload();
                                            }}
                                        >
                                            {t("skills.reload")}
                                        </button>
                                    )}
                                </>
                            )}
                            {cleanupPending && (
                                <>
                                    <p className={styles["hint"]} role="status">
                                        {t("skills.delete.cleanupPending")}
                                    </p>
                                    <button
                                        className={buttons["secondary"]}
                                        type="button"
                                        onClick={() => {
                                            void cleanupDeleted();
                                        }}
                                    >
                                        {t("skills.delete.cleanup")}
                                    </button>
                                </>
                            )}
                        </div>
                    )}
                    <SkillRecoveryPanel />
                    {list.kind === "loading" && (
                        <p role="status">{t("skills.loading")}</p>
                    )}
                    {list.kind === "preview" && (
                        <p role="status">{t("skills.preview")}</p>
                    )}
                    {list.kind === "error" && (
                        <>
                            <p className={styles["error"]} role="alert">
                                {t(`skills.errors.${list.code}`)}
                            </p>
                            <button
                                className={buttons["secondary"]}
                                type="button"
                                onClick={() => {
                                    void reload();
                                }}
                            >
                                {t("skills.retry")}
                            </button>
                        </>
                    )}
                    {list.kind === "ready" && (
                        <>
                            {list.total > 0 && (
                                <p className={styles["count"]}>
                                    {t("skills.count", { count: list.total })}
                                </p>
                            )}
                            {list.items.length === 0 ? (
                                <p role="status">{t("skills.empty")}</p>
                            ) : (
                                <ul
                                    className={[
                                        surfaceList["list"],
                                        styles["list"],
                                    ].join(" ")}
                                    aria-label={t("skills.list")}
                                >
                                    {list.items.map((record) => (
                                        <li
                                            className={surfaceList["item"]}
                                            key={record.id}
                                        >
                                            <div className={styles["details"]}>
                                                <h2 className={styles["name"]}>
                                                    {record.name}
                                                </h2>
                                                {/* Long text is clamped here; the
                                                detail page shows it in full. */}
                                                <p
                                                    className={
                                                        styles["description"]
                                                    }
                                                >
                                                    {record.description}
                                                </p>
                                                <p className={styles["meta"]}>
                                                    {rowSourceText(record, t)}
                                                </p>
                                            </div>
                                            <div className={styles["trailing"]}>
                                                <IconButton
                                                    icon="edit"
                                                    blocked={blocked}
                                                    label={t(
                                                        "skills.detailsLabel",
                                                        { name: record.name },
                                                    )}
                                                    onClick={() => {
                                                        void openDetail(record);
                                                    }}
                                                />
                                                <IconButton
                                                    icon="trash"
                                                    destructive
                                                    blocked={blocked}
                                                    label={t(
                                                        "skills.delete.label",
                                                        { name: record.name },
                                                    )}
                                                    onClick={() => {
                                                        if (
                                                            blocked ||
                                                            mutationPending.current
                                                        )
                                                            return;
                                                        // Remember the row so
                                                        // focus can return to it,
                                                        // or to the heading once
                                                        // the row is gone.
                                                        deleteTrigger.current =
                                                            document.activeElement instanceof
                                                            HTMLElement
                                                                ? document.activeElement
                                                                : null;
                                                        setDeleteTarget(record);
                                                    }}
                                                />
                                            </div>
                                        </li>
                                    ))}
                                </ul>
                            )}
                            <p className={styles["hint"]}>
                                {t("skills.notDeployed")}
                            </p>
                            {list.nextCursor !== null && (
                                <button
                                    className={buttons["secondary"]}
                                    type="button"
                                    data-more="true"
                                    aria-disabled={blocked}
                                    onClick={() => {
                                        void more();
                                    }}
                                >
                                    {list.more === "loading"
                                        ? t("skills.loadingMore")
                                        : t("skills.loadMore")}
                                </button>
                            )}
                        </>
                    )}
                </>
            )}
            <Dialog.Root
                open={deleteTarget !== null}
                onOpenChange={(open) => {
                    if (!open && !mutationPending.current) closeDeleteDialog();
                }}
            >
                <Dialog.Portal>
                    <Dialog.Backdrop className={dialog["backdrop"]} />
                    <Dialog.Popup
                        className={dialog["popup"]}
                        initialFocus={cancelDelete}
                        // A successful delete removes its row; return focus to
                        // the stable heading when that trigger no longer exists.
                        finalFocus={() =>
                            deleteTrigger.current?.isConnected
                                ? deleteTrigger.current
                                : (root.current?.querySelector<HTMLElement>(
                                      '[data-focus="heading"]',
                                  ) ?? null)
                        }
                    >
                        <Dialog.Title className={dialog["title"]}>
                            {t("skills.delete.title")}
                        </Dialog.Title>
                        <Dialog.Description className={dialog["body"]}>
                            {t("skills.delete.body", {
                                name: deleteTarget?.name ?? "",
                            })}
                        </Dialog.Description>
                        {referencesState.kind === "loading" && (
                            <p className={dialog["body"]} role="status">
                                {t("skills.delete.checking")}
                            </p>
                        )}
                        {referencesState.kind === "preview" && (
                            <p className={dialog["body"]}>
                                {t("skills.delete.referencesPreview")}
                            </p>
                        )}
                        {referencesState.kind === "error" && (
                            <p className={dialog["body"]} role="alert">
                                {t(`skills.errors.${referencesState.code}`)}
                            </p>
                        )}
                        {referencesState.kind === "ready" &&
                            referencesState.items.length > 0 && (
                                <div className={dialog["references"]}>
                                    <p className={dialog["body"]} role="alert">
                                        {t("skills.delete.referencesHint")}
                                    </p>
                                    <ul
                                        className={dialog["referenceList"]}
                                        aria-label={t(
                                            "skills.delete.referencesLabel",
                                        )}
                                    >
                                        {referencesState.items.map((row) => (
                                            <li
                                                className={
                                                    dialog["referenceRow"]
                                                }
                                                key={`${row.agentId}:${row.configRoot}`}
                                            >
                                                {t("skills.deploy.rowSummary", {
                                                    agent: targetAgentLabel(
                                                        row.agentId,
                                                    ),
                                                    mode: t(
                                                        row.mode === "symlink"
                                                            ? "skills.deploy.modeSymlink"
                                                            : "skills.deploy.modeCopy",
                                                    ),
                                                    revision: row.skillRevision,
                                                    sync: t(
                                                        row.inSync
                                                            ? "skills.deploy.inSync"
                                                            : "skills.deploy.outOfSync",
                                                    ),
                                                })}
                                            </li>
                                        ))}
                                    </ul>
                                    <button
                                        type="button"
                                        className={buttons["secondary"]}
                                        disabled={action.kind === "deleting"}
                                        onClick={() => {
                                            if (
                                                deleteTarget === null ||
                                                mutationPending.current
                                            )
                                                return;
                                            const target = deleteTarget;
                                            closeDeleteDialog();
                                            void openDetail(target);
                                        }}
                                    >
                                        {t("skills.delete.manage")}
                                    </button>
                                </div>
                            )}
                        <div className={dialog["actions"]}>
                            <button
                                ref={cancelDelete}
                                type="button"
                                className={buttons["secondary"]}
                                disabled={action.kind === "deleting"}
                                onClick={() => {
                                    if (!mutationPending.current)
                                        closeDeleteDialog();
                                }}
                            >
                                {t("skills.delete.cancel")}
                            </button>
                            <button
                                type="button"
                                className={buttons["dangerSecondary"]}
                                aria-disabled={!canDeleteSkill}
                                onClick={() => {
                                    if (!canDeleteSkill) return;
                                    void remove();
                                }}
                            >
                                {t(
                                    action.kind === "deleting"
                                        ? "skills.delete.deleting"
                                        : "skills.delete.confirm",
                                )}
                            </button>
                        </div>
                    </Dialog.Popup>
                </Dialog.Portal>
            </Dialog.Root>
            <SkillInstallDialog
                open={installOpen}
                onOpenChange={setInstallOpen}
                onInstalled={installed}
                returnFocus={installTrigger}
            />
        </section>
    );
}
