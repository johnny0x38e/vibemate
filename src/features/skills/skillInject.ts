import {
    SkillRequestError,
    type SkillDeployAgent,
    type SkillDeployMode,
    type SkillDeployPreview,
    type SkillDeploymentRecord,
    type SkillErrorCode,
} from "../../lib/desktop/skills";
import type {
    AgentSkillTargetState,
    SkillResynced,
} from "../../lib/desktop/skillResync";

/** Agents that accept user-level skill injection, in display order. */
export const INJECT_AGENTS: readonly SkillDeployAgent[] = [
    "claudeCode",
    "pi",
    "grokBuild",
    "codex",
];

/**
 * What one Agent row tells the user about one skill:
 * - `not_injected`: nothing is placed in the Agent's skills folder yet.
 * - `injected`: vibemate placed the link or copy and it still matches.
 * - `needs_sync`: vibemate placed it, but it is missing or was edited outside.
 * - `occupied`: a folder vibemate does not manage already uses the name.
 * - `unreadable`: the batch read could not inspect the target, so nothing is
 *   offered there (it is not known to be free or occupied).
 */
export type SkillInjectStatus =
    "not_injected" | "injected" | "needs_sync" | "occupied" | "unreadable";

/** Row status plus the transient read states shown before an answer exists. */
export type SkillInjectRowStatus = SkillInjectStatus | "loading" | "unchecked";

/** The one state-driven action a row offers, or none for a foreign folder. */
export type SkillInjectAction = "inject" | "remove" | "resync" | "none";

/**
 * Link preference from the Advanced disclosure. `auto` places a symbolic link
 * when the system allows one and falls back to a copy otherwise; `copy`
 * always copies the files.
 */
export type SkillLinkPreference = "auto" | "copy";

/** Translation key for an Agent's visible name. */
export type InjectAgentNameKey =
    | "skills.deploy.agentPi"
    | "skills.deploy.agentGrokBuild"
    | "skills.deploy.agentClaudeCode"
    | "skills.deploy.agentCodex";

export function agentNameKey(agent: SkillDeployAgent): InjectAgentNameKey {
    if (agent === "pi") return "skills.deploy.agentPi";
    if (agent === "grokBuild") return "skills.deploy.agentGrokBuild";
    if (agent === "codex") return "skills.deploy.agentCodex";
    return "skills.deploy.agentClaudeCode";
}

/**
 * Hint for a Codex preview whose skill name is also present under
 * `~/.agents/skills`. Codex loads both locations and lists same-named skills
 * separately instead of merging them, so the user should know before injecting.
 */
export function sharedDuplicateNote(
    preview: SkillDeployPreview | null,
): string | null {
    const path = preview?.sharedDuplicatePath;
    return typeof path === "string" && path !== "" ? path : null;
}

/** Combine row notes; null when none apply. */
export function joinNotes(...notes: (string | null)[]): string | null {
    const present = notes.filter((note): note is string => note !== null);
    return present.length === 0 ? null : present.join(" ");
}

/** Pick the link mode for a new placement without asking the user. */
export function chooseDeployMode(
    symlinkAvailable: boolean,
    preference: SkillLinkPreference,
): SkillDeployMode {
    return preference === "auto" && symlinkAvailable ? "symlink" : "copy";
}

/**
 * Mode for a resync: keep the mode that was recorded, except that a link that
 * can no longer be created (or an explicit copy preference) becomes a copy.
 */
export function resyncMode(
    recorded: SkillDeployMode,
    symlinkAvailable: boolean,
    preference: SkillLinkPreference,
): SkillDeployMode {
    if (recorded === "copy") return "copy";
    return chooseDeployMode(symlinkAvailable, preference);
}

/**
 * Derive the row status from the ownership record for the Agent's current
 * configuration root and, when known, the live target preview. The record
 * wins: an owned placement is either in sync or needs a sync. Without a
 * record, a foreign target means the name is occupied.
 */
export function injectStatus(
    preview: SkillDeployPreview | null,
    deployment: SkillDeploymentRecord | null,
): SkillInjectStatus {
    if (deployment !== null)
        return deployment.inSync ? "injected" : "needs_sync";
    if (preview?.targetState === "foreign") return "occupied";
    return "not_injected";
}

/**
 * Status for a row that has neither a full preview nor an ownership record,
 * from the batch target classification alone. Null when the batch state says
 * nothing beyond the per-skill fallback.
 */
export function batchStatus(
    target: AgentSkillTargetState | undefined,
): "occupied" | "unreadable" | null {
    if (target === "foreign") return "occupied";
    if (target === "unreadable") return "unreadable";
    return null;
}

/** The single primary action for a status. */
export function primaryAction(status: SkillInjectStatus): SkillInjectAction {
    if (status === "not_injected") return "inject";
    if (status === "injected") return "remove";
    if (status === "needs_sync") return "resync";
    return "none";
}

/**
 * Whether an out-of-sync placement can be resynced.
 *
 * Rust classifies a recorded target as `absent` when it was deleted, and as
 * `foreign` when a copy's digest or a link's destination no longer matches.
 * `resync_skill_deploy` rewrites either from the library as long as vibemate
 * holds the ownership record (edited content is backed up first), so a live
 * preview plus a deployment record is all that is needed.
 */
export function canResync(
    preview: SkillDeployPreview | null,
    deployment: SkillDeploymentRecord | null,
): boolean {
    return preview !== null && deployment !== null;
}

/**
 * Backup folder a resync would use when it overwrites edits made outside
 * vibemate, or null when the resync replaces nothing the user changed.
 */
export function resyncOverwrite(
    preview: SkillDeployPreview,
): { backupDir: string | null } | null {
    return preview.overwritesUserChanges === true
        ? { backupDir: preview.backupDir ?? null }
        : null;
}

/** Where a finished resync backed up the replaced edits, if it replaced any. */
export function resyncBackupPath(result: SkillResynced): string | null {
    return result.overwroteUserChanges ? result.backupPath : null;
}

/** An owned placement whose files were edited or relinked outside vibemate. */
export function editedOutside(
    preview: SkillDeployPreview | null,
    deployment: SkillDeploymentRecord | null,
): boolean {
    return deployment !== null && preview?.targetState === "foreign";
}

/** Find the ownership record for one Agent's current configuration root. */
export function deploymentFor(
    deployments: readonly SkillDeploymentRecord[],
    agent: SkillDeployAgent,
    configRoot: string | null,
): SkillDeploymentRecord | null {
    return (
        deployments.find(
            (row) =>
                row.agentId === agent &&
                (configRoot === null || row.configRoot === configRoot),
        ) ?? null
    );
}

/**
 * Count, per skill id, how many Agents currently hold an injected placement.
 * Input is one bulk `listAgentSkillDeployments` answer per Agent, so the list
 * page needs three requests in total instead of one per skill card.
 */
export function countInjectedAgents(
    perAgent: readonly (readonly SkillDeploymentRecord[])[],
): Map<string, number> {
    const agents = new Map<string, Set<SkillDeployAgent>>();
    for (const rows of perAgent) {
        for (const row of rows) {
            const seen = agents.get(row.skillId) ?? new Set<SkillDeployAgent>();
            seen.add(row.agentId);
            agents.set(row.skillId, seen);
        }
    }
    return new Map(
        [...agents.entries()].map(([skillId, seen]) => [skillId, seen.size]),
    );
}

/** Safe error code for a failed skill request. */
export function skillErrorCode(error: unknown): SkillErrorCode {
    return error instanceof SkillRequestError ? error.code : "operation_failed";
}

/**
 * What a failed skill request was reading. Preview, resync and remove read
 * the copy already injected in the Agent folder, so their `read_failed` is
 * about that copy; inject and list failures stay about the library.
 */
export type SkillErrorScope = "library" | "target";

/** Default scope of a row action. */
export function actionErrorScope(
    action: "inject" | "remove" | "resync",
): SkillErrorScope {
    return action === "inject" ? "library" : "target";
}

/** Copy key for a failed skill request in the given scope. */
export function skillErrorKey(
    code: SkillErrorCode,
    scope: SkillErrorScope,
): `skills.errors.${SkillErrorCode}` | "skills.inject.targetReadFailed" {
    return scope === "target" && code === "read_failed"
        ? "skills.inject.targetReadFailed"
        : `skills.errors.${code}`;
}

/** Failures after which the filesystem or record may differ from this view. */
export function uncertainOutcome(code: SkillErrorCode): boolean {
    return [
        "operation_failed",
        "invalid_response",
        "outcome_unknown",
        "write_failed",
        "storage_unavailable",
        "revision_conflict",
        "target_changed",
    ].includes(code);
}
