import type { McpRecord } from "../lib/desktop/mcp";
import type {
    ClaudeMcpDeployment,
    ClaudeMcpHistory,
    ClaudeMcpPlan,
    ClaudeMcpRecovery,
    ClaudeMcpState,
} from "../lib/desktop/claudeMcp";

/** Synthetic central row; no executable or credential value is resolved. */
export const claudeMcpDefinition: McpRecord = {
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
        env: ["TOKEN"],
    },
    createdAtMs: 1,
    updatedAtMs: 1,
    cleanupPending: false,
};
/** Default isolated root with no registration; toggles are never offered. */
export const emptyClaudeMcpState: ClaudeMcpState = {
    configRoot: "/test/home",
    targetPath: "/test/home/.claude.json",
    capabilities: { toggle: false, transports: ["stdio", "http"] },
    deployments: [],
    nextCursor: null,
    recovery: null,
};
/** Masked review; confirmation must pass its approval, not this JSON body. */
export const claudeMcpPlan: ClaudeMcpPlan = {
    target: {
        path: "/test/home/.claude.json",
        exists: true,
        contentDigest: "c".repeat(64),
        symlink: false,
    },
    definitionId: claudeMcpDefinition.id,
    definitionRevision: 1,
    action: "deploy",
    serverName: "tools",
    beforeEntry: null,
    afterEntry: {
        type: "stdio",
        command: "never-start-test-server",
        args: [],
        env: { TOKEN: "••••••••" },
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
            {
                path: "mcpServers.tools.env.TOKEN",
                kind: "added",
                before: null,
                after: "••••••••",
                secret: true,
            },
        ],
        truncated: false,
        reason: null,
    },
    approval: "d".repeat(64),
};
/** A successful entry result, not a connection claim. */
export const claudeMcpDeployment: ClaudeMcpDeployment = {
    definitionId: claudeMcpDefinition.id,
    displayName: "Tools",
    configRoot: emptyClaudeMcpState.configRoot,
    ownership: {
        serverName: "tools",
        definitionRevision: 1,
        generation: "b".repeat(32),
    },
    state: "written",
};
/** Independent retained entry operation. */
export const claudeMcpHistory: ClaudeMcpHistory = {
    items: [
        {
            id: "b".repeat(32),
            action: "deploy",
            createdAt: 2,
            serverName: "tools",
            registered: true,
        },
    ],
    nextCursor: null,
};
/** Exact interruption observation with a confirmable approval. */
export const claudeMcpRecovery: ClaudeMcpRecovery = {
    id: "c".repeat(32),
    definitionId: claudeMcpDefinition.id,
    serverName: "tools",
    action: "deploy",
    observation: "before",
    approval: "f".repeat(64),
};
