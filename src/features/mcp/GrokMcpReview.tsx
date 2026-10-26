import { Dialog } from "@base-ui/react/dialog";
import type { JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import { VersionSummary } from "../agents/VersionRestoreCard";
import type { GrokMcpPlan } from "../../lib/desktop/grokMcp";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./McpDeleteDialog.module.css";
import styles from "./McpPiPreview.module.css";

/** Review one entry only. Values have already passed the masked IPC boundary;
 * the separate details dialog never contains unrelated native configuration. */
export function GrokMcpReview({ plan }: { plan: GrokMcpPlan }): JSX.Element {
    const { t } = useTranslation();
    return (
        <div className={styles["section"]}>
            <DetailPanel label={t("mcp.grok.manage.review")}>
                <DetailFact
                    label={t("mcp.grokPreview.target")}
                    value={plan.target.path}
                />
                <DetailFact
                    label={t("mcp.grok.manage.action")}
                    value={t(`mcp.grok.actions.${plan.action}`)}
                />
                <DetailFact
                    label={t("mcp.serverName")}
                    value={plan.serverName}
                />
                <DetailFact
                    label={t("mcp.grokPreview.enabled")}
                    value={t(
                        plan.afterEntry === null
                            ? "mcp.grok.manage.removed"
                            : plan.afterEntry.enabled
                              ? "mcp.grokPreview.on"
                              : "mcp.grokPreview.off",
                    )}
                />
            </DetailPanel>
            <VersionSummary file={{ summary: plan.summary }} />
            <p className={styles["notice"]}>{t("mcp.grok.manage.preserve")}</p>
            {plan.action === "deploy" && (
                <p className={styles["notice"]}>
                    {t("mcp.grok.manage.credentials")}
                </p>
            )}
            {plan.action === "restore" && (
                <p className={styles["notice"]}>
                    {t("mcp.grok.manage.historicalCredentials")}
                </p>
            )}
            {plan.disabledByUserList && (
                <p className={styles["notice"]}>
                    {t("mcp.grokPreview.disabledByUserList")}
                </p>
            )}
            <Dialog.Root>
                <Dialog.Trigger className={buttons["secondary"]}>
                    {t("mcp.grokPreview.details")}
                </Dialog.Trigger>
                <Dialog.Portal>
                    <Dialog.Backdrop className={dialog["backdrop"]} />
                    <Dialog.Popup
                        className={[dialog["popup"], styles["popup"]].join(" ")}
                    >
                        <Dialog.Title className={dialog["title"]}>
                            {t("mcp.grokPreview.detailsTitle")}
                        </Dialog.Title>
                        <Dialog.Description className={dialog["body"]}>
                            {t("mcp.grok.manage.entryOnly")}
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
                                {t("mcp.grokPreview.close")}
                            </Dialog.Close>
                        </div>
                    </Dialog.Popup>
                </Dialog.Portal>
            </Dialog.Root>
        </div>
    );
}
