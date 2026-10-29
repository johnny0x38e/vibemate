import { useEffect, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { DetailPanel } from "../../components/DetailForm";
import buttons from "../../components/actionButtons.module.css";
import { AgentIcon } from "../agents/AgentIcon";
import {
    applySkillDeploy,
    disableSkillDeploy,
    listSkillDeployments,
    previewSkillDeploy,
    type SkillDeployAgent,
    type SkillDeployPreview,
    type SkillDeploymentRecord,
    type SkillErrorCode,
    type SkillRecord,
} from "../../lib/desktop/skills";
import {
    resyncRequestFromPreview,
    resyncSkillDeploy,
} from "../../lib/desktop/skillResync";
import {
    INJECT_AGENTS,
    agentNameKey,
    canResync,
    chooseDeployMode,
    deploymentFor,
    editedOutside,
    injectStatus,
    primaryAction,
    joinNotes,
    resyncBackupPath,
    resyncMode,
    resyncOverwrite,
    sharedDuplicateNote,
    actionErrorScope,
    skillErrorCode,
    skillErrorKey,
    uncertainOutcome,
    type SkillErrorScope,
    type SkillInjectRowStatus,
    type SkillLinkPreference,
} from "./skillInject";
import { SkillInjectRow, type SkillInjectRowAction } from "./SkillInjectRow";
import { SkillLinkAdvanced } from "./SkillLinkAdvanced";
import { SkillRemoveDialog, type SkillRemoveTarget } from "./SkillRemoveDialog";
import { SkillResyncDialog, type SkillResyncTarget } from "./SkillResyncDialog";
import styles from "./SkillInject.module.css";

/** Live preview for one Agent, or the safe code its read failed with. */
type AgentRead =
    | { kind: "ready"; preview: SkillDeployPreview }
    | { kind: "error"; code: SkillErrorCode };

type PanelState =
    | { kind: "loading" }
    | { kind: "preview_only" }
    | { kind: "error"; code: SkillErrorCode }
    | {
          kind: "ready";
          deployments: SkillDeploymentRecord[];
          agents: Map<SkillDeployAgent, AgentRead>;
      };

type Answer = { skillId: string; skillRevision: number; state: PanelState };

type Pending = {
    agent: SkillDeployAgent;
    action: "inject" | "remove" | "resync";
};

type RowError = {
    code: SkillErrorCode;
    uncertain: boolean;
    scope: SkillErrorScope;
};

type ConfirmRemove = SkillRemoveTarget & {
    deployment: SkillDeploymentRecord;
};

type ConfirmResync = SkillResyncTarget & {
    preview: SkillDeployPreview;
    deployment: SkillDeploymentRecord;
};

/** Shown after a resync that replaced edits: where the backup went. */
type BackupNotice = { agentName: string; path: string };

/** Read one Agent's live target; failures stay on that Agent's row. */
async function readAgent(
    agent: SkillDeployAgent,
    skillId: string,
): Promise<AgentRead | { kind: "preview" }> {
    try {
        const result = await previewSkillDeploy(agent, skillId);
        return result.kind === "preview"
            ? { kind: "preview" }
            : { kind: "ready", preview: result.preview };
    } catch (error: unknown) {
        return { kind: "error", code: skillErrorCode(error) };
    }
}

/** Inputs for {@link SkillDeployPanel}. */
export interface SkillDeployPanelProps {
    record: SkillRecord;
    /** Protect sidebar navigation while an inject, remove or resync runs. */
    onBusyChange?: (busy: boolean) => void;
}

/**
 * Inject one library skill into every supported Agent at once. Each Agent is
 * its own row (mirroring the MCP inject card) with one state-driven action:
 * Inject when nothing is placed, Remove when it is injected, Resync when the
 * placement drifted, and no action when a foreign folder occupies the name.
 * Link mode is automatic (symlink when possible, copy otherwise); the only
 * override sits in a collapsed Advanced disclosure. Injection is always
 * user-level: the target is the Agent's user skills directory.
 */
export function SkillDeployPanel({
    record,
    onBusyChange,
}: SkillDeployPanelProps): JSX.Element {
    const { t } = useTranslation();
    const skillId = record.id;
    const skillRevision = record.revision;
    const [answer, setAnswer] = useState<Answer>({
        skillId,
        skillRevision,
        state: { kind: "loading" },
    });
    const [attempt, setAttempt] = useState(0);
    const [pending, setPending] = useState<Pending | null>(null);
    const [rowErrors, setRowErrors] = useState<
        ReadonlyMap<SkillDeployAgent, RowError>
    >(new Map());
    const [preference, setPreference] = useState<SkillLinkPreference>("auto");
    const [confirm, setConfirm] = useState<ConfirmRemove | null>(null);
    const [confirmResync, setConfirmResync] = useState<ConfirmResync | null>(
        null,
    );
    const [backupNotice, setBackupNotice] = useState<BackupNotice | null>(null);
    const generation = useRef(0);
    // Guards a second click that lands before React re-renders the blocked row.
    const inFlight = useRef(false);
    const busy = pending !== null;

    const sameRecord =
        answer.skillId === skillId && answer.skillRevision === skillRevision;
    // Reset in render when the detail page shows a different skill or revision,
    // so the effect never calls setState synchronously for "loading".
    if (!sameRecord) {
        setAnswer({ skillId, skillRevision, state: { kind: "loading" } });
    }
    const state: PanelState = sameRecord ? answer.state : { kind: "loading" };

    // Parents pass inline callbacks; a ref keeps busy reporting tied to
    // `busy` alone so a new callback identity never re-fires the effect.
    const busyListener = useRef(onBusyChange);
    useEffect(() => {
        busyListener.current = onBusyChange;
    }, [onBusyChange]);
    useEffect(() => {
        busyListener.current?.(busy);
        return () => {
            busyListener.current?.(false);
        };
    }, [busy]);

    useEffect(() => {
        // A reload keeps the previous rows on screen until the new answer
        // arrives; the generation token drops answers from an older read.
        const token = ++generation.current;
        void (async () => {
            try {
                const [listResult, reads] = await Promise.all([
                    listSkillDeployments(skillId),
                    Promise.all(
                        INJECT_AGENTS.map((agent) => readAgent(agent, skillId)),
                    ),
                ]);
                if (generation.current !== token) return;
                if (
                    listResult.kind === "preview" ||
                    reads.some((read) => read.kind === "preview")
                ) {
                    setAnswer({
                        skillId,
                        skillRevision,
                        state: { kind: "preview_only" },
                    });
                    return;
                }
                const agents = new Map<SkillDeployAgent, AgentRead>();
                INJECT_AGENTS.forEach((agent, index) => {
                    const read = reads[index];
                    if (read !== undefined && read.kind !== "preview")
                        agents.set(agent, read);
                });
                setAnswer({
                    skillId,
                    skillRevision,
                    state: {
                        kind: "ready",
                        deployments: listResult.items,
                        agents,
                    },
                });
            } catch (error: unknown) {
                if (generation.current !== token) return;
                setAnswer({
                    skillId,
                    skillRevision,
                    state: { kind: "error", code: skillErrorCode(error) },
                });
            }
        })();
        return () => {
            generation.current += 1;
        };
    }, [skillId, skillRevision, attempt]);

    function reload(): void {
        setAttempt((value) => value + 1);
    }

    /** Run one mutation for one Agent row, then re-read every row. */
    async function run(
        agent: SkillDeployAgent,
        action: Pending["action"],
        mutation: () => Promise<unknown>,
    ): Promise<void> {
        if (inFlight.current) return;
        inFlight.current = true;
        setPending({ agent, action });
        setBackupNotice(null);
        setRowErrors((current) => {
            const next = new Map(current);
            next.delete(agent);
            return next;
        });
        try {
            await mutation();
            reload();
        } catch (error: unknown) {
            const code = skillErrorCode(error);
            const uncertain = uncertainOutcome(code);
            setRowErrors((current) =>
                new Map(current).set(agent, {
                    code,
                    uncertain,
                    scope: actionErrorScope(action),
                }),
            );
            if (uncertain) reload();
        } finally {
            inFlight.current = false;
            setPending(null);
        }
    }

    function inject(preview: SkillDeployPreview): void {
        void run(preview.agentId, "inject", () =>
            applySkillDeploy({
                agentId: preview.agentId,
                skillId: preview.skillId,
                expectedSkillRevision: preview.skillRevision,
                configRoot: preview.configRoot,
                mode: chooseDeployMode(preview.symlinkAvailable, preference),
                targetState: preview.targetState,
            }),
        );
    }

    /**
     * Resync runs only from the confirmation dialog. `resync_skill_deploy`
     * rewrites an owned target from the library, including one edited outside
     * vibemate (backed up by Rust first); the dialog names that backup folder
     * and the notice afterwards names the exact backup.
     */
    function confirmResyncRun(): void {
        if (confirmResync === null) return;
        const { preview, deployment, agentName } = confirmResync;
        void run(preview.agentId, "resync", async () => {
            const result = await resyncSkillDeploy(
                resyncRequestFromPreview(
                    preview,
                    resyncMode(
                        deployment.mode,
                        preview.symlinkAvailable,
                        preference,
                    ),
                ),
            );
            const path = resyncBackupPath(result);
            if (path !== null) setBackupNotice({ agentName, path });
        }).finally(() => {
            setConfirmResync(null);
        });
    }

    function confirmRemove(): void {
        if (confirm === null) return;
        const { deployment } = confirm;
        void run(deployment.agentId, "remove", () =>
            disableSkillDeploy({
                agentId: deployment.agentId,
                skillId: deployment.skillId,
                configRoot: deployment.configRoot,
            }),
        ).finally(() => {
            setConfirm(null);
        });
    }

    function duplicateNote(preview: SkillDeployPreview | null): string | null {
        const path = sharedDuplicateNote(preview);
        return path === null
            ? null
            : t("skills.inject.sharedDuplicateNote", { path });
    }

    function row(agent: SkillDeployAgent): JSX.Element {
        const agentName = t(agentNameKey(agent));
        const labels = { agent: agentName, name: record.name };
        const read =
            state.kind === "ready" ? state.agents.get(agent) : undefined;
        const preview = read?.kind === "ready" ? read.preview : null;
        const deployment =
            state.kind === "ready" && preview !== null
                ? deploymentFor(state.deployments, agent, preview.configRoot)
                : null;
        const rowStatus: SkillInjectRowStatus =
            preview !== null
                ? injectStatus(preview, deployment)
                : read?.kind === "error"
                  ? "unchecked"
                  : "loading";
        const removeAction = (changedOutside: boolean): SkillInjectRowAction =>
            deployment === null
                ? {
                      kind: "remove",
                      label: t("skills.inject.remove", labels),
                      available: false,
                      onRun: () => undefined,
                  }
                : {
                      kind: "remove",
                      label: t("skills.inject.remove", labels),
                      available: true,
                      onRun: (trigger) => {
                          setConfirm({
                              skillName: record.name,
                              agentName,
                              changedOutside,
                              trigger,
                              deployment,
                          });
                      },
                  };
        let primary: SkillInjectRowAction | null = null;
        let secondary: SkillInjectRowAction | null = null;
        let note: string | null = null;
        if (preview !== null) {
            const status = injectStatus(preview, deployment);
            const action = primaryAction(status);
            if (action === "inject") {
                primary = {
                    kind: "inject",
                    label: t("skills.inject.inject", labels),
                    available: preview.canApply,
                    onRun: () => {
                        inject(preview);
                    },
                };
            } else if (action === "remove") {
                primary = removeAction(false);
            } else if (action === "resync" && deployment !== null) {
                const possible = canResync(preview, deployment);
                const edited = editedOutside(preview, deployment);
                primary = {
                    kind: "resync",
                    label: t(
                        possible
                            ? "skills.inject.resync"
                            : "skills.inject.resyncUnavailable",
                        labels,
                    ),
                    available: possible,
                    onRun: (trigger) => {
                        setConfirmResync({
                            skillName: record.name,
                            agentName,
                            targetPath: preview.targetPath,
                            overwrite: resyncOverwrite(preview),
                            trigger,
                            preview,
                            deployment,
                        });
                    },
                };
                note = t(
                    !possible
                        ? "skills.inject.resyncUnavailableNote"
                        : edited
                          ? "skills.inject.resyncEditedNote"
                          : "skills.inject.needsSyncNote",
                );
                // Edited files: Resync replaces them, while Remove stays the
                // way to keep the edits and just stop managing them.
                if (edited || !possible) secondary = removeAction(true);
            } else if (status === "occupied") {
                note = t("skills.inject.occupiedNote");
            }
        }
        const rowError = rowErrors.get(agent);
        const error =
            read?.kind === "error"
                ? // The single preview reads the Agent's injected copy.
                  t(skillErrorKey(read.code, "target"))
                : rowError !== undefined
                  ? `${t(skillErrorKey(rowError.code, rowError.scope))}${
                        rowError.uncertain
                            ? ` ${t("skills.reloadUnknown")}`
                            : ""
                    }`
                  : null;
        return (
            <SkillInjectRow
                key={agent}
                leading={<AgentIcon id={agent} size="row" />}
                name={agentName}
                regionLabel={t("skills.inject.rowLabel", labels)}
                status={rowStatus}
                progress={
                    pending?.agent === agent
                        ? t(`skills.inject.progress.${pending.action}`)
                        : null
                }
                note={joinNotes(note, duplicateNote(preview))}
                targetPath={preview?.targetPath ?? null}
                deployedMode={deployment?.mode ?? null}
                primary={primary}
                secondary={secondary}
                blocked={busy}
                error={error}
            />
        );
    }

    return (
        <DetailPanel label={t("skills.inject.title")}>
            {state.kind === "preview_only" && (
                <p className={styles["panelHint"]}>
                    {t("skills.inject.desktopRequired")}
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
            {backupNotice !== null && (
                <p className={styles["panelHint"]} role="status">
                    {t("skills.inject.resyncBackedUp", {
                        agent: backupNotice.agentName,
                        path: backupNotice.path,
                    })}
                </p>
            )}
            {(state.kind === "loading" || state.kind === "ready") && (
                <>
                    <ul
                        className={styles["list"]}
                        aria-label={t("skills.inject.listLabel")}
                        aria-busy={state.kind === "loading"}
                    >
                        {INJECT_AGENTS.map((agent) => row(agent))}
                    </ul>
                    <SkillLinkAdvanced
                        value={preference}
                        onChange={setPreference}
                        blocked={busy}
                    />
                </>
            )}
            <SkillRemoveDialog
                target={confirm}
                removing={busy && confirm !== null}
                onConfirm={confirmRemove}
                onCancel={() => {
                    if (!busy) setConfirm(null);
                }}
            />
            <SkillResyncDialog
                target={confirmResync}
                resyncing={busy && confirmResync !== null}
                onConfirm={confirmResyncRun}
                onCancel={() => {
                    if (!busy) setConfirmResync(null);
                }}
            />
        </DetailPanel>
    );
}
