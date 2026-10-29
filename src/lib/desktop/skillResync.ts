import { invoke, isTauri } from "@tauri-apps/api/core";
import * as z from "zod";
import { isSafePath } from "./agents";
import {
    SkillRequestError,
    sharedDuplicate,
    type SkillDeployAgent,
    type SkillDeployMode,
    type SkillDeployPreview,
    type SkillDeployTargetState,
    type SkillDeployed,
    type SkillErrorCode,
} from "./skills";

/**
 * Codes `resync_skill_deploy` and `preview_agent_skill_targets` can return.
 * `target_conflict` means the occupant is not drift vibemate may overwrite
 * (for example a plain file where an owned copy was); Remove is the way out.
 */
const CODES = [
    "not_found",
    "target_changed",
    "target_conflict",
    "revision_conflict",
    "symlink_unavailable",
    "write_failed",
    "outcome_unknown",
    "agent_location_unavailable",
    "invalid_request",
    "read_failed",
    "storage_unavailable",
    "invalid_stored_skill",
    "manifest_missing",
    "manifest_invalid",
    "source_unsupported_entry",
    "source_too_large",
    "operation_failed",
] as const satisfies readonly SkillErrorCode[];

/** Agents the Rust resync backend accepts (same set as deploy, Codex included). */
export type SkillResyncAgent = SkillDeployAgent;

/** Reviewed values from a preview of a recorded (owned) deployment. */
export interface SkillResyncRequest<
    A extends SkillResyncAgent = SkillResyncAgent,
> {
    agentId: A;
    skillId: string;
    configRoot: string;
    expectedSkillRevision: number;
    reviewedTargetPath: string;
    reviewedTargetState: SkillDeployTargetState;
    mode: SkillDeployMode;
}

/**
 * `SkillDeployed` keyed by the requested Agent, plus whether user edits were
 * replaced. `backupPath` is the private backup of the replaced content and is
 * non-null exactly when `overwroteUserChanges` is true.
 */
export type SkillResynced<A extends SkillResyncAgent = SkillResyncAgent> = Omit<
    SkillDeployed,
    "agentId"
> & {
    agentId: A;
    overwroteUserChanges: boolean;
    backupPath: string | null;
};

/**
 * Batch target state: the single-preview states plus `unreadable`, for a
 * target whose metadata could not be read (not safe to write either).
 */
export type AgentSkillTargetState = SkillDeployTargetState | "unreadable";

/** Read-only target classification for one library skill at one Agent root. */
export interface AgentSkillTarget {
    skillId: string;
    targetState: AgentSkillTargetState;
    targetPath: string;
    symlinkAvailable: boolean;
    /**
     * Codex only: same meaning and rules as
     * `SkillDeployPreview.sharedDuplicatePath`. Rust omits the key when there
     * is no duplicate; the parser normalizes that to null. Optional in the
     * type, like the single preview, so callers may build rows without it.
     */
    sharedDuplicatePath?: string | null;
}

const agentSchema = z.enum(["pi", "grokBuild", "claudeCode", "codex"]);
const modeSchema = z.enum(["symlink", "copy"]);
const stateSchema = z.enum([
    "absent",
    "owned_symlink",
    "owned_copy",
    "foreign",
]);
const idSchema = z.string().regex(/^[0-9a-f]{32}$/u);
const nameSchema = z.string().regex(/^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/u);
const pathSchema = z.string().refine(isSafePath);

const requestSchema = z.strictObject({
    agentId: agentSchema,
    skillId: idSchema,
    configRoot: pathSchema,
    expectedSkillRevision: z.number().int().min(1),
    reviewedTargetPath: pathSchema,
    reviewedTargetState: stateSchema,
    mode: modeSchema,
});
const resyncedSchema = z
    .strictObject({
        agentId: agentSchema,
        skillId: idSchema,
        skillName: nameSchema,
        skillRevision: z.number().int().min(1),
        mode: modeSchema,
        targetPath: pathSchema,
        overwroteUserChanges: z.boolean(),
        backupPath: pathSchema.nullable(),
    })
    .refine(
        (value) => value.overwroteUserChanges === (value.backupPath !== null),
    );
const targetSchema = z.strictObject({
    skillId: idSchema,
    targetState: z.enum([...stateSchema.options, "unreadable"]),
    targetPath: pathSchema,
    symlinkAvailable: z.boolean(),
    // Present only for a Codex duplicate; checked by `sharedDuplicate`.
    sharedDuplicatePath: z.unknown().optional(),
});
const targetsSchema = z.array(targetSchema).max(100_000);

function invalid(): never {
    throw new SkillRequestError("invalid_response");
}
function parse<T>(schema: z.ZodType<T>, value: unknown): T {
    const result = schema.safeParse(value);
    if (!result.success) invalid();
    return result.data;
}
async function call(
    command: string,
    args: Record<string, unknown>,
): Promise<unknown> {
    try {
        return await invoke<unknown>(command, args);
    } catch (error: unknown) {
        throw new SkillRequestError(
            CODES.find((code) => code === error) ?? "operation_failed",
        );
    }
}

/**
 * Build a resync request from a fresh preview of a recorded deployment. Pass
 * the recorded mode to keep it, or another mode to switch.
 */
export function resyncRequestFromPreview<A extends SkillResyncAgent>(
    preview: Omit<SkillDeployPreview, "agentId"> & { agentId: A },
    mode: SkillDeployMode,
): SkillResyncRequest<A> {
    return {
        agentId: preview.agentId,
        skillId: preview.skillId,
        configRoot: preview.configRoot,
        expectedSkillRevision: preview.skillRevision,
        reviewedTargetPath: preview.targetPath,
        reviewedTargetState: preview.targetState,
        mode,
    };
}

/**
 * Rewrite a recorded deployment from the library copy, including a copy edited
 * outside vibemate or an owned link that now points elsewhere. Rust refuses
 * any target without a vibemate ownership row (`not_found`) and re-checks the
 * reviewed path and state (`target_changed`). Edited content is backed up in
 * private app data before it is replaced.
 */
export async function resyncSkillDeploy<A extends SkillResyncAgent>(
    input: SkillResyncRequest<A>,
): Promise<SkillResynced<A>> {
    if (!isTauri()) throw new SkillRequestError("desktop_required");
    const request = requestSchema.safeParse(input);
    if (!request.success) throw new SkillRequestError("invalid_request");
    const result = parse(
        resyncedSchema,
        await call("resync_skill_deploy", { request: request.data }),
    );
    if (
        result.agentId !== input.agentId ||
        result.skillId !== input.skillId ||
        result.mode !== input.mode ||
        result.targetPath !== input.reviewedTargetPath
    )
        invalid();
    return { ...result, agentId: input.agentId };
}

/**
 * Classify every library skill's target at one Agent root without writing
 * anything, so a list can mark occupied targets before Inject. Browser mode
 * reports that the desktop app is required.
 */
export async function previewAgentSkillTargets(
    agentId: SkillResyncAgent,
): Promise<
    { kind: "preview" } | { kind: "desktop"; items: AgentSkillTarget[] }
> {
    if (!isTauri()) return { kind: "preview" };
    const rows = parse(
        targetsSchema,
        await call("preview_agent_skill_targets", { agentId }),
    );
    if (new Set(rows.map((row) => row.skillId)).size !== rows.length) invalid();
    // Same parser as the single preview: Codex only, omitted when absent.
    const items = rows.map((row) => ({
        ...row,
        sharedDuplicatePath: sharedDuplicate(row, agentId),
    }));
    return { kind: "desktop", items };
}
