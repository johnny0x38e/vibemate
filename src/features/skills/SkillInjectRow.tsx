import { useId, useState, type JSX, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../../components/IconButton";
import listActions from "../../components/listActions.module.css";
import hierarchy from "../../components/textHierarchy.module.css";
import type { SkillDeployMode } from "../../lib/desktop/skills";
import type { SkillInjectRowStatus } from "./skillInject";
import styles from "./SkillInject.module.css";

/** One icon action on a row. `available: false` keeps it focusable but inert. */
export interface SkillInjectRowAction {
    kind: "inject" | "remove" | "resync";
    label: string;
    available: boolean;
    onRun: (trigger: HTMLElement | null) => void;
}

/** Inputs for {@link SkillInjectRow}. */
export interface SkillInjectRowProps {
    /** Decorative leading mark, for example an Agent icon. */
    leading?: ReactNode;
    /** Visible row name: the Agent on a skill page, the skill on an Agent page. */
    name: string;
    /** Accessible name of the row region. */
    regionLabel: string;
    status: SkillInjectRowStatus;
    /** Text shown instead of the status while this row's action runs. */
    progress: string | null;
    /** Short explanation under the name, for example why a row has no action. */
    note: string | null;
    /** Live target path; shown on hover and inside the details disclosure. */
    targetPath: string | null;
    /** Recorded link mode when the skill is placed, otherwise null. */
    deployedMode: SkillDeployMode | null;
    /** The one state-driven action, or null when the row offers none. */
    primary: SkillInjectRowAction | null;
    /** Optional escape hatch, such as Remove on a row that cannot resync. */
    secondary?: SkillInjectRowAction | null;
    /** Another mutation is running; actions stay focusable but inert. */
    blocked: boolean;
    error: string | null;
}

const ICONS = {
    inject: "plus",
    remove: "trash",
    resync: "refresh",
} as const;

function activeTrigger(): HTMLElement | null {
    return document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
}

/**
 * One inject row, shared by the skill detail page (one row per Agent) and the
 * Agent detail Skill tab (one row per library skill). It mirrors the MCP
 * inject row: mark + name, a colored status that is also spelled out in text,
 * and icon actions on the right. The row body is not a hit target; only the
 * buttons act. The target path lives in the hover title and in a details
 * disclosure instead of taking a line of its own.
 */
export function SkillInjectRow({
    leading,
    name,
    regionLabel,
    status,
    progress,
    note,
    targetPath,
    deployedMode,
    primary,
    secondary = null,
    blocked,
    error,
}: SkillInjectRowProps): JSX.Element {
    const { t } = useTranslation();
    const factsId = useId();
    const [expanded, setExpanded] = useState(false);
    const hasFacts = targetPath !== null || deployedMode !== null;
    const actions = [secondary, primary].filter(
        (action): action is SkillInjectRowAction => action !== null,
    );
    return (
        <li className={styles["block"]} aria-label={regionLabel}>
            <div className={styles["row"]}>
                <div
                    className={styles["chrome"]}
                    title={targetPath ?? undefined}
                >
                    {leading}
                    <div className={styles["chromeText"]}>
                        <span
                            className={[
                                styles["name"],
                                hierarchy["value"],
                            ].join(" ")}
                        >
                            {name}
                        </span>
                        {note !== null && (
                            <p
                                className={[
                                    styles["note"],
                                    hierarchy["hint"],
                                ].join(" ")}
                            >
                                {note}
                            </p>
                        )}
                    </div>
                </div>
                <span
                    className={styles["status"]}
                    data-state={status}
                    aria-live="polite"
                >
                    {progress ?? t(`skills.inject.status.${status}`)}
                </span>
                <div className={listActions["group"]}>
                    {hasFacts && (
                        <IconButton
                            icon="chevron"
                            borderless
                            label={t("skills.inject.details", { name })}
                            expanded={expanded}
                            controls={factsId}
                            onClick={() => {
                                setExpanded((value) => !value);
                            }}
                        />
                    )}
                    {actions.map((action) => (
                        <IconButton
                            key={action.kind}
                            icon={ICONS[action.kind]}
                            destructive={action.kind === "remove"}
                            blocked={blocked || !action.available}
                            label={action.label}
                            onClick={() => {
                                if (blocked || !action.available) return;
                                action.onRun(activeTrigger());
                            }}
                        />
                    ))}
                </div>
            </div>
            {hasFacts && expanded && (
                <dl id={factsId} className={styles["facts"]}>
                    {targetPath !== null && (
                        <div className={styles["fact"]}>
                            <dt className={hierarchy["label"]}>
                                {t("skills.inject.location")}
                            </dt>
                            <dd className={hierarchy["value"]}>{targetPath}</dd>
                        </div>
                    )}
                    {deployedMode !== null && (
                        <div className={styles["fact"]}>
                            <dt className={hierarchy["label"]}>
                                {t("skills.inject.method")}
                            </dt>
                            <dd className={hierarchy["value"]}>
                                {t(
                                    deployedMode === "symlink"
                                        ? "skills.inject.methodSymlink"
                                        : "skills.inject.methodCopy",
                                )}
                            </dd>
                        </div>
                    )}
                </dl>
            )}
            {error !== null && (
                <p role="alert" className={styles["error"]}>
                    {error}
                </p>
            )}
        </li>
    );
}
