import { useId, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { FieldSelect } from "../../components/FieldSelect";
import hierarchy from "../../components/textHierarchy.module.css";
import type { SkillLinkPreference } from "./skillInject";
import styles from "./SkillInject.module.css";

/** Inputs for {@link SkillLinkAdvanced}. */
export interface SkillLinkAdvancedProps {
    value: SkillLinkPreference;
    onChange: (value: SkillLinkPreference) => void;
    blocked: boolean;
}

/**
 * Collapsed "Advanced" disclosure holding the only link-mode override. By
 * default vibemate links when the system allows it and copies otherwise, so
 * most users never open this. A native `<details>` gives keyboard and
 * screen-reader disclosure behavior without extra state.
 */
export function SkillLinkAdvanced({
    value,
    onChange,
    blocked,
}: SkillLinkAdvancedProps): JSX.Element {
    const { t } = useTranslation();
    const id = useId();
    return (
        <details className={styles["advanced"]}>
            <summary>{t("skills.inject.advanced")}</summary>
            <div className={styles["advancedBody"]}>
                <div className={styles["advancedRow"]}>
                    <span className={hierarchy["label"]}>
                        {t("skills.inject.linkMethod")}
                    </span>
                    <FieldSelect
                        id={`${id}-link`}
                        value={value}
                        aria-label={t("skills.inject.linkMethod")}
                        options={[
                            {
                                value: "auto",
                                label: t("skills.inject.linkAuto"),
                            },
                            {
                                value: "copy",
                                label: t("skills.inject.linkCopy"),
                            },
                        ]}
                        blocked={blocked}
                        onChange={(next) => {
                            if (next === "auto" || next === "copy")
                                onChange(next);
                        }}
                    />
                </div>
                <p className={styles["panelHint"]}>
                    {t("skills.inject.linkHint")}
                </p>
            </div>
        </details>
    );
}
