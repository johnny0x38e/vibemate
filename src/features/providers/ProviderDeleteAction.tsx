import { Dialog } from "@base-ui/react/dialog";
import { useEffect, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { IconButton } from "../../components/IconButton";
import { useNotify } from "../../components/Notifications";
import {
    clearProviderReference,
    deleteProvider,
    ProviderRequestError,
    providerDeletionImpact,
    type AgentReference,
    type DeletedProvider,
    type ProviderDeletionImpact,
    type ProviderErrorCode,
    type ProviderRecord,
} from "../../lib/desktop/providers";
import buttons from "../../components/actionButtons.module.css";
import { CodexReferenceRows } from "./CodexReferenceRows";
import styles from "./ProviderDeleteAction.module.css";

/** One destructive list action: confirm, delete, then report the outcome. */
export interface ProviderDeleteActionProps {
    record: ProviderRecord;
    /** True while the list or a form blocks mutations. */
    blocked: boolean;
    onDeleted: (deleted: DeletedProvider) => void;
    /** Re-read the first page after a conflict or unknown outcome. */
    onReload: () => void;
}

type DeleteState =
    | { kind: "idle" }
    | { kind: "deleting" }
    | { kind: "error"; code: ProviderErrorCode };

type ImpactState =
    | { kind: "idle" | "loading" }
    | { kind: "ready"; impact: ProviderDeletionImpact }
    | { kind: "error"; code: ProviderErrorCode };

function errorCode(error: unknown): ProviderErrorCode {
    return error instanceof ProviderRequestError
        ? error.code
        : "operation_failed";
}

function referenceKey(reference: AgentReference): string {
    return `${reference.agentId}:${reference.configRoot}:${reference.label}`;
}

/**
 * Delete one saved configuration after an explicit confirmation that names the
 * provider and its saved-model count. Pi/Grok ownership rows are listed before
 * deletion; active rows must be removed through the Agent configuration, while
 * stale rows can be cleared here. The dialog traps focus and returns it to the
 * row action. A conflict or unknown outcome reloads the list instead of
 * pretending the delete succeeded.
 */
export function ProviderDeleteAction({
    record,
    blocked,
    onDeleted,
    onReload,
}: ProviderDeleteActionProps): JSX.Element {
    const { t } = useTranslation();
    const notify = useNotify();
    const [open, setOpen] = useState(false);
    const [state, setState] = useState<DeleteState>({ kind: "idle" });
    const [impact, setImpact] = useState<{
        open: boolean;
        state: ImpactState;
    }>({ open: false, state: { kind: "idle" } });
    const [clearing, setClearing] = useState<string | null>(null);
    const [clearError, setClearError] = useState<ProviderErrorCode | null>(
        null,
    );
    const trigger = useRef<HTMLElement | null>(null);
    const cancelButton = useRef<HTMLButtonElement | null>(null);
    const mounted = useRef(true);
    const deleting = state.kind === "deleting";

    // Hiding or leaving the list unmounts this action; the reference must not
    // outlive the row it points at.
    useEffect(() => {
        mounted.current = true;
        return () => {
            mounted.current = false;
            trigger.current = null;
        };
    }, []);

    // Reset in render when the dialog opens or closes so the effect only starts
    // the read and never calls setState synchronously for the loading state.
    if (impact.open !== open) {
        setImpact({
            open,
            state: open ? { kind: "loading" } : { kind: "idle" },
        });
    }
    const impactState: ImpactState =
        impact.open === open ? impact.state : { kind: "loading" };

    useEffect(() => {
        if (!open) return;
        let active = true;
        void providerDeletionImpact(record.id).then(
            (next) => {
                if (active) {
                    setImpact({
                        open: true,
                        state: { kind: "ready", impact: next },
                    });
                }
            },
            (error: unknown) => {
                if (active) {
                    setImpact({
                        open: true,
                        state: { kind: "error", code: errorCode(error) },
                    });
                }
            },
        );
        return () => {
            active = false;
        };
    }, [open, record.id]);

    async function clear(reference: AgentReference): Promise<void> {
        if (clearing !== null || !reference.clearable) return;
        setClearing(referenceKey(reference));
        setClearError(null);
        try {
            await clearProviderReference({
                providerId: record.id,
                agentId: reference.agentId,
                configRoot: reference.configRoot,
                label: reference.label,
            });
            const next = await providerDeletionImpact(record.id);
            if (mounted.current) {
                setImpact({
                    open: true,
                    state: { kind: "ready", impact: next },
                });
            }
        } catch (error: unknown) {
            if (mounted.current) setClearError(errorCode(error));
        } finally {
            if (mounted.current) setClearing(null);
        }
    }

    async function confirm(): Promise<void> {
        if (
            deleting ||
            impactState.kind !== "ready" ||
            !impactState.impact.canDelete
        ) {
            return;
        }
        setState({ kind: "deleting" });
        try {
            const deleted = await deleteProvider({
                id: record.id,
                expectedRevision: record.revision,
            });
            setOpen(false);
            setState({ kind: "idle" });
            onDeleted(deleted);
            notify(
                t("providers.delete.done", {
                    name: deleted.displayName,
                }),
            );
        } catch (error: unknown) {
            const code = errorCode(error);
            // The list must not keep showing a row whose state is unknown.
            onReload();
            setState({ kind: "error", code });
            // Conflicts and unknown outcomes close the dialog and explain the
            // state through the notification; definite failures stay in view.
            if (
                code === "not_found" ||
                code === "delete_outcome_unknown" ||
                code === "operation_failed" ||
                code === "invalid_response"
            ) {
                setOpen(false);
                notify({
                    tone: "error",
                    message: t(`providers.errors.${code}`),
                });
            }
        }
    }

    const canDelete =
        state.kind === "idle" &&
        impactState.kind === "ready" &&
        impactState.impact.canDelete;
    const references =
        impactState.kind === "ready" ? impactState.impact.references : [];
    // Codex bindings block deletion too; they are resolved in Codex settings.
    const codexRefs =
        impactState.kind === "ready"
            ? (impactState.impact.codexReferences ?? [])
            : [];
    const remembered =
        impactState.kind === "ready"
            ? impactState.impact.rememberedPreferences
            : 0;

    return (
        <Dialog.Root
            open={open}
            onOpenChange={(nextOpen) => {
                // A delete in flight must finish; nothing else blocks closing.
                if (!nextOpen && deleting) return;
                setOpen(nextOpen);
                if (!nextOpen) {
                    setState({ kind: "idle" });
                    setClearError(null);
                }
            }}
        >
            <IconButton
                icon="trash"
                destructive
                label={t("providers.list.deleteLabel", {
                    name: record.displayName,
                })}
                blocked={blocked}
                onClick={() => {
                    if (blocked) return;
                    trigger.current =
                        document.activeElement instanceof HTMLElement
                            ? document.activeElement
                            : null;
                    setClearError(null);
                    setOpen(true);
                }}
            />
            <Dialog.Portal>
                <Dialog.Backdrop className={styles["backdrop"]} />
                <Dialog.Popup
                    className={styles["popup"]}
                    // Start on the safe action; the user must move to Delete on
                    // purpose. Base UI falls back to the popup when the ref is not
                    // ready yet, so no custom focus code is needed.
                    initialFocus={cancelButton}
                    finalFocus={() => trigger.current}
                >
                    <Dialog.Title className={styles["title"]}>
                        {t("providers.delete.title")}
                    </Dialog.Title>
                    <Dialog.Description className={styles["body"]}>
                        {t("providers.delete.body", {
                            name: record.displayName,
                            count: record.selectedModelCount,
                        })}
                    </Dialog.Description>
                    {impactState.kind === "loading" && (
                        <p className={styles["hint"]} role="status">
                            {t("providers.delete.checkingReferences")}
                        </p>
                    )}
                    {impactState.kind === "error" && (
                        <p className={styles["error"]} role="alert">
                            {t(`providers.errors.${impactState.code}`)}
                        </p>
                    )}
                    {impactState.kind === "ready" &&
                        (references.length > 0 || codexRefs.length > 0) && (
                            <div className={styles["references"]}>
                                <p className={styles["hint"]}>
                                    {t("providers.delete.referencesHint")}
                                </p>
                                <ul
                                    className={styles["referenceList"]}
                                    aria-label={t(
                                        "providers.delete.referencesLabel",
                                    )}
                                >
                                    {references.map((reference) => {
                                        const key = referenceKey(reference);
                                        return (
                                            <li
                                                className={
                                                    styles["referenceRow"]
                                                }
                                                key={key}
                                            >
                                                <div>
                                                    <p
                                                        className={
                                                            styles[
                                                                "referenceName"
                                                            ]
                                                        }
                                                    >
                                                        {t(
                                                            `providers.delete.referenceLabel.${reference.agentId}`,
                                                            {
                                                                label: reference.label,
                                                            },
                                                        )}
                                                    </p>
                                                    <p
                                                        className={
                                                            styles[
                                                                "referenceMeta"
                                                            ]
                                                        }
                                                    >
                                                        {reference.configRoot}
                                                        {" · "}
                                                        {t(
                                                            `providers.delete.referenceStates.${reference.state}`,
                                                        )}
                                                    </p>
                                                </div>
                                                {reference.clearable ? (
                                                    <button
                                                        type="button"
                                                        className={
                                                            buttons["secondary"]
                                                        }
                                                        disabled={
                                                            clearing !== null ||
                                                            deleting
                                                        }
                                                        onClick={() => {
                                                            void clear(
                                                                reference,
                                                            );
                                                        }}
                                                    >
                                                        {clearing === key
                                                            ? t(
                                                                  "providers.delete.clearing",
                                                              )
                                                            : t(
                                                                  "providers.delete.clear",
                                                              )}
                                                    </button>
                                                ) : (
                                                    <p
                                                        className={
                                                            styles[
                                                                "referenceAction"
                                                            ]
                                                        }
                                                    >
                                                        {t(
                                                            `providers.delete.referenceHints.${reference.state}`,
                                                        )}
                                                    </p>
                                                )}
                                            </li>
                                        );
                                    })}
                                </ul>
                                {codexRefs.length > 0 && (
                                    <ul
                                        className={styles["referenceList"]}
                                        aria-label={t(
                                            "providers.codexReferences.label",
                                        )}
                                    >
                                        <CodexReferenceRows
                                            references={codexRefs}
                                            classes={{
                                                row: styles["referenceRow"],
                                                name: styles["referenceName"],
                                                meta: styles["referenceMeta"],
                                                hint: styles["referenceAction"],
                                            }}
                                        />
                                    </ul>
                                )}
                                {clearError !== null && (
                                    <p className={styles["error"]} role="alert">
                                        {t(`providers.errors.${clearError}`)}
                                    </p>
                                )}
                            </div>
                        )}
                    {impactState.kind === "ready" && remembered > 0 && (
                        <p className={styles["hint"]}>
                            {t("providers.delete.remembered", {
                                count: remembered,
                            })}
                        </p>
                    )}
                    {state.kind === "error" && (
                        <p className={styles["error"]} role="alert">
                            {t(`providers.errors.${state.code}`)}
                        </p>
                    )}
                    <div className={styles["actions"]}>
                        <button
                            ref={cancelButton}
                            type="button"
                            className={buttons["secondary"]}
                            disabled={deleting}
                            onClick={() => {
                                setOpen(false);
                            }}
                        >
                            {t("providers.delete.cancel")}
                        </button>
                        <button
                            type="button"
                            className={buttons["dangerSecondary"]}
                            aria-disabled={!canDelete}
                            onClick={() => {
                                if (!canDelete) return;
                                void confirm();
                            }}
                        >
                            {t("providers.delete.confirm")}
                        </button>
                    </div>
                </Dialog.Popup>
            </Dialog.Portal>
        </Dialog.Root>
    );
}
