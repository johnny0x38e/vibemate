import { invoke, isTauri } from "@tauri-apps/api/core";

/** Supported vendors; the values match Rust `ProviderKind` and the database. */
export const PROVIDER_KINDS = [
    "command-code",
    "deepseek",
    "openrouter",
] as const;
/** A supported vendor identity, never a display name. */
export type ProviderKind = (typeof PROVIDER_KINDS)[number];

/** Request protocols known to Rust; each template lists the ones it allows. */
export const PROVIDER_PROTOCOLS = [
    "chat_completions",
    "responses",
    "anthropic_messages",
] as const;
/** A protocol value accepted at the IPC boundary. */
export type ProviderProtocol = (typeof PROVIDER_PROTOCOLS)[number];

/** Largest page Rust returns; smaller requests are allowed, larger are rejected. */
export const MAX_PROVIDER_PAGE_SIZE = 100;

/** Defaults and limits for one vendor, owned by Rust. */
export interface ProviderTemplate {
    kind: ProviderKind;
    /** Brand name shown as-is in every language. */
    brandName: string;
    /** Suggested base URL for {@link defaultProtocol}. */
    defaultBaseUrl: string;
    protocols: ProviderProtocol[];
    /**
     * Suggested HTTPS base URL for each entry in {@link protocols} (same order).
     * Different URLs mean a separate configuration when switching protocol.
     */
    protocolBaseUrls: string[];
    defaultProtocol: ProviderProtocol;
    /** Allowed extension keys; empty until evidence documents one. */
    extensionFields: string[];
}

/** Documented base URL for a template protocol, or the kind default. */
export function baseUrlForProtocol(
    template: ProviderTemplate,
    protocol: ProviderProtocol,
): string {
    const index = template.protocols.indexOf(protocol);
    if (index < 0) return template.defaultBaseUrl;
    return template.protocolBaseUrls[index] ?? template.defaultBaseUrl;
}

/** One saved, non-secret provider instance. `id` is its only identity. */
export interface ProviderRecord {
    id: string;
    kind: ProviderKind;
    displayName: string;
    /** Normalized by Rust; may differ from the text the user typed. */
    baseUrl: string;
    protocol: ProviderProtocol;
    extensions: Record<string, string>;
    /** Starts at 1; pass it back as `expectedRevision` when editing. */
    revision: number;
    createdAtMs: number;
    updatedAtMs: number;
    /** Selected models for this configuration; Rust counts rows in SQLite. */
    selectedModelCount: number;
}

/** One bounded page ordered by creation time, then ID. */
export interface ProviderPage {
    items: ProviderRecord[];
    /** Opaque; pass it back as `after` for the next page. `null` on the last page. */
    nextCursor: string | null;
    /** Stored instances in total, so a count stays correct without reading every page. */
    total: number;
}

/** Fields for a new instance. Rust validates and normalizes every field. */
export interface CreateProviderInput {
    kind: ProviderKind;
    displayName: string;
    baseUrl: string;
    protocol: ProviderProtocol;
    extensions: Record<string, string>;
    /**
     * Required API key. Held only for this call and sent only to Rust, which
     * validates it and stores it in the OS credential store; the form clears it
     * afterwards. It never appears in a response, an error or a log.
     */
    secret: string;
}

/** Fields for an edit. The kind cannot change, so it is not part of the request. */
export interface UpdateProviderInput {
    /** Omit to preserve the stored key; supplied values replace it in the same save. */
    secret?: string;
    id: string;
    expectedRevision: number;
    displayName: string;
    baseUrl: string;
    protocol: ProviderProtocol;
    extensions: Record<string, string>;
}

/** Confirmation input for removing one provider. */
export interface DeleteProviderInput {
    id: string;
    /** The revision the confirmation loaded; a newer stored revision is a conflict. */
    expectedRevision: number;
}

/** What a successful deletion removed, for the interface message. */
export interface DeletedProvider {
    id: string;
    displayName: string;
    /** Saved models removed with the instance. */
    selectedModelCount: number;
}

/** Live state of one Agent ownership row. */
export type AgentReferenceState = "active" | "stale" | "unverified";

/** One Pi/Grok binding that still names a central Provider or model. */
export interface AgentReference {
    agentId: "pi" | "grokBuild";
    configRoot: string;
    /** Provider key in Pi or model alias in Grok. */
    label: string;
    modelId: string | null;
    state: AgentReferenceState;
    /** True only when the Agent file no longer contains the entry. */
    clearable: boolean;
}

/**
 * Which owned Codex state blocks a deletion:
 * - `provider`: the owned `[model_providers.<providerKey>]` table; revoke it
 *   in Codex settings;
 * - `default`: the owned top-level default (`modelId` is its model); clear the
 *   default in Codex settings;
 * - `pending`: an interrupted Codex settings change; resolve its recovery.
 */
export type CodexBindingKind = "provider" | "default" | "pending";

/** One owned Codex row that blocks deleting a central Provider or model. */
export interface CodexReference {
    /** Absolute `$CODEX_HOME` recorded with the row. */
    configRoot: string;
    /** `vibemate-…` key under `[model_providers]`. */
    providerKey: string;
    /** Model of the owned default or a pending default change, else null. */
    modelId: string | null;
    binding: CodexBindingKind;
    /**
     * `active` while the owned key is in `config.toml`, `stale` once it is
     * gone, `unverified` when unreadable and always for `pending`. Never
     * clearable from the delete dialog, whatever the state.
     */
    state: AgentReferenceState;
}

/** Agent references and remembered choices affected by a Provider deletion. */
export interface ProviderDeletionImpact {
    providerId: string;
    references: AgentReference[];
    /**
     * Codex rows that block deletion. The wrapper always sets it (`[]` when
     * none); optional only so existing mocks keep type-checking.
     */
    codexReferences?: CodexReference[];
    /** Claude Code remembered choices removed by the same transaction. */
    rememberedPreferences: number;
    /** True only when no binding row blocks deletion. */
    canDelete: boolean;
}

/** Request to clear one stale Agent binding. */
export interface ClearProviderReferenceInput {
    providerId: string;
    agentId: "pi" | "grokBuild";
    configRoot: string;
    label: string;
}

/** Page request: `after` is `null` for the first page; `limit` is 1–100. */
export interface ListProvidersInput {
    after: string | null;
    limit: number;
}

/** Preview has no Rust runtime, so it has no templates or saved instances. */
export type ProviderTemplatesResult =
    { kind: "preview" } | { kind: "desktop"; templates: ProviderTemplate[] };

/** Preview must show that nothing is saved rather than an empty desktop list. */
export type ProviderPageResult =
    { kind: "preview" } | { kind: "desktop"; page: ProviderPage };

/** Codes Rust may return; each maps to one translated message. */
const RUST_PROVIDER_ERROR_CODES = [
    "storage_unavailable",
    "read_failed",
    "write_failed",
    "operation_failed",
    "invalid_stored_provider",
    "not_found",
    "revision_conflict",
    "invalid_request",
    "kind_not_supported",
    "protocol_not_supported",
    "display_name_invalid",
    "base_url_invalid",
    "base_url_not_https",
    "base_url_has_credentials",
    "extension_field_not_supported",
    "secret_invalid",
    "credential_store_unavailable",
    "credential_store_access_denied",
    "credential_store_failed",
    "create_outcome_unknown",
    "secret_outcome_unknown",
    "delete_outcome_unknown",
    "provider_in_use",
] as const;

/**
 * Safe Rust codes plus two detected here: `invalid_response` (malformed or
 * mismatched data) and `desktop_required` (browser preview cannot save).
 */
export type ProviderErrorCode =
    | (typeof RUST_PROVIDER_ERROR_CODES)[number]
    | "invalid_response"
    | "desktop_required";

/**
 * A sanitized provider failure. UI code translates `code` and never sees raw
 * runtime diagnostics. After `operation_failed` or `invalid_response` from a
 * save, the outcome is unknown: reload before claiming success or retrying.
 * `create_outcome_unknown` (create) and `secret_outcome_unknown` (key
 * replacement) are Rust's own unknown-outcome codes and are handled the same way.
 */
export class ProviderRequestError extends Error {
    /** Stable error code. */
    readonly code: ProviderErrorCode;

    /** Create a safe failure without retaining the original runtime error. */
    constructor(code: ProviderErrorCode) {
        super("Provider request failed.");
        this.name = "ProviderRequestError";
        this.code = code;
    }
}

function sanitize(error: unknown): ProviderRequestError {
    const known = RUST_PROVIDER_ERROR_CODES.find((code) => code === error);
    return new ProviderRequestError(known ?? "operation_failed");
}

/** True for a plain JSON object (not `null` or an array). */
export function isObject(value: unknown): value is Record<string, unknown> {
    return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * True when `value` has exactly these own keys. Responses are checked with a
 * strict key set, so an unexpected field (for example a key echoed back by
 * mistake) is rejected as `invalid_response` instead of being passed along.
 */
export function hasExactKeys(
    value: Record<string, unknown>,
    keys: readonly string[],
): boolean {
    const own = Object.keys(value);
    return own.length === keys.length && keys.every((key) => own.includes(key));
}

const RECORD_KEYS = [
    "id",
    "kind",
    "displayName",
    "baseUrl",
    "protocol",
    "extensions",
    "revision",
    "createdAtMs",
    "updatedAtMs",
    "selectedModelCount",
] as const;

/** Page response keys, checked strictly so an extra field is not passed along. */
const PAGE_KEYS = ["items", "nextCursor", "total"] as const;

/** Sanity bound for the stored total, matching the other list boundaries. */
const MAX_TOTAL = 100_000;

function isProviderKind(value: unknown): value is ProviderKind {
    return PROVIDER_KINDS.some((kind) => kind === value);
}

function isProviderProtocol(value: unknown): value is ProviderProtocol {
    return PROVIDER_PROTOCOLS.some((protocol) => protocol === value);
}

/** Stable provider identity format; shared with the Agent preview boundary. */
export function isProviderId(value: unknown): value is string {
    return typeof value === "string" && /^[0-9a-f]{32}$/.test(value);
}

function isTimestamp(value: unknown): value is number {
    return (
        Number.isSafeInteger(value) && typeof value === "number" && value >= 0
    );
}

/** The error for malformed or mismatched data from Rust. */
export function invalidResponse(): ProviderRequestError {
    return new ProviderRequestError("invalid_response");
}

/** Copy a string-to-string map, rejecting any other value type. */
function validateExtensions(value: unknown): Record<string, string> {
    if (!isObject(value)) throw invalidResponse();
    const extensions: Record<string, string> = {};
    for (const [key, entry] of Object.entries(value)) {
        if (typeof entry !== "string") throw invalidResponse();
        extensions[key] = entry;
    }
    return extensions;
}

function validateRecord(value: unknown): ProviderRecord {
    if (
        !isObject(value) ||
        !hasExactKeys(value, RECORD_KEYS) ||
        !isProviderId(value["id"]) ||
        !isProviderKind(value["kind"]) ||
        typeof value["displayName"] !== "string" ||
        typeof value["baseUrl"] !== "string" ||
        !isProviderProtocol(value["protocol"]) ||
        !Number.isSafeInteger(value["revision"]) ||
        typeof value["revision"] !== "number" ||
        value["revision"] < 1 ||
        !isTimestamp(value["createdAtMs"]) ||
        !isTimestamp(value["updatedAtMs"]) ||
        value["updatedAtMs"] < value["createdAtMs"] ||
        !Number.isSafeInteger(value["selectedModelCount"]) ||
        typeof value["selectedModelCount"] !== "number" ||
        value["selectedModelCount"] < 0
    ) {
        throw invalidResponse();
    }
    return {
        id: value["id"],
        kind: value["kind"],
        displayName: value["displayName"],
        baseUrl: value["baseUrl"],
        protocol: value["protocol"],
        extensions: validateExtensions(value["extensions"]),
        revision: value["revision"],
        createdAtMs: value["createdAtMs"],
        updatedAtMs: value["updatedAtMs"],
        selectedModelCount: value["selectedModelCount"],
    };
}

function validateTemplate(value: unknown): ProviderTemplate {
    if (
        !isObject(value) ||
        !isProviderKind(value["kind"]) ||
        typeof value["brandName"] !== "string" ||
        typeof value["defaultBaseUrl"] !== "string" ||
        !Array.isArray(value["protocols"]) ||
        !Array.isArray(value["protocolBaseUrls"]) ||
        !isProviderProtocol(value["defaultProtocol"]) ||
        !Array.isArray(value["extensionFields"])
    ) {
        throw invalidResponse();
    }
    const protocols: ProviderProtocol[] = [];
    for (const protocol of value["protocols"]) {
        if (!isProviderProtocol(protocol)) throw invalidResponse();
        protocols.push(protocol);
    }
    if (value["protocolBaseUrls"].length !== protocols.length) {
        throw invalidResponse();
    }
    const protocolBaseUrls: string[] = [];
    for (const url of value["protocolBaseUrls"]) {
        if (typeof url !== "string" || url.length === 0)
            throw invalidResponse();
        protocolBaseUrls.push(url);
    }
    const extensionFields: string[] = [];
    for (const field of value["extensionFields"]) {
        if (typeof field !== "string") throw invalidResponse();
        extensionFields.push(field);
    }
    if (!protocols.includes(value["defaultProtocol"])) throw invalidResponse();
    const defaultIndex = protocols.indexOf(value["defaultProtocol"]);
    if (protocolBaseUrls[defaultIndex] !== value["defaultBaseUrl"]) {
        throw invalidResponse();
    }
    return {
        kind: value["kind"],
        brandName: value["brandName"],
        defaultBaseUrl: value["defaultBaseUrl"],
        protocols,
        protocolBaseUrls,
        defaultProtocol: value["defaultProtocol"],
        extensionFields,
    };
}

function validatePage(value: unknown, limit: number): ProviderPage {
    if (
        !isObject(value) ||
        !hasExactKeys(value, PAGE_KEYS) ||
        !Array.isArray(value["items"]) ||
        value["items"].length > limit ||
        !(
            value["nextCursor"] === null ||
            typeof value["nextCursor"] === "string"
        ) ||
        !Number.isSafeInteger(value["total"]) ||
        typeof value["total"] !== "number" ||
        value["total"] < 0 ||
        value["total"] > MAX_TOTAL
    ) {
        throw invalidResponse();
    }
    const items = value["items"].map(validateRecord);
    // The total counts every stored instance, so it can never be smaller than
    // the page that was read; a wilder value is a bug, not a number to display.
    if (value["total"] < items.length) throw invalidResponse();
    return {
        items,
        nextCursor: value["nextCursor"],
        total: value["total"],
    };
}

/**
 * Call one command, converting any rejection into a safe error code. Shared
 * with `providerSecrets.ts`, so both modules keep the same code allowlist.
 */
export async function callProviderCommand(
    command: string,
    args?: Record<string, unknown>,
): Promise<unknown> {
    try {
        return await invoke<unknown>(command, args);
    } catch (error: unknown) {
        throw sanitize(error);
    }
}

/** Read the built-in templates, or identify preview without invoking IPC. */
export async function listProviderTemplates(): Promise<ProviderTemplatesResult> {
    if (!isTauri()) return { kind: "preview" };
    const data = await callProviderCommand("list_provider_templates");
    if (!Array.isArray(data)) throw invalidResponse();
    return { kind: "desktop", templates: data.map(validateTemplate) };
}

/**
 * Read one page through Rust, or identify preview without invoking IPC.
 * A page larger than the requested `limit` is rejected as `invalid_response`.
 */
export async function listProviders(
    input: ListProvidersInput,
): Promise<ProviderPageResult> {
    if (!isTauri()) return { kind: "preview" };
    const data = await callProviderCommand("list_providers", {
        request: { after: input.after, limit: input.limit },
    });
    return { kind: "desktop", page: validatePage(data, input.limit) };
}

/** Read one instance by ID. Preview has no instances, so it is rejected. */
export async function getProvider(id: string): Promise<ProviderRecord> {
    if (!isTauri()) throw new ProviderRequestError("desktop_required");
    const record = validateRecord(
        await callProviderCommand("get_provider", { id }),
    );
    if (record.id !== id) throw invalidResponse();
    return record;
}

/**
 * Save a new instance together with its required API key. Success is reported
 * only for a valid acknowledgment of revision 1 with the requested kind and
 * protocol and no extra fields. The display name and URL may be normalized by
 * Rust, so the returned values are the ones to show. The key is not checked
 * here (Rust is the only validator) and is never part of an error.
 * `create_outcome_unknown` means the list must be re-read before retrying.
 */
export async function createProvider(
    input: CreateProviderInput,
): Promise<ProviderRecord> {
    if (!isTauri()) throw new ProviderRequestError("desktop_required");
    const record = validateRecord(
        await callProviderCommand("create_provider", {
            request: {
                kind: input.kind,
                displayName: input.displayName,
                baseUrl: input.baseUrl,
                protocol: input.protocol,
                extensions: input.extensions,
                secret: input.secret,
            },
        }),
    );
    if (
        record.revision !== 1 ||
        record.kind !== input.kind ||
        record.protocol !== input.protocol
    ) {
        throw invalidResponse();
    }
    return record;
}

/**
 * Edit an instance identified by `id`, never by its name. Success requires the
 * same ID, the requested protocol, and exactly the next revision. A
 * `revision_conflict` means another save happened first: reload the instance.
 */
export async function updateProvider(
    input: UpdateProviderInput,
): Promise<ProviderRecord> {
    if (!isTauri()) throw new ProviderRequestError("desktop_required");
    const record = validateRecord(
        await callProviderCommand("update_provider", {
            request: {
                id: input.id,
                expectedRevision: input.expectedRevision,
                displayName: input.displayName,
                baseUrl: input.baseUrl,
                protocol: input.protocol,
                extensions: input.extensions,
            },
            secret: input.secret ?? null,
        }),
    );
    if (
        record.id !== input.id ||
        record.revision !== input.expectedRevision + 1 ||
        record.protocol !== input.protocol
    ) {
        throw invalidResponse();
    }
    return record;
}

/**
 * Remove one provider, its saved models and its API key. Success requires an
 * acknowledgment of the exact ID that was deleted. `revision_conflict` means the
 * configuration changed after the confirmation was opened and nothing was
 * deleted; `delete_outcome_unknown` means the list must be reloaded before any
 * retry or success claim.
 */
export async function deleteProvider(
    input: DeleteProviderInput,
): Promise<DeletedProvider> {
    if (!isTauri()) throw new ProviderRequestError("desktop_required");
    const data: unknown = await callProviderCommand("delete_provider", {
        request: {
            id: input.id,
            expectedRevision: input.expectedRevision,
        },
    });
    if (
        !isObject(data) ||
        !hasExactKeys(data, ["id", "displayName", "selectedModelCount"])
    ) {
        throw invalidResponse();
    }
    const id = data["id"];
    const displayName = data["displayName"];
    const selectedModelCount = data["selectedModelCount"];
    if (
        typeof id !== "string" ||
        id !== input.id ||
        typeof displayName !== "string" ||
        displayName.length === 0 ||
        typeof selectedModelCount !== "number" ||
        !Number.isSafeInteger(selectedModelCount) ||
        selectedModelCount < 0
    ) {
        throw invalidResponse();
    }
    return { id, displayName, selectedModelCount };
}

const REFERENCE_STATES = ["active", "stale", "unverified"] as const;

function referenceState(value: unknown): AgentReferenceState {
    for (const state of REFERENCE_STATES) if (value === state) return state;
    throw invalidResponse();
}

function reference(value: unknown): AgentReference {
    if (
        !isObject(value) ||
        !hasExactKeys(value, [
            "agentId",
            "configRoot",
            "label",
            "modelId",
            "state",
            "clearable",
        ]) ||
        (value["agentId"] !== "pi" && value["agentId"] !== "grokBuild") ||
        typeof value["configRoot"] !== "string" ||
        value["configRoot"].trim() === "" ||
        typeof value["label"] !== "string" ||
        value["label"].trim() === "" ||
        (value["modelId"] !== null && typeof value["modelId"] !== "string") ||
        typeof value["clearable"] !== "boolean"
    ) {
        throw invalidResponse();
    }
    return {
        agentId: value["agentId"],
        configRoot: value["configRoot"],
        label: value["label"],
        modelId: value["modelId"],
        state: referenceState(value["state"]),
        clearable: value["clearable"],
    };
}

const CODEX_BINDINGS = ["provider", "default", "pending"] as const;
const CODEX_REFERENCE_KEYS = [
    "configRoot",
    "providerKey",
    "modelId",
    "binding",
    "state",
] as const;

function codexReference(value: unknown): CodexReference {
    if (
        !isObject(value) ||
        !hasExactKeys(value, CODEX_REFERENCE_KEYS) ||
        typeof value["configRoot"] !== "string" ||
        value["configRoot"].trim() === "" ||
        typeof value["providerKey"] !== "string" ||
        !/^vibemate-[a-z0-9-]{1,60}$/u.test(value["providerKey"]) ||
        (value["modelId"] !== null &&
            (typeof value["modelId"] !== "string" ||
                value["modelId"] === "")) ||
        (value["binding"] === "provider" && value["modelId"] !== null) ||
        (value["binding"] === "default" && value["modelId"] === null)
    ) {
        throw invalidResponse();
    }
    const binding = CODEX_BINDINGS.find((kind) => kind === value["binding"]);
    const state = referenceState(value["state"]);
    if (
        binding === undefined ||
        (binding === "pending" && state !== "unverified")
    )
        throw invalidResponse();
    return {
        configRoot: value["configRoot"],
        providerKey: value["providerKey"],
        modelId: value["modelId"],
        binding,
        state,
    };
}

/** Validate the Codex rows of a deletion impact. */
export function codexReferences(value: unknown): CodexReference[] {
    if (!Array.isArray(value)) throw invalidResponse();
    return value.map(codexReference);
}

/** Validate a bounded list of Agent ownership references from Rust. */
export function agentReferences(value: unknown): AgentReference[] {
    if (!Array.isArray(value)) throw invalidResponse();
    return value.map(reference);
}

/** Read the Agent bindings that block deleting one Provider. */
export async function providerDeletionImpact(
    providerId: string,
): Promise<ProviderDeletionImpact> {
    if (!isTauri()) throw new ProviderRequestError("desktop_required");
    const data: unknown = await callProviderCommand(
        "provider_deletion_impact",
        { providerId },
    );
    if (
        !isObject(data) ||
        !hasExactKeys(data, [
            "providerId",
            "references",
            "codexReferences",
            "rememberedPreferences",
            "canDelete",
        ]) ||
        data["providerId"] !== providerId ||
        typeof data["rememberedPreferences"] !== "number" ||
        !Number.isSafeInteger(data["rememberedPreferences"]) ||
        data["rememberedPreferences"] < 0 ||
        typeof data["canDelete"] !== "boolean"
    ) {
        throw invalidResponse();
    }
    const references = agentReferences(data["references"]);
    const codex = codexReferences(data["codexReferences"]);
    // Every listed row blocks deletion.
    if (data["canDelete"] && (references.length > 0 || codex.length > 0))
        throw invalidResponse();
    return {
        providerId,
        references,
        codexReferences: codex,
        rememberedPreferences: data["rememberedPreferences"],
        canDelete: data["canDelete"],
    };
}

/** Remove one stale Agent binding after the Agent file no longer names it. */
export async function clearProviderReference(
    input: ClearProviderReferenceInput,
): Promise<void> {
    if (!isTauri()) throw new ProviderRequestError("desktop_required");
    if (
        (await callProviderCommand("clear_provider_reference", {
            request: input,
        })) !== null
    ) {
        throw invalidResponse();
    }
}
