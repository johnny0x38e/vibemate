import { invoke, isTauri } from "@tauri-apps/api/core";
import {
    PROVIDER_PROTOCOLS,
    agentReferences,
    codexReferences,
    hasExactKeys,
    isObject,
    type AgentReference,
    type CodexReference,
    type ProviderProtocol,
} from "./providers";

/** Safe codes emitted by Rust's ModelError; raw diagnostics never leave this module. */
const RUST_MODEL_ERROR_CODES = [
    "storage_unavailable",
    "read_failed",
    "write_failed",
    "invalid_request",
    "not_found",
    "invalid_stored_provider",
    "invalid_stored_model",
    "model_id_invalid",
    "model_alias_invalid",
    "model_parameters_invalid",
    "model_already_exists",
    "model_not_found",
    "model_in_use",
    "model_changed",
    "model_route_not_supported",
    "operation_failed",
    "secret_missing",
    "secret_invalid",
    "base_url_invalid",
    "credential_store_unavailable",
    "credential_store_access_denied",
    "credential_store_failed",
    "model_fetch_in_progress",
    "model_fetch_cancelled",
    "model_fetch_stale",
    "connection_failed",
    "tls_failed",
    "request_timed_out",
    "auth_rejected",
    "insufficient_balance",
    "rate_limited",
    "upstream_unavailable",
    "upstream_response_invalid",
    "response_too_large",
] as const;

/** Rust failures plus errors detected at this IPC boundary. */
export type ModelErrorCode =
    | (typeof RUST_MODEL_ERROR_CODES)[number]
    | "invalid_response"
    | "desktop_required";

/** A sanitized failure. Unknown mutation outcomes require a reload before retrying. */
export class ModelRequestError extends Error {
    readonly code: ModelErrorCode;

    /** Construct a safe error without keeping its original payload or cause. */
    constructor(code: ModelErrorCode) {
        super("Model request failed.");
        this.name = "ModelRequestError";
        this.code = code;
    }
}

export type CapabilityState = "unknown" | "supported" | "unsupported";

export type CapabilitySource =
    | "deepseek_model_input_modalities"
    | "deepseek_model_effort_levels"
    | "deepseek_model_features"
    | "openrouter_model_input_modalities"
    | "openrouter_model_supported_parameters"
    | "openrouter_model_reasoning";

export interface CapabilityEvidence {
    state: CapabilityState;
    source: CapabilitySource | null;
}

export interface ModelCapabilities {
    thinking: CapabilityEvidence;
    imageInput: CapabilityEvidence;
    toolCalls: CapabilityEvidence;
}

/**
 * Thinking-effort tokens verified against OpenRouter's gateway and DeepSeek's
 * model list (2026-10-14). `none` turns thinking off.
 */
export const EFFORT_LEVELS = [
    "max",
    "xhigh",
    "high",
    "medium",
    "low",
    "minimal",
    "none",
] as const;

/** One documented thinking-effort token. */
export type EffortLevel = (typeof EFFORT_LEVELS)[number];

/**
 * The model's stated thinking-effort options. `null` on a row means no statement.
 * A catalog refresh replaces it only when the catalog states one; a manual save
 * always writes the submitted value, including a clear.
 */
export type ThinkingEffort =
    | { kind: "any"; defaultLevel: EffortLevel | null }
    | {
          kind: "listed";
          levels: EffortLevel[];
          defaultLevel: EffortLevel | null;
      };

/** One user declaration about a capability. "unknown" is never a declaration. */
export type CapabilityOverride = "supported" | "unsupported";

/**
 * The user's capability declarations for one model. A null field follows the
 * catalog evidence in `capabilities`; an all-null set is "restore default".
 */
export interface CapabilityOverrides {
    thinking: CapabilityOverride | null;
    imageInput: CapabilityOverride | null;
    toolCalls: CapabilityOverride | null;
}

/** Metadata is not proof that inference or a particular capability works. */
export interface ProviderModel {
    providerId: string;
    modelId: string;
    source: "fetched" | "manual";
    selected: boolean;
    alias: string | null;
    upstreamName: string | null;
    contextWindow: number | null;
    maxOutputTokens: number | null;
    inputModalities: string[] | null;
    outputModalities: string[] | null;
    supportedEndpoints: string[] | null;
    capabilities: ModelCapabilities;
    capabilityOverrides: CapabilityOverrides;
    /** Stated thinking-effort options, or `null` when nobody has set them. */
    thinkingEffort: ThinkingEffort | null;
    routeSupport: "supported" | "unsupported" | "unknown" | "not_applicable";
    /** Protocol this instance would use (P15). Null when Command Code has no route. */
    resolvedProtocol: ProviderProtocol | null;
    upstreamState: "listed" | "missing" | "never_listed";
    lastSeenAtMs: number | null;
    missingSinceMs: number | null;
    createdAtMs: number;
    updatedAtMs: number;
}

/** Saved model plus an opaque Rust-generated token for detecting stale edits. */
export interface ModelSnapshot {
    model: ProviderModel;
    contentDigest: string;
}

/** Identity for a snapshot read, independent of the displayed alias. */
export interface ModelIdentity {
    providerId: string;
    modelId: string;
}

/** The model editor's fields, saved together behind one expected digest. */
export interface UpdateModelEditInput extends ModelIdentity {
    expectedDigest: string;
    /** Null clears the alias; Rust validates and trims nonempty labels. */
    alias: string | null;
    /** The complete declaration set; an all-null set restores catalog defaults. */
    capabilityOverrides: CapabilityOverrides;
    /** Final context window. `null` clears a stored limit. */
    contextWindow: number | null;
    /** Final maximum output. `null` clears a stored limit. */
    maxOutputTokens: number | null;
    /** Final thinking-effort statement. `null` clears a stored statement. */
    thinkingEffort: ThinkingEffort | null;
}

/** Opaque cursor pagination, or ranked search with a total and no cursor. */
export interface ProviderModelPage {
    items: ProviderModel[];
    nextCursor: string | null;
    totalMatches: number | null;
}

/** Rust validates limits (1–200), query length and cursor/query compatibility. */
export interface ListProviderModelsInput {
    providerId: string;
    after: string | null;
    limit: number;
    filter: "all" | "selected";
    query?: string;
}

/** One atomic selection batch, limited to 500 IDs by Rust. */
export interface SetModelsSelectedInput {
    providerId: string;
    modelIds: string[];
    selected: boolean;
}

/** Upstream metadata for one model kept on Save (P12.c.5). */
export interface SaveProviderModelEntry {
    modelId: string;
    upstreamName: string | null;
    contextWindow: number | null;
    maxOutputTokens: number | null;
    inputModalities: string[] | null;
    outputModalities: string[] | null;
    supportedEndpoints: string[] | null;
    capabilities: ModelCapabilities;
    /** Catalog effort statement. `null` does not clear a stored value. */
    thinkingEffort: ThinkingEffort | null;
}

/** Remove unchecked rows and upsert newly checked ones in one transaction. */
export interface SaveProviderModelSelectionsInput {
    providerId: string;
    removeModelIds: string[];
    add: SaveProviderModelEntry[];
}

/** One row from upstream browse (not yet persisted). */
export interface UpstreamBrowseModel {
    modelId: string;
    upstreamName: string | null;
    contextWindow: number | null;
    maxOutputTokens: number | null;
    inputModalities: string[] | null;
    outputModalities: string[] | null;
    supportedEndpoints: string[] | null;
    capabilities: ModelCapabilities;
    thinkingEffort: ThinkingEffort | null;
    routeSupport: ProviderModel["routeSupport"];
    resolvedProtocol: ProviderProtocol | null;
}

/** One lazy upstream page for the Models tab browse view. */
export interface UpstreamBrowsePage {
    items: UpstreamBrowseModel[];
    nextOffset: number | null;
    morePages: boolean;
}

/** Rust trims the model ID and validates the optional alias. */
export interface AddManualModelInput {
    providerId: string;
    modelId: string;
    alias: string | null;
    /** Capability declarations chosen in the create form; all-null means none. */
    capabilityOverrides: CapabilityOverrides;
    contextWindow: number | null;
    maxOutputTokens: number | null;
    thinkingEffort: ThinkingEffort | null;
}

/** Only a manual row can be deleted. */
export interface DeleteManualModelInput {
    providerId: string;
    modelId: string;
}

/** Agent bindings and remembered choices affected by a model deletion. */
export interface ModelDeletionImpact {
    providerId: string;
    modelId: string;
    references: AgentReference[];
    /**
     * The owned Codex default when it names this Provider/model pair (binding
     * `default`); it blocks deletion until cleared in Codex settings. The
     * wrapper always sets it; optional only so existing mocks type-check.
     */
    codexReferences?: CodexReference[];
    /** Claude Code remembered choices removed by the same transaction. */
    rememberedPreferences: number;
    /** True only when no binding row blocks deletion. */
    canDelete: boolean;
}

/** Request to clear one stale Grok alias binding. */
export interface ClearModelReferenceInput {
    providerId: string;
    modelId: string;
    agentId: "grokBuild";
    configRoot: string;
    label: string;
}

/** Why a successful fetch could not establish the complete upstream list. */
export type IncompleteReason = "model_limit" | "page_limit" | "empty_list";

/** Persisted merge counts, not an authentication or inference check. */
export interface ModelFetchSummary {
    complete: boolean;
    incompleteReason: IncompleteReason | null;
    listed: number;
    added: number;
    updated: number;
    markedMissing: number;
    pruned: number;
    skippedInvalid: number;
    fetchedAtMs: number;
}

/** A non-secret status read, with no network or credential access. */
export interface ModelFetchStatus {
    running: boolean;
    lastFetch: {
        fetchedAtMs: number;
        complete: boolean;
        listedCount: number;
    } | null;
}

/** How many models the user has added to the library (`selected = 1`). */
export interface ProviderModelCount {
    total: number;
}

/** Preview must not invent a stored total. */
export type ProviderModelCountResult =
    { kind: "preview" } | { kind: "desktop"; count: ProviderModelCount };

/** Preview never pretends to contain a persisted model list. */
export type ProviderModelPageResult =
    { kind: "preview" } | { kind: "desktop"; page: ProviderModelPage };
/** Preview has no persisted fetch history. */
export type ModelFetchStatusResult =
    { kind: "preview" } | { kind: "desktop"; status: ModelFetchStatus };
/** False means no cancellable fetch remained, including when merging had begun. */
export interface CancelModelFetchResult {
    wasRunning: boolean;
}

function invalidResponse(): ModelRequestError {
    return new ModelRequestError("invalid_response");
}

function object(
    value: unknown,
    keys: readonly string[],
): Record<string, unknown> {
    if (!isObject(value) || !hasExactKeys(value, keys)) throw invalidResponse();
    return value;
}

function integer(value: unknown): number {
    if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0)
        throw invalidResponse();
    return value;
}

function boolean(value: unknown): boolean {
    if (typeof value !== "boolean") throw invalidResponse();
    return value;
}

function nullableString(value: unknown): string | null {
    if (value === null || typeof value === "string") return value;
    throw invalidResponse();
}

function nullableInteger(value: unknown): number | null {
    return value === null ? null : integer(value);
}

/** A stored token limit: absent, or a positive safe integer. Zero is not a limit. */
function positiveLimit(value: unknown): number | null {
    const number = nullableInteger(value);
    if (number === 0) throw invalidResponse();
    return number;
}

function effortLevel(value: unknown): EffortLevel {
    return choice(value, EFFORT_LEVELS);
}

function thinkingEffort(value: unknown): ThinkingEffort | null {
    if (value === null) return null;
    if (!isObject(value) || typeof value["kind"] !== "string") {
        throw invalidResponse();
    }
    const defaultLevel =
        value["defaultLevel"] === null
            ? null
            : effortLevel(value["defaultLevel"]);
    if (value["kind"] === "any") {
        if (!hasExactKeys(value, ["kind", "defaultLevel"])) {
            throw invalidResponse();
        }
        return { kind: "any", defaultLevel };
    }
    if (value["kind"] !== "listed") throw invalidResponse();
    if (!hasExactKeys(value, ["kind", "levels", "defaultLevel"])) {
        throw invalidResponse();
    }
    if (!Array.isArray(value["levels"])) throw invalidResponse();
    const levels = value["levels"].map((entry: unknown) => effortLevel(entry));
    if (new Set(levels).size !== levels.length) throw invalidResponse();
    if (
        defaultLevel !== null &&
        defaultLevel !== "none" &&
        !levels.includes(defaultLevel)
    ) {
        throw invalidResponse();
    }
    return { kind: "listed", levels, defaultLevel };
}

/** Compare effort statements after putting lists into the documented order. */
function sameThinkingEffort(
    left: ThinkingEffort | null,
    right: ThinkingEffort | null,
): boolean {
    if (left === null || right === null) return left === right;
    if (left.kind === "any" && right.kind === "any") {
        return left.defaultLevel === right.defaultLevel;
    }
    if (left.kind !== "listed" || right.kind !== "listed") return false;
    const ordered = (levels: readonly EffortLevel[]) =>
        EFFORT_LEVELS.filter((level) => levels.includes(level));
    const leftLevels = ordered(left.levels);
    const rightLevels = ordered(right.levels);
    return (
        leftLevels.length === rightLevels.length &&
        leftLevels.every((level, index) => level === rightLevels[index]) &&
        left.defaultLevel === right.defaultLevel
    );
}

function stringList(value: unknown): string[] | null {
    if (value === null) return null;
    if (!Array.isArray(value)) throw invalidResponse();
    return value.map((entry: unknown) => {
        if (typeof entry !== "string") throw invalidResponse();
        return entry;
    });
}

function choice<T extends string>(value: unknown, choices: readonly T[]): T {
    const match = choices.find((entry) => entry === value);
    if (match === undefined) throw invalidResponse();
    return match;
}

const CAPABILITY_SOURCES = [
    "deepseek_model_input_modalities",
    "deepseek_model_effort_levels",
    "deepseek_model_features",
    "openrouter_model_input_modalities",
    "openrouter_model_supported_parameters",
    "openrouter_model_reasoning",
] as const satisfies readonly CapabilitySource[];

function capabilityEvidence(value: unknown): CapabilityEvidence {
    const data = object(value, ["state", "source"]);
    const state = choice(data["state"], [
        "unknown",
        "supported",
        "unsupported",
    ]);
    const source =
        data["source"] === null
            ? null
            : choice(data["source"], CAPABILITY_SOURCES);
    if ((state === "unknown") !== (source === null)) throw invalidResponse();
    return { state, source };
}

function modelCapabilities(value: unknown): ModelCapabilities {
    const data = object(value, ["thinking", "imageInput", "toolCalls"]);
    return {
        thinking: capabilityEvidence(data["thinking"]),
        imageInput: capabilityEvidence(data["imageInput"]),
        toolCalls: capabilityEvidence(data["toolCalls"]),
    };
}

function capabilityOverride(value: unknown): CapabilityOverride | null {
    return value === null
        ? null
        : choice<CapabilityOverride>(value, ["supported", "unsupported"]);
}

function modelCapabilityOverrides(value: unknown): CapabilityOverrides {
    const data = object(value, ["thinking", "imageInput", "toolCalls"]);
    return {
        thinking: capabilityOverride(data["thinking"]),
        imageInput: capabilityOverride(data["imageInput"]),
        toolCalls: capabilityOverride(data["toolCalls"]),
    };
}

function nullableProtocol(value: unknown): ProviderProtocol | null {
    if (value === null) return null;
    return choice(value, PROVIDER_PROTOCOLS);
}

const MODEL_KEYS = [
    "providerId",
    "modelId",
    "source",
    "selected",
    "alias",
    "upstreamName",
    "contextWindow",
    "maxOutputTokens",
    "inputModalities",
    "outputModalities",
    "supportedEndpoints",
    "capabilities",
    "capabilityOverrides",
    "thinkingEffort",
    "routeSupport",
    "resolvedProtocol",
    "upstreamState",
    "lastSeenAtMs",
    "missingSinceMs",
    "createdAtMs",
    "updatedAtMs",
] as const;

function validateModel(value: unknown, providerId: string): ProviderModel {
    const data = object(value, MODEL_KEYS);
    if (
        typeof data["providerId"] !== "string" ||
        !/^[0-9a-f]{32}$/.test(data["providerId"]) ||
        data["providerId"] !== providerId ||
        typeof data["modelId"] !== "string" ||
        data["modelId"].length === 0
    )
        throw invalidResponse();
    const model: ProviderModel = {
        providerId: data["providerId"],
        modelId: data["modelId"],
        source: choice(data["source"], ["fetched", "manual"]),
        selected: boolean(data["selected"]),
        alias: nullableString(data["alias"]),
        upstreamName: nullableString(data["upstreamName"]),
        contextWindow: positiveLimit(data["contextWindow"]),
        maxOutputTokens: positiveLimit(data["maxOutputTokens"]),
        inputModalities: stringList(data["inputModalities"]),
        outputModalities: stringList(data["outputModalities"]),
        supportedEndpoints: stringList(data["supportedEndpoints"]),
        capabilities: modelCapabilities(data["capabilities"]),
        capabilityOverrides: modelCapabilityOverrides(
            data["capabilityOverrides"],
        ),
        thinkingEffort: thinkingEffort(data["thinkingEffort"]),
        routeSupport: choice(data["routeSupport"], [
            "supported",
            "unsupported",
            "unknown",
            "not_applicable",
        ]),
        resolvedProtocol: nullableProtocol(data["resolvedProtocol"]),
        upstreamState: choice(data["upstreamState"], [
            "listed",
            "missing",
            "never_listed",
        ]),
        lastSeenAtMs: nullableInteger(data["lastSeenAtMs"]),
        missingSinceMs: nullableInteger(data["missingSinceMs"]),
        createdAtMs: integer(data["createdAtMs"]),
        updatedAtMs: integer(data["updatedAtMs"]),
    };
    if (model.updatedAtMs < model.createdAtMs) throw invalidResponse();
    return model;
}

function validateModels(value: unknown, providerId: string): ProviderModel[] {
    if (!Array.isArray(value)) throw invalidResponse();
    const models = value.map((entry: unknown) =>
        validateModel(entry, providerId),
    );
    if (new Set(models.map((model) => model.modelId)).size !== models.length)
        throw invalidResponse();
    return models;
}

async function call(
    command: string,
    args: Record<string, unknown>,
): Promise<unknown> {
    try {
        return await invoke<unknown>(command, args);
    } catch (error: unknown) {
        const code = RUST_MODEL_ERROR_CODES.find((entry) => entry === error);
        throw new ModelRequestError(code ?? "operation_failed");
    }
}

function requireDesktop(): void {
    if (!isTauri()) throw new ModelRequestError("desktop_required");
}

/** Read models through Rust, never HTTP. Browser preview makes no IPC call. */
export async function listProviderModels(
    input: ListProviderModelsInput,
): Promise<ProviderModelPageResult> {
    if (!isTauri()) return { kind: "preview" };
    const data = object(
        await call("list_provider_models", { request: input }),
        ["items", "nextCursor", "totalMatches"],
    );
    const page: ProviderModelPage = {
        items: validateModels(data["items"], input.providerId),
        nextCursor: nullableString(data["nextCursor"]),
        totalMatches: nullableInteger(data["totalMatches"]),
    };
    if (
        page.items.length > input.limit ||
        (page.totalMatches !== null &&
            (page.totalMatches < page.items.length ||
                page.nextCursor !== null)) ||
        (input.filter === "selected" &&
            page.items.some((model) => !model.selected))
    )
        throw invalidResponse();
    return { kind: "desktop", page };
}

/**
 * Count models selected into the library across every provider (`selected = 1`).
 * Unselected catalog-fetch rows are excluded. Browser preview invents no total.
 */
export async function countProviderModels(): Promise<ProviderModelCountResult> {
    if (!isTauri()) return { kind: "preview" };
    const data = object(await call("count_provider_models", {}), ["total"]);
    const total = integer(data["total"]);
    // Match sibling overview list bounds (providers / skills / MCP).
    if (total > 100_000) throw invalidResponse();
    return { kind: "desktop", count: { total } };
}

/** Validate an untrusted model snapshot for its exact binding.
 * Shared by model editing and Agent starting-preference IPC; failures retain no payload.
 */
export function validateModelSnapshot(
    value: unknown,
    input: ModelIdentity,
): ModelSnapshot {
    const data = object(value, ["model", "contentDigest"]);
    const model = validateModel(data["model"], input.providerId);
    const contentDigest = data["contentDigest"];
    if (
        model.modelId !== input.modelId ||
        typeof contentDigest !== "string" ||
        !/^[0-9a-f]{64}$/.test(contentDigest)
    )
        throw invalidResponse();
    return { model, contentDigest };
}

/** Read a saved model without fetching its provider catalog or accessing credentials. */
export async function getProviderModelSnapshot(
    input: ModelIdentity,
): Promise<ModelSnapshot> {
    requireDesktop();
    return validateModelSnapshot(
        await call("get_provider_model_snapshot", { request: input }),
        input,
    );
}

/**
 * Apply the model editor's fields (alias and capability declarations) together,
 * rejecting stale content in Rust and mismatched acknowledgments here.
 */
export async function updateProviderModelEdit(
    input: UpdateModelEditInput,
): Promise<ModelSnapshot> {
    requireDesktop();
    const snapshot = validateModelSnapshot(
        await call("update_provider_model_edit", { request: input }),
        input,
    );
    const overrides = snapshot.model.capabilityOverrides;
    const expected = input.capabilityOverrides;
    if (
        snapshot.model.alias !== (input.alias?.trim() ?? null) ||
        overrides.thinking !== expected.thinking ||
        overrides.imageInput !== expected.imageInput ||
        overrides.toolCalls !== expected.toolCalls ||
        snapshot.model.contextWindow !== input.contextWindow ||
        snapshot.model.maxOutputTokens !== input.maxOutputTokens ||
        !sameThinkingEffort(snapshot.model.thinkingEffort, input.thinkingEffort)
    )
        throw invalidResponse();
    return snapshot;
}

/** Persist a batch only in desktop mode; reject mismatched acknowledgments. */
export async function setProviderModelsSelected(
    input: SetModelsSelectedInput,
): Promise<ProviderModel[]> {
    requireDesktop();
    const models = validateModels(
        await call("set_provider_models_selected", { request: input }),
        input.providerId,
    );
    const expected = new Set(input.modelIds);
    if (
        models.length !== expected.size ||
        models.some(
            (model) =>
                !expected.has(model.modelId) ||
                model.selected !== input.selected,
        )
    )
        throw invalidResponse();
    return models;
}

/** Add a manual row; Rust is responsible for input validation and persistence. */
export async function addManualProviderModel(
    input: AddManualModelInput,
): Promise<ProviderModel> {
    requireDesktop();
    const model = validateModel(
        await call("add_manual_provider_model", { request: input }),
        input.providerId,
    );
    const overrides = model.capabilityOverrides;
    const expected = input.capabilityOverrides;
    if (
        model.modelId !== input.modelId.trim() ||
        model.source !== "manual" ||
        !model.selected ||
        overrides.thinking !== expected.thinking ||
        overrides.imageInput !== expected.imageInput ||
        overrides.toolCalls !== expected.toolCalls ||
        model.contextWindow !== input.contextWindow ||
        model.maxOutputTokens !== input.maxOutputTokens ||
        !sameThinkingEffort(model.thinkingEffort, input.thinkingEffort)
    )
        throw invalidResponse();
    return model;
}

/** Delete a manual row, requiring Rust's null acknowledgment before reporting success. */
export async function deleteManualProviderModel(
    input: DeleteManualModelInput,
): Promise<void> {
    requireDesktop();
    if (
        (await call("delete_manual_provider_model", { request: input })) !==
        null
    )
        throw invalidResponse();
}

/** Read the Agent bindings that block deleting one saved model. */
export async function modelDeletionImpact(
    providerId: string,
    modelId: string,
): Promise<ModelDeletionImpact> {
    requireDesktop();
    const data = object(
        await call("model_deletion_impact", { providerId, modelId }),
        [
            "providerId",
            "modelId",
            "references",
            "codexReferences",
            "rememberedPreferences",
            "canDelete",
        ],
    );
    if (
        data["providerId"] !== providerId ||
        data["modelId"] !== modelId ||
        typeof data["rememberedPreferences"] !== "number" ||
        !Number.isSafeInteger(data["rememberedPreferences"]) ||
        data["rememberedPreferences"] < 0 ||
        typeof data["canDelete"] !== "boolean"
    ) {
        throw invalidResponse();
    }
    let references: AgentReference[];
    let codex: CodexReference[];
    try {
        references = agentReferences(data["references"]);
        codex = codexReferences(data["codexReferences"]);
    } catch {
        throw invalidResponse();
    }
    if (
        codex.some(
            (row) => row.binding !== "default" || row.modelId !== modelId,
        ) ||
        (data["canDelete"] && (references.length > 0 || codex.length > 0))
    )
        throw invalidResponse();
    return {
        providerId,
        modelId,
        references,
        codexReferences: codex,
        rememberedPreferences: data["rememberedPreferences"],
        canDelete: boolean(data["canDelete"]),
    };
}

/** Remove one stale Grok alias binding after the Agent file no longer names it. */
export async function clearModelReference(
    input: ClearModelReferenceInput,
): Promise<void> {
    requireDesktop();
    if ((await call("clear_model_reference", { request: input })) !== null) {
        throw invalidResponse();
    }
}

/**
 * Download one upstream browse page without writing SQLite (P12.c.5).
 * `offset` is the next result index (full list or ranked search). Optional
 * `query` uses the same fuzzy rules as `listProviderModels`.
 */
export async function browseUpstreamModelsPage(input: {
    providerId: string;
    offset: number | null;
    query?: string;
}): Promise<UpstreamBrowsePage> {
    requireDesktop();
    const request: Record<string, unknown> = {
        providerId: input.providerId,
        offset: input.offset,
    };
    const trimmedQuery = input.query?.trim();
    if (trimmedQuery !== undefined && trimmedQuery.length > 0) {
        request["query"] = trimmedQuery;
    }
    const data = object(
        await call("browse_upstream_models_page", {
            request,
        }),
        ["items", "nextOffset", "morePages"],
    );
    if (!Array.isArray(data["items"])) throw invalidResponse();
    const items = data["items"].map((entry: unknown) => {
        const row = object(entry, [
            "modelId",
            "upstreamName",
            "contextWindow",
            "maxOutputTokens",
            "inputModalities",
            "outputModalities",
            "supportedEndpoints",
            "capabilities",
            "thinkingEffort",
            "routeSupport",
            "resolvedProtocol",
        ]);
        if (typeof row["modelId"] !== "string" || row["modelId"].length === 0)
            throw invalidResponse();
        return {
            modelId: row["modelId"],
            upstreamName: nullableString(row["upstreamName"]),
            contextWindow: positiveLimit(row["contextWindow"]),
            maxOutputTokens: positiveLimit(row["maxOutputTokens"]),
            inputModalities: stringList(row["inputModalities"]),
            outputModalities: stringList(row["outputModalities"]),
            supportedEndpoints: stringList(row["supportedEndpoints"]),
            capabilities: modelCapabilities(row["capabilities"]),
            thinkingEffort: thinkingEffort(row["thinkingEffort"]),
            routeSupport: choice(row["routeSupport"], [
                "supported",
                "unsupported",
                "unknown",
                "not_applicable",
            ]),
            resolvedProtocol: nullableProtocol(row["resolvedProtocol"]),
        };
    });
    const nextOffset =
        data["nextOffset"] === null ? null : integer(data["nextOffset"]);
    return {
        items,
        nextOffset,
        morePages: boolean(data["morePages"]),
    };
}

/** Persist draft checkbox changes on the Models tab. */
export async function saveProviderModelSelections(
    input: SaveProviderModelSelectionsInput,
): Promise<void> {
    requireDesktop();
    if (
        (await call("save_provider_model_selections", { request: input })) !==
        null
    )
        throw invalidResponse();
}

/** Explicitly fetch and merge the upstream list. Success does not prove the key is valid. */
export async function fetchProviderModels(
    providerId: string,
): Promise<ModelFetchSummary> {
    requireDesktop();
    const data = object(await call("fetch_provider_models", { providerId }), [
        "complete",
        "incompleteReason",
        "listed",
        "added",
        "updated",
        "markedMissing",
        "pruned",
        "skippedInvalid",
        "fetchedAtMs",
    ]);
    const summary: ModelFetchSummary = {
        complete: boolean(data["complete"]),
        incompleteReason:
            data["incompleteReason"] === null
                ? null
                : choice<IncompleteReason>(data["incompleteReason"], [
                      "model_limit",
                      "page_limit",
                      "empty_list",
                  ]),
        listed: integer(data["listed"]),
        added: integer(data["added"]),
        updated: integer(data["updated"]),
        markedMissing: integer(data["markedMissing"]),
        pruned: integer(data["pruned"]),
        skippedInvalid: integer(data["skippedInvalid"]),
        fetchedAtMs: integer(data["fetchedAtMs"]),
    };
    if (
        summary.complete !== (summary.incompleteReason === null) ||
        summary.added + summary.updated !== summary.listed
    )
        throw invalidResponse();
    return summary;
}

/** What one saved-model refresh changed. Never inserts or deletes rows. */
export interface ModelRefreshSummary {
    complete: boolean;
    incompleteReason: IncompleteReason | null;
    refreshed: number;
    markedMissing: number;
    skippedInvalid: number;
    fetchedAtMs: number;
}

/**
 * Refresh the metadata of already saved models from the provider catalog (P16.c).
 * The confirmation and blocked states live in the Models tab; this wrapper only
 * validates the summary and requires the desktop runtime.
 */
export async function refreshSavedProviderModels(
    providerId: string,
): Promise<ModelRefreshSummary> {
    requireDesktop();
    const data = object(
        await call("refresh_saved_provider_models", { providerId }),
        [
            "complete",
            "incompleteReason",
            "refreshed",
            "markedMissing",
            "skippedInvalid",
            "fetchedAtMs",
        ],
    );
    const summary: ModelRefreshSummary = {
        complete: boolean(data["complete"]),
        incompleteReason:
            data["incompleteReason"] === null
                ? null
                : choice<IncompleteReason>(data["incompleteReason"], [
                      "model_limit",
                      "page_limit",
                      "empty_list",
                  ]),
        refreshed: integer(data["refreshed"]),
        markedMissing: integer(data["markedMissing"]),
        skippedInvalid: integer(data["skippedInvalid"]),
        fetchedAtMs: integer(data["fetchedAtMs"]),
    };
    if (summary.complete !== (summary.incompleteReason === null))
        throw invalidResponse();
    return summary;
}

/** Signal cancellation; preview has no fetch to cancel and does not call Rust. */
export async function cancelProviderModelFetch(
    providerId: string,
): Promise<CancelModelFetchResult> {
    if (!isTauri()) return { wasRunning: false };
    const data = object(
        await call("cancel_provider_model_fetch", { providerId }),
        ["wasRunning"],
    );
    return { wasRunning: boolean(data["wasRunning"]) };
}

/** Read persisted fetch history and running state, or identify browser preview. */
export async function getProviderModelFetchStatus(
    providerId: string,
): Promise<ModelFetchStatusResult> {
    if (!isTauri()) return { kind: "preview" };
    const data = object(
        await call("get_provider_model_fetch_status", { providerId }),
        ["running", "lastFetch"],
    );
    let lastFetch: ModelFetchStatus["lastFetch"] = null;
    if (data["lastFetch"] !== null) {
        const last = object(data["lastFetch"], [
            "fetchedAtMs",
            "complete",
            "listedCount",
        ]);
        lastFetch = {
            fetchedAtMs: integer(last["fetchedAtMs"]),
            complete: boolean(last["complete"]),
            listedCount: integer(last["listedCount"]),
        };
    }
    return {
        kind: "desktop",
        status: { running: boolean(data["running"]), lastFetch },
    };
}
