import { invoke, isTauri } from "@tauri-apps/api/core";
import * as z from "zod";
import { isSafePath } from "./agents";
import { summarySchema } from "./versionRestore";

const CODES = [
    "invalid_request",
    "storage_unavailable",
    "provider_changed",
    "target_unavailable",
    "invalid_config",
    "name_conflict",
    "ownership_conflict",
    "plan_stale",
    "credential_unavailable",
    "recovery_required",
    "write_failed",
    "outcome_unknown",
    "protocol_unsupported",
    "base_url_unverified",
    "model_unsupported",
    "default_in_use",
    "not_deployed",
] as const;
/** Safe Codex settings failures; native/credential diagnostics never cross IPC. */
export type CodexSettingsErrorCode =
    (typeof CODES)[number] | "desktop_required" | "invalid_response";
/** A stable code only, never an IPC payload or schema diagnostic. */
export class CodexSettingsError extends Error {
    readonly code: CodexSettingsErrorCode;
    constructor(code: CodexSettingsErrorCode) {
        super("Codex settings request failed.");
        this.name = "CodexSettingsError";
        this.code = code;
    }
}

/** What the UI may offer for Codex 0.165.0 (P41). */
export const CODEX_CAPABILITIES = {
    /** Codex only speaks `wire_api = "responses"`. */
    wireApi: "responses",
    /** Provider tables, an optional default, MCP entries and Skills. */
    providers: true,
    setDefault: true,
    mcp: true,
    skills: true,
    /** Never written by vibemate; a selected profile can override the default. */
    profiles: false,
    /** The key is stored as plaintext `experimental_bearer_token` (file mode 0600). */
    plaintextCredential: true,
    /** DeepSeek models documented for Codex; other DeepSeek IDs are refused. */
    deepseekModels: ["deepseek-flash", "deepseek-v4-pro"],
} as const;

const PLACEHOLDER = "••••••••";
const id = z.string().regex(/^[0-9a-f]{32}$/u);
const digest = z.string().regex(/^[0-9a-f]{64}$/u);
const providerKey = z.string().regex(/^vibemate-[a-z0-9-]{1,60}$/u);
const modelId = z.string().min(1).max(160);
const path = z.string().max(4096).refine(isSafePath);
const revision = z.int().min(1);
const action = z.enum(["deploy", "revoke", "set_default", "clear_default"]);
/** Explicit `config.toml` changes; each needs its own review and approval. */
export type CodexSettingsAction = z.infer<typeof action>;
/** Identity/action only; never a native path, URL or credential. */
export interface CodexSettingsSelection {
    providerId: string;
    /**
     * The revision the user reviewed. Which one depends on `action`:
     *
     * - `deploy` / `set_default` copy central content, so send the current
     *   central provider `revision` (from the Providers list).
     * - `revoke` / `clear_default` only undo what vibemate owns, so send the
     *   owned binding's `owner.providerRevision` from
     *   {@link readCodexSettingsState} (see {@link ownedBindingSelection}).
     *   This keeps working when the binding is `outdated` (central provider
     *   edited) and does not need the central provider to exist. Sending the
     *   central revision of an outdated binding fails with `provider_changed`.
     *
     * Rust echoes the checked value as the plan's `providerRevision`.
     */
    expectedRevision: number;
    action: CodexSettingsAction;
    /** Required for `set_default`, otherwise null. */
    modelId: string | null;
}
const notice = z.enum([
    "plaintext_credential",
    "profile_overrides_default",
    "release_only",
]);
/** Review hints the UI must show before confirmation. */
export type CodexSettingsNotice = z.infer<typeof notice>;
const providerEntry = z.strictObject({
    base_url: z.string().min(1).max(2048),
    experimental_bearer_token: z.literal(PLACEHOLDER),
    name: z.string().min(1).max(200),
    wire_api: z.literal("responses"),
    supports_websockets: z.literal(false).optional(),
});
const providerFragment = z
    .strictObject({
        model_providers: z.record(providerKey, providerEntry),
    })
    .refine((value) => Object.keys(value.model_providers).length === 1);
const defaultsFragment = z.strictObject({
    model: modelId.nullable(),
    model_provider: z.string().max(200).nullable(),
    model_reasoning_effort: z.string().max(32).nullable(),
});
const fragment = z.union([providerFragment, defaultsFragment]);
/** Masked before/after fragment: one provider table or the three default keys. */
export type CodexSettingsFragment = z.infer<typeof fragment>;
const target = z
    .strictObject({
        path,
        exists: z.boolean(),
        contentDigest: digest.nullable(),
        symlink: z.literal(false),
    })
    .refine((value) => value.exists === (value.contentDigest !== null));
const summary = summarySchema.refine(
    (value) =>
        value.format === "json" &&
        value.reason === null &&
        value.changes.every(
            (change) =>
                !(
                    change.secret ||
                    change.path.endsWith("experimental_bearer_token")
                ) ||
                [change.before, change.after].every(
                    (item) =>
                        item === null ||
                        item === PLACEHOLDER ||
                        item === "[complex value]",
                ),
        ),
);
const previewSchema = z.strictObject({
    target,
    providerId: id,
    providerRevision: revision,
    action,
    providerKey,
    modelId: modelId.nullable(),
    before: fragment.nullable(),
    after: fragment.nullable(),
    summary,
    notices: z.array(notice).max(8),
    writesFile: z.boolean(),
    approval: digest,
});
/** Exact, masked plan. Rust rebuilds and rechecks it before any write. */
export type CodexSettingsPlan = z.infer<typeof previewSchema>;
/** Ownership metadata; the entry digest stays in Rust (it hashes secrets). */
const owner = z.strictObject({
    providerKey,
    providerRevision: revision,
    generation: id,
});
const observed = z.enum(["written", "modified", "missing", "unreadable"]);
const providerBinding = z.strictObject({
    providerId: id,
    displayName: z.string().min(1).max(200),
    owner,
    outdated: z.boolean(),
    state: observed,
});
/** One owned provider table; `outdated` means the central provider changed. */
export type CodexProviderBinding = z.infer<typeof providerBinding>;
const defaultBinding = z.strictObject({
    providerId: id,
    modelId,
    providerKey,
    state: observed,
});
/**
 * The owned default, observed with the provider-binding vocabulary:
 * - `written`: the top-level keys are exactly what vibemate wrote;
 * - `modified`: changed by Codex `/model` or by hand;
 * - `missing`: both `model` and `model_provider` were removed (or the file
 *   is gone);
 * - `unreadable`: the file or a key cannot be interpreted.
 *
 * `clear_default` restores the pre-vibemate values only when `written`. For
 * `modified` and `missing` it only releases ownership (`release_only` notice,
 * `writesFile: false`) and never re-adds a removed value. It is refused while
 * `unreadable`. While any default binding exists, its provider cannot be
 * revoked (`default_in_use`), so the UI should offer clear_default in every
 * state except `unreadable`.
 */
export type CodexDefaultBinding = z.infer<typeof defaultBinding>;
const recovery = z
    .strictObject({
        id,
        providerId: id,
        providerKey: z.string().min(1).max(200),
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
export type CodexSettingsRecovery = z.infer<typeof recovery>;
const stateSchema = z.strictObject({
    configRoot: path,
    targetPath: path,
    providers: z.array(providerBinding).max(1000),
    default: defaultBinding.nullable(),
    profileActive: z.boolean(),
    recovery: recovery.nullable(),
});
/** Read-only root state. Nothing here proves Codex has reloaded. */
export type CodexSettingsState = z.infer<typeof stateSchema>;
const historyEntry = z
    .strictObject({
        id,
        action,
        providerId: id,
        providerKey: z.string().min(1).max(200),
        modelId: modelId.nullable(),
        createdAt: z.int().nonnegative().max(8_640_000_000_000_000),
    })
    .refine(
        (value) =>
            (value.action === "set_default" ||
                value.action === "clear_default") ===
            (value.modelId !== null),
    );
/**
 * One completed change in the active root. `modelId` is the model a default
 * change set (`set_default`) or released (`clear_default`); it is null for
 * `deploy` and `revoke`, which involve no model. `createdAt` is Unix ms.
 */
export type CodexSettingsHistoryEntry = z.infer<typeof historyEntry>;
const historySchema = z.strictObject({
    items: z.array(historyEntry).max(100),
    nextCursor: id.nullable(),
});
/** Bounded newest-first history; values and backups never cross IPC. */
export type CodexSettingsHistory = z.infer<typeof historySchema>;
const applied = z.strictObject({
    changeId: id,
    restartRequired: z.boolean(),
});
/** Recorded result; a new Codex session is needed when `restartRequired`. */
export type CodexSettingsApplied = z.infer<typeof applied>;

function invalid(): never {
    throw new CodexSettingsError("invalid_response");
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
    if (!isTauri()) throw new CodexSettingsError("desktop_required");
    try {
        return await invoke<unknown>(command, args);
    } catch (error: unknown) {
        throw new CodexSettingsError(
            CODES.find((code) => code === error) ?? "outcome_unknown",
        );
    }
}

/**
 * Selection for an owned-only action on one binding row. It always carries the
 * owned `owner.providerRevision`, never the central revision.
 */
export function ownedBindingSelection(
    binding: Pick<CodexProviderBinding, "providerId" | "owner">,
    action: Extract<CodexSettingsAction, "revoke" | "clear_default">,
): CodexSettingsSelection {
    return {
        providerId: binding.providerId,
        expectedRevision: binding.owner.providerRevision,
        action,
        modelId: null,
    };
}

/** Read owned tables, the owned default and a pending recovery. */
export async function readCodexSettingsState(): Promise<CodexSettingsState> {
    const result = parse(stateSchema, await call("codex_settings_state", {}));
    const ids = result.providers.map((row) => row.providerId);
    const keys = result.providers.map((row) => row.owner.providerKey);
    if (
        new Set(ids).size !== ids.length ||
        new Set(keys).size !== keys.length ||
        !/[\\/]config\.toml$/u.test(result.targetPath)
    )
        invalid();
    return result;
}

/** Read one newest-first page; reject contradictory cursors or order. */
export async function readCodexSettingsHistory(
    providerId: string | null = null,
    after: string | null = null,
    limit = 20,
): Promise<CodexSettingsHistory> {
    const result = parse(
        historySchema,
        await call("codex_settings_history", {
            request: { providerId, after, limit },
        }),
    );
    const items = result.items;
    if (
        items.length > limit ||
        new Set(items.map((row) => row.id)).size !== items.length ||
        (providerId !== null &&
            items.some((row) => row.providerId !== providerId)) ||
        items.some(
            (row, index) =>
                row.id === after ||
                (index > 0 &&
                    ((items[index - 1]?.createdAt ?? 0) < row.createdAt ||
                        ((items[index - 1]?.createdAt ?? 0) === row.createdAt &&
                            (items[index - 1]?.id ?? "") <= row.id))),
        ) ||
        (result.nextCursor !== null &&
            result.nextCursor !== items[items.length - 1]?.id)
    )
        invalid();
    return result;
}

/** Obtain an exact masked plan. Rust reads no credential here. */
export async function previewCodexSettingsChange(
    selection: CodexSettingsSelection,
): Promise<CodexSettingsPlan> {
    if ((selection.action === "set_default") !== (selection.modelId !== null))
        throw new CodexSettingsError("invalid_request");
    const result = parse(
        previewSchema,
        await call("preview_codex_settings_change", { selection }),
    );
    const providerShape = (value: CodexSettingsFragment | null) =>
        value === null || "model_providers" in value;
    if (
        result.providerId !== selection.providerId ||
        result.providerRevision !== selection.expectedRevision ||
        result.action !== selection.action ||
        result.modelId !== selection.modelId ||
        !/[\\/]config\.toml$/u.test(result.target.path) ||
        (selection.action === "deploy" &&
            (result.after === null ||
                !providerShape(result.after) ||
                !providerShape(result.before))) ||
        (selection.action === "revoke" &&
            (result.after !== null || !providerShape(result.before))) ||
        ((selection.action === "set_default" ||
            selection.action === "clear_default") &&
            (result.after === null ||
                providerShape(result.after) ||
                providerShape(result.before))) ||
        (selection.action === "set_default" &&
            result.after !== null &&
            "model" in result.after &&
            result.after.model !== selection.modelId) ||
        (result.notices.includes("release_only") && result.writesFile)
    )
        invalid();
    return result;
}

/** Confirm only the reviewed approval; Rust reads the key for `deploy` only. */
export async function applyCodexSettingsChange(
    selection: CodexSettingsSelection,
    plan: CodexSettingsPlan,
): Promise<CodexSettingsApplied> {
    const result = parse(
        applied,
        await call("apply_codex_settings_change", {
            request: { selection, approval: plan.approval },
        }),
    );
    if (result.restartRequired !== plan.writesFile) invalid();
    return result;
}

/** Confirm an observation without replaying a write or loading OS keys. */
export async function recoverCodexSettingsChange(
    reviewed: CodexSettingsRecovery,
): Promise<CodexSettingsApplied> {
    if (reviewed.approval === null)
        throw new CodexSettingsError("recovery_required");
    const result = parse(
        applied,
        await call("recover_codex_settings_change", {
            request: { changeId: reviewed.id, approval: reviewed.approval },
        }),
    );
    if (result.changeId !== reviewed.id) invalid();
    return result;
}
