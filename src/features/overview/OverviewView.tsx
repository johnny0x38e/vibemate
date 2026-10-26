import {
    useCallback,
    useEffect,
    useId,
    useRef,
    useState,
    type JSX,
} from "react";
import { useTranslation } from "react-i18next";
import { Icon, type IconName } from "../../components/Icon";
import { PageModuleHeader } from "../../components/PageModuleHeader";
import { discoverAgents } from "../../lib/desktop/agents";
import { countProviderModels } from "../../lib/desktop/models";
import { listMcpDefinitions } from "../../lib/desktop/mcp";
import { listProviders } from "../../lib/desktop/providers";
import { listSkills } from "../../lib/desktop/skills";
import { RecoveryHub } from "./RecoveryHub";
import styles from "./OverviewView.module.css";

/** Sidebar destinations a stat card can open. Models open Providers (no models nav). */
export type OverviewDestination = "providers" | "agents" | "skills" | "mcp";

export interface OverviewViewProps {
    onNavigate: (destination: OverviewDestination) => void;
}

/** One overview metric after an honest desktop or preview read. */
type StatState =
    | { kind: "loading" }
    | { kind: "preview" }
    | { kind: "ready"; total: number }
    | { kind: "error" };

type CardId = "providers" | "models" | "agents" | "skills" | "mcp";

interface CardSpec {
    id: CardId;
    icon: IconName;
    destination: OverviewDestination;
}

const CARDS: readonly CardSpec[] = [
    { id: "providers", icon: "providers", destination: "providers" },
    { id: "models", icon: "models", destination: "providers" },
    { id: "agents", icon: "agents", destination: "agents" },
    { id: "skills", icon: "skills", destination: "skills" },
    { id: "mcp", icon: "mcp", destination: "mcp" },
];

function cardLabel(
    id: CardId,
    t: (
        key:
            | "desktop.overview.cards.providers"
            | "desktop.overview.cards.models"
            | "desktop.overview.cards.agents"
            | "desktop.overview.cards.skills"
            | "desktop.overview.cards.mcp",
    ) => string,
): string {
    switch (id) {
        case "providers":
            return t("desktop.overview.cards.providers");
        case "models":
            return t("desktop.overview.cards.models");
        case "agents":
            return t("desktop.overview.cards.agents");
        case "skills":
            return t("desktop.overview.cards.skills");
        case "mcp":
            return t("desktop.overview.cards.mcp");
    }
}

const EMPTY_PATHS = {
    claudePath: null,
    codexPath: null,
    opencodePath: null,
    hermesPath: null,
    piPath: null,
    grokPath: null,
} as const;

/**
 * Home destination: five configuration totals from existing list/discovery
 * reads. The models card uses `count_provider_models` (selected-into-library
 * rows only, not the unselected catalog fetch cache). Nothing here claims a
 * live connection or that skills/MCP are already injected into Agents.
 */
export function OverviewView({ onNavigate }: OverviewViewProps): JSX.Element {
    const { t } = useTranslation();
    const headingId = useId();
    const [providers, setProviders] = useState<StatState>({ kind: "loading" });
    const [models, setModels] = useState<StatState>({ kind: "loading" });
    const [agents, setAgents] = useState<StatState>({ kind: "loading" });
    const [skills, setSkills] = useState<StatState>({ kind: "loading" });
    const [mcp, setMcp] = useState<StatState>({ kind: "loading" });
    const [attempt, setAttempt] = useState(0);
    const generation = useRef(0);

    const reload = useCallback((): void => {
        const current = ++generation.current;
        setProviders({ kind: "loading" });
        setModels({ kind: "loading" });
        setAgents({ kind: "loading" });
        setSkills({ kind: "loading" });
        setMcp({ kind: "loading" });

        void listProviders({ after: null, limit: 1 }).then(
            (result) => {
                if (generation.current !== current) return;
                setProviders(
                    result.kind === "preview"
                        ? { kind: "preview" }
                        : { kind: "ready", total: result.page.total },
                );
            },
            () => {
                if (generation.current === current)
                    setProviders({ kind: "error" });
            },
        );

        void countProviderModels().then(
            (result) => {
                if (generation.current !== current) return;
                setModels(
                    result.kind === "preview"
                        ? { kind: "preview" }
                        : { kind: "ready", total: result.count.total },
                );
            },
            () => {
                if (generation.current === current)
                    setModels({ kind: "error" });
            },
        );

        void discoverAgents({ ...EMPTY_PATHS }).then(
            (result) => {
                if (generation.current !== current) return;
                if (result.kind === "preview") {
                    setAgents({ kind: "preview" });
                    return;
                }
                // Catalog has six slots; only `found` entries count. Discovery
                // does not launch version probes.
                const total = result.items.filter(
                    (row) => row.state === "found",
                ).length;
                setAgents({ kind: "ready", total });
            },
            () => {
                if (generation.current === current)
                    setAgents({ kind: "error" });
            },
        );

        void listSkills(null, 1).then(
            (result) => {
                if (generation.current !== current) return;
                setSkills(
                    result.kind === "preview"
                        ? { kind: "preview" }
                        : { kind: "ready", total: result.page.total },
                );
            },
            () => {
                if (generation.current === current)
                    setSkills({ kind: "error" });
            },
        );

        void listMcpDefinitions(null, 1).then(
            (result) => {
                if (generation.current !== current) return;
                setMcp(
                    result.kind === "preview"
                        ? { kind: "preview" }
                        : { kind: "ready", total: result.page.total },
                );
            },
            () => {
                if (generation.current === current) setMcp({ kind: "error" });
            },
        );
    }, []);

    useEffect(() => {
        // Defer the first read like SkillLibrary so the effect body does not
        // call setState synchronously (react-hooks/set-state-in-effect).
        const timer = window.setTimeout(() => {
            reload();
        }, 0);
        return () => {
            window.clearTimeout(timer);
            generation.current += 1;
        };
    }, [reload, attempt]);

    const stateFor = (id: CardId): StatState => {
        switch (id) {
            case "providers":
                return providers;
            case "models":
                return models;
            case "agents":
                return agents;
            case "skills":
                return skills;
            case "mcp":
                return mcp;
        }
    };

    function valueText(state: StatState): string {
        switch (state.kind) {
            case "loading":
                return t("desktop.overview.loading");
            case "preview":
                return t("desktop.overview.preview");
            case "ready":
                return String(state.total);
            case "error":
                return t("desktop.overview.unavailable");
        }
    }

    return (
        <section aria-labelledby={headingId}>
            <PageModuleHeader
                className={styles["pageHeader"] ?? ""}
                icon="home"
                title={t("desktop.nav.overview")}
                headingId={headingId}
            />
            <RecoveryHub onNavigate={onNavigate} />
            <ul
                className={styles["grid"]}
                aria-label={t("desktop.overview.stats")}
            >
                {CARDS.map((card) => {
                    const state = stateFor(card.id);
                    const label = cardLabel(card.id, t);
                    const value = valueText(state);
                    const isError = state.kind === "error";
                    const isLoading = state.kind === "loading";
                    const isZero = state.kind === "ready" && state.total === 0;
                    return (
                        <li key={card.id} className={styles["item"]}>
                            <div className={styles["card"]}>
                                <button
                                    type="button"
                                    className={styles["open"]}
                                    aria-label={`${label}: ${value}`}
                                    onClick={() => {
                                        onNavigate(card.destination);
                                    }}
                                >
                                    <span className={styles["top"]}>
                                        <span
                                            className={styles["icon"]}
                                            aria-hidden="true"
                                        >
                                            <Icon name={card.icon} />
                                        </span>
                                        <span className={styles["name"]}>
                                            {label}
                                        </span>
                                    </span>
                                    {isLoading ? (
                                        <span
                                            className={styles["value"]}
                                            data-kind="loading"
                                            aria-busy="true"
                                            aria-label={value}
                                        >
                                            <span
                                                className={styles["skeleton"]}
                                                aria-hidden="true"
                                            />
                                        </span>
                                    ) : (
                                        <span
                                            className={styles["value"]}
                                            data-kind={state.kind}
                                            {...(isZero
                                                ? { "data-zero": "" }
                                                : {})}
                                        >
                                            {value}
                                        </span>
                                    )}
                                </button>
                                {isError && (
                                    <button
                                        type="button"
                                        className={styles["retry"]}
                                        aria-label={t(
                                            "desktop.overview.retryLabel",
                                            { name: label },
                                        )}
                                        onClick={() => {
                                            setAttempt(
                                                (previous) => previous + 1,
                                            );
                                        }}
                                    >
                                        {t("desktop.overview.retry")}
                                    </button>
                                )}
                            </div>
                        </li>
                    );
                })}
            </ul>
        </section>
    );
}
