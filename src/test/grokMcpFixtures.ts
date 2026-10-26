import type { McpRecord } from "../lib/desktop/mcp";
import type { GrokMcpPreview } from "../lib/desktop/grokMcpPreview";

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
