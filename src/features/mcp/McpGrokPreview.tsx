import { Dialog } from "@base-ui/react/dialog";
import { useEffect, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import buttons from "../../components/actionButtons.module.css";
import {
    GrokMcpPreviewError,
    previewGrokMcpRegistration,
    type GrokMcpPreviewErrorCode,
    type GrokMcpPreview,
} from "../../lib/desktop/grokMcpPreview";
import type { McpRecord } from "../../lib/desktop/mcp";
import dialog from "./McpDeleteDialog.module.css";
import styles from "./McpPiPreview.module.css";

type PreviewState =
    | { kind: "loading" }
    | { kind: "ready"; preview: GrokMcpPreview }
    | { kind: "error"; code: GrokMcpPreviewErrorCode };

/** Read-only review of one central revision. Closing/unmounting ignores late reads;
 * no save, Agent execution or application action is available from this dialog. */
export function McpGrokPreview({
    record,
    returnFocus,
    onClose,
    onReload,
}: {
    record: McpRecord;
    returnFocus: HTMLElement | null;
    onClose: () => void;
    onReload: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    const [state, setState] = useState<PreviewState>({ kind: "loading" });
    const [attempt, setAttempt] = useState(0);
    const closeButton = useRef<HTMLButtonElement>(null);
    useEffect(() => {
        // Tauri reads are not cancellable; only the current mounted attempt may
        // publish a result. StrictMode cleanup invalidates its first request too.
        let current = true;
        previewGrokMcpRegistration(record.id, record.revision).then(
            (preview) => {
                if (current) setState({ kind: "ready", preview });
            },
            (error: unknown) => {
                if (current)
                    setState({
                        kind: "error",
                        code:
                            error instanceof GrokMcpPreviewError
                                ? error.code
                                : "operation_failed",
                    });
            },
        );
        return () => {
            current = false;
        };
    }, [record.id, record.revision, attempt]);
    const proposal = state.kind === "ready" ? state.preview.proposal : null;
    return (
        <Dialog.Root
            open
            onOpenChange={(open) => {
                if (!open) onClose();
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
                        {t("mcp.grokPreview.title", {
                            name: record.displayName,
                        })}
                    </Dialog.Title>
                    <Dialog.Description className={dialog["body"]}>
                        {t("mcp.grokPreview.readOnly")}
                    </Dialog.Description>
                    <div className={styles["content"]}>
                        {state.kind === "loading" && (
                            <p role="status">{t("mcp.grokPreview.loading")}</p>
                        )}
                        {state.kind === "error" && (
                            <div className={styles["section"]}>
                                <p role="alert" className={dialog["error"]}>
                                    {t(`mcp.grokPreview.errors.${state.code}`)}
                                </p>
                                <div className={dialog["actions"]}>
                                    <button
                                        type="button"
                                        className={buttons["secondary"]}
                                        onClick={() => {
                                            setState({ kind: "loading" });
                                            setAttempt((value) => value + 1);
                                        }}
                                    >
                                        {t("mcp.retry")}
                                    </button>
                                    <button
                                        type="button"
                                        className={buttons["secondary"]}
                                        onClick={onReload}
                                    >
                                        {t("mcp.grokPreview.reload")}
                                    </button>
                                </div>
                            </div>
                        )}
                        {state.kind === "ready" && proposal !== null && (
                            <div className={styles["section"]}>
                                <DetailPanel
                                    label={t("mcp.grokPreview.summary")}
                                >
                                    <DetailFact
                                        label={t("mcp.grokPreview.target")}
                                        value={state.preview.target.path}
                                    />
                                    <DetailFact
                                        label={t("mcp.grokPreview.change")}
                                        value={t("mcp.grokPreview.add", {
                                            name: proposal.serverName,
                                        })}
                                    />
                                    <DetailFact
                                        label={t("mcp.grokPreview.file")}
                                        value={t(
                                            state.preview.target.exists
                                                ? "mcp.grokPreview.existing"
                                                : "mcp.grokPreview.missing",
                                        )}
                                    />
                                    <DetailFact
                                        label={t("mcp.grokPreview.enabled")}
                                        value={t(
                                            proposal.redactedEntry.enabled
                                                ? "mcp.grokPreview.on"
                                                : "mcp.grokPreview.off",
                                        )}
                                    />
                                </DetailPanel>
                                <p className={styles["notice"]}>
                                    {t("mcp.grokPreview.credentials")}
                                </p>
                                {proposal.disabledByUserList && (
                                    <p className={styles["notice"]}>
                                        {t(
                                            "mcp.grokPreview.disabledByUserList",
                                        )}
                                    </p>
                                )}
                                {state.preview.target.symlink && (
                                    <p className={styles["notice"]}>
                                        {t("mcp.grokPreview.symlink")}
                                    </p>
                                )}
                                <Dialog.Root>
                                    <Dialog.Trigger
                                        className={buttons["secondary"]}
                                    >
                                        {t("mcp.grokPreview.details")}
                                    </Dialog.Trigger>
                                    <Dialog.Portal>
                                        <Dialog.Backdrop
                                            className={dialog["backdrop"]}
                                        />
                                        <Dialog.Popup
                                            className={[
                                                dialog["popup"],
                                                styles["popup"],
                                            ].join(" ")}
                                        >
                                            <Dialog.Title
                                                className={dialog["title"]}
                                            >
                                                {t(
                                                    "mcp.grokPreview.detailsTitle",
                                                )}
                                            </Dialog.Title>
                                            <Dialog.Description
                                                className={dialog["body"]}
                                            >
                                                {t(
                                                    "mcp.grokPreview.newEntryOnly",
                                                )}
                                            </Dialog.Description>
                                            <pre className={styles["code"]}>
                                                {JSON.stringify(
                                                    {
                                                        mcp_servers: {
                                                            [proposal.serverName]:
                                                                proposal.redactedEntry,
                                                        },
                                                    },
                                                    null,
                                                    2,
                                                )}
                                            </pre>
                                            <div className={dialog["actions"]}>
                                                <Dialog.Close
                                                    className={
                                                        buttons["secondary"]
                                                    }
                                                >
                                                    {t("mcp.grokPreview.close")}
                                                </Dialog.Close>
                                            </div>
                                        </Dialog.Popup>
                                    </Dialog.Portal>
                                </Dialog.Root>
                            </div>
                        )}
                    </div>
                    <div className={dialog["actions"]}>
                        <button
                            ref={closeButton}
                            type="button"
                            className={buttons["secondary"]}
                            onClick={onClose}
                        >
                            {t("mcp.grokPreview.close")}
                        </button>
                    </div>
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
