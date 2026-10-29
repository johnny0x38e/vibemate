import { useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import type { AgentId } from "../../lib/desktop/agents";
import { usePiMcpRegistrations } from "../mcp/usePiMcpRegistrations";
import { useGrokMcpRegistrations } from "../mcp/useGrokMcpRegistrations";
import { useClaudeMcpRegistrations } from "../mcp/useClaudeMcpRegistrations";
import buttons from "../../components/actionButtons.module.css";
import styles from "./AgentList.module.css";

/** Read-only managed MCP observations for Agents with a write adapter.
 * Unsupported Agents do not run IPC; management stays in the central MCP module. */
export function AgentMcpPanel({
    agentId,
    hidden,
    onOpenMcp,
}: {
    agentId: AgentId;
    hidden: boolean;
    onOpenMcp: () => void;
}): JSX.Element {
    const { t } = useTranslation();
    const [epoch, setEpoch] = useState(0);
    const supported = agentId === "pi" || agentId === "grokBuild";
    const claudeSupported = agentId === "claudeCode";
    const pi = usePiMcpRegistrations(hidden || agentId !== "pi", epoch);
    const grok = useGrokMcpRegistrations(
        hidden || agentId !== "grokBuild",
        epoch,
    );
    const claude = useClaudeMcpRegistrations(
        hidden || agentId !== "claudeCode",
        epoch,
    );
    const result = agentId === "grokBuild" ? grok : pi;
    return (
        <div className={styles["skillPanel"]}>
            {claudeSupported ? (
                // Claude Code user scope: registration only, no enable flag.
                <>
                    <p className={styles["hint"]}>
                        {t("agents.mcpPanel.readOnlyClaude")}
                    </p>
                    {claude.kind === "loading" && (
                        <p role="status">{t("mcp.claude.manage.loading")}</p>
                    )}
                    {claude.kind === "error" && (
                        <p role="alert">
                            {t(`mcp.claude.errors.${claude.code}`)}
                        </p>
                    )}
                    {claude.kind === "ready" && (
                        <>
                            {claude.value.recovery && (
                                <p role="status">
                                    {t("mcp.claude.recovery.pending", {
                                        name: claude.value.recovery.serverName,
                                    })}
                                </p>
                            )}
                            {claude.value.deployments.length === 0 ? (
                                <p>{t("agents.mcpPanel.emptyClaude")}</p>
                            ) : (
                                <ul
                                    className={styles["skillDeployList"]}
                                    aria-label={t(
                                        "agents.mcpPanel.listLabelClaude",
                                    )}
                                >
                                    {claude.value.deployments.map((row) => (
                                        <li key={row.definitionId}>
                                            <p
                                                className={
                                                    styles["skillDeployRow"]
                                                }
                                            >
                                                {t(
                                                    "agents.mcpPanel.rowClaude",
                                                    {
                                                        name: row.displayName,
                                                        state: t(
                                                            `mcp.claude.states.${row.state}`,
                                                        ),
                                                    },
                                                )}
                                            </p>
                                        </li>
                                    ))}
                                </ul>
                            )}
                        </>
                    )}
                    <button
                        type="button"
                        className={buttons["secondary"]}
                        onClick={() => {
                            setEpoch((value) => value + 1);
                        }}
                    >
                        {t("mcp.claude.manage.refresh")}
                    </button>
                </>
            ) : !supported ? (
                <p className={styles["hint"]}>
                    {t("agents.mcpPanel.unsupported")}
                </p>
            ) : (
                <>
                    <p className={styles["hint"]}>
                        {t(
                            agentId === "grokBuild"
                                ? "agents.mcpPanel.readOnlyGrok"
                                : "agents.mcpPanel.readOnly",
                        )}
                    </p>
                    {result.kind === "loading" && (
                        <p role="status">
                            {t(
                                agentId === "grokBuild"
                                    ? "mcp.grok.manage.loading"
                                    : "mcp.pi.manage.loading",
                            )}
                        </p>
                    )}
                    {result.kind === "error" && (
                        <p role="alert">
                            {agentId === "grokBuild"
                                ? t(`mcp.grok.errors.${result.code}`)
                                : t(
                                      `mcp.pi.errors.${result.code as Exclude<typeof result.code, "unsupported_server_name" | "dynamic_value_unsupported" | "path_semantics_unverified">}`,
                                  )}
                        </p>
                    )}
                    {result.kind === "ready" && (
                        <>
                            {result.value.recovery && (
                                <p role="status">
                                    {t(
                                        agentId === "grokBuild"
                                            ? "mcp.grok.recovery.pending"
                                            : "mcp.pi.recovery.pending",
                                        {
                                            name: result.value.recovery
                                                .serverName,
                                        },
                                    )}
                                </p>
                            )}
                            {result.value.deployments.length === 0 ? (
                                <p>
                                    {t(
                                        agentId === "grokBuild"
                                            ? "agents.mcpPanel.emptyGrok"
                                            : "agents.mcpPanel.empty",
                                    )}
                                </p>
                            ) : (
                                <ul
                                    className={styles["skillDeployList"]}
                                    aria-label={t(
                                        agentId === "grokBuild"
                                            ? "agents.mcpPanel.listLabelGrok"
                                            : "agents.mcpPanel.listLabel",
                                    )}
                                >
                                    {result.value.deployments.map((row) => (
                                        <li key={row.definitionId}>
                                            <p
                                                className={
                                                    styles["skillDeployRow"]
                                                }
                                            >
                                                {t("agents.mcpPanel.row", {
                                                    name: row.displayName,
                                                    state: t(
                                                        agentId === "grokBuild"
                                                            ? `mcp.grok.states.${row.state}`
                                                            : `mcp.pi.states.${row.state}`,
                                                    ),
                                                    enabled: t(
                                                        row.ownership.enabled
                                                            ? agentId ===
                                                              "grokBuild"
                                                                ? "mcp.grokPreview.on"
                                                                : "mcp.piPreview.on"
                                                            : agentId ===
                                                                "grokBuild"
                                                              ? "mcp.grokPreview.off"
                                                              : "mcp.piPreview.off",
                                                    ),
                                                })}
                                            </p>
                                        </li>
                                    ))}
                                </ul>
                            )}
                        </>
                    )}
                    <button
                        type="button"
                        className={buttons["secondary"]}
                        onClick={() => {
                            setEpoch((value) => value + 1);
                        }}
                    >
                        {t(
                            agentId === "grokBuild"
                                ? "mcp.grok.manage.refresh"
                                : "mcp.pi.manage.refresh",
                        )}
                    </button>
                </>
            )}
            <button
                type="button"
                className={buttons["secondary"]}
                onClick={onOpenMcp}
            >
                {t("agents.mcpPanel.openMcp")}
            </button>
        </div>
    );
}
