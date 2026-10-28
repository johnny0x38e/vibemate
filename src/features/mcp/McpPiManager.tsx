import { Dialog } from "@base-ui/react/dialog";
import { useEffect, useRef, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import type { McpRecord } from "../../lib/desktop/mcp";
import { PiMcpReview } from "./PiMcpReview";
import { usePiMcpManager } from "./usePiMcpManager";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./McpDeleteDialog.module.css";
import styles from "./McpPiPreview.module.css";
import manager from "./McpPiManager.module.css";

/** Why the Manager dialog opened: full manage, register review, or history. */
export type McpPiManagerIntent = "manage" | "register" | "history";

/**
 * Complete per-definition Pi registration workflow in a Dialog. Reads expire on
 * retry/close; a confirmed write is never cancelled by navigation, duplicated,
 * or auto-retried. Parents retain a busy guard even if hiding this surface
 * unmounts its view. `register` auto-starts deploy review; `history` focuses
 * retained-entry history (restore still uses review→confirm).
 */
export function McpPiManager({
    record,
    intent = "manage",
    returnFocus = null,
    onClose,
    onReload,
    onBusyChange,
    onChanged,
}: {
    record: McpRecord;
    intent?: McpPiManagerIntent;
    returnFocus?: HTMLElement | null;
    onClose?: () => void;
    onReload: () => void;
    onBusyChange: (busy: boolean) => void;
    onChanged: () => void;
}): JSX.Element {
    const { t, i18n } = useTranslation();
    const session = usePiMcpManager(record, onBusyChange, onChanged);
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
                    {t(`mcp.pi.errors.${step.code}`)}
                </p>
            )}
            {intent === "register" &&
                step.kind === "idle" &&
                read.kind === "loading" && (
                    <p role="status">{t("mcp.pi.manage.loading")}</p>
                )}
            {step.kind === "done" && (
                <p role="status">
                    {t(
                        step.result.reloadRequired
                            ? "mcp.pi.manage.written"
                            : "mcp.pi.manage.recorded",
                    )}
                </p>
            )}
            {(step.kind === "previewing" || step.kind === "running") && (
                <p role="status">
                    {t(
                        step.kind === "running"
                            ? "mcp.pi.manage.writing"
                            : "mcp.piPreview.loading",
                    )}
                </p>
            )}
            {step.kind === "review" && <PiMcpReview plan={step.plan} />}
            {step.kind === "recovery" && (
                <section
                    className={styles["section"]}
                    aria-label={t("mcp.pi.recovery.review")}
                >
                    <p>{t("mcp.pi.recovery.noReplay")}</p>
                    <p>{t(`mcp.pi.recovery.${step.recovery.observation}`)}</p>
                </section>
            )}
            {intent !== "register" &&
                !reviewing &&
                step.kind !== "running" &&
                step.kind !== "previewing" && (
                    <>
                        {read.kind === "loading" && (
                            <p role="status">{t("mcp.pi.manage.loading")}</p>
                        )}
                        {read.kind === "error" && (
                            <p role="alert">
                                {t(`mcp.pi.errors.${read.code}`)}
                            </p>
                        )}
                        {data !== null && (
                            <>
                                <DetailPanel
                                    label={t("mcp.pi.manage.registration")}
                                >
                                    <DetailFact
                                        label={t("mcp.piPreview.target")}
                                        value={data.state.targetPath}
                                    />
                                    <DetailFact
                                        label={t("mcp.pi.manage.diskState")}
                                        value={t(
                                            current
                                                ? `mcp.pi.states.${current.state}`
                                                : "mcp.pi.states.unregistered",
                                        )}
                                    />
                                    {current && (
                                        <DetailFact
                                            label={t("mcp.piPreview.enabled")}
                                            value={t(
                                                current.ownership.enabled
                                                    ? "mcp.piPreview.on"
                                                    : "mcp.piPreview.off",
                                            )}
                                        />
                                    )}
                                    {current && (
                                        <DetailFact
                                            label={t("mcp.pi.manage.revision")}
                                            value={t(
                                                "mcp.pi.manage.revisionValue",
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
                                            {t("mcp.pi.manage.otherRoot", {
                                                path: row.configRoot,
                                            })}
                                        </p>
                                    ))}
                                {pending && (
                                    <section
                                        className={manager["recovery"]}
                                        aria-label={t("mcp.pi.recovery.title")}
                                    >
                                        <h3>{t("mcp.pi.recovery.title")}</h3>
                                        <p>
                                            {t("mcp.pi.recovery.pending", {
                                                name: pending.serverName,
                                            })}
                                        </p>
                                        <p>
                                            {t(
                                                `mcp.pi.recovery.${pending.observation}`,
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
                                                {t("mcp.pi.recovery.review")}
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
                                                    ? "mcp.pi.manage.update"
                                                    : "mcp.pi.manage.deploy",
                                            )}
                                        </button>
                                        {current && (
                                            <>
                                                <button
                                                    type="button"
                                                    className={
                                                        buttons["secondary"]
                                                    }
                                                    disabled={
                                                        actionsLocked ||
                                                        !!pending
                                                    }
                                                    onClick={() => {
                                                        void review(
                                                            current.ownership
                                                                .enabled
                                                                ? "disable"
                                                                : "enable",
                                                        );
                                                    }}
                                                >
                                                    {t(
                                                        current.ownership
                                                            .enabled
                                                            ? "mcp.pi.actions.disable"
                                                            : "mcp.pi.actions.enable",
                                                    )}
                                                </button>
                                                <button
                                                    type="button"
                                                    className={
                                                        buttons["secondary"]
                                                    }
                                                    disabled={
                                                        actionsLocked ||
                                                        !!pending
                                                    }
                                                    onClick={() => {
                                                        void review("revoke");
                                                    }}
                                                >
                                                    {t("mcp.pi.actions.revoke")}
                                                </button>
                                            </>
                                        )}
                                    </div>
                                )}
                                <section
                                    className={styles["section"]}
                                    aria-label={t("mcp.pi.history.title")}
                                >
                                    <h3>{t("mcp.pi.history.title")}</h3>
                                    {data.history.items.length === 0 && (
                                        <p>{t("mcp.pi.history.empty")}</p>
                                    )}
                                    <ul className={manager["history"]}>
                                        {data.history.items.map((row) => (
                                            <li key={row.id}>
                                                <div>
                                                    <span>
                                                        {t(
                                                            `mcp.pi.actions.${row.action}`,
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
                                                        "mcp.pi.history.restoreLabel",
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
                                                        "mcp.pi.actions.restore",
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
                            ? "mcp.pi.recovery.confirm"
                            : "mcp.pi.manage.confirm",
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
                    {t("mcp.pi.manage.cancelReview")}
                </button>
            )}
            {intent !== "register" && !reviewing && (
                <button
                    type="button"
                    className={buttons["secondary"]}
                    disabled={actionsLocked}
                    onClick={refresh}
                >
                    {t("mcp.pi.manage.refresh")}
                </button>
            )}
            {step.kind === "error" && step.code === "definition_changed" && (
                <button
                    type="button"
                    className={buttons["secondary"]}
                    onClick={onReload}
                >
                    {t("mcp.piPreview.reload")}
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
                {t("mcp.piPreview.close")}
            </button>
        </div>
    );

    const titleKey =
        intent === "history"
            ? "mcp.inject.historyTitle"
            : "mcp.pi.manage.title";

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
                            agent: t("mcp.inject.pi"),
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
