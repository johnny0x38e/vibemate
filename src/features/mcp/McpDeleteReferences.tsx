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
import buttons from "../../components/actionButtons.module.css";
import styles from "./McpPiPreview.module.css";

type AgentState<T, E> =
    | { kind: "loading" }
    | { kind: "ready"; value: T }
    | { kind: "error"; code: E };

function allowed(
    pi: AgentState<PiMcpState, PiMcpErrorCode>,
    grok: AgentState<GrokMcpState, GrokMcpErrorCode>,
    definitionId: string,
): boolean {
    if (pi.kind !== "ready" || grok.kind !== "ready") return false;
    return (
        pi.value.deployments.length === 0 &&
        pi.value.recovery?.definitionId !== definitionId &&
        grok.value.deployments.length === 0 &&
        grok.value.recovery?.definitionId !== definitionId
    );
}

/** Explain every retained Pi/Grok root reference before deletion. Advisory only;
 * the backend repeats its transactional reference check at actual deletion. */
export function McpDeleteReferences({
    definitionId,
    onChecked,
    onManage,
}: {
    definitionId: string;
    onChecked: (allowed: boolean) => void;
    onManage: (agent: "pi" | "grok") => void;
}): JSX.Element {
    const { t } = useTranslation();
    const [attempt, setAttempt] = useState(0);
    const key = `${definitionId}:${String(attempt)}`;
    const [answer, setAnswer] = useState<{
        key: string;
        pi: AgentState<PiMcpState, PiMcpErrorCode>;
        grok: AgentState<GrokMcpState, GrokMcpErrorCode>;
    }>({
        key,
        pi: { kind: "loading" },
        grok: { kind: "loading" },
    });
    // Reset loading state during render when the selection or retry changes,
    // matching usePiMcpRegistrations — avoid synchronous setState inside effects.
    if (answer.key !== key) {
        setAnswer({
            key,
            pi: { kind: "loading" },
            grok: { kind: "loading" },
        });
    }
    const pi = answer.key === key ? answer.pi : { kind: "loading" as const };
    const grok =
        answer.key === key ? answer.grok : { kind: "loading" as const };
    useEffect(() => {
        let current = true;
        onChecked(false);
        void Promise.allSettled([
            readPiMcpState(definitionId),
            readGrokMcpState(definitionId),
        ]).then(([piResult, grokResult]) => {
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
            setAnswer({ key, pi: nextPi, grok: nextGrok });
            onChecked(allowed(nextPi, nextGrok, definitionId));
        });
        return () => {
            current = false;
        };
    }, [definitionId, attempt, onChecked, key]);
    return (
        <div className={styles["section"]}>
            {(pi.kind === "loading" || grok.kind === "loading") && (
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
        </div>
    );
}
