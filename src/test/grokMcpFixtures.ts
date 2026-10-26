import type { McpRecord } from "../lib/desktop/mcp";
import type { GrokMcpPreview } from "../lib/desktop/grokMcpPreview";
import type {
    GrokMcpDeployment,
    GrokMcpHistory,
    GrokMcpPlan,
    GrokMcpRecovery,
    GrokMcpState,
} from "../lib/desktop/grokMcp";

/** Synthetic central row; no executable or credential value is resolved. */
export const grokMcpRecord: McpRecord = {
    id: "0123456789abcdef0123456789abcdef",
    displayName: "Tools",
    serverName: "tools",
    enabled: true,
    revision: 2,
    connection: {
        type: "stdio",
        command: "nonexistent-vibemate-test-server",
        args: ["literal space"],
        cwd: null,
        env: ["TOKEN"],
    },
    createdAtMs: 1,
    updatedAtMs: 2,
    cleanupPending: false,
};

/** Masked first-registration fixture, not a deployment or a connection result. */
export const grokMcpPreview: GrokMcpPreview = {
    target: {
        path: "/isolated/grok/config.toml",
        exists: false,
        contentDigest: null,
        symlink: false,
    },
    proposal: {
        definitionId: grokMcpRecord.id,
        revision: grokMcpRecord.revision,
        serverName: grokMcpRecord.serverName,
        redactedEntry: {
            command: "nonexistent-vibemate-test-server",
            args: ["literal space"],
            env: { TOKEN: "••••••••" },
            enabled: true,
        },
        disabledByUserList: true,
    },
};

/** Synthetic central row for management workflow fixtures. */
export const grokMcpDefinition: McpRecord = {
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
export const emptyGrokMcpState: GrokMcpState = {
    configRoot: "/test/grok",
    targetPath: "/test/grok/config.toml",
    deployments: [],
    nextCursor: null,
    recovery: null,
};
/** Masked review; confirmation must pass its approval, not this JSON body. */
export const grokMcpPlan: GrokMcpPlan = {
    target: {
        path: "/test/grok/config.toml",
        exists: false,
        contentDigest: null,
        symlink: false,
    },
    definitionId: grokMcpDefinition.id,
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
                path: "mcp_servers.tools.command",
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
    disabledByUserList: false,
};
/** A successful entry result, not a connection claim. */
export const grokMcpDeployment: GrokMcpDeployment = {
    definitionId: grokMcpDefinition.id,
    displayName: "Tools",
    configRoot: emptyGrokMcpState.configRoot,
    ownership: {
        serverName: "tools",
        definitionRevision: 1,
        entryDigest: "e".repeat(64),
        generation: "b".repeat(32),
        enabled: true,
    },
    state: "written",
};
/** Independent retained entry operation. */
export const grokMcpHistory: GrokMcpHistory = {
    items: [
        {
            id: "b".repeat(32),
            action: "deploy",
            createdAt: 2,
            serverName: "tools",
            enabled: true,
        },
    ],
    nextCursor: null,
};
/** Exact interruption observation with a confirmable approval. */
export const grokMcpRecovery: GrokMcpRecovery = {
    id: "c".repeat(32),
    definitionId: grokMcpDefinition.id,
    serverName: "tools",
    action: "deploy",
    observation: "before",
    approval: "f".repeat(64),
};
