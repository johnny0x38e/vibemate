import { beforeEach, expect, test, vi } from "vitest";
import { isTauri } from "@tauri-apps/api/core";
import { readConfigurationHistory } from "./history";
import { readClaudeMcpState } from "./claudeMcp";
import { readGrokMcpState } from "./grokMcp";
import { readPiMcpState } from "./piMcp";
import { listSkillDeployRecoveries } from "./skills";
import { listRecoverySummary } from "./recoverySummary";
import { readCodexSettingsState } from "./codex";
import { readCodexMcpState } from "./codexMcp";
import {
    codexMcpRecovery,
    codexSettingsRecovery,
    codexSettingsState,
    emptyCodexMcpState,
} from "../../test/codexFixtures";

vi.mock(import("@tauri-apps/api/core"), async (importOriginal) => ({
    ...(await importOriginal()),
    isTauri: vi.fn(),
}));
vi.mock(import("./history"), async (importOriginal) => ({
    ...(await importOriginal()),
    readConfigurationHistory: vi.fn(),
}));
vi.mock(import("./skills"), async (importOriginal) => ({
    ...(await importOriginal()),
    listSkillDeployRecoveries: vi.fn(),
}));
vi.mock(import("./piMcp"), async (importOriginal) => ({
    ...(await importOriginal()),
    readPiMcpState: vi.fn(),
}));
vi.mock(import("./claudeMcp"), async (importOriginal) => ({
    ...(await importOriginal()),
    readClaudeMcpState: vi.fn(),
}));
vi.mock(import("./grokMcp"), async (importOriginal) => ({
    ...(await importOriginal()),
    readGrokMcpState: vi.fn(),
}));

vi.mock(import("./codex"), async (importOriginal) => ({
    ...(await importOriginal()),
    readCodexSettingsState: vi.fn(),
}));
vi.mock(import("./codexMcp"), async (importOriginal) => ({
    ...(await importOriginal()),
    readCodexMcpState: vi.fn(),
}));

const tauri = vi.mocked(isTauri);
const codexSettings = vi.mocked(readCodexSettingsState);
const codexMcp = vi.mocked(readCodexMcpState);
const history = vi.mocked(readConfigurationHistory);
const skills = vi.mocked(listSkillDeployRecoveries);
const pi = vi.mocked(readPiMcpState);
const grok = vi.mocked(readGrokMcpState);
const claude = vi.mocked(readClaudeMcpState);

beforeEach(() => {
    tauri.mockReset().mockReturnValue(true);
    history.mockReset();
    skills.mockReset();
    pi.mockReset();
    grok.mockReset();
    claude.mockReset().mockRejectedValue(new Error("claude down"));
    codexSettings.mockReset().mockRejectedValue(new Error("codex down"));
    codexMcp.mockReset().mockRejectedValue(new Error("codex mcp down"));
});

test("browser preview stays honest without inventing rows", async () => {
    tauri.mockReturnValue(false);
    await expect(listRecoverySummary()).resolves.toEqual({ kind: "preview" });
    expect(history).not.toHaveBeenCalled();
});

test("aggregates agent, skill and mcp interruptions without replaying writes", async () => {
    history.mockResolvedValue({
        kind: "desktop",
        page: {
            current: {
                status: "unrecorded",
                matchedEntryKey: null,
                files: [
                    {
                        targetPath: "/tmp/settings.json",
                        currentDigest: "a".repeat(64),
                        readable: true,
                    },
                ],
                providerLabel: null,
                modelId: null,
                modelLabel: null,
            },
            active: { kind: "unknown" },
            currentEntry: null,
            entries: [
                {
                    entryKey: "operation:cccccccccccccccccccccccccccccccc",
                    operationId: "cccccccccccccccccccccccccccccccc",
                    intent: "apply",
                    ownerRef: null,
                    ownerLabel: "Work",
                    requiresResolution: true,
                    sourceState: "unknown",
                    createdAt: 1,
                    updatedAt: 1,
                    sourceSteps: [
                        {
                            ordinal: 0,
                            targetPath: "/tmp/settings.json",
                            expectedDigest: "a".repeat(64),
                            appliedDigest: null,
                            previousExisted: null,
                            backupRecorded: false,
                            backupState: "not_needed",
                            state: "failed",
                            errorCode: "outcome_unknown",
                            restoreState: null,
                        },
                    ],
                    revisions: [],
                },
            ],
            nextCursor: null,
        },
    });
    skills.mockResolvedValue({
        kind: "desktop",
        items: [
            {
                id: "dddddddddddddddddddddddddddddddd",
                kind: "apply",
                state: "prepared",
                classification: "needs_record",
                skillId: "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee",
                skillName: "demo",
                agentId: "pi",
                configRoot: "/tmp/pi",
                targetPath: "/tmp/pi/skills/demo",
                mode: "symlink",
                canFinish: true,
                canDismiss: false,
            },
        ],
    });
    pi.mockResolvedValue({
        configRoot: "/tmp/pi",
        targetPath: "/tmp/pi/mcp.json",
        deployments: [],
        nextCursor: null,
        recovery: {
            id: "ffffffffffffffffffffffffffffffff",
            definitionId: "11111111111111111111111111111111",
            serverName: "tools",
            action: "deploy",
            observation: "intended",
            approval: "b".repeat(64),
        },
    });
    grok.mockResolvedValue({
        configRoot: "/tmp/grok",
        targetPath: "/tmp/grok/config.toml",
        deployments: [],
        nextCursor: null,
        recovery: null,
    });

    const result = await listRecoverySummary();
    expect(result.kind).toBe("desktop");
    if (result.kind !== "desktop") return;
    expect(result.items.map((item) => item.domain)).toEqual([
        "agentConfig",
        "agentConfig",
        "agentConfig",
        "skillDeploy",
        "mcp",
    ]);
    expect(result.items.some((item) => item.subject === "demo")).toBe(true);
    expect(result.items.some((item) => item.subject === "tools")).toBe(true);
});

test("keeps other domains when one read fails", async () => {
    history.mockRejectedValue(new Error("history down"));
    skills.mockResolvedValue({ kind: "desktop", items: [] });
    pi.mockRejectedValue(new Error("pi down"));
    grok.mockResolvedValue({
        configRoot: "/tmp/grok",
        targetPath: "/tmp/grok/config.toml",
        deployments: [],
        nextCursor: null,
        recovery: {
            id: "22222222222222222222222222222222",
            definitionId: "33333333333333333333333333333333",
            serverName: "grok-tools",
            action: "revoke",
            observation: "before",
            approval: "c".repeat(64),
        },
    });
    const result = await listRecoverySummary();
    expect(result).toEqual({
        kind: "desktop",
        items: [
            {
                id: "mcp:grok:22222222222222222222222222222222",
                domain: "mcp",
                destination: "mcp",
                agentId: "grokBuild",
                subject: "grok-tools",
                detail: "revoke:before",
            },
        ],
    });
});

test("surfaces an interrupted Claude Code user-scope MCP change", async () => {
    history.mockRejectedValue(new Error("history down"));
    skills.mockResolvedValue({ kind: "desktop", items: [] });
    pi.mockRejectedValue(new Error("pi down"));
    grok.mockRejectedValue(new Error("grok down"));
    claude.mockResolvedValue({
        configRoot: "/tmp/home",
        targetPath: "/tmp/home/.claude.json",
        capabilities: { toggle: false, transports: ["stdio", "http"] },
        deployments: [],
        nextCursor: null,
        recovery: {
            id: "44444444444444444444444444444444",
            definitionId: "55555555555555555555555555555555",
            serverName: "claude-tools",
            action: "deploy",
            observation: "intended",
            approval: "d".repeat(64),
        },
    });
    await expect(listRecoverySummary()).resolves.toEqual({
        kind: "desktop",
        items: [
            {
                id: "mcp:claude:44444444444444444444444444444444",
                domain: "mcp",
                destination: "mcp",
                agentId: "claudeCode",
                subject: "claude-tools",
                detail: "deploy:intended",
            },
        ],
    });
});

test("surfaces interrupted Codex MCP and provider changes", async () => {
    history.mockRejectedValue(new Error("history down"));
    skills.mockResolvedValue({ kind: "desktop", items: [] });
    pi.mockRejectedValue(new Error("pi down"));
    grok.mockRejectedValue(new Error("grok down"));
    codexMcp.mockResolvedValue({
        ...emptyCodexMcpState,
        recovery: codexMcpRecovery,
    });
    codexSettings.mockResolvedValue({
        ...codexSettingsState,
        recovery: codexSettingsRecovery,
    });
    await expect(listRecoverySummary()).resolves.toEqual({
        kind: "desktop",
        items: [
            {
                id: `mcp:codex:${codexMcpRecovery.id}`,
                domain: "mcp",
                destination: "mcp",
                agentId: "codex",
                subject: codexMcpRecovery.serverName,
                detail: `${codexMcpRecovery.action}:${codexMcpRecovery.observation}`,
            },
            {
                id: `agent:codex:${codexSettingsRecovery.id}`,
                domain: "agentConfig",
                destination: "agents",
                agentId: "codex",
                subject: "deploy",
                detail: "vibemate-deep-seek:before",
            },
        ],
    });
});
