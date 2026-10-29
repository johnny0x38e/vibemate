import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, expect, test, vi } from "vitest";
import {
    CODEX_CAPABILITIES,
    CodexSettingsError,
    applyCodexSettingsChange,
    ownedBindingSelection,
    previewCodexSettingsChange,
    readCodexSettingsHistory,
    readCodexSettingsState,
    recoverCodexSettingsChange,
    type CodexSettingsSelection,
} from "./codex";
import {
    codexDeployPlan,
    codexProviderId,
    codexSetDefaultPlan,
    codexSettingsHistory,
    codexSettingsRecovery,
    codexSettingsState,
} from "../../test/codexFixtures";
vi.mock(import("@tauri-apps/api/core"));

const deploy: CodexSettingsSelection = {
    providerId: codexProviderId,
    expectedRevision: 1,
    action: "deploy",
    modelId: null,
};
const setDefault: CodexSettingsSelection = {
    ...deploy,
    action: "set_default",
    modelId: "deepseek-flash",
};
beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});

async function failure(operation: () => Promise<unknown>): Promise<string> {
    try {
        await operation();
    } catch (error: unknown) {
        if (error instanceof CodexSettingsError) return error.code;
    }
    return "no error";
}

test("review, confirm, observe, history and recovery send scoped requests only", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(codexDeployPlan);
    expect(await previewCodexSettingsChange(deploy)).toEqual(codexDeployPlan);
    expect(invoke).toHaveBeenLastCalledWith("preview_codex_settings_change", {
        selection: deploy,
    });
    const outcome = { changeId: "b".repeat(32), restartRequired: true };
    vi.mocked(invoke).mockResolvedValueOnce(outcome);
    expect(await applyCodexSettingsChange(deploy, codexDeployPlan)).toEqual(
        outcome,
    );
    expect(invoke).toHaveBeenLastCalledWith("apply_codex_settings_change", {
        request: { selection: deploy, approval: codexDeployPlan.approval },
    });
    vi.mocked(invoke).mockResolvedValueOnce(codexSetDefaultPlan);
    expect(await previewCodexSettingsChange(setDefault)).toEqual(
        codexSetDefaultPlan,
    );
    vi.mocked(invoke).mockResolvedValueOnce(codexSettingsState);
    expect((await readCodexSettingsState()).profileActive).toBe(true);
    expect(invoke).toHaveBeenLastCalledWith("codex_settings_state", {});
    vi.mocked(invoke).mockResolvedValueOnce(codexSettingsHistory);
    await readCodexSettingsHistory(codexProviderId);
    expect(invoke).toHaveBeenLastCalledWith("codex_settings_history", {
        request: { providerId: codexProviderId, after: null, limit: 20 },
    });
    vi.mocked(invoke).mockResolvedValueOnce({
        changeId: codexSettingsRecovery.id,
        restartRequired: false,
    });
    await recoverCodexSettingsChange(codexSettingsRecovery);
    expect(invoke).toHaveBeenLastCalledWith("recover_codex_settings_change", {
        request: {
            changeId: codexSettingsRecovery.id,
            approval: codexSettingsRecovery.approval,
        },
    });
});

test("browser mode refuses every native operation without IPC", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    for (const operation of [
        () => previewCodexSettingsChange(deploy),
        () => applyCodexSettingsChange(deploy, codexDeployPlan),
        () => readCodexSettingsState(),
        () => readCodexSettingsHistory(),
        () => recoverCodexSettingsChange(codexSettingsRecovery),
    ])
        expect(await failure(operation)).toBe("desktop_required");
    expect(invoke).not.toHaveBeenCalled();
});

test("unmasked credentials, other wire APIs and mismatched plans are rejected", async () => {
    const entry = {
        base_url: "https://api.deepseek.com",
        experimental_bearer_token: "••••••••",
        name: "vibemate-Deep Seek",
        wire_api: "responses",
    };
    for (const changed of [
        { ...entry, experimental_bearer_token: "sk-real" },
        { ...entry, wire_api: "chat" },
        { ...entry, env_key: "KEY" },
    ]) {
        vi.mocked(invoke).mockResolvedValueOnce({
            ...codexDeployPlan,
            after: { model_providers: { "vibemate-deep-seek": changed } },
        });
        expect(await failure(() => previewCodexSettingsChange(deploy))).toBe(
            "invalid_response",
        );
    }
    const exposed = structuredClone(codexDeployPlan);
    exposed.summary.changes[0] = {
        path: "model_providers.vibemate-deep-seek.experimental_bearer_token",
        kind: "added",
        before: null,
        after: "sk-real",
        secret: true,
    };
    vi.mocked(invoke).mockResolvedValueOnce(exposed);
    expect(await failure(() => previewCodexSettingsChange(deploy))).toBe(
        "invalid_response",
    );
    vi.mocked(invoke).mockResolvedValueOnce({
        ...codexDeployPlan,
        providerRevision: 2,
    });
    expect(await failure(() => previewCodexSettingsChange(deploy))).toBe(
        "invalid_response",
    );
    vi.mocked(invoke).mockResolvedValueOnce({
        ...codexSetDefaultPlan,
        writesFile: true,
        notices: ["release_only"],
    });
    expect(await failure(() => previewCodexSettingsChange(setDefault))).toBe(
        "invalid_response",
    );
    expect(
        await failure(() =>
            previewCodexSettingsChange({ ...deploy, modelId: "x" }),
        ),
    ).toBe("invalid_request");
});

test("stable backend codes pass through and unknown errors stay opaque", async () => {
    for (const code of [
        "plan_stale",
        "name_conflict",
        "default_in_use",
        "base_url_unverified",
        "model_unsupported",
    ]) {
        vi.mocked(invoke).mockRejectedValueOnce(code);
        expect(await failure(() => previewCodexSettingsChange(deploy))).toBe(
            code,
        );
    }
    vi.mocked(invoke).mockRejectedValueOnce(new Error("/Users/secret path"));
    expect(await failure(() => readCodexSettingsState())).toBe(
        "outcome_unknown",
    );
    expect(CODEX_CAPABILITIES.wireApi).toBe("responses");
    expect(CODEX_CAPABILITIES.profiles).toBe(false);
});

test("contradictory state, history and recovery responses are refused", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
        ...codexSettingsState,
        providers: [
            ...codexSettingsState.providers,
            ...codexSettingsState.providers,
        ],
    });
    expect(await failure(() => readCodexSettingsState())).toBe(
        "invalid_response",
    );
    vi.mocked(invoke).mockResolvedValueOnce({
        ...codexSettingsHistory,
        items: [...codexSettingsHistory.items].reverse(),
    });
    expect(await failure(() => readCodexSettingsHistory())).toBe(
        "invalid_response",
    );
    expect(
        await failure(() =>
            recoverCodexSettingsChange({
                ...codexSettingsRecovery,
                approval: null,
            }),
        ),
    ).toBe("recovery_required");
    vi.mocked(invoke).mockResolvedValueOnce({
        changeId: "b".repeat(32),
        restartRequired: false,
    });
    expect(
        await failure(() => applyCodexSettingsChange(deploy, codexDeployPlan)),
    ).toBe("invalid_response");
});

function fixtures() {
    const fallback = codexSettingsState.default;
    const [binding] = codexSettingsState.providers;
    const [setItem, deployItem] = codexSettingsHistory.items;
    if (!fallback || !binding || !setItem || !deployItem)
        throw new Error("fixture shape changed");
    return { fallback, binding, setItem, deployItem };
}

test("missing defaults and history model ids follow the Rust contract", async () => {
    const { fallback, setItem, deployItem } = fixtures();
    vi.mocked(invoke).mockResolvedValueOnce({
        ...codexSettingsState,
        default: { ...fallback, state: "missing" },
    });
    expect((await readCodexSettingsState()).default?.state).toBe("missing");
    vi.mocked(invoke).mockResolvedValueOnce({
        ...codexSettingsState,
        default: { ...fallback, state: "removed" },
    });
    expect(await failure(() => readCodexSettingsState())).toBe(
        "invalid_response",
    );

    vi.mocked(invoke).mockResolvedValueOnce(codexSettingsHistory);
    const history = await readCodexSettingsHistory();
    expect(history.items.map((item) => item.modelId)).toEqual([
        "deepseek-flash",
        null,
    ]);
    for (const items of [
        // A default change must name its model; deploy/revoke never do.
        [{ ...setItem, modelId: null }],
        [{ ...deployItem, modelId: "deepseek-flash" }],
        [{ ...deployItem, modelId: undefined }],
    ]) {
        vi.mocked(invoke).mockResolvedValueOnce({
            ...codexSettingsHistory,
            items,
        });
        expect(await failure(() => readCodexSettingsHistory())).toBe(
            "invalid_response",
        );
    }
});

test("owned-only actions pin the owned binding revision", () => {
    const owned = fixtures().binding;
    const binding = {
        ...owned,
        owner: { ...owned.owner, providerRevision: 3 },
    };
    expect(ownedBindingSelection(binding, "clear_default")).toEqual({
        providerId: binding.providerId,
        expectedRevision: 3,
        action: "clear_default",
        modelId: null,
    });
    expect(ownedBindingSelection(binding, "revoke").expectedRevision).toBe(3);
});

test("owned bindings never carry the entry digest", async () => {
    const { binding } = fixtures();
    vi.mocked(invoke).mockResolvedValueOnce({
        ...codexSettingsState,
        providers: [
            {
                ...binding,
                owner: { ...binding.owner, entryDigest: "a".repeat(64) },
            },
        ],
    });
    expect(await failure(() => readCodexSettingsState())).toBe(
        "invalid_response",
    );
});
