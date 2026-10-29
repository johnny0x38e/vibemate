import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
    addManualProviderModel,
    cancelProviderModelFetch,
    clearModelReference,
    deleteManualProviderModel,
    fetchProviderModels,
    getProviderModelFetchStatus,
    getProviderModelSnapshot,
    refreshSavedProviderModels,
    updateProviderModelEdit,
    countProviderModels,
    listProviderModels,
    modelDeletionImpact,
    ModelRequestError,
    setProviderModelsSelected,
    type ProviderModel,
} from "./models";

vi.mock(import("@tauri-apps/api/core"));
const ID = "0123456789abcdef0123456789abcdef";
const model: ProviderModel = {
    providerId: ID,
    modelId: "mistral/mistral-large-4",
    source: "fetched",
    selected: false,
    alias: null,
    upstreamName: "Mistral Large",
    contextWindow: 128000,
    maxOutputTokens: null,
    inputModalities: ["text"],
    outputModalities: null,
    supportedEndpoints: ["/chat/completions"],
    capabilities: {
        thinking: { state: "unknown", source: null },
        imageInput: { state: "unknown", source: null },
        toolCalls: { state: "unknown", source: null },
    },
    capabilityOverrides: {
        thinking: null,
        imageInput: null,
        toolCalls: null,
    },
    thinkingEffort: null,
    routeSupport: "supported",
    resolvedProtocol: "chat_completions",
    upstreamState: "listed",
    lastSeenAtMs: 1000,
    missingSinceMs: null,
    createdAtMs: 1000,
    updatedAtMs: 1000,
};
const input = {
    providerId: ID,
    after: null,
    limit: 50,
    filter: "all",
} as const;
const limits = {
    contextWindow: model.contextWindow,
    maxOutputTokens: model.maxOutputTokens,
    thinkingEffort: null,
} as const;
const page = { items: [model], nextCursor: null, totalMatches: null };
const summary = {
    complete: true,
    incompleteReason: null,
    listed: 1,
    added: 1,
    updated: 0,
    markedMissing: 0,
    pruned: 0,
    skippedInvalid: 0,
    fetchedAtMs: 1000,
};

beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});

describe("model IPC boundary", () => {
    it("reads model snapshots with exact identity and version", async () => {
        const identity = { providerId: ID, modelId: model.modelId };
        const contentDigest = "a".repeat(64);
        vi.mocked(invoke).mockResolvedValueOnce({ model, contentDigest });
        expect(await getProviderModelSnapshot(identity)).toEqual({
            model,
            contentDigest,
        });
        expect(invoke).toHaveBeenLastCalledWith("get_provider_model_snapshot", {
            request: identity,
        });
    });

    it("applies alias and capability declarations together and restores defaults", async () => {
        const identity = { providerId: ID, modelId: model.modelId };
        const contentDigest = "a".repeat(64);
        const declarations = {
            thinking: "supported",
            imageInput: "unsupported",
            toolCalls: null,
        } as const;
        const edit = {
            ...identity,
            alias: " My model ",
            expectedDigest: contentDigest,
            capabilityOverrides: declarations,
            ...limits,
        };
        const response = {
            model: {
                ...model,
                alias: "My model",
                capabilityOverrides: declarations,
            },
            contentDigest: "b".repeat(64),
        };
        vi.mocked(invoke).mockResolvedValueOnce(response);
        expect(await updateProviderModelEdit(edit)).toEqual(response);
        expect(invoke).toHaveBeenLastCalledWith("update_provider_model_edit", {
            request: edit,
        });
        // Restore default: an all-null declaration set clears every declaration.
        const restore = {
            ...identity,
            alias: "My model",
            expectedDigest: response.contentDigest,
            capabilityOverrides: {
                thinking: null,
                imageInput: null,
                toolCalls: null,
            },
            ...limits,
        };
        const restored = {
            model: {
                ...model,
                alias: "My model",
                capabilityOverrides: restore.capabilityOverrides,
            },
            contentDigest: "c".repeat(64),
        };
        vi.mocked(invoke).mockResolvedValueOnce(restored);
        expect(await updateProviderModelEdit(restore)).toEqual(restored);
        // Clearing the alias is the same edit path with a null label.
        const clear = {
            ...identity,
            alias: null,
            expectedDigest: restored.contentDigest,
            capabilityOverrides: restore.capabilityOverrides,
            ...limits,
        };
        const cleared = {
            model: {
                ...model,
                alias: null,
                capabilityOverrides: clear.capabilityOverrides,
            },
            contentDigest: "d".repeat(64),
        };
        vi.mocked(invoke).mockResolvedValueOnce(cleared);
        expect(await updateProviderModelEdit(clear)).toEqual(cleared);
        expect(invoke).toHaveBeenLastCalledWith("update_provider_model_edit", {
            request: clear,
        });
    });

    it("rejects mismatched edit acknowledgments and malformed declarations", async () => {
        const identity = { providerId: ID, modelId: model.modelId };
        const edit = {
            ...identity,
            alias: null,
            expectedDigest: "a".repeat(64),
            capabilityOverrides: {
                thinking: "supported",
                imageInput: null,
                toolCalls: null,
            },
            ...limits,
        } as const;
        // Rust acknowledged a different declaration or label than requested.
        for (const changed of [
            {
                ...model,
                capabilityOverrides: {
                    thinking: "unsupported",
                    imageInput: null,
                    toolCalls: null,
                },
            },
            { ...model, alias: "Other" },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce({
                model: changed,
                contentDigest: "b".repeat(64),
            });
            await expect(updateProviderModelEdit(edit)).rejects.toMatchObject({
                code: "invalid_response",
            });
        }
        // Declaration shapes the boundary cannot interpret never reach the UI.
        for (const capabilityOverrides of [
            { thinking: "unknown", imageInput: null, toolCalls: null },
            { thinking: true, imageInput: null, toolCalls: null },
            { imageInput: null, toolCalls: null },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce({
                model: { ...model, capabilityOverrides },
                contentDigest: "b".repeat(64),
            });
            await expect(updateProviderModelEdit(edit)).rejects.toMatchObject({
                code: "invalid_response",
            });
        }
        vi.mocked(invoke).mockRejectedValueOnce("model_changed");
        await expect(updateProviderModelEdit(edit)).rejects.toMatchObject({
            code: "model_changed",
        });
    });

    it("rejects malformed snapshots, wrong identities, and alias acknowledgments", async () => {
        const identity = { providerId: ID, modelId: model.modelId };
        const contentDigest = "a".repeat(64);
        for (const response of [
            { model, contentDigest: "bad" },
            { model },
            { model, contentDigest, extra: true },
            { model: { ...model, modelId: "another" }, contentDigest },
            { model: { ...model, providerId: "f".repeat(32) }, contentDigest },
            { model: { ...model, selected: "true" }, contentDigest },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(
                getProviderModelSnapshot(identity),
            ).rejects.toMatchObject({ code: "invalid_response" });
        }
        vi.mocked(invoke).mockResolvedValueOnce({ model, contentDigest });
        await expect(
            updateProviderModelEdit({
                ...identity,
                alias: "New",
                expectedDigest: contentDigest,
                capabilityOverrides: model.capabilityOverrides,
                ...limits,
            }),
        ).rejects.toMatchObject({ code: "invalid_response" });
        vi.mocked(invoke).mockRejectedValueOnce("model_changed");
        await expect(
            updateProviderModelEdit({
                ...identity,
                alias: null,
                expectedDigest: contentDigest,
                capabilityOverrides: model.capabilityOverrides,
                ...limits,
            }),
        ).rejects.toMatchObject({ code: "model_changed" });
        vi.mocked(isTauri).mockReturnValue(false);
        vi.mocked(invoke).mockClear();
        await expect(getProviderModelSnapshot(identity)).rejects.toMatchObject({
            code: "desktop_required",
        });
        await expect(
            updateProviderModelEdit({
                ...identity,
                alias: null,
                expectedDigest: contentDigest,
                capabilityOverrides: model.capabilityOverrides,
                ...limits,
            }),
        ).rejects.toMatchObject({ code: "desktop_required" });
        expect(invoke).not.toHaveBeenCalled();
    });

    it("makes no requests on import or in preview and refuses every mutation", async () => {
        expect(invoke).not.toHaveBeenCalled();
        vi.mocked(isTauri).mockReturnValue(false);
        expect(await listProviderModels(input)).toEqual({ kind: "preview" });
        expect(await countProviderModels()).toEqual({ kind: "preview" });
        expect(await getProviderModelFetchStatus(ID)).toEqual({
            kind: "preview",
        });
        expect(await cancelProviderModelFetch(ID)).toEqual({
            wasRunning: false,
        });
        for (const operation of [
            () => fetchProviderModels(ID),
            () =>
                setProviderModelsSelected({
                    providerId: ID,
                    modelIds: [model.modelId],
                    selected: true,
                }),
            () =>
                addManualProviderModel({
                    providerId: ID,
                    modelId: "manual",
                    alias: null,
                    capabilityOverrides: {
                        thinking: null,
                        imageInput: null,
                        toolCalls: null,
                    },
                    contextWindow: null,
                    maxOutputTokens: null,
                    thinkingEffort: null,
                }),
            () =>
                deleteManualProviderModel({
                    providerId: ID,
                    modelId: "manual",
                }),
        ])
            await expect(operation()).rejects.toMatchObject({
                code: "desktop_required",
            });
        expect(invoke).not.toHaveBeenCalled();
    });

    it("counts selected-into-library models with exact keys and rejects bad totals", async () => {
        vi.mocked(invoke).mockResolvedValueOnce({ total: 0 });
        expect(await countProviderModels()).toEqual({
            kind: "desktop",
            count: { total: 0 },
        });
        expect(invoke).toHaveBeenLastCalledWith("count_provider_models", {});
        vi.mocked(invoke).mockResolvedValueOnce({ total: 3 });
        expect(await countProviderModels()).toEqual({
            kind: "desktop",
            count: { total: 3 },
        });
        for (const bad of [
            { total: -1 },
            { total: 1.5 },
            { total: 100_001 },
            { total: 0, extra: true },
            {},
            null,
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(bad);
            await expect(countProviderModels()).rejects.toMatchObject({
                code: "invalid_response",
            });
        }
    });

    it("forwards cursor/filter and optional search without transforming model IDs", async () => {
        vi.mocked(invoke).mockResolvedValueOnce({
            ...page,
            nextCursor: "6d69737472616c",
        });
        expect(await listProviderModels(input)).toEqual({
            kind: "desktop",
            page: { ...page, nextCursor: "6d69737472616c" },
        });
        expect(invoke).toHaveBeenLastCalledWith("list_provider_models", {
            request: input,
        });
        const search = { ...input, query: "mistral large" };
        vi.mocked(invoke).mockResolvedValueOnce({ ...page, totalMatches: 5 });
        expect(await listProviderModels(search)).toEqual({
            kind: "desktop",
            page: { ...page, totalMatches: 5 },
        });
        expect(invoke).toHaveBeenLastCalledWith("list_provider_models", {
            request: search,
        });
    });

    it("requires a matching selection acknowledgment and accepts Rust's deduplication", async () => {
        const request = {
            providerId: ID,
            modelIds: [model.modelId, model.modelId],
            selected: true,
        };
        vi.mocked(invoke).mockResolvedValueOnce([{ ...model, selected: true }]);
        expect(await setProviderModelsSelected(request)).toEqual([
            { ...model, selected: true },
        ]);
        expect(invoke).toHaveBeenLastCalledWith(
            "set_provider_models_selected",
            { request },
        );
        for (const response of [
            [],
            [model],
            [{ ...model, selected: true, modelId: "other" }],
            [{ ...model, selected: true, providerId: "f".repeat(32) }],
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(
                setProviderModelsSelected(request),
            ).rejects.toMatchObject({ code: "invalid_response" });
        }
    });

    it("accepts normalized manual IDs and validates add/delete acknowledgments", async () => {
        const request = {
            providerId: ID,
            modelId: "  manual/model  ",
            alias: " Mine ",
            capabilityOverrides: {
                thinking: "supported",
                imageInput: null,
                toolCalls: null,
            },
            contextWindow: model.contextWindow,
            maxOutputTokens: null,
            thinkingEffort: null,
        } as const;
        const manual = {
            ...model,
            modelId: "manual/model",
            source: "manual",
            selected: true,
            alias: "Mine",
            capabilityOverrides: {
                thinking: "supported",
                imageInput: null,
                toolCalls: null,
            },
        };
        vi.mocked(invoke).mockResolvedValueOnce(manual);
        expect(await addManualProviderModel(request)).toEqual(manual);
        expect(invoke).toHaveBeenLastCalledWith("add_manual_provider_model", {
            request,
        });
        for (const response of [
            model,
            { ...manual, modelId: "other" },
            { ...manual, selected: false },
            { ...manual, capabilityOverrides: model.capabilityOverrides },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(addManualProviderModel(request)).rejects.toMatchObject(
                { code: "invalid_response" },
            );
        }
        const deletion = { providerId: ID, modelId: "manual/model" };
        vi.mocked(invoke).mockResolvedValueOnce(null);
        await expect(
            deleteManualProviderModel(deletion),
        ).resolves.toBeUndefined();
        expect(invoke).toHaveBeenLastCalledWith(
            "delete_manual_provider_model",
            { request: deletion },
        );
        vi.mocked(invoke).mockResolvedValueOnce({
            secret: "synthetic-private-value",
        });
        await expect(deleteManualProviderModel(deletion)).rejects.toMatchObject(
            { code: "invalid_response" },
        );
    });

    it("validates saved-model refresh summaries before reporting them", async () => {
        const refresh = {
            complete: true,
            incompleteReason: null,
            refreshed: 2,
            markedMissing: 1,
            skippedInvalid: 0,
            fetchedAtMs: 1000,
        };
        vi.mocked(invoke).mockResolvedValueOnce(refresh);
        expect(await refreshSavedProviderModels(ID)).toEqual(refresh);
        expect(invoke).toHaveBeenLastCalledWith(
            "refresh_saved_provider_models",
            { providerId: ID },
        );
        const partial = {
            ...refresh,
            complete: false,
            incompleteReason: "page_limit",
        };
        vi.mocked(invoke).mockResolvedValueOnce(partial);
        expect(await refreshSavedProviderModels(ID)).toEqual(partial);
        for (const response of [
            { ...refresh, complete: false },
            { ...partial, incompleteReason: null },
            { ...refresh, extra: null },
            { ...refresh, refreshed: -1 },
            { ...refresh, incompleteReason: "timeout" },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(refreshSavedProviderModels(ID)).rejects.toMatchObject({
                code: "invalid_response",
            });
        }
        vi.mocked(isTauri).mockReturnValue(false);
        vi.mocked(invoke).mockClear();
        await expect(refreshSavedProviderModels(ID)).rejects.toMatchObject({
            code: "desktop_required",
        });
        expect(invoke).not.toHaveBeenCalled();
    });

    it("fetches only when called and validates complete/incomplete summaries", async () => {
        vi.mocked(invoke).mockResolvedValueOnce(summary);
        expect(await fetchProviderModels(ID)).toEqual(summary);
        expect(invoke).toHaveBeenLastCalledWith("fetch_provider_models", {
            providerId: ID,
        });
        for (const reason of ["model_limit", "page_limit", "empty_list"]) {
            const incomplete = {
                ...summary,
                complete: false,
                incompleteReason: reason,
            };
            vi.mocked(invoke).mockResolvedValueOnce(incomplete);
            expect(await fetchProviderModels(ID)).toEqual(incomplete);
        }
    });

    it("reads status and reports both cancellation results with exact argument names", async () => {
        for (const lastFetch of [
            null,
            { fetchedAtMs: 1000, complete: false, listedCount: 5 },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce({
                running: true,
                lastFetch,
            });
            expect(await getProviderModelFetchStatus(ID)).toEqual({
                kind: "desktop",
                status: { running: true, lastFetch },
            });
            expect(invoke).toHaveBeenLastCalledWith(
                "get_provider_model_fetch_status",
                { providerId: ID },
            );
        }
        for (const wasRunning of [true, false]) {
            vi.mocked(invoke).mockResolvedValueOnce({ wasRunning });
            expect(await cancelProviderModelFetch(ID)).toEqual({ wasRunning });
            expect(invoke).toHaveBeenLastCalledWith(
                "cancel_provider_model_fetch",
                { providerId: ID },
            );
        }
    });

    it.each([
        ["providerId", "invalid"],
        ["providerId", "f".repeat(32)],
        ["modelId", null],
        ["modelId", ""],
        ["source", "unknown"],
        ["selected", 1],
        ["alias", {}],
        ["upstreamName", []],
        ["contextWindow", -1],
        ["maxOutputTokens", 1.5],
        ["inputModalities", [1]],
        ["outputModalities", "text"],
        ["supportedEndpoints", [null]],
        ["routeSupport", "yes"],
        ["resolvedProtocol", "openai"],
        ["upstreamState", "available"],
        ["lastSeenAtMs", NaN],
        ["missingSinceMs", -1],
        ["createdAtMs", Number.MAX_SAFE_INTEGER + 1],
        ["updatedAtMs", 999],
    ])("rejects malformed model field %s", async (field, value) => {
        vi.mocked(invoke).mockResolvedValue({
            ...page,
            items: [{ ...model, [field]: value }],
        });
        await expect(listProviderModels(input)).rejects.toMatchObject({
            code: "invalid_response",
        });
    });

    it("rejects each missing model field and unknown nested/top-level fields", async () => {
        for (const key of Object.keys(model)) {
            const partial = Object.fromEntries(
                Object.entries(model).filter(([field]) => field !== key),
            );
            vi.mocked(invoke).mockResolvedValueOnce({
                ...page,
                items: [partial],
            });
            await expect(listProviderModels(input)).rejects.toMatchObject({
                code: "invalid_response",
            });
        }
        for (const response of [
            { ...page, secret: "synthetic-private-value" },
            {
                ...page,
                items: [{ ...model, secret: "synthetic-private-value" }],
            },
            { ...page, items: [model, model] },
            { ...page, nextCursor: 1 },
            { ...page, totalMatches: -1 },
            { ...page, totalMatches: 0 },
            { ...page, totalMatches: 1, nextCursor: "cursor" },
            null,
            {},
            { ...page, items: null },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(listProviderModels(input)).rejects.toMatchObject({
                code: "invalid_response",
            });
        }
        vi.mocked(invoke).mockResolvedValueOnce(page);
        await expect(
            listProviderModels({ ...input, limit: 0 }),
        ).rejects.toMatchObject({ code: "invalid_response" });
        vi.mocked(invoke).mockResolvedValueOnce(page);
        await expect(
            listProviderModels({ ...input, filter: "selected" }),
        ).rejects.toMatchObject({ code: "invalid_response" });
    });

    it("accepts nullable metadata and each supported source/route/upstream enum", async () => {
        for (const routeSupport of [
            "supported",
            "unsupported",
            "unknown",
            "not_applicable",
        ]) {
            const row = {
                ...model,
                source: "manual",
                routeSupport,
                resolvedProtocol: null,
                upstreamState: "never_listed",
                contextWindow: null,
                inputModalities: null,
                supportedEndpoints: null,
                lastSeenAtMs: null,
            };
            vi.mocked(invoke).mockResolvedValueOnce({ ...page, items: [row] });
            expect(await listProviderModels(input)).toEqual({
                kind: "desktop",
                page: { ...page, items: [row] },
            });
        }
        const missing = {
            ...model,
            upstreamState: "missing",
            missingSinceMs: 2000,
        };
        vi.mocked(invoke).mockResolvedValueOnce({ ...page, items: [missing] });
        expect(await listProviderModels(input)).toMatchObject({
            page: { items: [missing] },
        });
    });

    it.each([
        { ...summary, extra: null },
        { ...summary, complete: "true" },
        { ...summary, incompleteReason: "timeout" },
        { ...summary, incompleteReason: "page_limit" },
        { ...summary, complete: false },
        { ...summary, listed: 2 },
        ...[
            "listed",
            "added",
            "updated",
            "markedMissing",
            "pruned",
            "skippedInvalid",
            "fetchedAtMs",
        ].map((key) => ({ ...summary, [key]: -1 })),
        null,
        {},
    ])("rejects malformed fetch summaries %#", async (response) => {
        vi.mocked(invoke).mockResolvedValue(response);
        await expect(fetchProviderModels(ID)).rejects.toMatchObject({
            code: "invalid_response",
        });
    });

    it.each([
        null,
        {},
        { running: "yes", lastFetch: null },
        { running: true, lastFetch: {} },
        {
            running: true,
            lastFetch: { fetchedAtMs: 1, complete: true, listedCount: -1 },
        },
        {
            running: true,
            lastFetch: {
                fetchedAtMs: 1,
                complete: true,
                listedCount: 1,
                secret: "synthetic-private-value",
            },
        },
        { running: false, lastFetch: null, secret: "synthetic-private-value" },
    ])("rejects malformed status %#", async (response) => {
        vi.mocked(invoke).mockResolvedValue(response);
        await expect(getProviderModelFetchStatus(ID)).rejects.toMatchObject({
            code: "invalid_response",
        });
    });

    it.each([null, {}, { wasRunning: 1 }, { wasRunning: true, extra: null }])(
        "rejects malformed cancellation %#",
        async (response) => {
            vi.mocked(invoke).mockResolvedValue(response);
            await expect(cancelProviderModelFetch(ID)).rejects.toMatchObject({
                code: "invalid_response",
            });
        },
    );

    it.each([
        "storage_unavailable",
        "read_failed",
        "write_failed",
        "operation_failed",
        "invalid_request",
        "not_found",
        "invalid_stored_provider",
        "invalid_stored_model",
        "model_id_invalid",
        "model_alias_invalid",
        "model_already_exists",
        "model_not_found",
        "model_route_not_supported",
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
    ])("retains only the allowlisted Rust code %s", async (code) => {
        vi.mocked(invoke).mockRejectedValue(code);
        await expect(fetchProviderModels(ID)).rejects.toMatchObject({ code });
        await expect(listProviderModels(input)).rejects.toMatchObject({ code });
    });

    it("validates model deletion references and clears one stale alias", async () => {
        const impact = {
            providerId: ID,
            modelId: "mistral/mistral-large-4",
            references: [
                {
                    agentId: "grokBuild",
                    configRoot: "/home/test/.grok",
                    label: "vibemate-mistral",
                    modelId: "mistral/mistral-large-4",
                    state: "stale",
                    clearable: true,
                },
            ],
            codexReferences: [
                {
                    configRoot: "/home/test/.codex",
                    providerKey: "vibemate-mistral",
                    modelId: "mistral/mistral-large-4",
                    binding: "default",
                    state: "active",
                },
            ],
            rememberedPreferences: 0,
            canDelete: false,
        };
        vi.mocked(invoke).mockResolvedValueOnce(impact);
        expect(
            await modelDeletionImpact(ID, "mistral/mistral-large-4"),
        ).toEqual(impact);
        expect(invoke).toHaveBeenLastCalledWith("model_deletion_impact", {
            providerId: ID,
            modelId: "mistral/mistral-large-4",
        });
        for (const response of [
            { ...impact, modelId: "other" },
            {
                ...impact,
                references: [{ ...impact.references[0], state: "other" }],
            },
            { ...impact, rememberedPreferences: -1 },
            { ...impact, canDelete: "yes" },
            { ...impact, extra: null },
            { ...impact, codexReferences: undefined },
            { ...impact, references: [], canDelete: true },
            // A model impact only ever lists the owned default of this model.
            {
                ...impact,
                codexReferences: [
                    { ...impact.codexReferences[0], modelId: "other" },
                ],
            },
            {
                ...impact,
                codexReferences: [
                    {
                        ...impact.codexReferences[0],
                        binding: "provider",
                        modelId: null,
                    },
                ],
            },
        ]) {
            vi.mocked(invoke).mockResolvedValueOnce(response);
            await expect(
                modelDeletionImpact(ID, "mistral/mistral-large-4"),
            ).rejects.toMatchObject({ code: "invalid_response" });
        }
        const clear = {
            providerId: ID,
            modelId: "mistral/mistral-large-4",
            agentId: "grokBuild" as const,
            configRoot: "/home/test/.grok",
            label: "vibemate-mistral",
        };
        vi.mocked(invoke).mockResolvedValueOnce(null);
        await expect(clearModelReference(clear)).resolves.toBeUndefined();
        expect(invoke).toHaveBeenLastCalledWith("clear_model_reference", {
            request: clear,
        });
        vi.mocked(invoke).mockRejectedValueOnce("model_in_use");
        await expect(clearModelReference(clear)).rejects.toMatchObject({
            code: "model_in_use",
        });
    });

    it("discards raw errors, structured payloads and frontend-only codes", async () => {
        const secret = "synthetic-private-value";
        for (const rejection of [
            new Error(secret),
            `request failed: ${secret}`,
            { code: "auth_rejected", secret },
            "desktop_required",
            "invalid_response",
        ]) {
            vi.mocked(invoke).mockRejectedValueOnce(rejection);
            const error: unknown = await fetchProviderModels(ID).catch(
                (failure: unknown) => failure,
            );
            expect(error).toBeInstanceOf(ModelRequestError);
            expect(error).toMatchObject({
                code: "operation_failed",
                message: "Model request failed.",
            });
            expect(String(error)).not.toContain(secret);
            expect(JSON.stringify(error)).not.toContain(secret);
            if (error instanceof Error)
                expect(error.stack).not.toContain(secret);
        }
    });
});
