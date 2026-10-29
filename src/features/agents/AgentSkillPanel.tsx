import { useEffect, useRef, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";
import {
    applySkillDeploy,
    disableSkillDeploy,
    listAgentSkillDeployments,
    listSkills,
    previewSkillDeploy,
    SkillRequestError,
    type SkillDeployAgent,
    type SkillDeployPreview,
    type SkillDeploymentRecord,
    type SkillErrorCode,
    type SkillRecord,
} from "../../lib/desktop/skills";
import {
    previewAgentSkillTargets,
    resyncRequestFromPreview,
    resyncSkillDeploy,
    type AgentSkillTargetState,
} from "../../lib/desktop/skillResync";
import type { AgentId } from "../../lib/desktop/agents";
import buttons from "../../components/actionButtons.module.css";
import {
    agentNameKey,
    batchStatus,
    canResync,
    chooseDeployMode,
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
    type SkillLinkPreference,
} from "../skills/skillInject";
import {
    SkillInjectRow,
    type SkillInjectRowAction,
} from "../skills/SkillInjectRow";
import { SkillLinkAdvanced } from "../skills/SkillLinkAdvanced";
import {
    SkillRemoveDialog,
    type SkillRemoveTarget,
} from "../skills/SkillRemoveDialog";
import {
    SkillResyncDialog,
    type SkillResyncTarget,
} from "../skills/SkillResyncDialog";
import inject from "../skills/SkillInject.module.css";
import styles from "./AgentList.module.css";

type PanelState =
    | { kind: "loading" }
    | { kind: "preview" }
    | { kind: "error"; code: SkillErrorCode }
    | {
          kind: "ready";
          skills: SkillRecord[];
          nextCursor: string | null;
          more: "idle" | "loading";
          deployments: SkillDeploymentRecord[];
          /** Live targets read for rows whose status needs one. */
          previews: ReadonlyMap<string, SkillDeployPreview>;
          /** Batch target states for every library skill (no full preview). */
          targets: ReadonlyMap<string, AgentSkillTargetState>;
      };

type Pending = { skillId: string; action: "inject" | "remove" | "resync" };
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
type BackupNotice = { skillName: string; path: string };

function asDeployAgent(agentId: AgentId): SkillDeployAgent | null {
    if (
        agentId === "pi" ||
        agentId === "grokBuild" ||
        agentId === "claudeCode" ||
        agentId === "codex"
    )
        return agentId;
    return null;
}

/**
 * One read-only batch classification of every library skill's target, so an
 * occupied name shows before Inject. A failed or browser read yields an empty
 * map and rows fall back to the per-skill previews.
 */
async function readTargets(
    agent: SkillDeployAgent,
): Promise<Map<string, AgentSkillTargetState>> {
    try {
        const result = await previewAgentSkillTargets(agent);
        if (result.kind !== "desktop") return new Map();
        return new Map(
            result.items.map((item) => [item.skillId, item.targetState]),
        );
    } catch {
        return new Map();
    }
}

/**
 * Read live targets for the given skills. A failed read only drops that
 * preview; the row then falls back to what the ownership record says.
 */
async function readPreviews(
    agent: SkillDeployAgent,
    skillIds: readonly string[],
): Promise<Map<string, SkillDeployPreview>> {
    const reads = await Promise.all(
        skillIds.map(async (skillId) => {
            try {
                const result = await previewSkillDeploy(agent, skillId);
                return result.kind === "desktop" ? result.preview : null;
            } catch {
                return null;
            }
        }),
    );
    const out = new Map<string, SkillDeployPreview>();
    for (const preview of reads) {
        if (preview !== null) out.set(preview.skillId, preview);
    }
    return out;
}

/** Inputs for {@link AgentSkillPanel}. */
export interface AgentSkillPanelProps {
    agentId: AgentId;
    /** Hidden tabs do not read; becoming visible re-reads current state. */
    hidden?: boolean;
    /** Jump to the Skills module home (offered when the library is empty). */
    onOpenSkills: () => void;
    /** Protect Agent navigation while an inject, remove or resync runs. */
    onBusyChange?: (busy: boolean) => void;
}

/**
 * Skill tab on an Agent detail page. For Claude Code, Pi, Grok Build and Codex it
 * lists every library skill with the same inject row as the skill detail
 * page, so a skill can be injected, removed or resynced here without leaving
 * the Agent. Unsupported Agents keep an honest notice and run no IPC.
 *
 * Reads: one library page, one bulk deployment list and one batch target
 * classification for this Agent, so occupied names show up front. Full
 * previews are read only for rows that need one: out-of-sync rows (Resync
 * needs the reviewed target), rows found occupied by an Inject attempt, and
 * (Codex) unrecorded rows for the shared-folder duplicate hint.
 */
export function AgentSkillPanel({
    agentId,
    hidden = false,
    onOpenSkills,
    onBusyChange,
}: AgentSkillPanelProps): JSX.Element {
    const { t } = useTranslation();
    const deployAgent = asDeployAgent(agentId);
    const [answer, setAnswer] = useState<{
        agentId: AgentId;
        state: PanelState;
    }>({ agentId, state: { kind: "loading" } });
    const [attempt, setAttempt] = useState(0);
    const [pending, setPending] = useState<Pending | null>(null);
    const [rowErrors, setRowErrors] = useState<ReadonlyMap<string, RowError>>(
        new Map(),
    );
    const [preference, setPreference] = useState<SkillLinkPreference>("auto");
    const [confirm, setConfirm] = useState<ConfirmRemove | null>(null);
    const [confirmResync, setConfirmResync] = useState<ConfirmResync | null>(
        null,
    );
    const [backupNotice, setBackupNotice] = useState<BackupNotice | null>(null);
    const generation = useRef(0);
    const inFlight = useRef(false);
    // Skills found occupied stay re-checked on every read until they clear.
    const occupied = useRef<{ agentId: AgentId; ids: Set<string> }>({
        agentId,
        ids: new Set(),
    });
    const busy = pending !== null;

    // When the Agent identity changes, reset in render so the effect only
    // starts the supported read and never calls setState synchronously.
    if (answer.agentId !== agentId) {
        setAnswer({ agentId, state: { kind: "loading" } });
        setRowErrors(new Map());
    }
    const state: PanelState | { kind: "unsupported" } =
        deployAgent === null
            ? { kind: "unsupported" }
            : answer.agentId === agentId
              ? answer.state
              : { kind: "loading" };

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
        if (deployAgent === null || hidden) return;
        const token = ++generation.current;
        if (occupied.current.agentId !== agentId)
            occupied.current = { agentId, ids: new Set() };
        void (async () => {
            try {
                const [page, deployed, targets] = await Promise.all([
                    listSkills(null),
                    listAgentSkillDeployments(deployAgent),
                    readTargets(deployAgent),
                ]);
                if (generation.current !== token) return;
                if (page.kind === "preview" || deployed.kind === "preview") {
                    setAnswer({ agentId, state: { kind: "preview" } });
                    return;
                }
                const recorded = new Set(
                    deployed.items.map((row) => row.skillId),
                );
                const wanted = [
                    ...deployed.items
                        .filter((row) => !row.inSync)
                        .map((row) => row.skillId),
                    ...[...occupied.current.ids].filter(
                        (id) => !recorded.has(id),
                    ),
                    // Codex also loads ~/.agents/skills; read the live target
                    // of not-yet-injected skills so a same-named duplicate is
                    // visible before Inject is pressed.
                    ...(deployAgent === "codex"
                        ? page.page.items
                              .map((skill) => skill.id)
                              .filter((id) => !recorded.has(id))
                        : []),
                ];
                const previews = await readPreviews(deployAgent, wanted);
                if (generation.current !== token) return;
                occupied.current = {
                    agentId,
                    ids: new Set(
                        [...previews.values()]
                            .filter(
                                (preview) =>
                                    preview.targetState === "foreign" &&
                                    !recorded.has(preview.skillId),
                            )
                            .map((preview) => preview.skillId),
                    ),
                };
                setAnswer({
                    agentId,
                    state: {
                        kind: "ready",
                        skills: page.page.items,
                        nextCursor: page.page.nextCursor,
                        more: "idle",
                        deployments: deployed.items,
                        previews,
                        targets,
                    },
                });
            } catch (error: unknown) {
                if (generation.current !== token) return;
                setAnswer({
                    agentId,
                    state: { kind: "error", code: skillErrorCode(error) },
                });
            }
        })();
        return () => {
            generation.current += 1;
        };
    }, [agentId, deployAgent, hidden, attempt]);

    function reload(): void {
        setAttempt((value) => value + 1);
    }

    async function more(): Promise<void> {
        if (state.kind !== "ready" || state.nextCursor === null || busy) return;
        if (state.more === "loading") return;
        const token = generation.current;
        const snapshot = state;
        setAnswer({ agentId, state: { ...snapshot, more: "loading" } });
        try {
            const next = await listSkills(snapshot.nextCursor);
            if (generation.current !== token) return;
            if (next.kind === "preview") {
                setAnswer({ agentId, state: { kind: "preview" } });
                return;
            }
            const seen = new Set(snapshot.skills.map((row) => row.id));
            setAnswer({
                agentId,
                state: {
                    ...snapshot,
                    skills: [
                        ...snapshot.skills,
                        ...next.page.items.filter((row) => !seen.has(row.id)),
                    ],
                    nextCursor: next.page.nextCursor,
                    more: "idle",
                },
            });
        } catch (error: unknown) {
            if (generation.current !== token) return;
            setAnswer({
                agentId,
                state: { kind: "error", code: skillErrorCode(error) },
            });
        }
    }

    /** Run one mutation for one skill row, then re-read the panel. */
    async function run(
        skillId: string,
        action: Pending["action"],
        mutation: () => Promise<unknown>,
        scope: () => SkillErrorScope = () => actionErrorScope(action),
    ): Promise<void> {
        if (inFlight.current) return;
        inFlight.current = true;
        setPending({ skillId, action });
        setBackupNotice(null);
        setRowErrors((current) => {
            const next = new Map(current);
            next.delete(skillId);
            return next;
        });
        try {
            await mutation();
            reload();
        } catch (error: unknown) {
            const code = skillErrorCode(error);
            const uncertain = uncertainOutcome(code);
            setRowErrors((current) =>
                new Map(current).set(skillId, {
                    code,
                    uncertain,
                    scope: scope(),
                }),
            );
            if (uncertain) reload();
        } finally {
            inFlight.current = false;
            setPending(null);
        }
    }

    /**
     * Inject reads the live target first. A foreign folder is reported as
     * occupied and nothing is written; otherwise the reviewed preview is
     * applied with the automatic (or Advanced) link mode.
     */
    function injectSkill(agent: SkillDeployAgent, skill: SkillRecord): void {
        // The preview step reads the Agent folder; the apply step then reads
        // the library, so a failure is reported against the step that ran.
        let step: SkillErrorScope = "target";
        void run(
            skill.id,
            "inject",
            async () => {
                const result = await previewSkillDeploy(agent, skill.id);
                if (result.kind === "preview")
                    throw new SkillRequestError("desktop_required");
                const preview = result.preview;
                if (!preview.canApply) {
                    occupied.current.ids.add(skill.id);
                    return;
                }
                step = "library";
                await applySkillDeploy({
                    agentId: agent,
                    skillId: skill.id,
                    expectedSkillRevision: preview.skillRevision,
                    configRoot: preview.configRoot,
                    mode: chooseDeployMode(
                        preview.symlinkAvailable,
                        preference,
                    ),
                    targetState: preview.targetState,
                });
            },
            () => step,
        );
    }

    /**
     * Resync runs only from the confirmation dialog, which says when edits
     * made outside vibemate are overwritten and where they are backed up.
     */
    function confirmResyncRun(): void {
        if (confirmResync === null) return;
        const { preview, deployment, skillName } = confirmResync;
        void run(preview.skillId, "resync", async () => {
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
            if (path !== null) setBackupNotice({ skillName, path });
        }).finally(() => {
            setConfirmResync(null);
        });
    }

    function confirmRemove(): void {
        if (confirm === null) return;
        const { deployment } = confirm;
        void run(deployment.skillId, "remove", () =>
            disableSkillDeploy({
                agentId: deployment.agentId,
                skillId: deployment.skillId,
                configRoot: deployment.configRoot,
            }),
        ).finally(() => {
            setConfirm(null);
        });
    }

    function row(
        agent: SkillDeployAgent,
        ready: Extract<PanelState, { kind: "ready" }>,
        skill: SkillRecord,
    ): JSX.Element {
        const agentName = t(agentNameKey(agent));
        const labels = { agent: agentName, name: skill.name };
        const deployment =
            ready.deployments.find((item) => item.skillId === skill.id) ?? null;
        const preview = ready.previews.get(skill.id) ?? null;
        // Without a full preview, the batch classification still marks a
        // name another folder occupies, or a target it could not read.
        const fromBatch =
            preview === null && deployment === null
                ? batchStatus(ready.targets.get(skill.id))
                : null;
        const status = fromBatch ?? injectStatus(preview, deployment);
        const action = primaryAction(status);
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
                              skillName: skill.name,
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
        if (action === "inject") {
            primary = {
                kind: "inject",
                label: t("skills.inject.inject", labels),
                available: true,
                onRun: () => {
                    injectSkill(agent, skill);
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
                    if (preview === null) return;
                    setConfirmResync({
                        skillName: skill.name,
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
            if (edited || !possible) secondary = removeAction(true);
        } else if (status === "occupied") {
            note = t("skills.inject.occupiedNote");
        } else if (status === "unreadable") {
            note = t("skills.inject.unreadableNote");
        }
        const rowError = rowErrors.get(skill.id);
        const duplicatePath = sharedDuplicateNote(preview);
        return (
            <SkillInjectRow
                key={skill.id}
                name={skill.name}
                regionLabel={t("skills.inject.rowLabel", labels)}
                status={status}
                progress={
                    pending?.skillId === skill.id
                        ? t(`skills.inject.progress.${pending.action}`)
                        : null
                }
                note={joinNotes(
                    note,
                    duplicatePath === null
                        ? null
                        : t("skills.inject.sharedDuplicateNote", {
                              path: duplicatePath,
                          }),
                )}
                targetPath={preview?.targetPath ?? null}
                deployedMode={deployment?.mode ?? null}
                primary={primary}
                secondary={secondary}
                blocked={busy}
                error={
                    rowError === undefined
                        ? null
                        : `${t(skillErrorKey(rowError.code, rowError.scope))}${
                              rowError.uncertain
                                  ? ` ${t("skills.reloadUnknown")}`
                                  : ""
                          }`
                }
            />
        );
    }

    return (
        <div className={styles["skillPanel"]}>
            {state.kind === "unsupported" && (
                <p className={styles["hint"]}>
                    {t("agents.skillPanel.unsupported")}
                </p>
            )}
            {state.kind === "loading" && (
                <p className={styles["hint"]} role="status">
                    {t("agents.skillPanel.loading")}
                </p>
            )}
            {state.kind === "preview" && (
                <p className={styles["hint"]}>
                    {t("agents.skillPanel.desktopRequired")}
                </p>
            )}
            {state.kind === "error" && (
                <>
                    <p className={styles["hint"]} role="alert">
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
            {state.kind === "ready" &&
                deployAgent !== null &&
                state.skills.length === 0 && (
                    <>
                        <p className={styles["hint"]}>
                            {t("agents.skillPanel.empty")}
                        </p>
                        <button
                            className={buttons["secondary"]}
                            type="button"
                            onClick={onOpenSkills}
                        >
                            {t("agents.skillPanel.openSkills")}
                        </button>
                    </>
                )}
            {backupNotice !== null && (
                <p className={styles["hint"]} role="status">
                    {t("skills.inject.resyncBackedUpSkill", {
                        name: backupNotice.skillName,
                        path: backupNotice.path,
                    })}
                </p>
            )}
            {state.kind === "ready" &&
                deployAgent !== null &&
                state.skills.length > 0 && (
                    <div className={inject["agentPanel"]}>
                        <ul
                            className={inject["list"]}
                            aria-label={t("agents.skillPanel.listLabel")}
                        >
                            {state.skills.map((skill) =>
                                row(deployAgent, state, skill),
                            )}
                        </ul>
                        {state.nextCursor !== null && (
                            <div className={inject["agentActions"]}>
                                <button
                                    className={buttons["secondary"]}
                                    type="button"
                                    aria-disabled={
                                        busy || state.more === "loading"
                                    }
                                    onClick={() => {
                                        void more();
                                    }}
                                >
                                    {t(
                                        state.more === "loading"
                                            ? "skills.loadingMore"
                                            : "skills.loadMore",
                                    )}
                                </button>
                            </div>
                        )}
                        <SkillLinkAdvanced
                            value={preference}
                            onChange={setPreference}
                            blocked={busy}
                        />
                    </div>
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
        </div>
    );
}
