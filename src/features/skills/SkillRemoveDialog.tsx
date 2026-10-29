import { Dialog } from "@base-ui/react/dialog";
import { useRef, type JSX } from "react";
import { useTranslation } from "react-i18next";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./SkillDeleteDialog.module.css";

/** What the confirmation names: the skill, the Agent, and whether it drifted. */
export interface SkillRemoveTarget {
    skillName: string;
    agentName: string;
    /** The placement was edited outside vibemate, so Rust leaves it in place. */
    changedOutside: boolean;
    /** Control that opened the dialog; focus returns here when it still exists. */
    trigger: HTMLElement | null;
}

/** Inputs for {@link SkillRemoveDialog}. */
export interface SkillRemoveDialogProps {
    target: SkillRemoveTarget | null;
    removing: boolean;
    onConfirm: () => void;
    onCancel: () => void;
}

/**
 * Confirmation before removing a skill from one Agent. It reuses the skill
 * delete dialog shell, whose backdrop composes the shared full-window
 * `dialogOverlay.module.css` scrim. Cancel takes the initial focus so Enter
 * never removes by accident; Escape and the backdrop cancel unless a removal
 * is already running.
 */
export function SkillRemoveDialog({
    target,
    removing,
    onConfirm,
    onCancel,
}: SkillRemoveDialogProps): JSX.Element {
    const { t } = useTranslation();
    const cancel = useRef<HTMLButtonElement>(null);
    return (
        <Dialog.Root
            open={target !== null}
            onOpenChange={(open) => {
                if (!open && !removing) onCancel();
            }}
        >
            <Dialog.Portal>
                <Dialog.Backdrop className={dialog["backdrop"]} />
                <Dialog.Popup
                    className={dialog["popup"]}
                    initialFocus={cancel}
                    finalFocus={() =>
                        target?.trigger?.isConnected === true
                            ? target.trigger
                            : null
                    }
                >
                    <Dialog.Title className={dialog["title"]}>
                        {t("skills.inject.removeTitle", {
                            agent: target?.agentName ?? "",
                        })}
                    </Dialog.Title>
                    <Dialog.Description className={dialog["body"]}>
                        {t("skills.inject.removeBody", {
                            name: target?.skillName ?? "",
                            agent: target?.agentName ?? "",
                        })}
                    </Dialog.Description>
                    {target?.changedOutside === true && (
                        <p className={dialog["body"]}>
                            {t("skills.inject.removeChangedNote")}
                        </p>
                    )}
                    <div className={dialog["actions"]}>
                        <button
                            ref={cancel}
                            type="button"
                            className={buttons["secondary"]}
                            disabled={removing}
                            onClick={onCancel}
                        >
                            {t("skills.inject.removeCancel")}
                        </button>
                        <button
                            type="button"
                            className={buttons["dangerSecondary"]}
                            aria-disabled={removing}
                            onClick={() => {
                                if (!removing) onConfirm();
                            }}
                        >
                            {t(
                                removing
                                    ? "skills.inject.progress.remove"
                                    : "skills.inject.removeConfirm",
                            )}
                        </button>
                    </div>
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
