import { useEffect, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import buttons from "../../components/actionButtons.module.css";
import {
    listRecoverySummary,
    type RecoverySummaryItem,
} from "../../lib/desktop/recoverySummary";
import type { OverviewDestination } from "./OverviewView";
import styles from "./OverviewView.module.css";

type HubState =
    | { kind: "loading" | "hidden" | "preview_only" }
    | { kind: "ready"; items: RecoverySummaryItem[] }
    | { kind: "error" };

type Answer = { attempt: number; state: HubState };

/**
 * Unified discovery entry for unfinished Agent config, Skill deploy, and MCP
 * writes. Navigates to the owning module; never replays a mutation itself.
 */
export function RecoveryHub({
    onNavigate,
}: {
    onNavigate: (destination: OverviewDestination) => void;
}): JSX.Element | null {
    const { t } = useTranslation();
    const [attempt, setAttempt] = useState(0);
    const [answer, setAnswer] = useState<Answer>({
        attempt: 0,
        state: { kind: "loading" },
    });
    const generation = useRef(0);

    if (answer.attempt !== attempt) {
        setAnswer({ attempt, state: { kind: "loading" } });
    }
    const state: HubState =
        answer.attempt === attempt ? answer.state : { kind: "loading" };

    useEffect(() => {
        const token = ++generation.current;
        void listRecoverySummary().then(
            (result) => {
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
            },
            () => {
                if (generation.current === token)
                    setAnswer({ attempt, state: { kind: "error" } });
            },
        );
        return () => {
            generation.current += 1;
        };
    }, [attempt]);

    if (state.kind === "hidden" || state.kind === "preview_only") {
        return null;
    }

    return (
        <section
            className={styles["recovery"]}
            aria-label={t("desktop.overview.recovery.title")}
        >
            <h2 className={styles["recoveryTitle"]}>
                {t("desktop.overview.recovery.title")}
            </h2>
            <p className={styles["recoveryHint"]}>
                {t("desktop.overview.recovery.description")}
            </p>
            {state.kind === "loading" && (
                <p className={styles["recoveryHint"]} role="status">
                    {t("desktop.overview.recovery.loading")}
                </p>
            )}
            {state.kind === "error" && (
                <>
                    <p className={styles["recoveryError"]} role="alert">
                        {t("desktop.overview.recovery.error")}
                    </p>
                    <button
                        className={buttons["secondary"]}
                        type="button"
                        onClick={() => {
                            setAttempt((value) => value + 1);
                        }}
                    >
                        {t("desktop.overview.retry")}
                    </button>
                </>
            )}
            {state.kind === "ready" && (
                <ul className={styles["recoveryList"]}>
                    {state.items.map((item) => (
                        <li key={item.id} className={styles["recoveryRow"]}>
                            <div className={styles["recoveryBody"]}>
                                <p className={styles["recoverySubject"]}>
                                    {t("desktop.overview.recovery.rowSummary", {
                                        domain: t(
                                            `desktop.overview.recovery.domains.${item.domain}`,
                                        ),
                                        subject: item.subject,
                                        detail: item.detail,
                                    })}
                                </p>
                            </div>
                            <button
                                className={buttons["secondary"]}
                                type="button"
                                onClick={() => {
                                    onNavigate(item.destination);
                                }}
                            >
                                {t(
                                    `desktop.overview.recovery.open.${item.destination}`,
                                )}
                            </button>
                        </li>
                    ))}
                </ul>
            )}
        </section>
    );
}
