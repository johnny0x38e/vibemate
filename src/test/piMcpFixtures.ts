import type { McpRecord } from "../lib/desktop/mcp";
import type {
    PiMcpDeployment,
    PiMcpHistory,
    PiMcpPlan,
    PiMcpRecovery,
    PiMcpState,
} from "../lib/desktop/piMcp";

/** Synthetic fixtures only; values never come from native user files or keys. */
export const piMcpDefinition: McpRecord = {
    id: "a".repeat(32),
    displayName: "Tools",
    serverName: "tools",
    enabled: true,
    revision: 1,
    connection: {
        type: "stdio",
        command: "never-start-test-server",
        args: [],
        cwd: null,
        env: [],
    },
    createdAtMs: 1,
    updatedAtMs: 1,
    cleanupPending: false,
};
/** Default isolated root with no registration. */
export const emptyPiMcpState: PiMcpState = {
    configRoot: "/test/pi",
    targetPath: "/test/pi/mcp.json",
    deployments: [],
    nextCursor: null,
    recovery: null,
};
/** Masked review; confirmation must pass its approval, not this JSON body. */
export const piMcpPlan: PiMcpPlan = {
    target: {
        path: "/test/pi/mcp.json",
        exists: false,
        contentDigest: null,
        symlink: false,
    },
    definitionId: piMcpDefinition.id,
    definitionRevision: 1,
    action: "deploy",
    serverName: "tools",
    beforeEntry: null,
    afterEntry: {
        command: "never-start-test-server",
        args: [],
        env: {},
        enabled: true,
    },
    summary: {
        format: "json",
        changes: [
            {
                path: "mcpServers.tools.command",
                kind: "added",
                before: null,
                after: "never-start-test-server",
                secret: false,
            },
        ],
        truncated: false,
        reason: null,
    },
    approval: "d".repeat(64),
    sessionRelativeCwd: false,
    expandsHome: false,
};
/** A successful entry result, not a connection claim. */
export const piMcpDeployment: PiMcpDeployment = {
    definitionId: piMcpDefinition.id,
    displayName: "Tools",
    configRoot: emptyPiMcpState.configRoot,
    ownership: {
        serverName: "tools",
        definitionRevision: 1,
        generation: "b".repeat(32),
        enabled: true,
    },
    state: "written",
};
/** Independent retained entry operation. */
export const piMcpHistory: PiMcpHistory = {
    items: [
        {
            id: "b".repeat(32),
            action: "deploy",
            createdAt: 10,
            serverName: "tools",
            enabled: true,
        },
    ],
    nextCursor: null,
};
/** Exact intended observation; confirming it does not replay a write. */
export const piMcpRecovery: PiMcpRecovery = {
    id: "c".repeat(32),
    definitionId: piMcpDefinition.id,
    serverName: "tools",
    action: "deploy",
    observation: "intended",
    approval: "f".repeat(64),
};
