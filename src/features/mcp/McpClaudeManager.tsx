import { Dialog } from "@base-ui/react/dialog";
import { useEffect, useRef, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import type { McpRecord } from "../../lib/desktop/mcp";
import { ClaudeMcpReview } from "./ClaudeMcpReview";
import { useClaudeMcpManager } from "./useClaudeMcpManager";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./McpDeleteDialog.module.css";
import styles from "./McpPiPreview.module.css";
import manager from "./McpPiManager.module.css";

/**
 * Whether the adapter reports a per-server toggle. Claude Code's user scope
 * always reports `false` (see `CLAUDE_MCP_TOGGLE_SUPPORTED`); the parameter is
 * typed as plain boolean so this gate stays explicit and lint-clean.
 */
function toggleOffered(capabilities: { readonly toggle: boolean }): boolean {
    return capabilities.toggle;
}

/** Why the Manager dialog opened: full manage, register review, or history. */
export type McpClaudeManagerIntent = "manage" | "register" | "history";

/**
 * Complete per-definition Claude Code (user scope) registration workflow in a
 * Dialog, mirroring the Pi/Grok managers. Only deploy, revoke and restore are
 * offered: Claude Code has no user-level per-server enable/disable, so no
 * toggle is rendered. Reads expire on retry/close; a confirmed write is never
 * cancelled by navigation, duplicated, or auto-retried.
 */
export function McpClaudeManager({
    record,
    intent = "manage",
    returnFocus = null,
    onClose,
    onReload,
    onBusyChange,
    onChanged,
}: {
    record: McpRecord;
    intent?: McpClaudeManagerIntent;
    returnFocus?: HTMLElement | null;
    onClose?: () => void;
    onReload: () => void;
    onBusyChange: (busy: boolean) => void;
    onChanged: () => void;
}): JSX.Element {
    const { t, i18n } = useTranslation();
    const session = useClaudeMcpManager(record, onBusyChange, onChanged);
    const {
        read,
        step,
        locked,
        current,
        pending,
        reviewing,
        review,
        confirm,
        refresh,
        loadMore,
        setStep,
        mutating,
    } = session;
    const closeButton = useRef<HTMLButtonElement>(null);
    const registerStarted = useRef(false);
    const data = read.kind === "ready" ? read : null;
    const actionsLocked = locked;
    // Register from detail: jump straight into deploy review once ready.
    useEffect(() => {
        if (intent !== "register" || registerStarted.current) return;
        if (read.kind !== "ready" || current !== undefined) return;
        if (step.kind !== "idle" || pending) return;
        registerStarted.current = true;
        void review("deploy");
    }, [intent, read.kind, current, step.kind, pending, review]);

    const body = (
        <>
            {step.kind === "error" && (
                <p role="alert" className={dialog["error"]}>
                    {t(`mcp.claude.errors.${step.code}`)}
                </p>
            )}
            {intent === "register" &&
                step.kind === "idle" &&
                read.kind === "loading" && (
                    <p role="status">{t("mcp.claude.manage.loading")}</p>
                )}
            {step.kind === "done" && (
                <p role="status">
                    {t(
                        step.result.reloadRequired
                            ? "mcp.claude.manage.written"
                            : "mcp.claude.manage.recorded",
                    )}
                </p>
            )}
            {(step.kind === "previewing" || step.kind === "running") && (
                <p role="status">
                    {t(
                        step.kind === "running"
                            ? "mcp.claude.manage.writing"
                            : "mcp.claude.manage.previewing",
                    )}
                </p>
            )}
            {step.kind === "review" && <ClaudeMcpReview plan={step.plan} />}
            {step.kind === "recovery" && (
                <section
                    className={styles["section"]}
                    aria-label={t("mcp.claude.recovery.review")}
                >
                    <p>{t("mcp.claude.recovery.noReplay")}</p>
                    <p>
                        {t(`mcp.claude.recovery.${step.recovery.observation}`)}
                    </p>
                </section>
            )}
            {intent !== "register" &&
                !reviewing &&
                step.kind !== "running" &&
                step.kind !== "previewing" && (
                    <>
                        {read.kind === "loading" && (
                            <p role="status">
                                {t("mcp.claude.manage.loading")}
                            </p>
                        )}
                        {read.kind === "error" && (
                            <p role="alert">
                                {t(`mcp.claude.errors.${read.code}`)}
                            </p>
                        )}
                        {data !== null && (
                            <>
                                <DetailPanel
                                    label={t("mcp.claude.manage.registration")}
                                >
                                    <DetailFact
                                        label={t("mcp.claude.manage.target")}
                                        value={data.state.targetPath}
                                    />
                                    <DetailFact
                                        label={t("mcp.claude.manage.diskState")}
                                        value={t(
                                            current
                                                ? `mcp.claude.states.${current.state}`
                                                : "mcp.claude.states.unregistered",
                                        )}
                                    />
                                    {current && (
                                        <DetailFact
                                            label={t(
                                                "mcp.claude.manage.revision",
                                            )}
                                            value={t(
                                                "mcp.claude.manage.revisionValue",
                                                {
                                                    written:
                                                        current.ownership
                                                            .definitionRevision,
                                                    central: record.revision,
                                                },
                                            )}
                                        />
                                    )}
                                </DetailPanel>
                                {/* User scope has no per-server toggle; say so
                                    instead of rendering enable/disable. */}
                                {!toggleOffered(data.state.capabilities) && (
                                    <p className={styles["notice"]}>
                                        {t("mcp.claude.manage.noToggle")}
                                    </p>
                                )}
                                <p className={styles["notice"]}>
                                    {t("mcp.claude.manage.localOverride")}
                                </p>
                                {data.state.deployments
                                    .filter(
                                        (row) =>
                                            row.configRoot !==
                                            data.state.configRoot,
                                    )
                                    .map((row) => (
                                        <p
                                            key={row.configRoot}
                                            className={styles["notice"]}
                                        >
                                            {t("mcp.claude.manage.otherRoot", {
                                                path: row.configRoot,
                                            })}
                                        </p>
                                    ))}
                                {pending && (
                                    <section
                                        className={manager["recovery"]}
                                        aria-label={t(
                                            "mcp.claude.recovery.title",
                                        )}
                                    >
                                        <h3>
                                            {t("mcp.claude.recovery.title")}
                                        </h3>
                                        <p>
                                            {t("mcp.claude.recovery.pending", {
                                                name: pending.serverName,
                                            })}
                                        </p>
                                        <p>
                                            {t(
                                                `mcp.claude.recovery.${pending.observation}`,
                                            )}
                                        </p>
                                        {pending.approval !== null && (
                                            <button
                                                type="button"
                                                className={buttons["secondary"]}
                                                disabled={actionsLocked}
                                                onClick={() => {
                                                    setStep({
                                                        kind: "recovery",
                                                        recovery: pending,
                                                    });
                                                }}
                                            >
                                                {t(
                                                    "mcp.claude.recovery.review",
                                                )}
                                            </button>
                                        )}
                                    </section>
                                )}
                                {intent !== "history" && (
                                    <div className={manager["actions"]}>
                                        <button
                                            type="button"
                                            className={buttons["primary"]}
                                            disabled={
                                                actionsLocked || !!pending
                                            }
                                            onClick={() => {
                                                void review("deploy");
                                            }}
                                        >
                                            {t(
                                                current
                                                    ? "mcp.claude.manage.update"
                                                    : "mcp.claude.manage.deploy",
                                            )}
                                        </button>
                                        {current && (
                                            <button
                                                type="button"
                                                className={buttons["secondary"]}
                                                disabled={
                                                    actionsLocked || !!pending
                                                }
                                                onClick={() => {
                                                    void review("revoke");
                                                }}
                                            >
                                                {t("mcp.claude.actions.revoke")}
                                            </button>
                                        )}
                                    </div>
                                )}
                                <section
                                    className={styles["section"]}
                                    aria-label={t("mcp.claude.history.title")}
                                >
                                    <h3>{t("mcp.claude.history.title")}</h3>
                                    {data.history.items.length === 0 && (
                                        <p>{t("mcp.claude.history.empty")}</p>
                                    )}
                                    <ul className={manager["history"]}>
                                        {data.history.items.map((row) => (
                                            <li key={row.id}>
                                                <div>
                                                    <span>
                                                        {t(
                                                            `mcp.claude.actions.${row.action}`,
                                                        )}
                                                    </span>
                                                    {/* History reports the
                                                        retained `registered`
                                                        result, never `enabled`. */}
                                                    <span>
                                                        {t(
                                                            row.registered
                                                                ? "mcp.claude.history.registered"
                                                                : "mcp.claude.history.unregistered",
                                                        )}
                                                    </span>
                                                    <time
                                                        dateTime={new Date(
                                                            row.createdAt,
                                                        ).toISOString()}
                                                    >
                                                        {new Intl.DateTimeFormat(
                                                            i18n.language,
                                                            {
                                                                dateStyle:
                                                                    "short",
                                                                timeStyle:
                                                                    "short",
                                                            },
                                                        ).format(row.createdAt)}
                                                    </time>
                                                </div>
                                                <button
                                                    type="button"
                                                    className={
                                                        buttons["secondary"]
                                                    }
                                                    disabled={
                                                        actionsLocked ||
                                                        !!pending
                                                    }
                                                    aria-label={t(
                                                        "mcp.claude.history.restoreLabel",
                                                        {
                                                            id: row.id.slice(
                                                                0,
                                                                8,
                                                            ),
                                                        },
                                                    )}
                                                    onClick={() => {
                                                        void review(
                                                            "restore",
                                                            row.id,
                                                        );
                                                    }}
                                                >
                                                    {t(
                                                        "mcp.claude.actions.restore",
                                                    )}
                                                </button>
                                            </li>
                                        ))}
                                    </ul>
                                    {data.history.nextCursor !== null && (
                                        <button
                                            type="button"
                                            className={buttons["secondary"]}
                                            disabled={actionsLocked}
                                            onClick={() => {
                                                void loadMore();
                                            }}
                                        >
                                            {t("mcp.loadMore")}
                                        </button>
                                    )}
                                </section>
                            </>
                        )}
                    </>
                )}
        </>
    );

    const footer = (
        <div className={dialog["actions"]}>
            {reviewing && (
                <button
                    type="button"
                    className={buttons["primary"]}
                    onClick={() => {
                        void confirm();
                    }}
                >
                    {t(
                        step.kind === "recovery"
                            ? "mcp.claude.recovery.confirm"
                            : "mcp.claude.manage.confirm",
                    )}
                </button>
            )}
            {reviewing && (
                <button
                    type="button"
                    className={buttons["secondary"]}
                    onClick={() => {
                        if (intent === "register") onClose?.();
                        else setStep({ kind: "idle" });
                    }}
                >
                    {t("mcp.claude.manage.cancelReview")}
                </button>
            )}
            {intent !== "register" && !reviewing && (
                <button
                    type="button"
                    className={buttons["secondary"]}
                    disabled={actionsLocked}
                    onClick={refresh}
                >
                    {t("mcp.claude.manage.refresh")}
                </button>
            )}
            {step.kind === "error" && step.code === "definition_changed" && (
                <button
                    type="button"
                    className={buttons["secondary"]}
                    onClick={onReload}
                >
                    {t("mcp.claude.manage.reload")}
                </button>
            )}
            <button
                ref={closeButton}
                type="button"
                className={buttons["secondary"]}
                disabled={step.kind === "running"}
                onClick={() => {
                    if (!mutating()) onClose?.();
                }}
            >
                {t("mcp.claude.manage.close")}
            </button>
        </div>
    );

    const titleKey =
        intent === "history"
            ? "mcp.inject.historyTitle"
            : "mcp.claude.manage.title";

    return (
        <Dialog.Root
            open
            onOpenChange={(open) => {
                if (!open && !mutating()) onClose?.();
            }}
        >
            <Dialog.Portal>
                <Dialog.Backdrop className={dialog["backdrop"]} />
                <Dialog.Popup
                    className={[dialog["popup"], styles["popup"]].join(" ")}
                    initialFocus={closeButton}
                    finalFocus={() =>
                        returnFocus?.isConnected ? returnFocus : null
                    }
                >
                    <Dialog.Title className={dialog["title"]}>
                        {t(titleKey, {
                            agent: t("mcp.inject.claude"),
                            name: record.displayName,
                        })}
                    </Dialog.Title>
                    <div className={styles["content"]}>{body}</div>
                    {footer}
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
