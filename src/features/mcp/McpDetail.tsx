import type { JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailFact, DetailPanel } from "../../components/DetailForm";
import { SecondaryPageActions } from "../../components/SecondaryPageActions";
import { ProviderPage } from "../providers/ProviderPage";
import type { McpRecord } from "../../lib/desktop/mcp";
import { McpInjectRow, type McpInjectAgent } from "./McpInjectRow";
import buttons from "../../components/actionButtons.module.css";
import styles from "./Mcp.module.css";

/** Inputs for {@link McpDetail}. */
export interface McpDetailProps {
    record: McpRecord;
    /** Hide registration reads while the MCP module is not visible. */
    hidden?: boolean;
    onBack: () => void;
    /** Open the existing definition editor for this record. */
    onEdit: () => void;
    backBlocked?: boolean;
    /** Open Register review dialog for an unregistered Agent. */
    onRegister: (agent: McpInjectAgent, trigger: HTMLElement | null) => void;
    /** Open registration-history dialog for an Agent. */
    onHistory: (agent: McpInjectAgent, trigger: HTMLElement | null) => void;
}

/**
 * MCP detail: definition facts + clear Pi/Grok inject rows.
 * Page-level Edit is the only primary. Delete stays on the list.
 * Register and History open Manager dialogs; nothing is crammed into the row.
 */
export function McpDetail({
    record,
    hidden = false,
    onBack,
    onEdit,
    backBlocked = false,
    onRegister,
    onHistory,
}: McpDetailProps): JSX.Element {
    const { t, i18n } = useTranslation();
    const updated = new Intl.DateTimeFormat(i18n.language, {
        dateStyle: "medium",
        timeStyle: "short",
    }).format(new Date(record.updatedAtMs));
    const transportLabel = t(
        record.connection.type === "stdio" ? "mcp.stdio" : "mcp.http",
    );
    const statusLabel = t(record.enabled ? "mcp.enabled" : "mcp.disabled");
    const connectionValue =
        record.connection.type === "stdio"
            ? record.connection.command
            : record.connection.url;

    return (
        <ProviderPage
            title={record.displayName}
            meta={`${statusLabel} · ${transportLabel}`}
            onBack={onBack}
            backBlocked={backBlocked}
            backAriaLabel={t("mcp.backLabel")}
            scrollContent
        >
            <div className={styles["detailGroup"]}>
                {/* Card A: read-only definition. Status lives in header meta. */}
                <DetailPanel label={t("mcp.factsTitle")}>
                    <DetailFact
                        label={t("mcp.factName")}
                        value={record.displayName}
                    />
                    <DetailFact
                        label={t("mcp.factServerName")}
                        value={record.serverName}
                    />
                    <DetailFact
                        label={t("mcp.factTransport")}
                        value={transportLabel}
                    />
                    <DetailFact
                        label={
                            record.connection.type === "stdio"
                                ? t("mcp.factCommand")
                                : t("mcp.factUrl")
                        }
                        value={connectionValue}
                    />
                    <DetailFact label={t("mcp.factUpdated")} value={updated} />
                    {record.cleanupPending && (
                        <DetailFact
                            label={t("mcp.cleanup")}
                            value={t("mcp.cleanupPending")}
                        />
                    )}
                </DetailPanel>

                {/*
                 * Card B: clear Pi / Grok rows. Register (unregistered only)
                 * and History open dialogs owned by the parent view.
                 */}
                <DetailPanel label={t("mcp.inject.title")}>
                    {!hidden && (
                        <>
                            <McpInjectRow
                                agent="pi"
                                record={record}
                                blocked={backBlocked}
                                onRegister={(trigger) => {
                                    onRegister("pi", trigger);
                                }}
                                onHistory={(trigger) => {
                                    onHistory("pi", trigger);
                                }}
                            />
                            <McpInjectRow
                                agent="grok"
                                record={record}
                                blocked={backBlocked}
                                onRegister={(trigger) => {
                                    onRegister("grok", trigger);
                                }}
                                onHistory={(trigger) => {
                                    onHistory("grok", trigger);
                                }}
                            />
                        </>
                    )}
                </DetailPanel>
            </div>
            <SecondaryPageActions>
                <button
                    type="button"
                    className={buttons["primary"]}
                    aria-disabled={backBlocked}
                    onClick={() => {
                        if (!backBlocked) onEdit();
                    }}
                >
                    {t("mcp.editAction")}
                </button>
            </SecondaryPageActions>
        </ProviderPage>
    );
}
