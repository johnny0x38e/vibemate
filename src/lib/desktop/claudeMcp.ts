import { invoke, isTauri } from "@tauri-apps/api/core";
import * as z from "zod";
import { isSafePath } from "./agents";
import { summarySchema } from "./versionRestore";

const CODES = [
    "invalid_request",
    "storage_unavailable",
    "definition_changed",
    "target_unavailable",
    "invalid_config",
    "name_conflict",
    "ownership_conflict",
    "plan_stale",
    "credential_unavailable",
    "recovery_required",
    "snapshot_unavailable",
    "write_failed",
    "outcome_unknown",
    "unsupported_server_name",
    "dynamic_value_unsupported",
    "path_semantics_unverified",
    "unsupported_field",
    "toggle_unsupported",
] as const;
/** Safe Claude Code MCP failures; native/credential diagnostics are discarded. */
export type ClaudeMcpErrorCode =
    (typeof CODES)[number] | "desktop_required" | "invalid_response";
/** A stable code only, never an IPC payload or schema diagnostic. */
export class ClaudeMcpError extends Error {
    readonly code: ClaudeMcpErrorCode;
    constructor(code: ClaudeMcpErrorCode) {
        super("Claude Code MCP request failed.");
        this.name = "ClaudeMcpError";
        this.code = code;
    }
}
/**
 * Claude Code has no user-level per-server enable/disable. The UI must not
 * render a toggle; Rust also refuses enable/disable with `toggle_unsupported`.
 */
export const CLAUDE_MCP_TOGGLE_SUPPORTED = false;
const id = z.string().regex(/^[0-9a-f]{32}$/u);
const digest = z.string().regex(/^[0-9a-f]{64}$/u);
const serverName = z.string().regex(/^[A-Za-z0-9_-]{1,64}$/u);
const path = z.string().max(4096).refine(isSafePath);
const revision = z.int().min(1);
const writable = z.enum(["deploy", "revoke", "restore"]);
/** Wire actions. `enable`/`disable` always fail with `toggle_unsupported`. */
export type ClaudeMcpAction = z.infer<typeof writable> | "enable" | "disable";
/** Fixed identity/action selection, never a native path or configuration body. */
export interface ClaudeMcpSelection {
    definitionId: string;
    expectedRevision: number;
    action: ClaudeMcpAction;
    historyId: string | null;
}
const protectedValues = (kind: "env" | "headers") =>
    z
        .record(
            z
                .string()
                .max(128)
                .regex(
                    kind === "env"
                        ? /^[A-Za-z_][A-Za-z0-9_]*$/u
                        : /^[!#$%&'*+.^_`|~a-z0-9-]+$/u,
                ),
            z.literal("••••••••"),
        )
        .refine((values) => Object.keys(values).length <= 32);
const entry = z.union([
    z.strictObject({
        type: z.literal("stdio"),
        command: z.string().min(1).max(2048),
        args: z.array(z.string().max(2048)).max(64),
        env: protectedValues("env").optional(),
    }),
    z.strictObject({
        type: z.literal("http"),
        url: z.string().max(2048),
        headers: protectedValues("headers").optional(),
    }),
]);
/** Native `mcpServers.<name>` entry with placeholder-only protected fields. */
export type ClaudeMcpEntry = z.infer<typeof entry>;
const target = z
    .strictObject({
        path,
        exists: z.boolean(),
        contentDigest: digest.nullable(),
        symlink: z.boolean(),
    })
    .refine((value) => value.exists === (value.contentDigest !== null));
const summary = summarySchema.refine(
    (value) =>
        value.format === "json" &&
        value.reason === null &&
        value.changes.every((change) => {
            if (
                !/(?:^|\.)(?:env|headers)(?:\.|\[|$)/u.test(change.path) &&
                !change.secret
            )
                return true;
            return [change.before, change.after].every(
                (value) =>
                    value === null ||
                    value === "••••••••" ||
                    value === "[complex value]",
            );
        }),
);
const previewSchema = z.strictObject({
    target,
    definitionId: id,
    definitionRevision: revision,
    action: writable,
    serverName,
    beforeEntry: entry.nullable(),
    afterEntry: entry.nullable(),
    summary,
    approval: digest,
});
/** Exact, sanitized entry plan. Approval is rechecked in Rust before any write. */
export type ClaudeMcpPlan = z.infer<typeof previewSchema>;
const capabilities = z.strictObject({
    toggle: z.literal(false),
    transports: z.tuple([z.literal("stdio"), z.literal("http")]),
});
/** Fixed adapter capabilities; `toggle` is always false. */
export type ClaudeMcpCapabilities = z.infer<typeof capabilities>;
/** Ownership metadata; the entry digest stays in Rust (it hashes secrets). */
const ownership = z.strictObject({
    serverName,
    definitionRevision: revision,
    generation: id,
});
const deployment = z.strictObject({
    definitionId: id,
    displayName: z.string().min(1).max(64),
    configRoot: path,
    ownership,
    state: z.enum(["written", "modified", "missing", "unreadable"]),
});
/** Registration metadata and observed disk state, never a connection status. */
export type ClaudeMcpDeployment = z.infer<typeof deployment>;
const recovery = z
    .strictObject({
        id,
        definitionId: id,
        serverName,
        action: writable,
        observation: z.enum(["before", "intended", "conflict", "unreadable"]),
        approval: digest.nullable(),
    })
    .refine(
        (value) =>
            value.approval === null ||
            value.observation === "before" ||
            value.observation === "intended",
    );
/** Explicit interruption observation; only its exact approval may be confirmed. */
export type ClaudeMcpRecovery = z.infer<typeof recovery>;
const stateSchema = z.strictObject({
    configRoot: path,
    targetPath: path,
    capabilities,
    deployments: z.array(deployment).max(100),
    nextCursor: id.nullable(),
    recovery: recovery.nullable(),
});
/** Read-only Claude Code user-scope state, optionally scoped to one definition. */
export type ClaudeMcpState = z.infer<typeof stateSchema>;
const historyEntry = z.strictObject({
    id,
    action: writable,
    createdAt: z.int().nonnegative().max(8_640_000_000_000_000),
    serverName,
    registered: z.boolean(),
});
/** One successful retained entry result; `registered=false` means revoked. */
export type ClaudeMcpHistoryEntry = z.infer<typeof historyEntry>;
const historySchema = z.strictObject({
    items: z.array(historyEntry).max(100),
    nextCursor: id.nullable(),
});
/** Bounded newest-first history. Raw bytes/backup locations never cross IPC. */
export type ClaudeMcpHistory = z.infer<typeof historySchema>;
const applied = z.strictObject({
    changeId: id,
    registered: z.boolean(),
    reloadRequired: z.boolean(),
});
/** Written/recorded result, not proof Claude Code reloaded or connected. */
export type ClaudeMcpApplied = z.infer<typeof applied>;
function invalid(): never {
    throw new ClaudeMcpError("invalid_response");
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
    if (!isTauri()) throw new ClaudeMcpError("desktop_required");
    try {
        return await invoke<unknown>(command, args);
    } catch (error: unknown) {
        throw new ClaudeMcpError(
            CODES.find((code) => code === error) ?? "outcome_unknown",
        );
    }
}
const TARGET = /[\\/]\.claude\.json$/u;
/** Read registrations or a root page without credentials or file mutation. */
export async function readClaudeMcpState(
    definitionId: string | null = null,
    after: string | null = null,
    limit = 20,
): Promise<ClaudeMcpState> {
    const result = parse(
        stateSchema,
        await call("claude_mcp_state", {
            request: { definitionId, after, limit },
        }),
    );
    const rows = result.deployments;
    if (
        !TARGET.test(result.targetPath) ||
        new Set(rows.map((row) => `${row.definitionId}:${row.configRoot}`))
            .size !== rows.length ||
        (definitionId !== null
            ? rows.some((row) => row.definitionId !== definitionId) ||
              result.nextCursor !== null
            : rows.length > limit ||
              rows.some(
                  (row, index) =>
                      row.configRoot !== result.configRoot ||
                      (after !== null && row.definitionId <= after) ||
                      (index > 0 &&
                          row.definitionId <=
                              (rows[index - 1]?.definitionId ?? "")),
              )) ||
        (result.nextCursor !== null &&
            result.nextCursor !== rows[rows.length - 1]?.definitionId)
    )
        invalid();
    return result;
}
/** Read another retained-entry page; reject contradictory cursors/order. */
export async function readClaudeMcpHistory(
    definitionId: string,
    after: string | null = null,
    limit = 20,
): Promise<ClaudeMcpHistory> {
    const result = parse(
        historySchema,
        await call("claude_mcp_history", {
            request: { definitionId, after, limit },
        }),
    );
    if (
        result.items.length > limit ||
        new Set(result.items.map((row) => row.id)).size !==
            result.items.length ||
        result.items.some(
            (row, index) =>
                row.id === after ||
                (row.action === "revoke" && row.registered) ||
                (row.action === "deploy" && !row.registered) ||
                (index > 0 &&
                    ((result.items[index - 1]?.createdAt ?? 0) <
                        row.createdAt ||
                        ((result.items[index - 1]?.createdAt ?? 0) ===
                            row.createdAt &&
                            (result.items[index - 1]?.id ?? "") <= row.id))),
        ) ||
        (result.nextCursor !== null &&
            result.nextCursor !== result.items[result.items.length - 1]?.id)
    )
        invalid();
    return result;
}
/** Obtain an exact action preview. Browser mode never reads native files. */
export async function previewClaudeMcpChange(
    selection: ClaudeMcpSelection,
): Promise<ClaudeMcpPlan> {
    const result = parse(
        previewSchema,
        await call("preview_claude_mcp_change", { selection }),
    );
    if (
        result.definitionId !== selection.definitionId ||
        result.definitionRevision !== selection.expectedRevision ||
        result.action !== selection.action ||
        !TARGET.test(result.target.path) ||
        result.target.symlink ||
        (result.action === "revoke" && result.afterEntry !== null) ||
        (result.action === "deploy" && result.afterEntry === null)
    )
        invalid();
    return result;
}
/** Confirm only the reviewed approval. Rust rechecks and resolves keys itself. */
export async function applyClaudeMcpChange(
    selection: ClaudeMcpSelection,
    plan: ClaudeMcpPlan,
): Promise<ClaudeMcpApplied> {
    const result = parse(
        applied,
        await call("apply_claude_mcp_change", {
            request: { selection, approval: plan.approval },
        }),
    );
    if (result.registered !== (plan.afterEntry !== null)) invalid();
    return result;
}
/** Confirm an observation without replaying a native write or loading OS keys. */
export async function recoverClaudeMcpChange(
    reviewed: ClaudeMcpRecovery,
): Promise<ClaudeMcpApplied> {
    if (reviewed.approval === null)
        throw new ClaudeMcpError("recovery_required");
    const result = parse(
        applied,
        await call("recover_claude_mcp_change", {
            request: { changeId: reviewed.id, approval: reviewed.approval },
        }),
    );
    if (result.changeId !== reviewed.id) invalid();
    return result;
}
