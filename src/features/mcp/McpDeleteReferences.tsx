import { useEffect, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import {
    PiMcpError,
    readPiMcpState,
    type PiMcpErrorCode,
    type PiMcpState,
} from "../../lib/desktop/piMcp";
import {
    GrokMcpError,
    readGrokMcpState,
    type GrokMcpErrorCode,
    type GrokMcpState,
} from "../../lib/desktop/grokMcp";
import {
    ClaudeMcpError,
    readClaudeMcpState,
    type ClaudeMcpErrorCode,
    type ClaudeMcpState,
} from "../../lib/desktop/claudeMcp";
import type { McpInjectAgent } from "./McpInjectRow";
import buttons from "../../components/actionButtons.module.css";
import styles from "./McpPiPreview.module.css";

type AgentState<T, E> =
    | { kind: "loading" }
    | { kind: "ready"; value: T }
    | { kind: "error"; code: E };

function allowed(
    pi: AgentState<PiMcpState, PiMcpErrorCode>,
    grok: AgentState<GrokMcpState, GrokMcpErrorCode>,
    claude: AgentState<ClaudeMcpState, ClaudeMcpErrorCode>,
    definitionId: string,
): boolean {
    if (pi.kind !== "ready" || grok.kind !== "ready" || claude.kind !== "ready")
        return false;
    return (
        pi.value.deployments.length === 0 &&
        pi.value.recovery?.definitionId !== definitionId &&
        grok.value.deployments.length === 0 &&
        grok.value.recovery?.definitionId !== definitionId &&
        claude.value.deployments.length === 0 &&
        claude.value.recovery?.definitionId !== definitionId
    );
}

/** Explain every retained Pi/Grok/Claude Code root reference before deletion. Advisory only;
 * the backend repeats its transactional reference check at actual deletion. */
export function McpDeleteReferences({
    definitionId,
    onChecked,
    onManage,
}: {
    definitionId: string;
    onChecked: (allowed: boolean) => void;
    onManage: (agent: McpInjectAgent) => void;
}): JSX.Element {
    const { t } = useTranslation();
    const [attempt, setAttempt] = useState(0);
    const key = `${definitionId}:${String(attempt)}`;
    const [answer, setAnswer] = useState<{
        key: string;
        pi: AgentState<PiMcpState, PiMcpErrorCode>;
        grok: AgentState<GrokMcpState, GrokMcpErrorCode>;
        claude: AgentState<ClaudeMcpState, ClaudeMcpErrorCode>;
    }>({
        key,
        pi: { kind: "loading" },
        grok: { kind: "loading" },
        claude: { kind: "loading" },
    });
    // Reset loading state during render when the selection or retry changes,
    // matching usePiMcpRegistrations — avoid synchronous setState inside effects.
    if (answer.key !== key) {
        setAnswer({
            key,
            pi: { kind: "loading" },
            grok: { kind: "loading" },
            claude: { kind: "loading" },
        });
    }
    const pi = answer.key === key ? answer.pi : { kind: "loading" as const };
    const grok =
        answer.key === key ? answer.grok : { kind: "loading" as const };
    const claude =
        answer.key === key ? answer.claude : { kind: "loading" as const };
    useEffect(() => {
        let current = true;
        onChecked(false);
        void Promise.allSettled([
            readPiMcpState(definitionId),
            readGrokMcpState(definitionId),
            readClaudeMcpState(definitionId),
        ]).then(([piResult, grokResult, claudeResult]) => {
            if (!current) return;
            const nextPi: AgentState<PiMcpState, PiMcpErrorCode> =
                piResult.status === "fulfilled"
                    ? { kind: "ready", value: piResult.value }
                    : {
                          kind: "error",
                          code:
                              piResult.reason instanceof PiMcpError
                                  ? piResult.reason.code
                                  : "outcome_unknown",
                      };
            const nextGrok: AgentState<GrokMcpState, GrokMcpErrorCode> =
                grokResult.status === "fulfilled"
                    ? { kind: "ready", value: grokResult.value }
                    : {
                          kind: "error",
                          code:
                              grokResult.reason instanceof GrokMcpError
                                  ? grokResult.reason.code
                                  : "outcome_unknown",
                      };
            const nextClaude: AgentState<ClaudeMcpState, ClaudeMcpErrorCode> =
                claudeResult.status === "fulfilled"
                    ? { kind: "ready", value: claudeResult.value }
                    : {
                          kind: "error",
                          code:
                              claudeResult.reason instanceof ClaudeMcpError
                                  ? claudeResult.reason.code
                                  : "outcome_unknown",
                      };
            setAnswer({
                key,
                pi: nextPi,
                grok: nextGrok,
                claude: nextClaude,
            });
            onChecked(allowed(nextPi, nextGrok, nextClaude, definitionId));
        });
        return () => {
            current = false;
        };
    }, [definitionId, attempt, onChecked, key]);
    return (
        <div className={styles["section"]}>
            {(pi.kind === "loading" ||
                grok.kind === "loading" ||
                claude.kind === "loading") && (
                <p role="status">{t("mcp.delete.checkingReferences")}</p>
            )}
            {pi.kind === "error" && (
                <>
                    <p role="alert">{t(`mcp.pi.errors.${pi.code}`)}</p>
                    <button
                        type="button"
                        className={buttons["secondary"]}
                        onClick={() => {
                            setAttempt((value) => value + 1);
                        }}
                    >
                        {t("mcp.retry")}
                    </button>
                </>
            )}
            {grok.kind === "error" && (
                <>
                    <p role="alert">{t(`mcp.grok.errors.${grok.code}`)}</p>
                    <button
                        type="button"
                        className={buttons["secondary"]}
                        onClick={() => {
                            setAttempt((value) => value + 1);
                        }}
                    >
                        {t("mcp.retry")}
                    </button>
                </>
            )}
            {claude.kind === "error" && (
                <>
                    <p role="alert">{t(`mcp.claude.errors.${claude.code}`)}</p>
                    <button
                        type="button"
                        className={buttons["secondary"]}
                        onClick={() => {
                            setAttempt((value) => value + 1);
                        }}
                    >
                        {t("mcp.retry")}
                    </button>
                </>
            )}
            {pi.kind === "ready" && (
                <>
                    {pi.value.deployments.map((row) => (
                        <p key={`pi:${row.configRoot}`}>
                            {t("mcp.pi.manage.deleteReference", {
                                path: row.configRoot,
                                state: t(`mcp.pi.states.${row.state}`),
                            })}
                        </p>
                    ))}
                    {pi.value.recovery?.definitionId === definitionId && (
                        <p>
                            {t("mcp.pi.recovery.pending", {
                                name: pi.value.recovery.serverName,
                            })}
                        </p>
                    )}
                    {(pi.value.deployments.length > 0 ||
                        pi.value.recovery?.definitionId === definitionId) && (
                        <button
                            type="button"
                            className={buttons["secondary"]}
                            onClick={() => {
                                onManage("pi");
                            }}
                        >
                            {t("mcp.pi.manage.resolveReferences")}
                        </button>
                    )}
                </>
            )}
            {grok.kind === "ready" && (
                <>
                    {grok.value.deployments.map((row) => (
                        <p key={`grok:${row.configRoot}`}>
                            {t("mcp.grok.manage.deleteReference", {
                                path: row.configRoot,
                                state: t(`mcp.grok.states.${row.state}`),
                            })}
                        </p>
                    ))}
                    {grok.value.recovery?.definitionId === definitionId && (
                        <p>
                            {t("mcp.grok.recovery.pending", {
                                name: grok.value.recovery.serverName,
                            })}
                        </p>
                    )}
                    {(grok.value.deployments.length > 0 ||
                        grok.value.recovery?.definitionId === definitionId) && (
                        <button
                            type="button"
                            className={buttons["secondary"]}
                            onClick={() => {
                                onManage("grok");
                            }}
                        >
                            {t("mcp.grok.manage.resolveReferences")}
                        </button>
                    )}
                </>
            )}
            {claude.kind === "ready" && (
                <>
                    {claude.value.deployments.map((row) => (
                        <p key={`claude:${row.configRoot}`}>
                            {t("mcp.claude.manage.deleteReference", {
                                path: row.configRoot,
                                state: t(`mcp.claude.states.${row.state}`),
                            })}
                        </p>
                    ))}
                    {claude.value.recovery?.definitionId === definitionId && (
                        <p>
                            {t("mcp.claude.recovery.pending", {
                                name: claude.value.recovery.serverName,
                            })}
                        </p>
                    )}
                    {(claude.value.deployments.length > 0 ||
                        claude.value.recovery?.definitionId ===
                            definitionId) && (
                        <button
                            type="button"
                            className={buttons["secondary"]}
                            onClick={() => {
                                onManage("claude");
                            }}
                        >
                            {t("mcp.claude.manage.resolveReferences")}
                        </button>
                    )}
                </>
            )}
        </div>
    );
}
