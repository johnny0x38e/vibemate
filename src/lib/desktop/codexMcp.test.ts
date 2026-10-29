import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, expect, test, vi } from "vitest";
import {
    CodexMcpError,
    applyCodexMcpChange,
    previewCodexMcpChange,
    readCodexMcpHistory,
    readCodexMcpState,
    recoverCodexMcpChange,
} from "./codexMcp";
import {
    codexMcpDefinitionId,
    codexMcpDeployment,
    codexMcpHistory,
    codexMcpPlan,
    codexMcpRecovery,
    emptyCodexMcpState,
} from "../../test/codexFixtures";
vi.mock(import("@tauri-apps/api/core"));
const selection = {
    definitionId: codexMcpDefinitionId,
    expectedRevision: 1,
    action: "deploy",
    historyId: null,
} as const;
beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});
async function failure(operation: () => Promise<unknown>): Promise<string> {
    try {
        await operation();
    } catch (error: unknown) {
        if (error instanceof CodexMcpError) return error.code;
    }
    return "no error";
}

test("review, confirm, observe, history and recovery use Codex commands", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(codexMcpPlan);
    expect(await previewCodexMcpChange(selection)).toEqual(codexMcpPlan);
    expect(invoke).toHaveBeenLastCalledWith("preview_codex_mcp_change", {
        selection,
    });
    const outcome = {
        changeId: "b".repeat(32),
        registered: true,
        reloadRequired: true,
    };
    vi.mocked(invoke).mockResolvedValueOnce(outcome);
    expect(await applyCodexMcpChange(selection, codexMcpPlan)).toEqual(outcome);
    expect(invoke).toHaveBeenLastCalledWith("apply_codex_mcp_change", {
        request: { selection, approval: codexMcpPlan.approval },
    });
    vi.mocked(invoke).mockResolvedValueOnce({
        ...emptyCodexMcpState,
        deployments: [codexMcpDeployment],
    });
    expect((await readCodexMcpState()).deployments).toHaveLength(1);
    expect(invoke).toHaveBeenLastCalledWith("codex_mcp_state", {
        request: { definitionId: null, after: null, limit: 20 },
    });
    vi.mocked(invoke).mockResolvedValueOnce(codexMcpHistory);
    await readCodexMcpHistory(codexMcpDefinitionId);
    expect(invoke).toHaveBeenLastCalledWith("codex_mcp_history", {
        request: { definitionId: codexMcpDefinitionId, after: null, limit: 20 },
    });
    vi.mocked(invoke).mockResolvedValueOnce({
        ...outcome,
        changeId: codexMcpRecovery.id,
    });
    await recoverCodexMcpChange(codexMcpRecovery);
    expect(invoke).toHaveBeenLastCalledWith("recover_codex_mcp_change", {
        request: {
            changeId: codexMcpRecovery.id,
            approval: codexMcpRecovery.approval,
        },
    });
});

test("Codex names, unmanaged keys and masking are validated", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
        ...codexMcpPlan,
        serverName: "team/tools:v1@host.x",
        beforeEntry: {
            url: "https://example.com/mcp",
            http_headers: {},
            enabled: true,
            unmanagedKeys: ["startup_timeout_sec"],
        },
    });
    expect((await previewCodexMcpChange(selection)).serverName).toBe(
        "team/tools:v1@host.x",
    );
    for (const plan of [
        { ...codexMcpPlan, serverName: "bad name" },
        {
            ...codexMcpPlan,
            afterEntry: {
                url: "https://example.com/mcp",
                http_headers: { Authorization: "Bearer real" },
                enabled: true,
            },
        },
        {
            ...codexMcpPlan,
            afterEntry: {
                url: "https://example.com/mcp",
                headers: {},
                enabled: true,
            },
        },
        { ...codexMcpPlan, disabledByUserList: false },
    ]) {
        vi.mocked(invoke).mockResolvedValueOnce(plan);
        expect(await failure(() => previewCodexMcpChange(selection))).toBe(
            "invalid_response",
        );
    }
});

test("browser mode never invokes IPC", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    expect(await failure(() => readCodexMcpState())).toBe("desktop_required");
    expect(invoke).not.toHaveBeenCalled();
});

test("native enabled flags and unmanaged key names are surfaced per row", async () => {
    const handDisabled = {
        ...codexMcpDeployment,
        state: "modified",
        nativeEnabled: false,
        unmanagedKeys: ["disabled_tools", "enabled_tools"],
    };
    vi.mocked(invoke).mockResolvedValueOnce({
        ...emptyCodexMcpState,
        deployments: [handDisabled],
    });
    const [row] = (await readCodexMcpState()).deployments;
    expect(row?.nativeEnabled).toBe(false);
    expect(row?.unmanagedKeys).toEqual(["disabled_tools", "enabled_tools"]);
    for (const deployment of [
        // A written row is exactly what vibemate wrote.
        { ...codexMcpDeployment, unmanagedKeys: ["enabled_tools"] },
        { ...codexMcpDeployment, nativeEnabled: null },
        // A missing row has no native facts.
        { ...codexMcpDeployment, state: "missing", nativeEnabled: false },
        { ...codexMcpDeployment, unmanagedKeys: undefined },
    ]) {
        vi.mocked(invoke).mockResolvedValueOnce({
            ...emptyCodexMcpState,
            deployments: [deployment],
        });
        expect(await failure(() => readCodexMcpState())).toBe(
            "invalid_response",
        );
    }
});

test("registration rows never carry the entry digest", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
        ...emptyCodexMcpState,
        deployments: [
            {
                ...codexMcpDeployment,
                ownership: {
                    ...codexMcpDeployment.ownership,
                    entryDigest: "e".repeat(64),
                },
            },
        ],
    });
    expect(await failure(() => readCodexMcpState())).toBe("invalid_response");
});
