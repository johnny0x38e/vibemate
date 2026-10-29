import { Dialog } from "@base-ui/react/dialog";
import type { JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import { VersionSummary } from "../agents/VersionRestoreCard";
import type {
    CodexMcpDeployment,
    CodexMcpPlan,
} from "../../lib/desktop/codexMcp";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./McpDeleteDialog.module.css";
import styles from "./McpPiPreview.module.css";

/** Review one entry only. Values have already passed the masked IPC boundary;
 * the separate details dialog never contains unrelated native configuration. */
export function CodexMcpReview({
    plan,
    row,
}: {
    plan: CodexMcpPlan;
    /** Observed registration; its `unmanagedKeys` are the hand-added keys. */
    row?: Pick<CodexMcpDeployment, "unmanagedKeys"> | undefined;
}): JSX.Element {
    const { t } = useTranslation();
    // Hand-edited keys vibemate does not manage stay in the native entry.
    const unmanaged = row?.unmanagedKeys ?? [];
    return (
        <div className={styles["section"]}>
            <DetailPanel label={t("mcp.codex.manage.review")}>
                <DetailFact
                    label={t("mcp.codexPreview.target")}
                    value={plan.target.path}
                />
                <DetailFact
                    label={t("mcp.codex.manage.action")}
                    value={t(`mcp.codex.actions.${plan.action}`)}
                />
                <DetailFact
                    label={t("mcp.serverName")}
                    value={plan.serverName}
                />
                <DetailFact
                    label={t("mcp.codexPreview.enabled")}
                    value={t(
                        plan.afterEntry === null
                            ? "mcp.codex.manage.removed"
                            : plan.afterEntry.enabled
                              ? "mcp.codexPreview.on"
                              : "mcp.codexPreview.off",
                    )}
                />
            </DetailPanel>
            <VersionSummary file={{ summary: plan.summary }} />
            <p className={styles["notice"]}>{t("mcp.codex.manage.preserve")}</p>
            {plan.action === "deploy" && (
                <p className={styles["notice"]}>
                    {t("mcp.codex.manage.credentials")}
                </p>
            )}
            {plan.action === "restore" && (
                <p className={styles["notice"]}>
                    {t("mcp.codex.manage.historicalCredentials")}
                </p>
            )}
            {unmanaged.length > 0 && (
                <p className={styles["notice"]}>
                    {t("mcp.codexPreview.unmanagedKeys", {
                        keys: unmanaged.join(", "),
                    })}
                </p>
            )}
            <Dialog.Root>
                <Dialog.Trigger className={buttons["secondary"]}>
                    {t("mcp.codexPreview.details")}
                </Dialog.Trigger>
                <Dialog.Portal>
                    <Dialog.Backdrop className={dialog["backdrop"]} />
                    <Dialog.Popup
                        className={[dialog["popup"], styles["popup"]].join(" ")}
                    >
                        <Dialog.Title className={dialog["title"]}>
                            {t("mcp.codexPreview.detailsTitle")}
                        </Dialog.Title>
                        <Dialog.Description className={dialog["body"]}>
                            {t("mcp.codex.manage.entryOnly")}
                        </Dialog.Description>
                        <pre className={styles["code"]}>
                            {JSON.stringify(
                                {
                                    before: plan.beforeEntry,
                                    after: plan.afterEntry,
                                },
                                null,
                                2,
                            )}
                        </pre>
                        <div className={dialog["actions"]}>
                            <Dialog.Close className={buttons["secondary"]}>
                                {t("mcp.codexPreview.close")}
                            </Dialog.Close>
                        </div>
                    </Dialog.Popup>
                </Dialog.Portal>
            </Dialog.Root>
        </div>
    );
}
