import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
    clearProviderReference,
    createProvider,
    deleteProvider,
    getProvider,
    listProviderTemplates,
    listProviders,
    ProviderRequestError,
    providerDeletionImpact,
    updateProvider,
    type CreateProviderInput,
    type ProviderRecord,
} from "./providers";

// Auto-mocking keeps invoke's generic signature without unsafe casts.
vi.mock(import("@tauri-apps/api/core"));

const ID = "0123456789abcdef0123456789abcdef";

const record: ProviderRecord = {
    id: ID,
    kind: "deepseek",
    displayName: "Personal",
    baseUrl: "https://api.deepseek.com",
    protocol: "chat_completions",
    extensions: {},
    revision: 1,
    createdAtMs: 1000,
    updatedAtMs: 1000,
    selectedModelCount: 0,
};

const createInput: CreateProviderInput = {
    kind: "deepseek",
    displayName: " Personal ",
    baseUrl: "https://api.deepseek.com/",
    protocol: "chat_completions",
    extensions: {},
    // Synthetic; never a real key.
    secret: "sk-synthetic-create-0000",
};

// An edit carries no kind and no key; only the create request has them.
const editInput = {
    displayName: createInput.displayName,
    baseUrl: createInput.baseUrl,
    protocol: createInput.protocol,
    extensions: createInput.extensions,
};

beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});

describe("provider desktop boundary", () => {
    it("identifies preview without invoking Rust or pretending to save", async () => {
        vi.mocked(isTauri).mockReturnValue(false);
        expect(await listProviderTemplates()).toEqual({ kind: "preview" });
        expect(await listProviders({ after: null, limit: 50 })).toEqual({
            kind: "preview",
        });
        await expect(createProvider(createInput)).rejects.toMatchObject({
            code: "desktop_required",
        });
        await expect(
            updateProvider({ ...editInput, id: ID, expectedRevision: 1 }),
        ).rejects.toMatchObject({ code: "desktop_required" });
        await expect(getProvider(ID)).rejects.toMatchObject({
            code: "desktop_required",
        });
        expect(invoke).not.toHaveBeenCalled();
    });

    it("sends the documented request shapes and returns validated data", async () => {
        vi.mocked(invoke).mockResolvedValueOnce({
            items: [record],
            nextCursor: `1000.${ID}`,
            total: 1,
        });
        expect(await listProviders({ after: null, limit: 1 })).toEqual({
            kind: "desktop",
            page: { items: [record], nextCursor: `1000.${ID}`, total: 1 },
        });
        expect(invoke).toHaveBeenLastCalledWith("list_providers", {
            request: { after: null, limit: 1 },
        });

        vi.mocked(invoke).mockResolvedValueOnce(record);
        expect(await createProvider(createInput)).toEqual(record);
        // The key travels in the request, exactly once, next to the other fields.
        expect(invoke).toHaveBeenLastCalledWith("create_provider", {
            request: {
                kind: "deepseek",
                displayName: " Personal ",
                baseUrl: "https://api.deepseek.com/",
                protocol: "chat_completions",
                extensions: {},
                secret: "sk-synthetic-create-0000",
            },
        });

        const edited = { ...record, displayName: "Renamed", revision: 2 };
        vi.mocked(invoke).mockResolvedValueOnce(edited);
        expect(
            await updateProvider({
                id: ID,
                expectedRevision: 1,
                displayName: "Renamed",
                baseUrl: record.baseUrl,
                protocol: "chat_completions",
                extensions: {},
            }),
        ).toEqual(edited);
        // The edit is addressed by ID and revision; no kind is sent.
        expect(invoke).toHaveBeenLastCalledWith("update_provider", {
            request: {
                id: ID,
                expectedRevision: 1,
                displayName: "Renamed",
                baseUrl: record.baseUrl,
                protocol: "chat_completions",
                extensions: {},
            },
            secret: null,
        });

        vi.mocked(invoke).mockResolvedValueOnce(record);
        expect(await getProvider(ID)).toEqual(record);
        expect(invoke).toHaveBeenLastCalledWith("get_provider", { id: ID });
    });

    it("sends a replacement key separately from non-secret settings in one update", async () => {
        const secret = "synthetic-unified-key";
        vi.mocked(invoke).mockResolvedValue({ ...record, revision: 2 });
        await updateProvider({
            ...editInput,
            id: ID,
            expectedRevision: 1,
            secret,
        });
        expect(invoke).toHaveBeenCalledExactlyOnceWith("update_provider", {
            request: { ...editInput, id: ID, expectedRevision: 1 },
            secret,
        });
    });

    it("does not retain a replacement key in an error or response", async () => {
        const secret = "synthetic-unified-key";
        vi.mocked(invoke).mockRejectedValue(new Error(secret));
        await expect(
            updateProvider({
                ...editInput,
                id: ID,
                expectedRevision: 1,
                secret,
            }),
        ).rejects.toMatchObject({
            code: "operation_failed",
            message: "Provider request failed.",
        });
        vi.mocked(invoke).mockResolvedValue({ ...record, revision: 2, secret });
        await expect(
            updateProvider({
                ...editInput,
                id: ID,
                expectedRevision: 1,
                secret,
            }),
        ).rejects.toMatchObject({
            code: "invalid_response",
            message: "Provider request failed.",
        });
    });

    it("validates templates from Rust", async () => {
        const template = {
            kind: "openrouter",
            brandName: "OpenRouter",
            defaultBaseUrl: "https://openrouter.ai/api/v1",
            protocols: ["chat_completions"],
            protocolBaseUrls: ["https://openrouter.ai/api/v1"],
            defaultProtocol: "chat_completions",
            extensionFields: [],
        };
        vi.mocked(invoke).mockResolvedValueOnce([template]);
        expect(await listProviderTemplates()).toEqual({
            kind: "desktop",
            templates: [template],
        });
        vi.mocked(invoke).mockResolvedValueOnce([
            { ...template, defaultProtocol: "responses" },
        ]);
        await expect(listProviderTemplates()).rejects.toMatchObject({
            code: "invalid_response",
        });
    });

    it.each([
        null,
        { ...record, id: "Personal" },
        { ...record, kind: "custom" },
        { ...record, protocol: "openai-completions" },
        { ...record, revision: 0 },
        { ...record, updatedAtMs: 1 },
        { ...record, extensions: { zdr: true } },
        { ...record, extensions: null },
        // A strict key set: no extra field, such as a key, is passed along.
        { ...record, secret: "sk-synthetic-create-0000" },
        { ...record, note: null },
    ])("rejects a malformed record %j", async (response) => {
        vi.mocked(invoke).mockResolvedValue(response);
        await expect(getProvider(ID)).rejects.toMatchObject({
            code: "invalid_response",
        });
    });

    it("rejects pages larger than requested or with a bad cursor", async () => {
        vi.mocked(invoke).mockResolvedValueOnce({
            items: [record, { ...record, id: "f".repeat(32) }],
            nextCursor: null,
            total: 2,
        });
        await expect(
            listProviders({ after: null, limit: 1 }),
        ).rejects.toMatchObject({ code: "invalid_response" });
        vi.mocked(invoke).mockResolvedValueOnce({
            items: [],
            nextCursor: 5,
            total: 0,
        });
        await expect(
            listProviders({ after: null, limit: 1 }),
        ).rejects.toMatchObject({ code: "invalid_response" });
    });

    it.each([
        { items: [record], nextCursor: null },
        { items: [record], nextCursor: null, total: -1 },
        { items: [record], nextCursor: null, total: 0 },
        { items: [record], nextCursor: null, total: 100_001 },
        { items: [record], nextCursor: null, total: 1.5 },
        { items: [record], nextCursor: null, total: 1, extra: true },
    ])("rejects a malformed page %j", async (response) => {
        vi.mocked(invoke).mockResolvedValue(response);
        await expect(
            listProviders({ after: null, limit: 10 }),
        ).rejects.toMatchObject({ code: "invalid_response" });
    });

    it("rejects a create acknowledgment with an extra or missing field", async () => {
        vi.mocked(invoke).mockResolvedValueOnce({
            ...record,
            secret: "sk-synthetic-create-0000",
        });
        await expect(createProvider(createInput)).rejects.toMatchObject({
            code: "invalid_response",
        });
        const partial: Partial<ProviderRecord> = { ...record };
        delete partial.updatedAtMs;
        vi.mocked(invoke).mockResolvedValueOnce(partial);
        await expect(createProvider(createInput)).rejects.toMatchObject({
            code: "invalid_response",
        });
    });

    it("does not confirm a save whose acknowledgment does not match", async () => {
        vi.mocked(invoke).mockResolvedValueOnce({ ...record, revision: 2 });
        await expect(createProvider(createInput)).rejects.toMatchObject({
            code: "invalid_response",
        });
        vi.mocked(invoke).mockResolvedValueOnce({
            ...record,
            kind: "openrouter",
        });
        await expect(createProvider(createInput)).rejects.toMatchObject({
            code: "invalid_response",
        });
        // An edit acknowledged with the old revision, or for another instance.
        for (const response of [
            { ...record, revision: 1 },
            { ...record, id: "f".repeat(32), revision: 2 },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(
                updateProvider({ ...editInput, id: ID, expectedRevision: 1 }),
            ).rejects.toMatchObject({ code: "invalid_response" });
        }
    });

    it.each([
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
    ])("retains the known safe error code %s", async (code) => {
        vi.mocked(invoke).mockRejectedValue(code);
        await expect(createProvider(createInput)).rejects.toMatchObject({
            code,
        });
        await expect(
            listProviders({ after: null, limit: 10 }),
        ).rejects.toMatchObject({ code });
    });

    it("validates the delete acknowledgment and maps its safe failure codes", async () => {
        vi.mocked(invoke).mockResolvedValueOnce({
            id: ID,
            displayName: "Personal",
            selectedModelCount: 2,
        });
        expect(await deleteProvider({ id: ID, expectedRevision: 3 })).toEqual({
            id: ID,
            displayName: "Personal",
            selectedModelCount: 2,
        });
        expect(invoke).toHaveBeenLastCalledWith("delete_provider", {
            request: { id: ID, expectedRevision: 3 },
        });
        for (const response of [
            { id: "f".repeat(32), displayName: "X", selectedModelCount: 0 },
            { id: ID, displayName: "", selectedModelCount: 0 },
            { id: ID, displayName: "X", selectedModelCount: -1 },
            { id: ID, displayName: "X", selectedModelCount: 1.5 },
            { id: ID, displayName: "X" },
            { id: ID, displayName: "X", selectedModelCount: 0, extra: null },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(
                deleteProvider({ id: ID, expectedRevision: 3 }),
            ).rejects.toMatchObject({ code: "invalid_response" });
        }
        vi.mocked(invoke).mockRejectedValueOnce("delete_outcome_unknown");
        await expect(
            deleteProvider({ id: ID, expectedRevision: 3 }),
        ).rejects.toMatchObject({ code: "delete_outcome_unknown" });
        vi.mocked(isTauri).mockReturnValue(false);
        vi.mocked(invoke).mockClear();
        await expect(
            deleteProvider({ id: ID, expectedRevision: 3 }),
        ).rejects.toMatchObject({ code: "desktop_required" });
        expect(invoke).not.toHaveBeenCalled();
    });

    it("validates Provider deletion references and clears one stale binding", async () => {
        const impact = {
            providerId: ID,
            references: [
                {
                    agentId: "pi",
                    configRoot: "/home/test/.pi/agent",
                    label: "vibemate-Pi-test",
                    modelId: null,
                    state: "stale",
                    clearable: true,
                },
            ],
            codexReferences: [
                {
                    configRoot: "/home/test/.codex",
                    providerKey: "vibemate-pi-test",
                    modelId: null,
                    binding: "provider",
                    state: "active",
                },
                {
                    configRoot: "/home/test/.codex",
                    providerKey: "vibemate-pi-test",
                    modelId: "deepseek-flash",
                    binding: "default",
                    state: "stale",
                },
                {
                    configRoot: "/home/test/.codex",
                    providerKey: "vibemate-pi-test",
                    modelId: null,
                    binding: "pending",
                    state: "unverified",
                },
            ],
            rememberedPreferences: 1,
            canDelete: false,
        };
        vi.mocked(invoke).mockResolvedValueOnce(impact);
        expect(await providerDeletionImpact(ID)).toEqual(impact);
        expect(invoke).toHaveBeenLastCalledWith("provider_deletion_impact", {
            providerId: ID,
        });
        for (const response of [
            { ...impact, providerId: "f".repeat(32) },
            {
                ...impact,
                references: [{ ...impact.references[0], state: "other" }],
            },
            { ...impact, rememberedPreferences: -1 },
            { ...impact, canDelete: "yes" },
            { ...impact, extra: null },
            // Codex rows are a required, separate list and always block.
            { ...impact, codexReferences: undefined },
            { ...impact, references: [], canDelete: true },
            {
                ...impact,
                codexReferences: [
                    { ...impact.codexReferences[0], modelId: "deepseek-flash" },
                ],
            },
            {
                ...impact,
                codexReferences: [
                    { ...impact.codexReferences[1], modelId: null },
                ],
            },
            {
                ...impact,
                codexReferences: [
                    { ...impact.codexReferences[2], state: "active" },
                ],
            },
            {
                ...impact,
                codexReferences: [
                    { ...impact.codexReferences[0], providerKey: "openai" },
                ],
            },
            {
                ...impact,
                codexReferences: [
                    { ...impact.codexReferences[0], clearable: false },
                ],
            },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(providerDeletionImpact(ID)).rejects.toMatchObject({
                code: "invalid_response",
            });
        }
        const clear = {
            providerId: ID,
            agentId: "pi" as const,
            configRoot: "/home/test/.pi/agent",
            label: "vibemate-Pi-test",
        };
        vi.mocked(invoke).mockResolvedValueOnce(null);
        await expect(clearProviderReference(clear)).resolves.toBeUndefined();
        expect(invoke).toHaveBeenLastCalledWith("clear_provider_reference", {
            request: clear,
        });
        vi.mocked(invoke).mockResolvedValueOnce("ok");
        await expect(clearProviderReference(clear)).rejects.toMatchObject({
            code: "invalid_response",
        });
        vi.mocked(invoke).mockRejectedValueOnce("provider_in_use");
        await expect(clearProviderReference(clear)).rejects.toMatchObject({
            code: "provider_in_use",
        });
    });

    it("drops unknown runtime diagnostics instead of forwarding them", async () => {
        vi.mocked(invoke).mockRejectedValue(
            "invalid args `request` for command `create_provider`: secret-ish detail",
        );
        try {
            await createProvider(createInput);
            throw new Error("Expected a failed save");
        } catch (error: unknown) {
            expect(error).toMatchObject({
                code: "operation_failed",
                message: "Provider request failed.",
            });
            expect(String(error)).not.toContain("secret-ish");
        }
    });

    it("never puts the key into an error", async () => {
        for (const rejection of [
            "secret_invalid",
            "create_outcome_unknown",
            `failed for ${createInput.secret}`,
            new Error(createInput.secret),
        ]) {
            vi.mocked(invoke).mockRejectedValueOnce(rejection);
            const error: unknown = await createProvider(createInput).then(
                () => new Error("Expected a failed save"),
                (failure: unknown) => failure,
            );
            expect(error).toBeInstanceOf(ProviderRequestError);
            expect(JSON.stringify(error)).not.toContain(createInput.secret);
            expect(String(error)).not.toContain(createInput.secret);
            if (error instanceof Error)
                expect(error.stack ?? "").not.toContain(createInput.secret);
        }
    });
});
