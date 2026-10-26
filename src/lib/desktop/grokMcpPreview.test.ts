import { invoke, isTauri } from "@tauri-apps/api/core";
import { beforeEach, expect, test, vi } from "vitest";
import {
    grokMcpPreview as preview,
    grokMcpRecord as row,
} from "../../test/grokMcpFixtures";
import {
    GrokMcpPreviewError,
    previewGrokMcpRegistration,
} from "./grokMcpPreview";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: vi.fn() }));
const call = vi.mocked(invoke);
beforeEach(() => {
    call.mockReset().mockResolvedValue(preview);
    vi.mocked(isTauri).mockReturnValue(true);
});

test("sends only identity and revision and validates a masked Grok proposal", async () => {
    expect(await previewGrokMcpRegistration(row.id, row.revision)).toEqual(
        preview,
    );
    expect(call).toHaveBeenCalledExactlyOnceWith(
        "preview_grok_mcp_registration",
        {
            request: { definitionId: row.id, expectedRevision: row.revision },
        },
    );
});

test("browser preview never invokes a native command", async () => {
    vi.mocked(isTauri).mockReturnValue(false);
    await expect(
        previewGrokMcpRegistration(row.id, row.revision),
    ).rejects.toMatchObject({ code: "desktop_required" });
    expect(call).not.toHaveBeenCalled();
});

test.each([
    ["invalid", 1],
    [row.id, 0],
    [row.id, 1.5],
    [row.id, Number.MAX_SAFE_INTEGER + 1],
])("invalid selection %s / %s is rejected before IPC", async (id, revision) => {
    await expect(
        previewGrokMcpRegistration(id, revision),
    ).rejects.toMatchObject({ code: "invalid_request" });
    expect(call).not.toHaveBeenCalled();
});

test.each([
    ["extra document", { ...preview, nativeBytes: "synthetic-private-native" }],
    [
        "extra target property",
        {
            ...preview,
            target: { ...preview.target, content: "synthetic-private-native" },
        },
    ],
    [
        "wrong target",
        { ...preview, target: { ...preview.target, path: "/grok/auth.json" } },
    ],
    [
        "relative target",
        { ...preview, target: { ...preview.target, path: "grok/config.toml" } },
    ],
    [
        "dangling symlink result",
        { ...preview, target: { ...preview.target, symlink: true } },
    ],
    [
        "missing digest",
        { ...preview, target: { ...preview.target, exists: true } },
    ],
    [
        "unexpected digest",
        {
            ...preview,
            target: { ...preview.target, contentDigest: "a".repeat(64) },
        },
    ],
    [
        "wrong identity",
        {
            ...preview,
            proposal: { ...preview.proposal, definitionId: "a".repeat(32) },
        },
    ],
    [
        "wrong revision",
        {
            ...preview,
            proposal: { ...preview.proposal, revision: row.revision + 1 },
        },
    ],
    [
        "missing disable observation",
        {
            ...preview,
            proposal: { ...preview.proposal, disabledByUserList: undefined },
        },
    ],
    [
        "inadmissible prefix",
        { ...preview, proposal: { ...preview.proposal, serverName: "1tools" } },
    ],
    [
        "ambiguous prefix",
        { ...preview, proposal: { ...preview.proposal, serverName: "tools_" } },
    ],
    [
        "double underscore prefix",
        {
            ...preview,
            proposal: { ...preview.proposal, serverName: "my__tools" },
        },
    ],
    [
        "mixed transports",
        {
            ...preview,
            proposal: {
                ...preview.proposal,
                redactedEntry: {
                    command: "probe",
                    args: [],
                    env: {},
                    enabled: true,
                    url: "https://example.com/mcp",
                    headers: {},
                },
            },
        },
    ],
    [
        "unmasked env",
        {
            ...preview,
            proposal: {
                ...preview.proposal,
                redactedEntry: {
                    command: "probe",
                    args: [],
                    env: { TOKEN: "synthetic-private-secret" },
                    enabled: true,
                },
            },
        },
    ],
    [
        "unmasked headers",
        {
            ...preview,
            proposal: {
                ...preview.proposal,
                redactedEntry: {
                    url: "https://example.com/mcp",
                    headers: { authorization: "synthetic-private-secret" },
                    enabled: true,
                },
            },
        },
    ],
    [
        "URL credentials",
        {
            ...preview,
            proposal: {
                ...preview.proposal,
                redactedEntry: {
                    url: "https://user:synthetic-private-secret@example.com/mcp",
                    headers: {},
                    enabled: true,
                },
            },
        },
    ],
    [
        "dynamic command",
        {
            ...preview,
            proposal: {
                ...preview.proposal,
                redactedEntry: {
                    command: "${HOME}/probe",
                    args: [],
                    env: {},
                    enabled: true,
                },
            },
        },
    ],
    [
        "dynamic argument",
        {
            ...preview,
            proposal: {
                ...preview.proposal,
                redactedEntry: {
                    command: "probe",
                    args: ["${HOME}"],
                    env: {},
                    enabled: true,
                },
            },
        },
    ],
    [
        "unverified cwd",
        {
            ...preview,
            proposal: {
                ...preview.proposal,
                redactedEntry: {
                    command: "probe",
                    args: [],
                    cwd: "relative",
                    env: {},
                    enabled: true,
                },
            },
        },
    ],
    [
        "home expansion",
        {
            ...preview,
            proposal: {
                ...preview.proposal,
                redactedEntry: {
                    command: "~/probe",
                    args: [],
                    env: {},
                    enabled: true,
                },
            },
        },
    ],
])("discards %s without retaining raw data", async (_, response) => {
    call.mockResolvedValue(response);
    const error: unknown = await previewGrokMcpRegistration(
        row.id,
        row.revision,
    ).catch((error: unknown) => error);
    expect(error).toBeInstanceOf(GrokMcpPreviewError);
    expect(error).toMatchObject({ code: "invalid_response" });
    expect(JSON.stringify(error)).not.toContain("synthetic-private");
});

test.each([
    "name_conflict",
    "path_semantics_unverified",
    "unsupported_server_name",
    "dynamic_value_unsupported",
    "revision_conflict",
])("preserves safe error code %s", async (code) => {
    call.mockRejectedValue(code);
    await expect(
        previewGrokMcpRegistration(row.id, row.revision),
    ).rejects.toMatchObject({ code });
});

test("unknown native diagnostics are discarded", async () => {
    call.mockRejectedValue({
        message: "synthetic-private-native",
        path: "/private",
    });
    await expect(
        previewGrokMcpRegistration(row.id, row.revision),
    ).rejects.toEqual(new GrokMcpPreviewError("operation_failed"));
});

test("accepts masked HTTP independently from stdio and existing target metadata", async () => {
    const response = {
        ...preview,
        target: {
            ...preview.target,
            exists: true,
            contentDigest: "a".repeat(64),
            symlink: true,
        },
        proposal: {
            ...preview.proposal,
            redactedEntry: {
                url: "https://example.com/mcp",
                headers: { authorization: "••••••••" },
                enabled: false,
            },
        },
    };
    call.mockResolvedValue(response);
    expect(await previewGrokMcpRegistration(row.id, row.revision)).toEqual(
        response,
    );
});
