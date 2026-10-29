import { Dialog } from "@base-ui/react/dialog";
import { useRef, type JSX } from "react";
import { useTranslation } from "react-i18next";
import buttons from "../../components/actionButtons.module.css";
import dialog from "./SkillDeleteDialog.module.css";

/** What a resync confirmation names, and whether it replaces user edits. */
export interface SkillResyncTarget {
    skillName: string;
    agentName: string;
    /** Folder the resync rewrites from the library. */
    targetPath: string;
    /**
     * Set when the target holds edits made outside vibemate: Rust backs them
     * up into `backupDir` before overwriting them. Null when nothing the user
     * changed is replaced.
     */
    overwrite: { backupDir: string | null } | null;
    /** Control that opened the dialog; focus returns here when it still exists. */
    trigger: HTMLElement | null;
}

/** Inputs for {@link SkillResyncDialog}. */
export interface SkillResyncDialogProps {
    target: SkillResyncTarget | null;
    resyncing: boolean;
    onConfirm: () => void;
    onCancel: () => void;
}

/**
 * Confirmation before resyncing one placed skill from the library. Same shell
 * and focus rules as {@link SkillRemoveDialog}: Cancel takes the initial
 * focus, and Escape or the backdrop cancel unless the resync is running. When
 * the target was edited outside vibemate it says plainly that those edits are
 * overwritten and where the backup goes.
 */
export function SkillResyncDialog({
    target,
    resyncing,
    onConfirm,
    onCancel,
}: SkillResyncDialogProps): JSX.Element {
    const { t } = useTranslation();
    const cancel = useRef<HTMLButtonElement>(null);
    const overwrite = target?.overwrite ?? null;
    return (
        <Dialog.Root
            open={target !== null}
            onOpenChange={(open) => {
                if (!open && !resyncing) onCancel();
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
                        {t("skills.inject.resyncTitle", {
                            agent: target?.agentName ?? "",
                        })}
                    </Dialog.Title>
                    <Dialog.Description className={dialog["body"]}>
                        {t("skills.inject.resyncBody", {
                            name: target?.skillName ?? "",
                            agent: target?.agentName ?? "",
                        })}
                    </Dialog.Description>
                    {overwrite !== null && (
                        <p className={dialog["body"]} role="note">
                            {overwrite.backupDir === null
                                ? t("skills.inject.resyncOverwriteNoDir", {
                                      path: target?.targetPath ?? "",
                                  })
                                : t("skills.inject.resyncOverwrite", {
                                      path: target?.targetPath ?? "",
                                      backupDir: overwrite.backupDir,
                                  })}
                        </p>
                    )}
                    <div className={dialog["actions"]}>
                        <button
                            ref={cancel}
                            type="button"
                            className={buttons["secondary"]}
                            disabled={resyncing}
                            onClick={onCancel}
                        >
                            {t("skills.inject.removeCancel")}
                        </button>
                        <button
                            type="button"
                            className={
                                buttons[
                                    overwrite === null
                                        ? "primary"
                                        : "dangerSecondary"
                                ]
                            }
                            aria-disabled={resyncing}
                            onClick={() => {
                                if (!resyncing) onConfirm();
                            }}
                        >
                            {t(
                                resyncing
                                    ? "skills.inject.progress.resync"
                                    : overwrite === null
                                      ? "skills.inject.resyncConfirm"
                                      : "skills.inject.resyncConfirmOverwrite",
                            )}
                        </button>
                    </div>
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
