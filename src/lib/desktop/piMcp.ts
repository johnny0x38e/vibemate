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
    "definition_in_use",
] as const;
/** Safe Pi MCP failures; arbitrary native/credential diagnostics are discarded. */
export type PiMcpErrorCode =
    (typeof CODES)[number] | "desktop_required" | "invalid_response";
/** A stable code only, never an IPC payload or schema diagnostic. */
export class PiMcpError extends Error {
    readonly code: PiMcpErrorCode;
    constructor(code: PiMcpErrorCode) {
        super("Pi MCP request failed.");
        this.name = "PiMcpError";
        this.code = code;
    }
}
const id = z.string().regex(/^[0-9a-f]{32}$/u);
const digest = z.string().regex(/^[0-9a-f]{64}$/u);
const serverName = z.string().regex(/^[A-Za-z0-9_-]{1,64}$/u);
const path = z.string().max(4096).refine(isSafePath);
const revision = z.int().min(1);
const action = z.enum(["deploy", "enable", "disable", "revoke", "restore"]);
/** Supported entry actions; none start or contact a server. */
export type PiMcpAction = z.infer<typeof action>;
/** Fixed identity/action selection, never a native path or configuration body. */
export interface PiMcpSelection {
    definitionId: string;
    expectedRevision: number;
    action: PiMcpAction;
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
        command: z.string().min(1).max(2048),
        args: z.array(z.string().max(2048)).max(64),
        cwd: z.string().max(2048).optional(),
        env: protectedValues("env"),
        enabled: z.boolean(),
    }),
    z.strictObject({
        url: z.string().max(2048),
        headers: protectedValues("headers"),
        enabled: z.boolean(),
    }),
]);
/** New or retained native entry, with placeholder-only protected fields. */
export type PiMcpEntry = z.infer<typeof entry>;
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
    action,
    serverName,
    beforeEntry: entry.nullable(),
    afterEntry: entry.nullable(),
    summary,
    approval: digest,
    sessionRelativeCwd: z.boolean(),
    expandsHome: z.boolean(),
});
/** Exact, sanitized entry plan. Approval is rechecked in Rust before any write. */
export type PiMcpPlan = z.infer<typeof previewSchema>;
/** Ownership metadata; the entry digest stays in Rust (it hashes secrets). */
const ownership = z.strictObject({
    serverName,
    definitionRevision: revision,
    generation: id,
    enabled: z.boolean(),
});
const deployment = z.strictObject({
    definitionId: id,
    displayName: z.string().min(1).max(64),
    configRoot: path,
    ownership,
    state: z.enum(["written", "modified", "missing", "unreadable"]),
});
/** Registration metadata and observed disk state, never a connection status. */
export type PiMcpDeployment = z.infer<typeof deployment>;
const recovery = z
    .strictObject({
        id,
        definitionId: id,
        serverName,
        action,
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
export type PiMcpRecovery = z.infer<typeof recovery>;
const stateSchema = z.strictObject({
    configRoot: path,
    targetPath: path,
    deployments: z.array(deployment).max(100),
    nextCursor: id.nullable(),
    recovery: recovery.nullable(),
});
/** Read-only Pi root state, optionally scoped to one definition across roots. */
export type PiMcpState = z.infer<typeof stateSchema>;
const historyEntry = z.strictObject({
    id,
    action,
    createdAt: z.int().nonnegative().max(8_640_000_000_000_000),
    serverName,
    enabled: z.boolean().nullable(),
});
/** One successful retained entry result; revoked states have enabled=null. */
export type PiMcpHistoryEntry = z.infer<typeof historyEntry>;
const historySchema = z.strictObject({
    items: z.array(historyEntry).max(100),
    nextCursor: id.nullable(),
});
/** Bounded newest-first history. Raw bytes/backup locations never cross IPC. */
export type PiMcpHistory = z.infer<typeof historySchema>;
const applied = z.strictObject({
    changeId: id,
    registered: z.boolean(),
    reloadRequired: z.boolean(),
});
/** Confirmed written/recorded result, not proof Pi has reloaded or connected. */
export type PiMcpApplied = z.infer<typeof applied>;
function invalid(): never {
    throw new PiMcpError("invalid_response");
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
    if (!isTauri()) throw new PiMcpError("desktop_required");
    try {
        return await invoke<unknown>(command, args);
    } catch (error: unknown) {
        throw new PiMcpError(
            CODES.find((code) => code === error) ?? "outcome_unknown",
        );
    }
}
/** Read registrations or a root page without credentials or file mutation. */
export async function readPiMcpState(
    definitionId: string | null = null,
    after: string | null = null,
    limit = 20,
): Promise<PiMcpState> {
    const result = parse(
        stateSchema,
        await call("pi_mcp_state", { request: { definitionId, after, limit } }),
    );
    const rows = result.deployments;
    if (
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
export async function readPiMcpHistory(
    definitionId: string,
    after: string | null = null,
    limit = 20,
): Promise<PiMcpHistory> {
    const result = parse(
        historySchema,
        await call("pi_mcp_history", {
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
export async function previewPiMcpChange(
    selection: PiMcpSelection,
): Promise<PiMcpPlan> {
    const result = parse(
        previewSchema,
        await call("preview_pi_mcp_change", { selection }),
    );
    if (
        result.definitionId !== selection.definitionId ||
        result.definitionRevision !== selection.expectedRevision ||
        result.action !== selection.action ||
        result.target.symlink ||
        (selection.action === "revoke" && result.afterEntry !== null) ||
        (selection.action === "enable" &&
            result.afterEntry?.enabled !== true) ||
        (selection.action === "disable" && result.afterEntry?.enabled !== false)
    )
        invalid();
    return result;
}
/** Confirm only the reviewed approval. Rust rechecks and resolves keys itself. */
export async function applyPiMcpChange(
    selection: PiMcpSelection,
    plan: PiMcpPlan,
): Promise<PiMcpApplied> {
    const result = parse(
        applied,
        await call("apply_pi_mcp_change", {
            request: { selection, approval: plan.approval },
        }),
    );
    if (result.registered !== (plan.afterEntry !== null)) invalid();
    return result;
}
/** Confirm an observation without replaying a native write or loading OS keys. */
export async function recoverPiMcpChange(
    reviewed: PiMcpRecovery,
): Promise<PiMcpApplied> {
    if (reviewed.approval === null) throw new PiMcpError("recovery_required");
    const result = parse(
        applied,
        await call("recover_pi_mcp_change", {
            request: { changeId: reviewed.id, approval: reviewed.approval },
        }),
    );
    if (result.changeId !== reviewed.id) invalid();
    return result;
}
