import { Dialog } from "@base-ui/react/dialog";
import { useRef, type JSX } from "react";
import { useTranslation } from "react-i18next";
import buttons from "../../components/actionButtons.module.css";
import { CodexSettingsPanel } from "./CodexSettingsPanel";
import { WriteCurrentSummary } from "./RestoreView";
import dialog from "./VersionTextDialog.module.css";

/**
 * List-row "Write" dialog for Codex. Same chrome as WriteConfigDialog (shared
 * backdrop, compact popup, focus trap) but the body is the Codex provider /
 * default flow from `codex.ts`, which has its own review contract.
 */
export function CodexWriteDialog({
    providerLabel,
    modelLabel,
    blocked,
    onBusyChange,
    onApplied,
    onClose,
    returnFocus,
}: {
    providerLabel: string | null;
    modelLabel: string | null;
    blocked: boolean;
    onBusyChange: (busy: boolean) => void;
    onApplied: () => void;
    onClose: () => void;
    returnFocus: () => HTMLElement | null;
}): JSX.Element {
    const { t } = useTranslation();
    const title = useRef<HTMLHeadingElement>(null);

    return (
        <Dialog.Root
            open
            modal="trap-focus"
            disablePointerDismissal
            onOpenChange={(open) => {
                if (!open && !blocked) onClose();
            }}
        >
            <Dialog.Portal>
                <Dialog.Backdrop className={dialog["backdrop"]} />
                <Dialog.Popup
                    className={dialog["popupCompact"]}
                    initialFocus={title}
                    finalFocus={returnFocus}
                >
                    <Dialog.Title
                        ref={title}
                        tabIndex={-1}
                        className={dialog["title"]}
                    >
                        {t("agents.writeDialog.title", {
                            name: t("agents.names.codex"),
                        })}
                    </Dialog.Title>
                    <WriteCurrentSummary
                        providerLabel={providerLabel}
                        modelLabel={modelLabel}
                    />
                    <CodexSettingsPanel
                        mode="write"
                        blocked={blocked}
                        onBusyChange={onBusyChange}
                        onApplied={onApplied}
                    />
                    <div className={dialog["actions"]}>
                        <button
                            type="button"
                            className={buttons["secondary"]}
                            aria-disabled={blocked}
                            onClick={() => {
                                if (!blocked) onClose();
                            }}
                        >
                            {t("agents.writeDialog.close")}
                        </button>
                    </div>
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
