import { Dialog } from "@base-ui/react/dialog";
import type { JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import { VersionSummary } from "../agents/VersionRestoreCard";
import type { ClaudeMcpPlan } from "../../lib/desktop/claudeMcp";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./McpDeleteDialog.module.css";
import styles from "./McpPiPreview.module.css";

/** Review one Claude Code user-scope entry only. Values have already passed the
 * masked IPC boundary; the details dialog never shows unrelated `.claude.json`
 * content (account, session or per-project state). */
export function ClaudeMcpReview({
    plan,
}: {
    plan: ClaudeMcpPlan;
}): JSX.Element {
    const { t } = useTranslation();
    return (
        <div className={styles["section"]}>
            <DetailPanel label={t("mcp.claude.manage.review")}>
                <DetailFact
                    label={t("mcp.claude.manage.target")}
                    value={plan.target.path}
                />
                <DetailFact
                    label={t("mcp.claude.manage.action")}
                    value={t(`mcp.claude.actions.${plan.action}`)}
                />
                <DetailFact
                    label={t("mcp.serverName")}
                    value={plan.serverName}
                />
                <DetailFact
                    label={t("mcp.claude.manage.afterChange")}
                    value={t(
                        plan.afterEntry === null
                            ? "mcp.claude.manage.removed"
                            : "mcp.claude.manage.present",
                    )}
                />
            </DetailPanel>
            <VersionSummary file={{ summary: plan.summary }} />
            <p className={styles["notice"]}>
                {t("mcp.claude.manage.preserve")}
            </p>
            <p className={styles["notice"]}>
                {t("mcp.claude.manage.localOverride")}
            </p>
            {plan.action === "deploy" && (
                <p className={styles["notice"]}>
                    {t("mcp.claude.manage.credentials")}
                </p>
            )}
            {plan.action === "restore" && (
                <p className={styles["notice"]}>
                    {t("mcp.claude.manage.historicalCredentials")}
                </p>
            )}
            <Dialog.Root>
                <Dialog.Trigger className={buttons["secondary"]}>
                    {t("mcp.claude.manage.details")}
                </Dialog.Trigger>
                <Dialog.Portal>
                    <Dialog.Backdrop className={dialog["backdrop"]} />
                    <Dialog.Popup
                        className={[dialog["popup"], styles["popup"]].join(" ")}
                    >
                        <Dialog.Title className={dialog["title"]}>
                            {t("mcp.claude.manage.detailsTitle")}
                        </Dialog.Title>
                        <Dialog.Description className={dialog["body"]}>
                            {t("mcp.claude.manage.entryOnly")}
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
                                {t("mcp.claude.manage.close")}
                            </Dialog.Close>
                        </div>
                    </Dialog.Popup>
                </Dialog.Portal>
            </Dialog.Root>
        </div>
    );
}
