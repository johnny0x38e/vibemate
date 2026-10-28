import type { JSX } from "react";
import { useTranslation } from "react-i18next";
import { AgentIcon } from "../agents/AgentIcon";
import hierarchy from "../../components/textHierarchy.module.css";
import type { AgentId } from "../../lib/desktop/agents";
import styles from "./Mcp.module.css";

/** Short registration label for an inject row header. */
export function injectStatusLabel(
    kind: "ready" | "loading" | "error",
    registered: boolean,
    t: (key: string) => string,
): string {
    if (kind === "ready")
        return t(
            registered ? "mcp.inject.registered" : "mcp.inject.unregistered",
        );
    if (kind === "loading") return t("mcp.inject.statusLoading");
    return t("mcp.inject.statusUnchecked");
}

/** Semantic state for status coloring on the inject row. */
export type InjectStatusState =
    "registered" | "unregistered" | "loading" | "unchecked";

export function injectStatusState(
    kind: "ready" | "loading" | "error",
    registered: boolean,
): InjectStatusState {
    if (kind === "ready") return registered ? "registered" : "unregistered";
    if (kind === "loading") return "loading";
    return "unchecked";
}

/**
 * Agent mark + name + target path for an MCP inject row.
 * Decorative icon; the visible agent name identifies the row.
 * Status sits beside the row actions for consistent right-edge alignment.
 */
export function McpInjectAgentChrome({
    agentId,
    nameKey,
    targetPath,
}: {
    agentId: AgentId;
    nameKey: "mcp.inject.pi" | "mcp.inject.grok";
    targetPath: string | null;
}): JSX.Element {
    const { t } = useTranslation();
    return (
        <div className={styles["injectChrome"]}>
            <AgentIcon id={agentId} size="row" />
            <div className={styles["injectChromeText"]}>
                <span
                    className={[styles["injectAgent"], hierarchy["value"]].join(
                        " ",
                    )}
                >
                    {t(nameKey)}
                </span>
                <p
                    className={[styles["injectPath"], hierarchy["hint"]].join(
                        " ",
                    )}
                    title={targetPath ?? undefined}
                >
                    {targetPath ?? t("mcp.inject.pathPending")}
                </p>
            </div>
        </div>
    );
}
