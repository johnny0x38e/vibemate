import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, expect, test, vi } from "vitest";
import {
    CLAUDE_MCP_TOGGLE_SUPPORTED,
    applyClaudeMcpChange,
    previewClaudeMcpChange,
    readClaudeMcpHistory,
    readClaudeMcpState,
    recoverClaudeMcpChange,
} from "./claudeMcp";
import {
    claudeMcpDefinition,
    claudeMcpDeployment,
    claudeMcpHistory,
    claudeMcpPlan,
    claudeMcpRecovery,
    emptyClaudeMcpState,
} from "../../test/claudeMcpFixtures";
vi.mock(import("@tauri-apps/api/core"));
const selection = {
    definitionId: claudeMcpDefinition.id,
    expectedRevision: 1,
    action: "deploy",
    historyId: null,
} as const;
beforeEach(() => {
    vi.mocked(invoke).mockReset();
    vi.mocked(isTauri).mockReturnValue(true);
});

test("review, confirm, observe, history and recovery expose only scoped requests", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(claudeMcpPlan);
    expect(await previewClaudeMcpChange(selection)).toEqual(claudeMcpPlan);
    expect(invoke).toHaveBeenLastCalledWith("preview_claude_mcp_change", {
        selection,
    });
    const outcome = {
        changeId: "b".repeat(32),
        registered: true,
        reloadRequired: true,
    };
    vi.mocked(invoke).mockResolvedValueOnce(outcome);
    expect(await applyClaudeMcpChange(selection, claudeMcpPlan)).toEqual(
        outcome,
    );
    expect(invoke).toHaveBeenLastCalledWith("apply_claude_mcp_change", {
        request: { selection, approval: claudeMcpPlan.approval },
    });
    vi.mocked(invoke).mockResolvedValueOnce(emptyClaudeMcpState);
    expect((await readClaudeMcpState()).capabilities.toggle).toBe(false);
    expect(invoke).toHaveBeenLastCalledWith("claude_mcp_state", {
        request: { definitionId: null, after: null, limit: 20 },
    });
    vi.mocked(invoke).mockResolvedValueOnce(claudeMcpHistory);
    await readClaudeMcpHistory(selection.definitionId);
    expect(invoke).toHaveBeenLastCalledWith("claude_mcp_history", {
        request: {
            definitionId: selection.definitionId,
            after: null,
            limit: 20,
        },
    });
    vi.mocked(invoke).mockResolvedValueOnce({
        ...outcome,
        changeId: claudeMcpRecovery.id,
    });
    await recoverClaudeMcpChange(claudeMcpRecovery);
    expect(invoke).toHaveBeenLastCalledWith("recover_claude_mcp_change", {
        request: {
            changeId: claudeMcpRecovery.id,
            approval: claudeMcpRecovery.approval,
        },
    });
});

test("browser mode refuses every native operation without invoking IPC", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    for (const operation of [
        () => previewClaudeMcpChange(selection),
        () => applyClaudeMcpChange(selection, claudeMcpPlan),
        () => readClaudeMcpState(),
        () => readClaudeMcpHistory(selection.definitionId),
        () => recoverClaudeMcpChange(claudeMcpRecovery),
    ])
        await expect(operation()).rejects.toMatchObject({
            code: "desktop_required",
        });
    expect(invoke).not.toHaveBeenCalled();
});

test("toggles are reported unsupported by flag and by the native error code", async () => {
    expect(CLAUDE_MCP_TOGGLE_SUPPORTED).toBe(false);
    vi.mocked(invoke).mockRejectedValue("toggle_unsupported");
    for (const action of ["enable", "disable"] as const)
        await expect(
            previewClaudeMcpChange({ ...selection, action }),
        ).rejects.toMatchObject({ code: "toggle_unsupported" });
});

test("stdio and Streamable HTTP entries are parsed independently", async () => {
    const http = {
        ...claudeMcpPlan,
        afterEntry: {
            type: "http",
            url: "https://example.com/mcp",
            headers: { authorization: "••••••••" },
        },
        summary: { ...claudeMcpPlan.summary, changes: [] },
    };
    vi.mocked(invoke).mockResolvedValueOnce(http);
    expect(await previewClaudeMcpChange(selection)).toEqual(http);
    const bare = {
        ...claudeMcpPlan,
        afterEntry: { type: "stdio", command: "probe", args: [] },
    };
    vi.mocked(invoke).mockResolvedValueOnce(bare);
    expect(await previewClaudeMcpChange(selection)).toEqual(bare);
});

test.each([
    { ...claudeMcpPlan, nativeDocument: { private: "discard-this" } },
    { ...claudeMcpPlan, disabledByUserList: false },
    { ...claudeMcpPlan, definitionId: "e".repeat(32) },
    { ...claudeMcpPlan, definitionRevision: 2 },
    { ...claudeMcpPlan, action: "revoke" },
    { ...claudeMcpPlan, action: "enable" },
    { ...claudeMcpPlan, afterEntry: null },
    {
        ...claudeMcpPlan,
        target: { ...claudeMcpPlan.target, path: "relative/.claude.json" },
    },
    {
        ...claudeMcpPlan,
        target: { ...claudeMcpPlan.target, path: "/test/home/.mcp.json" },
    },
    {
        ...claudeMcpPlan,
        target: {
            ...claudeMcpPlan.target,
            path: "/etc/claude-code/managed-mcp.json",
        },
    },
    { ...claudeMcpPlan, target: { ...claudeMcpPlan.target, symlink: true } },
    {
        ...claudeMcpPlan,
        target: { ...claudeMcpPlan.target, contentDigest: null },
    },
    { ...claudeMcpPlan, approval: "not-an-approval" },
    {
        ...claudeMcpPlan,
        afterEntry: {
            type: "stdio",
            command: "probe",
            args: [],
            env: { TOKEN: "discard-this" },
        },
    },
    {
        ...claudeMcpPlan,
        afterEntry: {
            type: "stdio",
            command: "probe",
            args: [],
            cwd: "/tmp",
        },
    },
    {
        ...claudeMcpPlan,
        afterEntry: { command: "probe", args: [] },
    },
    {
        ...claudeMcpPlan,
        afterEntry: {
            type: "sse",
            url: "https://example.com/mcp",
        },
    },
    {
        ...claudeMcpPlan,
        beforeEntry: {
            type: "http",
            url: "https://example.com/mcp",
            headers: { "x-custom": "discard-this" },
        },
    },
    {
        ...claudeMcpPlan,
        afterEntry: {
            type: "stdio",
            command: "probe",
            args: [],
            enabled: true,
        },
    },
    {
        ...claudeMcpPlan,
        summary: {
            ...claudeMcpPlan.summary,
            changes: [
                {
                    path: "mcpServers.tools.env.TOKEN",
                    kind: "added",
                    before: null,
                    after: "discard-this",
                    secret: false,
                },
            ],
        },
    },
])("malformed or unmasked review is discarded %#", async (response) => {
    vi.mocked(invoke).mockResolvedValue(response);
    try {
        await previewClaudeMcpChange(selection);
        throw new Error("Expected rejection");
    } catch (error: unknown) {
        expect(error).toMatchObject({
            code: "invalid_response",
            message: "Claude Code MCP request failed.",
        });
        expect(JSON.stringify(error)).not.toContain("discard-this");
    }
});

test("restore may reinstate a revoked history state without an entry", async () => {
    const restore = {
        ...selection,
        action: "restore",
        historyId: "b".repeat(32),
    } as const;
    const plan = { ...claudeMcpPlan, action: "restore", afterEntry: null };
    vi.mocked(invoke).mockResolvedValueOnce(plan);
    expect(await previewClaudeMcpChange(restore)).toEqual(plan);
});

test.each([
    { ...emptyClaudeMcpState, nativeContent: "discard-this" },
    {
        ...emptyClaudeMcpState,
        capabilities: { toggle: true, transports: ["stdio", "http"] },
    },
    { ...emptyClaudeMcpState, capabilities: undefined },
    {
        ...emptyClaudeMcpState,
        targetPath: "/test/home/.claude/settings.json",
    },
    {
        ...emptyClaudeMcpState,
        deployments: [claudeMcpDeployment, claudeMcpDeployment],
    },
    {
        ...emptyClaudeMcpState,
        deployments: [{ ...claudeMcpDeployment, configRoot: "/another/root" }],
    },
    {
        ...emptyClaudeMcpState,
        deployments: [{ ...claudeMcpDeployment, state: "connected" }],
    },
    {
        ...emptyClaudeMcpState,
        deployments: [
            {
                ...claudeMcpDeployment,
                ownership: { ...claudeMcpDeployment.ownership, enabled: true },
            },
        ],
    },
    { ...emptyClaudeMcpState, nextCursor: "e".repeat(32) },
    {
        ...emptyClaudeMcpState,
        recovery: { ...claudeMcpRecovery, observation: "conflict" },
    },
    {
        ...emptyClaudeMcpState,
        recovery: { ...claudeMcpRecovery, action: "disable" },
    },
])("contradictory root observations are rejected %#", async (response) => {
    vi.mocked(invoke).mockResolvedValue(response);
    await expect(readClaudeMcpState()).rejects.toMatchObject({
        code: "invalid_response",
    });
});

test("definition reads allow multiple recorded roots but reject another identity", async () => {
    const other = { ...claudeMcpDeployment, configRoot: "/older/claude" };
    vi.mocked(invoke).mockResolvedValue({
        ...emptyClaudeMcpState,
        deployments: [claudeMcpDeployment, other],
    });
    expect(
        (await readClaudeMcpState(selection.definitionId)).deployments,
    ).toHaveLength(2);
    vi.mocked(invoke).mockResolvedValue({
        ...emptyClaudeMcpState,
        deployments: [{ ...other, definitionId: "f".repeat(32) }],
    });
    await expect(
        readClaudeMcpState(selection.definitionId),
    ).rejects.toMatchObject({ code: "invalid_response" });
});

test("history validates page order, cursors, registration and unique results", async () => {
    const item = claudeMcpHistory.items[0];
    if (!item) throw new Error("Missing fixture");
    vi.mocked(invoke).mockResolvedValueOnce({
        items: [
            {
                ...item,
                id: "e".repeat(32),
                action: "revoke",
                createdAt: 3,
                registered: false,
            },
            {
                ...item,
                id: "d".repeat(32),
                action: "restore",
                createdAt: 2,
                registered: false,
            },
            item,
        ],
        nextCursor: null,
    });
    expect(
        (await readClaudeMcpHistory(selection.definitionId)).items,
    ).toHaveLength(3);
    for (const response of [
        { items: [item, item], nextCursor: null },
        {
            items: [
                item,
                { ...item, id: "e".repeat(32), createdAt: item.createdAt + 1 },
            ],
            nextCursor: null,
        },
        { items: [item], nextCursor: "f".repeat(32) },
        {
            items: [{ ...item, createdAt: Number.MAX_SAFE_INTEGER }],
            nextCursor: null,
        },
        { items: [{ ...item, registered: false }], nextCursor: null },
        {
            items: [{ ...item, action: "revoke", registered: true }],
            nextCursor: null,
        },
        { items: [{ ...item, enabled: true }], nextCursor: null },
        { items: [{ ...item, action: "enable" }], nextCursor: null },
    ]) {
        vi.mocked(invoke).mockResolvedValue(response);
        await expect(
            readClaudeMcpHistory(selection.definitionId),
        ).rejects.toMatchObject({ code: "invalid_response" });
    }
});

test("uncertain writes and recovery never invent a result", async () => {
    vi.mocked(invoke).mockResolvedValue({
        changeId: "b".repeat(32),
        registered: false,
        reloadRequired: true,
    });
    await expect(
        applyClaudeMcpChange(selection, claudeMcpPlan),
    ).rejects.toMatchObject({ code: "invalid_response" });
    vi.mocked(invoke).mockResolvedValue({
        changeId: "b".repeat(32),
        registered: true,
        reloadRequired: true,
    });
    await expect(
        recoverClaudeMcpChange(claudeMcpRecovery),
    ).rejects.toMatchObject({ code: "invalid_response" });
    vi.mocked(invoke).mockClear();
    await expect(
        recoverClaudeMcpChange({
            ...claudeMcpRecovery,
            approval: null,
            observation: "conflict",
        }),
    ).rejects.toMatchObject({ code: "recovery_required" });
    expect(invoke).not.toHaveBeenCalled();
});

test.each([
    "credential_unavailable",
    "plan_stale",
    "recovery_required",
    "snapshot_unavailable",
    "unsupported_field",
    "unsupported_server_name",
    "dynamic_value_unsupported",
    "path_semantics_unverified",
    "name_conflict",
    "ownership_conflict",
    "invalid_config",
])("known safe failures survive: %s", async (error) => {
    vi.mocked(invoke).mockRejectedValue(error);
    await expect(previewClaudeMcpChange(selection)).rejects.toMatchObject({
        code: error,
    });
});

test.each([
    new Error("discard-this"),
    { message: "discard-this" },
    "discard-this",
    "definition_in_use",
    null,
])("unknown diagnostics never become user text %#", async (error) => {
    vi.mocked(invoke).mockRejectedValue(error);
    try {
        await previewClaudeMcpChange(selection);
        throw new Error("Expected rejection");
    } catch (error: unknown) {
        expect(error).toMatchObject({
            code: "outcome_unknown",
            message: "Claude Code MCP request failed.",
        });
        expect(JSON.stringify(error)).not.toContain("discard-this");
    }
});
