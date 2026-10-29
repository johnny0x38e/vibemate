import { useEffect, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../../components/IconButton";
import listActions from "../../components/listActions.module.css";
import {
    PiMcpError,
    readPiMcpState,
    type PiMcpErrorCode,
} from "../../lib/desktop/piMcp";
import {
    GrokMcpError,
    readGrokMcpState,
    type GrokMcpErrorCode,
} from "../../lib/desktop/grokMcp";
import {
    ClaudeMcpError,
    readClaudeMcpState,
    type ClaudeMcpErrorCode,
} from "../../lib/desktop/claudeMcp";
import type { AgentId } from "../../lib/desktop/agents";
import type { McpRecord } from "../../lib/desktop/mcp";
import {
    McpInjectAgentChrome,
    injectStatusLabel,
    injectStatusState,
} from "./McpInjectAgentChrome";
import styles from "./Mcp.module.css";

export type McpInjectAgent = "pi" | "grok" | "claude";

// Each Agent keeps its own error-code type so `t()` keys are never unioned
// across Pi/Grok/Claude namespaces.
type RowRead =
    | { kind: "loading" }
    | { kind: "error"; agent: "pi"; code: PiMcpErrorCode }
    | { kind: "error"; agent: "grok"; code: GrokMcpErrorCode }
    | { kind: "error"; agent: "claude"; code: ClaudeMcpErrorCode }
    | { kind: "ready"; registered: boolean; targetPath: string };

/** Fixed per-Agent chrome: icon id, visible name key and manager title key. */
const AGENT_CHROME = {
    pi: {
        icon: "pi",
        name: "mcp.inject.pi",
        title: "mcp.pi.manage.title",
    },
    grok: {
        icon: "grokBuild",
        name: "mcp.inject.grok",
        title: "mcp.grok.manage.title",
    },
    claude: {
        icon: "claudeCode",
        name: "mcp.inject.claude",
        title: "mcp.claude.manage.title",
    },
} as const satisfies Record<
    McpInjectAgent,
    { icon: AgentId; name: string; title: string }
>;

/**
 * Clear Agent inject list row: icon, name, target path, registration status.
 * Unregistered rows expose Register (opens review dialog). Every row exposes
 * Refresh and History. Neither action embeds preview/confirm inline.
 */
export function McpInjectRow({
    agent,
    record,
    blocked = false,
    onRegister,
    onHistory,
}: {
    agent: McpInjectAgent;
    record: McpRecord;
    blocked?: boolean;
    onRegister: (trigger: HTMLElement | null) => void;
    onHistory: (trigger: HTMLElement | null) => void;
}): JSX.Element {
    const { t } = useTranslation();
    const [read, setRead] = useState<RowRead>({ kind: "loading" });
    const [attempt, setAttempt] = useState(0);
    const generation = useRef(0);
    useEffect(() => {
        const token = ++generation.current;
        const load =
            agent === "pi"
                ? readPiMcpState(record.id)
                : agent === "grok"
                  ? readGrokMcpState(record.id)
                  : readClaudeMcpState(record.id);
        void load.then(
            (state) => {
                if (generation.current !== token) return;
                const registered = state.deployments.some(
                    (row) => row.configRoot === state.configRoot,
                );
                setRead({
                    kind: "ready",
                    registered,
                    targetPath: state.targetPath,
                });
            },
            (error: unknown) => {
                if (generation.current !== token) return;
                if (agent === "pi") {
                    setRead({
                        kind: "error",
                        agent: "pi",
                        code:
                            error instanceof PiMcpError
                                ? error.code
                                : "outcome_unknown",
                    });
                    return;
                }
                if (agent === "claude") {
                    setRead({
                        kind: "error",
                        agent: "claude",
                        code:
                            error instanceof ClaudeMcpError
                                ? error.code
                                : "outcome_unknown",
                    });
                    return;
                }
                setRead({
                    kind: "error",
                    agent: "grok",
                    code:
                        error instanceof GrokMcpError
                            ? error.code
                            : "outcome_unknown",
                });
            },
        );
        return () => {
            generation.current += 1;
        };
    }, [agent, record.id, record.revision, attempt]);

    const statusKind = read.kind === "ready" ? "ready" : read.kind;
    const status = injectStatusLabel(
        statusKind,
        read.kind === "ready" && read.registered,
        t,
    );
    const statusState = injectStatusState(
        statusKind,
        read.kind === "ready" && read.registered,
    );
    const targetPath = read.kind === "ready" ? read.targetPath : null;
    const unregistered = read.kind === "ready" && !read.registered;
    const chrome = AGENT_CHROME[agent];
    const agentName = t(chrome.name);
    const regionLabel = t(chrome.title, { name: record.displayName });

    return (
        <section className={styles["injectBlock"]} aria-label={regionLabel}>
            <div className={styles["injectRow"]}>
                <McpInjectAgentChrome
                    agentId={chrome.icon}
                    nameKey={chrome.name}
                    targetPath={targetPath}
                />
                <span
                    className={styles["injectStatus"]}
                    data-state={statusState}
                >
                    {status}
                </span>
                <div className={listActions["group"]}>
                    <IconButton
                        icon="refresh"
                        blocked={blocked || read.kind === "loading"}
                        label={t("mcp.inject.refresh", {
                            agent: agentName,
                            name: record.displayName,
                        })}
                        onClick={() => {
                            if (blocked || read.kind === "loading") return;
                            generation.current += 1;
                            setRead({ kind: "loading" });
                            setAttempt((value) => value + 1);
                        }}
                    />
                    {unregistered && (
                        <IconButton
                            icon="plus"
                            blocked={blocked}
                            label={t("mcp.inject.register", {
                                agent: agentName,
                                name: record.displayName,
                            })}
                            onClick={() => {
                                const trigger =
                                    document.activeElement instanceof
                                    HTMLElement
                                        ? document.activeElement
                                        : null;
                                onRegister(trigger);
                            }}
                        />
                    )}
                    <IconButton
                        icon="logFile"
                        blocked={blocked}
                        label={t("mcp.inject.history", {
                            agent: agentName,
                            name: record.displayName,
                        })}
                        onClick={() => {
                            const trigger =
                                document.activeElement instanceof HTMLElement
                                    ? document.activeElement
                                    : null;
                            onHistory(trigger);
                        }}
                    />
                </div>
            </div>
            {read.kind === "error" && read.agent === "pi" && (
                <p role="alert" className={styles["injectRowError"]}>
                    {t(`mcp.pi.errors.${read.code}`)}
                </p>
            )}
            {read.kind === "error" && read.agent === "grok" && (
                <p role="alert" className={styles["injectRowError"]}>
                    {t(`mcp.grok.errors.${read.code}`)}
                </p>
            )}
            {read.kind === "error" && read.agent === "claude" && (
                <p role="alert" className={styles["injectRowError"]}>
                    {t(`mcp.claude.errors.${read.code}`)}
                </p>
            )}
        </section>
    );
}
