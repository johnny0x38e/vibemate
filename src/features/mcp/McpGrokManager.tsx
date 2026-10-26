import { Dialog } from "@base-ui/react/dialog";
import { useEffect, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import {
    applyGrokMcpChange,
    GrokMcpError,
    previewGrokMcpChange,
    readGrokMcpHistory,
    readGrokMcpState,
    recoverGrokMcpChange,
    type GrokMcpAction,
    type GrokMcpApplied,
    type GrokMcpErrorCode,
    type GrokMcpHistory,
    type GrokMcpPlan,
    type GrokMcpRecovery,
    type GrokMcpSelection,
    type GrokMcpState,
} from "../../lib/desktop/grokMcp";
import type { McpRecord } from "../../lib/desktop/mcp";
import { GrokMcpReview } from "./GrokMcpReview";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./McpDeleteDialog.module.css";
import styles from "./McpPiPreview.module.css";
import manager from "./McpPiManager.module.css";

type ReadState =
    | { kind: "loading" }
    | { kind: "error"; code: GrokMcpErrorCode }
    | { kind: "ready"; state: GrokMcpState; history: GrokMcpHistory };
type Step =
    | { kind: "idle" }
    | { kind: "previewing" | "running" }
    | { kind: "review"; selection: GrokMcpSelection; plan: GrokMcpPlan }
    | { kind: "recovery"; recovery: GrokMcpRecovery }
    | { kind: "error"; code: GrokMcpErrorCode }
    | { kind: "done"; result: GrokMcpApplied };
function code(error: unknown): GrokMcpErrorCode {
    return error instanceof GrokMcpError ? error.code : "outcome_unknown";
}
/** Complete per-definition Grok registration workflow. Reads expire on retry/close;
 * a confirmed write is never cancelled by navigation, duplicated, or auto-retried.
 * Parents retain a busy guard even if hiding this dialog unmounts its view. */
export function McpGrokManager({
    record,
    returnFocus,
    onClose,
    onReload,
    onBusyChange,
    onChanged,
}: {
    record: McpRecord;
    returnFocus: HTMLElement | null;
    onClose: () => void;
    onReload: () => void;
    onBusyChange: (busy: boolean) => void;
    onChanged: () => void;
}): JSX.Element {
    const { t, i18n } = useTranslation();
    const [read, setRead] = useState<ReadState>({ kind: "loading" });
    const [step, setStep] = useState<Step>({ kind: "idle" });
    const [attempt, setAttempt] = useState(0);
    const [more, setMore] = useState(false);
    const alive = useRef(false),
        generation = useRef(0),
        mutation = useRef(false),
        readingMore = useRef(false);
    const closeButton = useRef<HTMLButtonElement>(null);
    useEffect(() => {
        alive.current = true;
        return () => {
            alive.current = false;
            generation.current += 1;
        };
    }, []);
    useEffect(() => {
        // IPC has no cancellation signal. Only this mounted read generation may
        // replace data, including after StrictMode's initial cleanup.
        const token = ++generation.current;
        Promise.all([
            readGrokMcpState(record.id),
            readGrokMcpHistory(record.id),
        ]).then(
            ([state, history]) => {
                if (alive.current && generation.current === token)
                    setRead({ kind: "ready", state, history });
            },
            (error: unknown) => {
                if (alive.current && generation.current === token)
                    setRead({ kind: "error", code: code(error) });
            },
        );
        return () => {
            generation.current += 1;
        };
    }, [record.id, record.revision, attempt]);
    const locked =
        step.kind === "previewing" || step.kind === "running" || more;
    function refresh(): void {
        if (mutation.current || readingMore.current) return;
        generation.current++;
        setRead({ kind: "loading" });
        setStep({ kind: "idle" });
        setAttempt((value) => value + 1);
    }
    async function review(
        action: GrokMcpAction,
        historyId: string | null = null,
    ): Promise<void> {
        if (locked || mutation.current) return;
        const token = ++generation.current;
        const selection: GrokMcpSelection = {
            definitionId: record.id,
            expectedRevision: record.revision,
            action,
            historyId,
        };
        setStep({ kind: "previewing" });
        try {
            const plan = await previewGrokMcpChange(selection);
            if (alive.current && token === generation.current)
                setStep({ kind: "review", selection, plan });
        } catch (error: unknown) {
            if (alive.current && token === generation.current)
                setStep({ kind: "error", code: code(error) });
        }
    }
    async function confirm(): Promise<void> {
        if (
            mutation.current ||
            (step.kind !== "review" && step.kind !== "recovery")
        )
            return;
        const reviewed = step;
        mutation.current = true;
        onBusyChange(true);
        setStep({ kind: "running" });
        try {
            const result =
                reviewed.kind === "review"
                    ? await applyGrokMcpChange(
                          reviewed.selection,
                          reviewed.plan,
                      )
                    : await recoverGrokMcpChange(reviewed.recovery);
            if (alive.current) {
                setStep({ kind: "done", result });
                setRead({ kind: "loading" });
                setAttempt((value) => value + 1);
            }
        } catch (error: unknown) {
            if (alive.current) {
                setStep({ kind: "error", code: code(error) });
                setRead({ kind: "loading" });
                setAttempt((value) => value + 1);
            }
        } finally {
            mutation.current = false;
            onBusyChange(false);
            onChanged();
        }
    }
    async function loadMore(): Promise<void> {
        if (
            read.kind !== "ready" ||
            read.history.nextCursor === null ||
            locked ||
            readingMore.current ||
            mutation.current
        )
            return;
        const snapshot = read;
        const token = generation.current;
        readingMore.current = true;
        setMore(true);
        try {
            const page = await readGrokMcpHistory(
                record.id,
                snapshot.history.nextCursor,
            );
            if (alive.current && generation.current === token) {
                const seen = new Set(
                    snapshot.history.items.map((row) => row.id),
                );
                setRead({
                    ...snapshot,
                    history: {
                        items: [
                            ...snapshot.history.items,
                            ...page.items.filter((row) => !seen.has(row.id)),
                        ],
                        nextCursor: page.nextCursor,
                    },
                });
            }
        } catch (error: unknown) {
            if (alive.current && generation.current === token)
                setStep({ kind: "error", code: code(error) });
        } finally {
            readingMore.current = false;
            if (alive.current) setMore(false);
        }
    }
    const data = read.kind === "ready" ? read : null;
    const current = data?.state.deployments.find(
        (row) => row.configRoot === data.state.configRoot,
    );
    const pending = data?.state.recovery;
    const reviewing = step.kind === "review" || step.kind === "recovery";
    return (
        <Dialog.Root
            open
            onOpenChange={(open) => {
                if (!open && !mutation.current) onClose();
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
                        {t("mcp.grok.manage.title", {
                            name: record.displayName,
                        })}
                    </Dialog.Title>
                    <Dialog.Description className={dialog["body"]}>
                        {t("mcp.grok.manage.description")}
                    </Dialog.Description>
                    <div className={styles["content"]}>
                        <p className={styles["notice"]}>
                            {t("mcp.grokPreview.runtime")}
                        </p>
                        {step.kind === "error" && (
                            <p role="alert" className={dialog["error"]}>
                                {t(`mcp.grok.errors.${step.code}`)}
                            </p>
                        )}
                        {step.kind === "done" && (
                            <p role="status">
                                {t(
                                    step.result.reloadRequired
                                        ? "mcp.grok.manage.written"
                                        : "mcp.grok.manage.recorded",
                                )}
                            </p>
                        )}
                        {(step.kind === "previewing" ||
                            step.kind === "running") && (
                            <p role="status">
                                {t(
                                    step.kind === "running"
                                        ? "mcp.grok.manage.writing"
                                        : "mcp.grokPreview.loading",
                                )}
                            </p>
                        )}
                        {step.kind === "review" && (
                            <GrokMcpReview plan={step.plan} />
                        )}
                        {step.kind === "recovery" && (
                            <section
                                className={styles["section"]}
                                aria-label={t("mcp.grok.recovery.review")}
                            >
                                <p>{t("mcp.grok.recovery.noReplay")}</p>
                                <p>
                                    {t(
                                        `mcp.grok.recovery.${step.recovery.observation}`,
                                    )}
                                </p>
                            </section>
                        )}
                        {!reviewing &&
                            step.kind !== "running" &&
                            step.kind !== "previewing" && (
                                <>
                                    {read.kind === "loading" && (
                                        <p role="status">
                                            {t("mcp.grok.manage.loading")}
                                        </p>
                                    )}
                                    {read.kind === "error" && (
                                        <p role="alert">
                                            {t(`mcp.grok.errors.${read.code}`)}
                                        </p>
                                    )}
                                    {data !== null && (
                                        <>
                                            <DetailPanel
                                                label={t(
                                                    "mcp.grok.manage.registration",
                                                )}
                                            >
                                                <DetailFact
                                                    label={t(
                                                        "mcp.grokPreview.target",
                                                    )}
                                                    value={
                                                        data.state.targetPath
                                                    }
                                                />
                                                <DetailFact
                                                    label={t(
                                                        "mcp.grok.manage.diskState",
                                                    )}
                                                    value={t(
                                                        current
                                                            ? `mcp.grok.states.${current.state}`
                                                            : "mcp.grok.states.unregistered",
                                                    )}
                                                />
                                                {current && (
                                                    <DetailFact
                                                        label={t(
                                                            "mcp.grokPreview.enabled",
                                                        )}
                                                        value={t(
                                                            current.ownership
                                                                .enabled
                                                                ? "mcp.grokPreview.on"
                                                                : "mcp.grokPreview.off",
                                                        )}
                                                    />
                                                )}
                                                {current && (
                                                    <DetailFact
                                                        label={t(
                                                            "mcp.grok.manage.revision",
                                                        )}
                                                        value={t(
                                                            "mcp.grok.manage.revisionValue",
                                                            {
                                                                written:
                                                                    current
                                                                        .ownership
                                                                        .definitionRevision,
                                                                central:
                                                                    record.revision,
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
                                                        className={
                                                            styles["notice"]
                                                        }
                                                    >
                                                        {t(
                                                            "mcp.grok.manage.otherRoot",
                                                            {
                                                                path: row.configRoot,
                                                            },
                                                        )}
                                                    </p>
                                                ))}
                                            {pending && (
                                                <section
                                                    className={
                                                        manager["recovery"]
                                                    }
                                                    aria-label={t(
                                                        "mcp.grok.recovery.title",
                                                    )}
                                                >
                                                    <h3>
                                                        {t(
                                                            "mcp.grok.recovery.title",
                                                        )}
                                                    </h3>
                                                    <p>
                                                        {t(
                                                            "mcp.grok.recovery.pending",
                                                            {
                                                                name: pending.serverName,
                                                            },
                                                        )}
                                                    </p>
                                                    <p>
                                                        {t(
                                                            `mcp.grok.recovery.${pending.observation}`,
                                                        )}
                                                    </p>
                                                    {pending.approval !==
                                                        null && (
                                                        <button
                                                            type="button"
                                                            className={
                                                                buttons[
                                                                    "secondary"
                                                                ]
                                                            }
                                                            disabled={locked}
                                                            onClick={() => {
                                                                setStep({
                                                                    kind: "recovery",
                                                                    recovery:
                                                                        pending,
                                                                });
                                                            }}
                                                        >
                                                            {t(
                                                                "mcp.grok.recovery.review",
                                                            )}
                                                        </button>
                                                    )}
                                                </section>
                                            )}
                                            <div className={manager["actions"]}>
                                                <button
                                                    type="button"
                                                    className={
                                                        buttons["primary"]
                                                    }
                                                    disabled={
                                                        locked || !!pending
                                                    }
                                                    onClick={() => {
                                                        void review("deploy");
                                                    }}
                                                >
                                                    {t(
                                                        current
                                                            ? "mcp.grok.manage.update"
                                                            : "mcp.grok.manage.deploy",
                                                    )}
                                                </button>
                                                {current && (
                                                    <>
                                                        <button
                                                            type="button"
                                                            className={
                                                                buttons[
                                                                    "secondary"
                                                                ]
                                                            }
                                                            disabled={
                                                                locked ||
                                                                !!pending
                                                            }
                                                            onClick={() => {
                                                                void review(
                                                                    current
                                                                        .ownership
                                                                        .enabled
                                                                        ? "disable"
                                                                        : "enable",
                                                                );
                                                            }}
                                                        >
                                                            {t(
                                                                current
                                                                    .ownership
                                                                    .enabled
                                                                    ? "mcp.grok.actions.disable"
                                                                    : "mcp.grok.actions.enable",
                                                            )}
                                                        </button>
                                                        <button
                                                            type="button"
                                                            className={
                                                                buttons[
                                                                    "secondary"
                                                                ]
                                                            }
                                                            disabled={
                                                                locked ||
                                                                !!pending
                                                            }
                                                            onClick={() => {
                                                                void review(
                                                                    "revoke",
                                                                );
                                                            }}
                                                        >
                                                            {t(
                                                                "mcp.grok.actions.revoke",
                                                            )}
                                                        </button>
                                                    </>
                                                )}
                                            </div>
                                            <section
                                                className={styles["section"]}
                                                aria-label={t(
                                                    "mcp.grok.history.title",
                                                )}
                                            >
                                                <h3>
                                                    {t(
                                                        "mcp.grok.history.title",
                                                    )}
                                                </h3>
                                                <p className={styles["notice"]}>
                                                    {t(
                                                        "mcp.grok.history.description",
                                                    )}
                                                </p>
                                                {data.history.items.length ===
                                                    0 && (
                                                    <p>
                                                        {t(
                                                            "mcp.grok.history.empty",
                                                        )}
                                                    </p>
                                                )}
                                                <ul
                                                    className={
                                                        manager["history"]
                                                    }
                                                >
                                                    {data.history.items.map(
                                                        (row) => (
                                                            <li key={row.id}>
                                                                <div>
                                                                    <span>
                                                                        {t(
                                                                            `mcp.grok.actions.${row.action}`,
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
                                                                        ).format(
                                                                            row.createdAt,
                                                                        )}
                                                                    </time>
                                                                </div>
                                                                <button
                                                                    type="button"
                                                                    className={
                                                                        buttons[
                                                                            "secondary"
                                                                        ]
                                                                    }
                                                                    disabled={
                                                                        locked ||
                                                                        !!pending
                                                                    }
                                                                    aria-label={t(
                                                                        "mcp.grok.history.restoreLabel",
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
                                                                        "mcp.grok.actions.restore",
                                                                    )}
                                                                </button>
                                                            </li>
                                                        ),
                                                    )}
                                                </ul>
                                                {data.history.nextCursor !==
                                                    null && (
                                                    <button
                                                        type="button"
                                                        className={
                                                            buttons["secondary"]
                                                        }
                                                        disabled={locked}
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
                    </div>
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
                                        ? "mcp.grok.recovery.confirm"
                                        : "mcp.grok.manage.confirm",
                                )}
                            </button>
                        )}
                        {reviewing && (
                            <button
                                type="button"
                                className={buttons["secondary"]}
                                onClick={() => {
                                    setStep({ kind: "idle" });
                                }}
                            >
                                {t("mcp.grok.manage.cancelReview")}
                            </button>
                        )}
                        {!reviewing && (
                            <button
                                type="button"
                                className={buttons["secondary"]}
                                disabled={locked}
                                onClick={refresh}
                            >
                                {t("mcp.grok.manage.refresh")}
                            </button>
                        )}
                        {step.kind === "error" &&
                            step.code === "definition_changed" && (
                                <button
                                    type="button"
                                    className={buttons["secondary"]}
                                    onClick={onReload}
                                >
                                    {t("mcp.grokPreview.reload")}
                                </button>
                            )}
                        <button
                            ref={closeButton}
                            type="button"
                            className={buttons["secondary"]}
                            disabled={step.kind === "running"}
                            onClick={() => {
                                if (!mutation.current) onClose();
                            }}
                        >
                            {t("mcp.grokPreview.close")}
                        </button>
                    </div>
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
