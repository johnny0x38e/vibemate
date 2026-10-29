import { useEffect, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import {
    dismissSkillDeployRecovery,
    finishSkillDeployRecovery,
    listSkillDeployRecoveries,
    SkillRequestError,
    type SkillDeployAgent,
    type SkillDeployRecoveryItem,
    type SkillErrorCode,
} from "../../lib/desktop/skills";
import buttons from "../../components/actionButtons.module.css";
import styles from "./SkillLibrary.module.css";

type PanelState =
    | { kind: "loading" | "hidden" | "preview_only" }
    | { kind: "ready"; items: SkillDeployRecoveryItem[] }
    | { kind: "error"; code: SkillErrorCode; uncertain: boolean };

type Answer = {
    attempt: number;
    state: PanelState;
};

function errorCode(error: unknown): SkillErrorCode {
    return error instanceof SkillRequestError ? error.code : "operation_failed";
}

function uncertain(code: SkillErrorCode): boolean {
    return [
        "operation_failed",
        "invalid_response",
        "outcome_unknown",
        "write_failed",
        "storage_unavailable",
        "target_changed",
    ].includes(code);
}

function agentLabel(
    agentId: SkillDeployAgent,
):
    | "skills.deploy.agentPi"
    | "skills.deploy.agentGrokBuild"
    | "skills.deploy.agentClaudeCode"
    | "skills.deploy.agentCodex" {
    switch (agentId) {
        case "pi":
            return "skills.deploy.agentPi";
        case "grokBuild":
            return "skills.deploy.agentGrokBuild";
        case "claudeCode":
            return "skills.deploy.agentClaudeCode";
        case "codex":
            return "skills.deploy.agentCodex";
    }
}

/**
 * Unfinished Skill deploy/disable journal rows. Hidden when none exist.
 * Finish and Dismiss re-classify before acting; the panel never replays a write.
 */
export function SkillRecoveryPanel(): JSX.Element | null {
    const { t } = useTranslation();
    const [attempt, setAttempt] = useState(0);
    const [answer, setAnswer] = useState<Answer>({
        attempt: 0,
        state: { kind: "loading" },
    });
    const [busyId, setBusyId] = useState<string | null>(null);
    const [actionError, setActionError] = useState<{
        code: SkillErrorCode;
        uncertain: boolean;
    } | null>(null);
    const generation = useRef(0);

    // Reset loading in render when a reload bumps `attempt`, so the effect never
    // calls setState synchronously for the loading placeholder.
    if (answer.attempt !== attempt) {
        setAnswer({ attempt, state: { kind: "loading" } });
    }
    const state: PanelState =
        answer.attempt === attempt ? answer.state : { kind: "loading" };

    useEffect(() => {
        const token = ++generation.current;
        void listSkillDeployRecoveries()
            .then((result) => {
                if (generation.current !== token) return;
                if (result.kind === "preview") {
                    setAnswer({ attempt, state: { kind: "preview_only" } });
                    return;
                }
                if (result.items.length === 0) {
                    setAnswer({ attempt, state: { kind: "hidden" } });
                    return;
                }
                setAnswer({
                    attempt,
                    state: { kind: "ready", items: result.items },
                });
            })
            .catch((error: unknown) => {
                if (generation.current !== token) return;
                const code = errorCode(error);
                setAnswer({
                    attempt,
                    state: { kind: "error", code, uncertain: uncertain(code) },
                });
            });
        return () => {
            generation.current += 1;
        };
    }, [attempt]);

    function reload(): void {
        setBusyId(null);
        setActionError(null);
        setAttempt((value) => value + 1);
    }

    async function finish(item: SkillDeployRecoveryItem): Promise<void> {
        if (busyId !== null || !item.canFinish) return;
        setBusyId(item.id);
        setActionError(null);
        try {
            await finishSkillDeployRecovery({
                id: item.id,
                expectedClassification: item.classification,
            });
            reload();
        } catch (error: unknown) {
            const code = errorCode(error);
            setActionError({ code, uncertain: uncertain(code) });
            if (uncertain(code) || code === "target_changed") reload();
            else setBusyId(null);
        }
    }

    async function dismiss(item: SkillDeployRecoveryItem): Promise<void> {
        if (busyId !== null || !item.canDismiss) return;
        setBusyId(item.id);
        setActionError(null);
        try {
            await dismissSkillDeployRecovery({
                id: item.id,
                expectedClassification: item.classification,
            });
            reload();
        } catch (error: unknown) {
            const code = errorCode(error);
            setActionError({ code, uncertain: uncertain(code) });
            if (uncertain(code) || code === "target_changed") reload();
            else setBusyId(null);
        }
    }

    if (state.kind === "hidden" || state.kind === "preview_only") {
        return null;
    }

    return (
        <section
            className={styles["recovery"]}
            aria-label={t("skills.recovery.title")}
        >
            <h2 className={styles["recoveryTitle"]}>
                {t("skills.recovery.title")}
            </h2>
            <p className={styles["hint"]}>{t("skills.recovery.description")}</p>
            {state.kind === "loading" && (
                <p className={styles["hint"]} role="status">
                    {t("skills.recovery.loading")}
                </p>
            )}
            {state.kind === "error" && (
                <>
                    <p className={styles["error"]} role="alert">
                        {t(`skills.errors.${state.code}`)}
                    </p>
                    <button
                        className={buttons["secondary"]}
                        type="button"
                        onClick={reload}
                    >
                        {t("skills.reload")}
                    </button>
                </>
            )}
            {state.kind === "ready" && (
                <ul className={styles["deployList"]}>
                    {state.items.map((item) => (
                        <li key={item.id} className={styles["deployRow"]}>
                            <div className={styles["recoveryBody"]}>
                                <p className={styles["longValue"]}>
                                    {t("skills.recovery.rowSummary", {
                                        kind: t(
                                            `skills.recovery.kinds.${item.kind}`,
                                        ),
                                        skill: item.skillName,
                                        agent: t(agentLabel(item.agentId)),
                                        classification: t(
                                            `skills.recovery.classifications.${item.classification}`,
                                        ),
                                        state: t(
                                            `skills.recovery.states.${item.state}`,
                                        ),
                                    })}
                                </p>
                                <p className={styles["longValue"]}>
                                    {item.targetPath}
                                </p>
                            </div>
                            <div className={styles["deployActions"]}>
                                {item.canFinish && (
                                    <button
                                        className={buttons["primary"]}
                                        type="button"
                                        disabled={busyId !== null}
                                        onClick={() => {
                                            void finish(item);
                                        }}
                                    >
                                        {busyId === item.id
                                            ? t("skills.recovery.finishing")
                                            : t("skills.recovery.finish")}
                                    </button>
                                )}
                                {item.canDismiss && (
                                    <button
                                        className={buttons["secondary"]}
                                        type="button"
                                        disabled={busyId !== null}
                                        onClick={() => {
                                            void dismiss(item);
                                        }}
                                    >
                                        {busyId === item.id
                                            ? t("skills.recovery.dismissing")
                                            : t("skills.recovery.dismiss")}
                                    </button>
                                )}
                            </div>
                        </li>
                    ))}
                </ul>
            )}
            {actionError !== null && (
                <>
                    <p className={styles["error"]} role="alert">
                        {t(`skills.errors.${actionError.code}`)}
                    </p>
                    {actionError.uncertain && (
                        <p className={styles["hint"]}>
                            {t("skills.recovery.reloadHint")}
                        </p>
                    )}
                </>
            )}
        </section>
    );
}
