import type {
    CodexSettingsHistory,
    CodexSettingsPlan,
    CodexSettingsRecovery,
    CodexSettingsState,
} from "../lib/desktop/codex";
import type {
    CodexMcpDeployment,
    CodexMcpHistory,
    CodexMcpPlan,
    CodexMcpRecovery,
    CodexMcpState,
} from "../lib/desktop/codexMcp";

const MASK = "••••••••";
/** Synthetic central provider identity; no credential is ever present. */
export const codexProviderId = "0123456789abcdef0123456789abcdef";
const target = {
    path: "/test/codex/config.toml",
    exists: true,
    contentDigest: "c".repeat(64),
    symlink: false,
} as const;

/** Masked first deployment of a DeepSeek provider table. */
export const codexDeployPlan: CodexSettingsPlan = {
    target,
    providerId: codexProviderId,
    providerRevision: 1,
    action: "deploy",
    providerKey: "vibemate-deep-seek",
    modelId: null,
    before: null,
    after: {
        model_providers: {
            "vibemate-deep-seek": {
                base_url: "https://api.deepseek.com",
                experimental_bearer_token: MASK,
                name: "vibemate-Deep Seek",
                wire_api: "responses",
            },
        },
    },
    summary: {
        format: "json",
        changes: [
            {
                path: "model_providers.vibemate-deep-seek.experimental_bearer_token",
                kind: "added",
                before: null,
                after: MASK,
                secret: true,
            },
            {
                path: "model_providers.vibemate-deep-seek.wire_api",
                kind: "added",
                before: null,
                after: "responses",
                secret: false,
            },
        ],
        truncated: false,
        reason: null,
    },
    notices: ["plaintext_credential"],
    writesFile: true,
    approval: "d".repeat(64),
};

/** Owned default replacing a hand-chosen model while a profile is active. */
export const codexSetDefaultPlan: CodexSettingsPlan = {
    target,
    providerId: codexProviderId,
    providerRevision: 1,
    action: "set_default",
    providerKey: "vibemate-deep-seek",
    modelId: "deepseek-flash",
    before: {
        model: "gpt-5.5",
        model_provider: null,
        model_reasoning_effort: "high",
    },
    after: {
        model: "deepseek-flash",
        model_provider: "vibemate-deep-seek",
        model_reasoning_effort: "high",
    },
    summary: {
        format: "json",
        changes: [
            {
                path: "model",
                kind: "replaced",
                before: "gpt-5.5",
                after: "deepseek-flash",
                secret: false,
            },
        ],
        truncated: false,
        reason: null,
    },
    notices: ["profile_overrides_default"],
    writesFile: true,
    approval: "e".repeat(64),
};

/** Root with one owned table and the owned default. */
export const codexSettingsState: CodexSettingsState = {
    configRoot: "/test/codex",
    targetPath: "/test/codex/config.toml",
    providers: [
        {
            providerId: codexProviderId,
            displayName: "Deep Seek",
            owner: {
                providerKey: "vibemate-deep-seek",
                providerRevision: 1,
                generation: "b".repeat(32),
            },
            outdated: false,
            state: "written",
        },
    ],
    default: {
        providerId: codexProviderId,
        modelId: "deepseek-flash",
        providerKey: "vibemate-deep-seek",
        state: "written",
    },
    profileActive: true,
    recovery: null,
};

/** Interrupted deploy whose file still shows the before bytes. */
export const codexSettingsRecovery: CodexSettingsRecovery = {
    id: "f".repeat(32),
    providerId: codexProviderId,
    providerKey: "vibemate-deep-seek",
    action: "deploy",
    observation: "before",
    approval: "9".repeat(64),
};

/** Newest-first completed changes. */
export const codexSettingsHistory: CodexSettingsHistory = {
    items: [
        {
            id: "2".repeat(32),
            action: "set_default",
            providerId: codexProviderId,
            providerKey: "vibemate-deep-seek",
            modelId: "deepseek-flash",
            createdAt: 2,
        },
        {
            id: "1".repeat(32),
            action: "deploy",
            providerId: codexProviderId,
            providerKey: "vibemate-deep-seek",
            modelId: null,
            createdAt: 1,
        },
    ],
    nextCursor: null,
};

/** Synthetic MCP definition identity. */
export const codexMcpDefinitionId = "a".repeat(32);

/** Masked HTTP entry; the bearer header is plaintext only on disk. */
export const codexMcpPlan: CodexMcpPlan = {
    target,
    definitionId: codexMcpDefinitionId,
    definitionRevision: 1,
    action: "deploy",
    serverName: "remote-tools",
    beforeEntry: null,
    afterEntry: {
        url: "https://example.com/mcp",
        http_headers: { Authorization: MASK },
        enabled: true,
    },
    summary: {
        format: "json",
        changes: [
            {
                path: "mcp_servers.remote-tools.http_headers.Authorization",
                kind: "added",
                before: null,
                after: MASK,
                secret: true,
            },
        ],
        truncated: false,
        reason: null,
    },
    approval: "d".repeat(64),
};

/** Empty isolated root. */
export const emptyCodexMcpState: CodexMcpState = {
    configRoot: "/test/codex",
    targetPath: "/test/codex/config.toml",
    deployments: [],
    nextCursor: null,
    recovery: null,
};

/** One owned entry that Codex reserialized without semantic change. */
export const codexMcpDeployment: CodexMcpDeployment = {
    definitionId: codexMcpDefinitionId,
    displayName: "Remote tools",
    configRoot: "/test/codex",
    ownership: {
        serverName: "remote-tools",
        definitionRevision: 1,
        generation: "b".repeat(32),
        enabled: true,
    },
    state: "written",
    nativeEnabled: true,
    unmanagedKeys: [],
};

/** Retained entry results. */
export const codexMcpHistory: CodexMcpHistory = {
    items: [
        {
            id: "b".repeat(32),
            action: "deploy",
            createdAt: 2,
            serverName: "remote-tools",
            enabled: true,
        },
    ],
    nextCursor: null,
};

/** Interrupted MCP write awaiting explicit confirmation. */
export const codexMcpRecovery: CodexMcpRecovery = {
    id: "c".repeat(32),
    definitionId: codexMcpDefinitionId,
    serverName: "remote-tools",
    action: "deploy",
    observation: "intended",
    approval: "8".repeat(64),
};
